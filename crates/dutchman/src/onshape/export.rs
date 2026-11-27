//! STEP export jobs are bound to the source of the initial export request.
//! Polling and download endpoints use opaque job/file IDs, not document path kinds.
use super::*;
use std::time::Duration;

impl<C: HttpClient> Fetcher<'_, '_, C> {
    pub(super) fn export_step(
        &mut self,
        source: &DocumentUrl,
        eid: &str,
        name: &str,
    ) -> Result<()> {
        let (path, body, cache_file) = self.step_request(source, eid)?;
        if cache_file.exists() {
            let bytes = read_bounded(&cache_file, MAX_RESPONSE)?;
            self.session.credentials.check_bytes(&bytes)?;
            self.hits += 1;
            self.add(name.into(), bytes)?;
            self.record_source(name, &source.kind);
            return Ok(());
        }
        // A job started before a budget or transport interruption resumes. A workspace job
        // sits with the other workspace replies, and is deleted with them if it turns out to
        // postdate a change.
        let job_file = cache_file.with_extension("job");
        // A finished job whose download was interrupted goes straight to the download.
        let result_file = cache_file.with_extension("result");
        if result_file.exists() {
            let fid = String::from_utf8(read_bounded(&result_file, 256)?)
                .map_err(|_| response_error())?;
            return self.download_step(source, &resource_id(&fid)?, &cache_file, name);
        }
        let (mut info, resumed_tid) = if job_file.exists() {
            let tid =
                String::from_utf8(read_bounded(&job_file, 256)?).map_err(|_| response_error())?;
            let tid = resource_id(&tid)?;
            (
                self.translation("GET", &format!("{API}/translations/{tid}"), &[])?,
                Some(tid),
            )
        } else {
            (self.translation("POST", &path, &body)?, None)
        };
        validate_job(&info, source, eid)?;
        let tid = info
            .get("id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(resource_id)
            .transpose()?
            .or(resumed_tid)
            .unwrap_or_default();
        if tid.is_empty() && info.get("requestState").and_then(Value::as_str) == Some("ACTIVE") {
            return Err(response_error());
        }
        if !tid.is_empty() {
            self.cache_response(&job_file, tid.as_bytes())?;
        }
        let mut delay = 1;
        let mut done = false;
        for poll in 0..=20 {
            match info.get("requestState").and_then(Value::as_str) {
                Some("DONE") => {
                    done = true;
                    break;
                }
                Some("FAILED") => {
                    if job_file.exists() {
                        std::fs::remove_file(&job_file).map_err(io_error)?;
                    }
                    return Err(Error::new(ErrorKind::Export, "Onshape could not export this Part Studio to STEP; check that its final bodies regenerate, then retry."));
                }
                Some("ACTIVE") if poll < 20 => {
                    if self.session.calls >= self.session.max_calls {
                        return Err(self.session.error(ErrorKind::Budget, "Onshape call budget reached; raise it with --max-calls (MCP: max_calls) to finish the STEP export."));
                    }
                    self.session.client.sleep(Duration::from_secs(delay));
                    delay = (delay * 2).min(30);
                    info = self.translation("GET", &format!("{API}/translations/{tid}"), &[])?;
                    validate_job(&info, source, eid)?;
                    if let Some(actual) = info
                        .get("id")
                        .and_then(Value::as_str)
                        .filter(|s| !s.is_empty())
                    {
                        if actual != tid {
                            return Err(response_error());
                        }
                    }
                }
                Some("ACTIVE") => break,
                _ => return Err(response_error()),
            }
        }
        if !done {
            return Err(Error::new(ErrorKind::Export, "Onshape's STEP export is still running after 20 polls; retry later using the cache."));
        }
        let files = info
            .get("resultExternalDataIds")
            .and_then(Value::as_array)
            .ok_or_else(response_error)?;
        let [file] = files.as_slice() else {
            return Err(Error::new(ErrorKind::Export, "Onshape returned more or fewer than one STEP file; export the Part Studio as one grouped STEP file and retry."));
        };
        let fid = resource_id(file.as_str().ok_or_else(response_error)?)?;
        self.cache_response(&result_file, fid.as_bytes())?;
        self.download_step(source, &fid, &cache_file, name)
    }

    /// The export request, its body, and where its STEP file is cached.
    fn step_request(&self, source: &DocumentUrl, eid: &str) -> Result<(String, Vec<u8>, PathBuf)> {
        let path = format!(
            "{API}/partstudios/d/{}/{}/{}/e/{eid}/export/step",
            source.document, source.kind, source.version
        );
        let body = serde_json::to_vec(&json!({
            "storeInDocument": false, "notifyUser": false, "triggerAutoDownload": false,
            "grouping": true, "isYAxisUp": false, "excludeHiddenEntities": false,
            "stepVersionString": "AP242", "stepUnit": "METER"
        }))
        .map_err(|_| response_error())?;
        let cache_file = self.cache_dir(&path).join(hash(
            format!("POST {path}\n{}", String::from_utf8_lossy(&body)).as_bytes(),
        ));
        Ok((path, body, cache_file))
    }

    /// Calls the STEP export still needs: none once cached, the download alone after a
    /// finished job, a poll and the download after a started one, else all of them.
    pub(super) fn step_calls_missing(&self, source: &DocumentUrl, eid: &str) -> Result<u32> {
        let (_, _, cache_file) = self.step_request(source, eid)?;
        Ok(if cache_file.exists() {
            0
        } else if cache_file.with_extension("result").exists() {
            1
        } else if cache_file.with_extension("job").exists() {
            STEP_CALLS - 1
        } else {
            STEP_CALLS
        })
    }

    fn download_step(
        &mut self,
        source: &DocumentUrl,
        fid: &str,
        cache_file: &Path,
        name: &str,
    ) -> Result<()> {
        let download = format!("{API}/documents/d/{}/externaldata/{fid}", source.document);
        let response = self.session.request("GET", &download, "", &[])?;
        if response.status != 200 {
            return Err(response_error());
        }
        self.cache_response(cache_file, &response.body)?;
        self.add(name.into(), response.body)?;
        self.record_source(name, &source.kind);
        Ok(())
    }

    fn translation(&mut self, method: &str, path: &str, body: &[u8]) -> Result<Value> {
        let response = self.session.request(method, path, "", body)?;
        if !(200..300).contains(&response.status) {
            return Err(response_error());
        }
        let value: Value = serde_json::from_slice(&response.body).map_err(|_| response_error())?;
        self.session.credentials.check_json(&value)?;
        privacy(&value)?;
        Ok(value)
    }
}

fn resource_id(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 256
        || matches!(value, "." | "..")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
    {
        return Err(response_error());
    }
    Ok(value.into())
}

fn validate_job(value: &Value, source: &DocumentUrl, eid: &str) -> Result<()> {
    for (key, expected) in [
        ("documentId", source.document.as_str()),
        ("requestElementId", eid),
        (
            if source.kind == "v" {
                "versionId"
            } else {
                "workspaceId"
            },
            source.version.as_str(),
        ),
        ("resultDocumentId", source.document.as_str()),
    ] {
        if let Some(actual) = value.get(key).filter(|v| !v.is_null()) {
            let actual = actual.as_str().ok_or_else(response_error)?;
            if !actual.is_empty() && actual != expected {
                return Err(response_error());
            }
        }
    }
    Ok(())
}
