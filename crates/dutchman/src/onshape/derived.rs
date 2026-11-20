//! Capture pinned references from Derived features. This module fetches bytes only.
use super::*;

fn reference(
    value: &Value,
    depth: usize,
    found: &mut BTreeSet<(String, String, String)>,
) -> Result<()> {
    if depth > 64 {
        return Err(response_error());
    }
    if let (Some(d), Some(v), Some(e)) = (
        value.get("documentId").and_then(Value::as_str),
        value.get("versionId").and_then(Value::as_str),
        value.get("elementId").and_then(Value::as_str),
    ) {
        found.insert((id(d)?, id(v)?, id(e)?));
        if found.len() > 1000 {
            return Err(response_error());
        }
    }
    match value {
        Value::Object(map) => {
            for child in map.values() {
                reference(child, depth + 1, found)?;
            }
        }
        Value::Array(list) if list.len() <= 10_000 => {
            for child in list {
                reference(child, depth + 1, found)?;
            }
        }
        Value::Array(_) => return Err(response_error()),
        _ => {}
    }
    Ok(())
}

pub(super) fn capture_references<C: HttpClient>(
    fetcher: &mut Fetcher<'_, '_, C>,
    features: &[Value],
    scope: &Path,
    ancestry: &BTreeSet<(String, String, String)>,
) -> Result<()> {
    let mut wanted = BTreeSet::new();
    for feature in features {
        if feature.get("suppressed").and_then(Value::as_bool) == Some(true)
            || feature.get("parentSuppressed").and_then(Value::as_bool) == Some(true)
            || !matches!(
                feature.get("featureType").and_then(Value::as_str),
                Some("derived" | "importDerived")
            )
        {
            continue;
        }
        reference(feature, 0, &mut wanted)?;
    }
    for (document, version, element) in wanted {
        let identity = (document.clone(), version.clone(), element.clone());
        if ancestry.len() >= 8 || ancestry.contains(&identity) {
            return Err(Error::new(ErrorKind::Unsupported, "Derived Part Studios repeat a reference or exceed eight levels; fetch a Part Studio without a reference cycle."));
        }
        let prefix = format!("references/{document}/{version}/{element}/");
        if fetcher
            .files
            .contains_key(&format!("{prefix}manifest.json"))
        {
            continue;
        }
        let source = DocumentUrl {
            document,
            kind: "v".into(),
            version,
            element,
        };
        let resolution = scope.join(format!(
            "{}-v-{}-resolution.json",
            source.document, source.version
        ));
        let mid = if resolution.exists() {
            String::from_utf8(read_bounded(&resolution, 128)?).map_err(|_| response_error())?
        } else {
            let mid = current_microversion(fetcher.session, &source)?;
            private_write(&resolution, mid.as_bytes())?;
            mid
        };
        let mid = id(&mid)?;
        let mut chain = ancestry.clone();
        chain.insert(identity);
        let directory = tempfile::tempdir_in(scope).map_err(io_error)?;
        let output = directory.path().join("reference.onshape-bundle");
        let mut hits = 0;
        let Capture::Complete(_) = capture(
            fetcher.session,
            &source,
            &output,
            scope,
            &mid,
            &mut hits,
            &chain,
        )?
        else {
            return Err(changed_error());
        };
        fetcher.hits += hits;
        let bytes = read_bounded(&output, MAX_TOTAL as u64)?;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(io_error)?;
        if archive.len() > 4096 {
            return Err(response_error());
        }
        let mut sources = BTreeMap::new();
        for i in 0..archive.len() {
            let mut member = archive.by_index(i).map_err(io_error)?;
            let name = member.name().map_err(io_error)?.into_owned();
            let mut data = Vec::new();
            member
                .by_ref()
                .take(MAX_TOTAL as u64 + 1)
                .read_to_end(&mut data)
                .map_err(io_error)?;
            if name == "manifest.json" {
                let manifest: Value =
                    serde_json::from_slice(&data).map_err(|_| response_error())?;
                if let Some(map) = manifest.get("file_sources").and_then(Value::as_object) {
                    sources.extend(map.clone());
                }
            }
            fetcher.add(format!("{prefix}{name}"), data)?;
        }
        for (path, source) in sources {
            fetcher
                .file_sources
                .insert(format!("{prefix}{path}"), source);
        }
    }
    Ok(())
}
