//! No test in this file opens a socket. Responses are synthetic OpenAPI-shaped records.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]
use dutchman::onshape::*;
use serde_json::{json, Value};
use std::collections::{BTreeMap, VecDeque};
use std::io::Read;
use std::time::{Duration, SystemTime};

const DID: &str = "111111111111111111111111";
const VID: &str = "222222222222222222222222";
const MID: &str = "333333333333333333333333";
const PS: &str = "444444444444444444444444";
const AS: &str = "555555555555555555555555";
const ACCESS: &str = "testACCESS123456";
const SECRET: &str = "testSECRET567890";

#[derive(Default)]
struct Mock {
    replies: VecDeque<Response>,
    sent: Vec<(String, String, String)>,
    sleeps: Vec<Duration>,
    bodies: Vec<Vec<u8>>,
}
impl HttpClient for Mock {
    fn send(&mut self, request: &Request) -> Result<Response> {
        assert!(request.headers["Authorization"].starts_with("On testACCESS123456:HmacSHA256:"));
        self.bodies.push(request.body.clone());
        self.sent.push((
            request.method.clone(),
            request.path.clone(),
            request.query.clone(),
        ));
        Ok(self.replies.pop_front().expect("unexpected HTTP call"))
    }
    fn now(&self) -> SystemTime {
        httpdate::parse_http_date("Wed, 11 Nov 2026 12:00:00 GMT").unwrap()
    }
    fn nonce(&self) -> Result<String> {
        Ok("fixedNONCE".into())
    }
    fn sleep(&mut self, delay: Duration) {
        self.sleeps.push(delay);
    }
}
fn response(status: u16, body: Value) -> Response {
    Response {
        status,
        headers: BTreeMap::new(),
        body: serde_json::to_vec(&body).unwrap(),
    }
}
fn credentials() -> Credentials {
    Credentials::new(ACCESS.into(), SECRET.into()).unwrap()
}
fn source(element: &str) -> DocumentUrl {
    DocumentUrl::parse(
        &format!("https://cad.onshape.com/documents/{DID}/v/{VID}/e/{element}"),
        None,
    )
    .unwrap()
}
fn prepare(element: &str, kind: &str) -> Mock {
    Mock {
        replies: VecDeque::from([
            response(200, json!({"microversion": MID})),
            // Only the name is retained; document metadata is never stored verbatim.
            response(
                200,
                json!({"name":"Synthetic mechanism", "owner":{"id":"private-account", "name":"Private person"}}),
            ),
            response(
                200,
                json!([{"id":element,"elementType":kind,"name":"Synthetic tab"}]),
            ),
        ]),
        ..Mock::default()
    }
}
fn assembly_mock() -> Mock {
    let mut mock = prepare(AS, "ASSEMBLY");
    mock.replies.push_back(response(
        200,
        serde_json::from_str(include_str!("fixtures/onshape/assembly.json")).unwrap(),
    ));
    mock.replies.push_back(response(
        200,
        serde_json::from_str(include_str!("fixtures/onshape/assembly-features.json")).unwrap(),
    ));
    mock
}
fn run(
    mock: &mut Mock,
    dir: &tempfile::TempDir,
    element: &str,
    budget: u32,
) -> Result<FetchReport> {
    fetch(
        mock,
        &credentials(),
        &source(element),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        budget,
    )
}
#[test]
fn signing_matches_independent_python_hmac_vector() {
    assert_eq!(
        credentials().authorization(
            "GET",
            "/api/v17/partstudios/d/ABC/m/DEF/e/123/features",
            "includeGeometryIds=true",
            "Wed, 11 Nov 2026 12:00:00 GMT",
            "fixedNONCE"
        ),
        "On testACCESS123456:HmacSHA256:E/2mu0PNtYohbmyGNrVXNmfwy1o+VGaZBVZrp3cu9vg="
    );
}
#[test]
fn parses_urls_and_version_override_without_echoing_input() {
    let url = format!("https://cad.onshape.com/documents/{DID}/w/{VID}/e/{PS}");
    let parsed = DocumentUrl::parse(&url, Some(MID)).unwrap();
    assert_eq!((parsed.kind.as_str(), parsed.version.as_str()), ("v", MID));
    for url in [
        format!("{url}?secret={SECRET}"),
        format!("https://{ACCESS}:{SECRET}@cad.onshape.com/documents/{DID}/w/{VID}/e/{PS}"),
        url.replace("cad.onshape.com", "evil.example"),
        url.replace("https:", "http:"),
        url.replace(PS, "../x"),
    ] {
        let error = DocumentUrl::parse(&url, None).unwrap_err().to_string();
        assert!(!error.contains(SECRET));
        assert!(!error.contains(ACCESS));
    }
}
#[test]
fn two_part_revolute_assembly_bundle_is_exact_and_version_cache_uses_no_calls() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = assembly_mock();
    let report = run(&mut mock, &dir, AS, 50).unwrap();
    assert_eq!(report.calls, 5);
    assert_eq!(
        mock.sent[0].1,
        format!("/api/v17/documents/d/{DID}/v/{VID}/currentmicroversion")
    );
    assert_eq!(mock.sent[1].1, format!("/api/v17/documents/{DID}"));
    for (_, path, _) in mock.sent.iter().skip(2) {
        assert!(path.contains(&format!("/v/{VID}/")));
    }
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(std::fs::read(&report.output).unwrap())).unwrap();
    let mut names: Vec<_> = zip
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            format!("assemblies/{AS}/definition.json"),
            format!("assemblies/{AS}/features.json"),
            "manifest.json".into()
        ]
    );
    let mut manifest_bytes = Vec::new();
    zip.by_name("manifest.json")
        .unwrap()
        .read_to_end(&mut manifest_bytes)
        .unwrap();
    let manifest: Value = serde_json::from_slice(&manifest_bytes).unwrap();
    assert_eq!(manifest["schema"], "dutchman.onshape-bundle/1");
    assert_eq!(manifest["document"]["microversion"], MID);
    assert_eq!(manifest["fetched_at"], "2026-11-11T12:00:00Z");
    assert_eq!(manifest["calls"], 5);
    assert_eq!(manifest["external"].as_array().unwrap().len(), 1);
    assert_eq!(
        manifest["elements"][0]["files"]["definition"],
        format!("assemblies/{AS}/definition.json")
    );
    for name in names {
        let mut bytes = Vec::new();
        zip.by_name(&name).unwrap().read_to_end(&mut bytes).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for forbidden in [
            ACCESS,
            SECRET,
            "Private person",
            "private-account",
            "Authorization",
        ] {
            assert!(!text.contains(forbidden));
        }
    }
    let mut cached = Mock::default();
    let again = run(&mut cached, &dir, AS, 0).unwrap();
    assert_eq!(again.calls, 0);
    assert_eq!(again.cache_hits, 3);
    assert!(cached.sent.is_empty());
}
#[test]
fn workspace_head_is_resolved_again_instead_of_returning_a_stale_cache() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = source(AS);
    source.kind = "w".into();
    let mut mock = assembly_mock();
    mock.replies
        .push_back(response(200, json!({"microversion":MID})));
    fetch(
        &mut mock,
        &credentials(),
        &source,
        &dir.path().join("a.zip"),
        &dir.path().join("cache"),
        50,
    )
    .unwrap();
    let mut second = Mock::default();
    second
        .replies
        .push_back(response(200, json!({"microversion":MID})));
    second
        .replies
        .push_back(response(200, json!({"microversion":MID})));
    let report = fetch(
        &mut second,
        &credentials(),
        &source,
        &dir.path().join("b.zip"),
        &dir.path().join("cache"),
        2,
    )
    .unwrap();
    assert_eq!(report.calls, 2);
    assert!(second.sent[0].1.contains("currentmicroversion"));
}
fn partstudio_replies(mid: &str, marker: &str) -> VecDeque<Response> {
    let json_at =
        |text: &str| response(200, serde_json::from_str(&text.replace(MID, mid)).unwrap());
    let mut variables: Value =
        serde_json::from_str(include_str!("fixtures/onshape/variables.json")).unwrap();
    variables[0]["variables"][0]["expression"] = json!(marker);
    variables[0]["variables"][0]["value"] = json!(marker);
    VecDeque::from([
        response(
            200,
            json!({"name":"Synthetic Part Studio", "owner":{"name":"Private person"}}),
        ),
        response(
            200,
            json!([{"id":PS,"elementType":"PARTSTUDIO","name":"Synthetic tab"}]),
        ),
        json_at(include_str!("fixtures/onshape/partstudio.json")),
        json_at(include_str!("fixtures/onshape/bodydetails.json")),
        json_at(include_str!("fixtures/onshape/configuration.json")),
        response(200, variables),
        response(
            200,
            json!({"id":"export-job","requestState":"ACTIVE","documentId":DID,"requestElementId":PS}),
        ),
        response(
            200,
            json!({"id":"export-job","requestState":"DONE","documentId":DID,"resultDocumentId":DID,"requestElementId":PS,"resultExternalDataIds":["step-file"]}),
        ),
        Response {
            status: 200,
            headers: BTreeMap::new(),
            body: include_bytes!("fixtures/onshape/export.step").to_vec(),
        },
    ])
}
fn partstudio_mock(mid: &str, marker: &str) -> Mock {
    let mut mock = Mock::default();
    mock.replies
        .push_back(response(200, json!({"microversion":mid})));
    mock.replies.extend(partstudio_replies(mid, marker));
    mock
}
fn zip_value(zip: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> Value {
    let mut bytes = Vec::new();
    zip.by_name(name).unwrap().read_to_end(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
// Read into memory: an open file can't be replaced on Windows, and the cache tests fetch again over the same bundle.
fn open_bundle(dir: &tempfile::TempDir) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    zip::ZipArchive::new(std::io::Cursor::new(
        std::fs::read(dir.path().join("test.onshape-bundle")).unwrap(),
    ))
    .unwrap()
}
fn workspace_source() -> DocumentUrl {
    let mut src = source(PS);
    src.kind = "w".into();
    src
}
fn workspace_run(mock: &mut Mock, dir: &tempfile::TempDir, budget: u32) -> Result<FetchReport> {
    fetch(
        mock,
        &credentials(),
        &workspace_source(),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        budget,
    )
}
fn assert_sources(
    zip: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    mid: &str,
    kind: &str,
) -> Value {
    let manifest = zip_value(zip, "manifest.json");
    assert_eq!(manifest["consistent"], true);
    assert_eq!(manifest["document"]["microversion"], mid);
    let names: Vec<_> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    assert_eq!(
        manifest["file_sources"].as_object().unwrap().len(),
        names.len()
    );
    for name in names {
        assert_eq!(manifest["file_sources"][&name]["microversion"], mid);
        let expected = if kind == "m"
            && (name.ends_with("/variables.json") || name.ends_with("/export.step"))
        {
            "w"
        } else {
            kind
        };
        assert_eq!(manifest["file_sources"][&name]["path_kind"], expected);
    }
    manifest
}
#[test]
fn version_partstudio_uses_version_paths_and_writes_complete_bundle_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    let report = run(&mut mock, &dir, PS, 50).unwrap();
    assert_eq!(report.calls, 10);
    for (_, path, _) in &mock.sent {
        // The API's document metadata, job polling and result download paths do not have a w/v/m parameter.
        if !path.ends_with(&format!("/documents/{DID}"))
            && !path.contains("/translations/")
            && !path.contains("/externaldata/")
        {
            assert!(path.contains(&format!("/v/{VID}/")), "{path}");
            assert!(!path.contains("/m/"));
            assert!(!path.contains("/w/"));
        }
    }
    let topology: Vec<_> = mock
        .sent
        .iter()
        .filter(|(_, p, _)| p.ends_with("/bodydetails"))
        .collect();
    assert_eq!(topology.len(), 1);
    assert_eq!(
        topology[0].2,
        "rollbackBarIndex=2&includeGeometricData=true"
    );
    let post = mock.sent.iter().position(|(m, _, _)| m == "POST").unwrap();
    let params: Value = serde_json::from_slice(&mock.bodies[post]).unwrap();
    assert_eq!(params["storeInDocument"], false);
    assert_eq!(params["notifyUser"], false);
    assert_eq!(params["grouping"], true);
    let mut zip = open_bundle(&dir);
    let mut names: Vec<_> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            "manifest.json".to_owned(),
            format!("partstudios/{PS}/bodydetails/2.json"),
            format!("partstudios/{PS}/bodydetails/index.json"),
            format!("partstudios/{PS}/configuration.json"),
            format!("partstudios/{PS}/export.step"),
            format!("partstudios/{PS}/features.json"),
            format!("partstudios/{PS}/variables.json")
        ]
    );
    let manifest = assert_sources(&mut zip, MID, "v");
    assert_eq!(manifest["calls"], 10);
    assert_eq!(
        manifest["elements"][0]["files"]["step"],
        format!("partstudios/{PS}/export.step")
    );
    assert_eq!(
        zip_value(
            &mut zip,
            &format!("partstudios/{PS}/bodydetails/index.json")
        ),
        json!([2])
    );
    let mut step = Vec::new();
    zip.by_name(&format!("partstudios/{PS}/export.step"))
        .unwrap()
        .read_to_end(&mut step)
        .unwrap();
    assert_eq!(step, include_bytes!("fixtures/onshape/export.step"));
    for name in names {
        let mut bytes = Vec::new();
        zip.by_name(&name).unwrap().read_to_end(&mut bytes).unwrap();
        for key in [ACCESS, SECRET, "Private person", "owner"] {
            assert!(!String::from_utf8_lossy(&bytes).contains(key));
        }
    }
    let mut cached = Mock::default();
    let again = run(&mut cached, &dir, PS, 0).unwrap();
    assert_eq!(again.calls, 0);
    assert!(cached.sent.is_empty());
}
#[test]
fn workspace_partstudio_no_change_is_consistent_and_mixes_only_supported_paths() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies
        .push_back(response(200, json!({"microversion":MID})));
    let report = workspace_run(&mut mock, &dir, 50).unwrap();
    assert_eq!(report.calls, 11);
    assert_sources(&mut open_bundle(&dir), MID, "m");
    for (_, path, _) in &mock.sent {
        if path.contains("/variables/")
            || path.ends_with("/export/step")
            || path.ends_with("/currentmicroversion")
        {
            assert!(path.contains(&format!("/w/{VID}/")));
        } else if path.ends_with("/features")
            || path.ends_with("/bodydetails")
            || path.ends_with("/configuration")
            || path.ends_with("/elements")
        {
            assert!(path.contains(&format!("/m/{MID}/")));
        }
    }
    let mut cached = Mock::default();
    for _ in 0..2 {
        cached
            .replies
            .push_back(response(200, json!({"microversion":MID})));
    }
    let again = workspace_run(&mut cached, &dir, 2).unwrap();
    assert_eq!(again.calls, 2);
    assert_eq!(cached.sent.len(), 2);
}
#[test]
fn workspace_change_then_stable_refetches_once_and_does_not_reuse_unverified_workspace_data() {
    const MID2: &str = "888888888888888888888888";
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies
        .push_back(response(200, json!({"microversion":MID2})));
    mock.replies.extend(partstudio_replies(MID2, "2"));
    mock.replies
        .push_back(response(200, json!({"microversion":MID2})));
    let report = workspace_run(&mut mock, &dir, 50).unwrap();
    assert_eq!(report.calls, 21);
    let mut zip = open_bundle(&dir);
    assert_sources(&mut zip, MID2, "m");
    let vars = zip_value(&mut zip, &format!("partstudios/{PS}/variables.json"));
    assert_eq!(vars[0]["variables"][0]["value"], "2");
    assert_eq!(mock.sent.iter().filter(|(m, _, _)| m == "POST").count(), 2);
    assert_eq!(
        mock.sent
            .iter()
            .filter(|(_, p, _)| p.ends_with("currentmicroversion"))
            .count(),
        3
    );
    // A discarded attempt must not populate its microversion's /w/ cache.
    let mut probe = Mock::default();
    probe
        .replies
        .push_back(response(200, json!({"microversion":MID})));
    let mut workspace_responses = partstudio_replies(MID, "3");
    for _ in 0..5 {
        workspace_responses.pop_front();
    }
    probe.replies.extend(workspace_responses);
    probe
        .replies
        .push_back(response(200, json!({"microversion":MID})));
    let checked = workspace_run(&mut probe, &dir, 50).unwrap();
    assert_eq!(checked.calls, 6);
    assert_eq!(probe.sent.iter().filter(|(m, _, _)| m == "POST").count(), 1);
    assert_eq!(
        zip_value(
            &mut open_bundle(&dir),
            &format!("partstudios/{PS}/variables.json")
        )[0]["variables"][0]["value"],
        "3"
    );
}
#[test]
fn workspace_change_twice_stops_with_stated_message_and_keeps_existing_output() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("test.onshape-bundle"), b"previous").unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies.push_back(response(
        200,
        json!({"microversion":"888888888888888888888888"}),
    ));
    mock.replies
        .extend(partstudio_replies("888888888888888888888888", "2"));
    mock.replies.push_back(response(
        200,
        json!({"microversion":"777777777777777777777777"}),
    ));
    let error = workspace_run(&mut mock, &dir, 50).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Changed);
    assert_eq!(error.calls, 21);
    assert_eq!(
        error.message,
        "the document changed while fetching; try again, or use a version link"
    );
    assert_eq!(mock.sent.iter().filter(|(m, _, _)| m == "POST").count(), 2);
    assert_eq!(
        std::fs::read(dir.path().join("test.onshape-bundle")).unwrap(),
        b"previous"
    );
}
#[test]
fn budget_stops_before_the_next_call_and_counts_failed_attempts() {
    for budget in 0..5 {
        let dir = tempfile::tempdir().unwrap();
        let mut mock = assembly_mock();
        let error = run(&mut mock, &dir, AS, budget).err().unwrap();
        assert_eq!(error.kind, ErrorKind::Budget);
        assert_eq!(error.calls, budget);
        assert_eq!(mock.sent.len(), budget as usize);
        assert!(!dir.path().join("test.onshape-bundle").exists());
    }
}
#[test]
fn yearly_allowance_error_is_actionable_and_does_not_echo_body() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = Mock::default();
    mock.replies
        .push_back(response(402, json!({"message":SECRET})));
    let error = run(&mut mock, &dir, AS, 50).err().unwrap();
    assert_eq!(error.kind, ErrorKind::YearlyLimit);
    assert_eq!(error.calls, 1);
    let report = dutchman::json::onshape_fetch(&Err(error));
    assert!(!report.to_string().contains(SECRET));
    assert!(report["error"]["message"]
        .as_str()
        .unwrap()
        .contains("yearly"));
}
#[test]
fn retry_after_seconds_and_http_date_are_honoured() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = assembly_mock();
    mock.replies.push_front(Response {
        status: 429,
        headers: BTreeMap::from([("retry-after".into(), "Wed, 11 Nov 2026 12:00:09 GMT".into())]),
        body: vec![],
    });
    mock.replies.push_front(Response {
        status: 429,
        headers: BTreeMap::from([("Retry-After".into(), "7".into())]),
        body: vec![],
    });
    let report = run(&mut mock, &dir, AS, 50).unwrap();
    assert_eq!(
        mock.sleeps,
        [Duration::from_secs(7), Duration::from_secs(9)]
    );
    assert_eq!(report.calls, 7);
}
#[test]
fn throttle_retries_are_bounded_and_never_shorten_long_retry_after() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = Mock::default();
    for _ in 0..4 {
        mock.replies.push_back(response(429, json!({})));
    }
    let error = run(&mut mock, &dir, AS, 50).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Throttle);
    assert_eq!(error.calls, 4);
    assert_eq!(mock.sleeps.len(), 3);
    let mut mock = Mock::default();
    mock.replies.push_back(Response {
        status: 429,
        headers: BTreeMap::from([("retry-after".into(), "4500".into())]),
        body: vec![],
    });
    let error = run(&mut mock, &dir, AS, 50).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Throttle);
    assert!(mock.sleeps.is_empty());
}
#[test]
fn echoed_credentials_and_account_fields_are_never_bundled() {
    for body in [
        json!({"features":[],"name":SECRET}),
        json!({"features":[],"userName":"Private person"}),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut mock = assembly_mock();
        mock.replies.pop_back();
        mock.replies.push_back(response(200, body));
        let error = run(&mut mock, &dir, AS, 50).err().unwrap();
        assert_eq!(error.kind, ErrorKind::Privacy);
        assert!(!error.to_string().contains(SECRET));
        assert!(!dir.path().join("test.onshape-bundle").exists());
    }
}
#[cfg(unix)]
#[test]
fn config_requires_mode_600_and_never_echoes_bad_contents() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.json");
    std::fs::write(
        &path,
        json!({"access_key":ACCESS,"secret_key":SECRET}).to_string(),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let error = Credentials::from_file(&path).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Auth);
    assert!(error.to_string().contains("chmod 600"));
    assert!(!error.to_string().contains(SECRET));
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(Credentials::from_file(&path).is_ok());
}
#[test]
fn mcp_advertises_fetch_input_and_output_schemas_and_sanitizes_url_errors() {
    let list = dutchman::mcp::handle(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
    let tool = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "onshape_fetch")
        .unwrap();
    assert_eq!(tool["inputSchema"]["required"], json!(["document_url"]));
    assert_eq!(
        tool["outputSchema"]["properties"]["schema"]["const"],
        SCHEMA
    );
    let message = json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"onshape_fetch","arguments":{"document_url":format!("https://{ACCESS}:{SECRET}@evil.example")}}});
    let reply = dutchman::mcp::handle(&message.to_string()).unwrap();
    assert_eq!(reply["result"]["isError"], true);
    assert_eq!(reply["result"]["structuredContent"]["calls"], 0);
    assert_eq!(
        reply["result"]["structuredContent"]["error"]["code"],
        "onshape/url"
    );
    for key in [ACCESS, SECRET] {
        assert!(!reply.to_string().contains(key));
    }
}
#[test]
fn redirects_are_counted_and_refused_without_following_or_echoing_location() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = Mock::default();
    mock.replies.push_back(Response {
        status: 307,
        headers: BTreeMap::from([("location".into(), format!("https://evil.example/{SECRET}"))]),
        body: vec![],
    });
    let error = run(&mut mock, &dir, AS, 50).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Transport);
    assert_eq!(error.calls, 1);
    assert_eq!(mock.sent.len(), 1);
    assert!(!error.to_string().contains(SECRET));
}
#[test]
fn throttle_does_not_sleep_when_retry_cannot_fit_budget() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = Mock::default();
    mock.replies.push_back(response(429, json!({})));
    let error = run(&mut mock, &dir, AS, 1).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(error.calls, 1);
    assert!(mock.sleeps.is_empty());
}
#[test]
fn escaped_credential_echo_is_rejected_before_caching() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = assembly_mock();
    mock.replies.pop_back();
    let escaped = SECRET
        .chars()
        .map(|c| format!("\\u{:04x}", c as u32))
        .collect::<String>();
    mock.replies.push_back(Response {
        status: 200,
        headers: BTreeMap::new(),
        body: format!(r#"{{"features":[],"name":"{escaped}"}}"#).into_bytes(),
    });
    let error = run(&mut mock, &dir, AS, 50).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Privacy);
    assert_eq!(error.calls, 5);
    assert!(!dir.path().join("test.onshape-bundle").exists());
}
#[test]
fn mcp_argument_errors_preserve_schema_without_echoing_unknown_fields() {
    for args in [
        json!({"document_url":42}),
        json!({"document_url":"ignored", "max_calls":-1}),
        json!({"document_url":"ignored", "input":SECRET}),
        json!({SECRET:SECRET}),
    ] {
        let message = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"onshape_fetch","arguments":args}});
        let reply = dutchman::mcp::handle(&message.to_string()).unwrap();
        assert_eq!(reply["result"]["structuredContent"]["schema"], SCHEMA);
        assert_eq!(reply["result"]["structuredContent"]["calls"], 0);
        assert!(!reply.to_string().contains(SECRET));
    }
}

#[test]
fn a_failed_fetch_keeps_an_existing_bundle_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("test.onshape-bundle");
    std::fs::write(&output, b"previous bundle").unwrap();
    let mut mock = assembly_mock();
    assert!(run(&mut mock, &dir, AS, 4).is_err());
    assert_eq!(std::fs::read(output).unwrap(), b"previous bundle");
}
#[test]
fn cli_routes_onshape_fetch_and_reports_url_failures_without_network() {
    let binary = env!("CARGO_BIN_EXE_dutchman");
    let help = std::process::Command::new(binary)
        .args(["onshape", "fetch", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let text = String::from_utf8_lossy(&help.stdout);
    for flag in ["--out", "--version", "--max-calls", "--json"] {
        assert!(text.contains(flag));
    }
    let output = std::process::Command::new(binary)
        .args([
            "onshape",
            "fetch",
            &format!("https://{ACCESS}:{SECRET}@evil.example"),
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema"], SCHEMA);
    assert_eq!(report["calls"], 0);
    assert_eq!(report["error"]["code"], "onshape/url");
    for bytes in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(bytes);
        assert!(!text.contains(ACCESS));
        assert!(!text.contains(SECRET));
    }
}
#[test]
fn a_response_reporting_a_different_microversion_is_not_bundled() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = assembly_mock();
    mock.replies.pop_back();
    mock.replies.push_back(response(
        200,
        json!({"sourceMicroversion": "888888888888888888888888", "features":[]}),
    ));
    let error = run(&mut mock, &dir, AS, 50).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Response);
    assert_eq!(error.calls, 5);
    assert!(!dir.path().join("test.onshape-bundle").exists());
}
#[cfg(unix)]
#[test]
fn report_handles_non_utf8_output_paths_without_panicking() {
    use std::os::unix::ffi::OsStringExt;
    let output = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![b'x', 0xff]));
    let result = Ok(FetchReport {
        output,
        microversion: MID.into(),
        calls: 0,
        cache_hits: 0,
    });
    let report = dutchman::json::onshape_fetch(&result);
    assert_eq!(report["ok"], true);
    assert!(report["output"].is_string());
}
#[test]
fn archive_preserves_response_whitespace_verbatim() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = assembly_mock();
    let definition = include_bytes!("fixtures/onshape/assembly.json");
    let features = include_bytes!("fixtures/onshape/assembly-features.json");
    mock.replies.get_mut(3).unwrap().body = definition.to_vec();
    mock.replies.get_mut(4).unwrap().body = features.to_vec();
    let report = run(&mut mock, &dir, AS, 50).unwrap();
    let mut zip =
        zip::ZipArchive::new(std::io::Cursor::new(std::fs::read(report.output).unwrap())).unwrap();
    for (name, expected) in [
        ("definition.json", definition.as_slice()),
        ("features.json", features.as_slice()),
    ] {
        let mut bytes = Vec::new();
        zip.by_name(&format!("assemblies/{AS}/{name}"))
            .unwrap()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(bytes, expected);
    }
}

#[test]
fn same_document_two_part_revolute_assembly_includes_referenced_partstudio_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = assembly_mock();
    mock.replies.get_mut(2).unwrap().body = serde_json::to_vec(&json!([
        {"id":AS,"elementType":"ASSEMBLY","name":"Assembly"},
        {"id":PS,"elementType":"PARTSTUDIO","name":"Parts"}
    ]))
    .unwrap();
    mock.replies.get_mut(3).unwrap().body = include_str!("fixtures/onshape/assembly.json")
        .replace("999999999999999999999999", DID)
        .replace("666666666666666666666666", PS)
        .replace("777777777777777777777777", MID)
        .into_bytes();
    let mut part = partstudio_replies(MID, "1");
    part.pop_front();
    part.pop_front();
    mock.replies.extend(part);
    let report = run(&mut mock, &dir, AS, 50).unwrap();
    assert_eq!(report.calls, 12);
    let mut zip = open_bundle(&dir);
    let manifest = assert_sources(&mut zip, MID, "v");
    assert_eq!(manifest["elements"].as_array().unwrap().len(), 2);
    assert_eq!(manifest["external"], json!([]));
    assert!(zip
        .by_name(&format!("partstudios/{PS}/export.step"))
        .is_ok());
    assert_eq!(mock.sent.iter().filter(|(m, _, _)| m == "POST").count(), 1);
}
#[test]
fn workspace_budget_must_cover_the_final_consistency_check() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    let error = workspace_run(&mut mock, &dir, 10).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(error.calls, 10);
    assert!(!dir.path().join("test.onshape-bundle").exists());
    let mut retry = Mock::default();
    retry
        .replies
        .push_back(response(200, json!({"microversion":MID})));
    let mut workspace_responses = partstudio_replies(MID, "2");
    for _ in 0..5 {
        workspace_responses.pop_front();
    }
    retry.replies.extend(workspace_responses);
    retry
        .replies
        .push_back(response(200, json!({"microversion":MID})));
    assert_eq!(workspace_run(&mut retry, &dir, 50).unwrap().calls, 6);
}
#[test]
fn workspace_refetch_uses_the_same_total_call_budget() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies.push_back(response(
        200,
        json!({"microversion":"888888888888888888888888"}),
    ));
    let error = workspace_run(&mut mock, &dir, 11).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(error.calls, 11);
    assert_eq!(mock.sent.len(), 11);
    assert!(!dir.path().join("test.onshape-bundle").exists());
}
#[test]
fn immutable_export_job_resumes_after_budget_interruption_without_a_second_post() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies.truncate(8);
    let error = run(&mut mock, &dir, PS, 8).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert!(!dir.path().join("test.onshape-bundle").exists());
    let mut retry = Mock::default();
    let mut part = partstudio_replies(MID, "1");
    for _ in 0..7 {
        part.pop_front();
    }
    retry.replies.extend(part);
    assert_eq!(run(&mut retry, &dir, PS, 2).unwrap().calls, 2);
    assert_eq!(retry.sent[0].1, "/api/v17/translations/export-job");
    assert!(retry.sent.iter().all(|(method, _, _)| method == "GET"));
}
#[test]
fn failed_export_has_a_typed_error_and_can_be_retried() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies.truncate(7);
    mock.replies.push_back(response(
        200,
        json!({"id":"export-job","requestState":"FAILED","failureReason":"private server details"}),
    ));
    let error = run(&mut mock, &dir, PS, 50).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Export);
    assert_eq!(error.calls, 8);
    assert!(!error.to_string().contains("private server details"));
    let mut retry = Mock::default();
    let mut part = partstudio_replies(MID, "1");
    for _ in 0..6 {
        part.pop_front();
    }
    retry.replies.extend(part);
    assert_eq!(run(&mut retry, &dir, PS, 50).unwrap().calls, 3);
    assert_eq!(retry.sent[0].0, "POST");
}
#[test]
fn active_export_polling_is_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies.truncate(8);
    for _ in 0..20 {
        mock.replies.push_back(response(
            200,
            json!({"id":"export-job","requestState":"ACTIVE"}),
        ));
    }
    let error = run(&mut mock, &dir, PS, 50).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Export);
    assert_eq!(error.calls, 28);
    assert_eq!(mock.sleeps.len(), 20);
    assert!(mock.sleeps.iter().all(|d| *d <= Duration::from_secs(30)));
    assert!(!dir.path().join("test.onshape-bundle").exists());
}
#[test]
fn absent_partstudio_variables_and_configuration_are_omitted_and_cached() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies.get_mut(5).unwrap().status = 404;
    mock.replies.get_mut(6).unwrap().status = 404;
    run(&mut mock, &dir, PS, 50).unwrap();
    let mut zip = open_bundle(&dir);
    assert!(zip
        .by_name(&format!("partstudios/{PS}/configuration.json"))
        .is_err());
    assert!(zip
        .by_name(&format!("partstudios/{PS}/variables.json"))
        .is_err());
    assert_sources(&mut zip, MID, "v");
    let mut cached = Mock::default();
    assert_eq!(run(&mut cached, &dir, PS, 0).unwrap().calls, 0);
}

#[test]
fn immediate_step_export_can_omit_unused_job_and_result_document_fields() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies.truncate(7);
    mock.replies.push_back(response(200,json!({"requestState":"DONE","resultExternalDataIds":["step-file"],"resultDocumentId":"","workspaceId":null})));
    mock.replies.push_back(Response {
        status: 200,
        headers: BTreeMap::new(),
        body: include_bytes!("fixtures/onshape/export.step").to_vec(),
    });
    assert_eq!(run(&mut mock, &dir, PS, 50).unwrap().calls, 9);
    assert!(mock.sleeps.is_empty());
}
#[test]
fn step_export_cannot_download_from_a_different_document() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies.truncate(7);
    mock.replies.push_back(response(200,json!({"id":"export-job","requestState":"DONE","resultExternalDataIds":["step-file"],"resultDocumentId":"999999999999999999999999"})));
    let error = run(&mut mock, &dir, PS, 50).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Response);
    assert_eq!(error.calls, 8);
    assert!(mock
        .sent
        .iter()
        .all(|(_, p, _)| !p.contains("/externaldata/")));
}

mod store {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// A credential store that lives in memory and can be made to fail.
    #[derive(Clone, Default)]
    struct Fake {
        kept: Rc<RefCell<Option<String>>>,
        broken: bool,
    }
    impl Fake {
        fn holding(access: &str, secret: &str) -> Self {
            let keys = Credentials::new(access.into(), secret.into()).unwrap();
            Self {
                kept: Rc::new(RefCell::new(Some(keys.to_json()))),
                broken: false,
            }
        }
        fn has(&self) -> bool {
            self.kept.borrow().is_some()
        }
    }
    impl Backend for Fake {
        fn load(&self) -> Result<Option<Credentials>> {
            if self.broken {
                return Err(Credentials::new(String::new(), String::new()).unwrap_err());
            }
            match &*self.kept.borrow() {
                Some(text) => Credentials::from_json(text.as_bytes()).map(Some),
                None => Ok(None),
            }
        }
        fn save(&self, credentials: &Credentials) -> Result<()> {
            if self.broken {
                return Err(Credentials::new(String::new(), String::new()).unwrap_err());
            }
            *self.kept.borrow_mut() = Some(credentials.to_json());
            Ok(())
        }
        fn forget(&self) -> Result<()> {
            *self.kept.borrow_mut() = None;
            Ok(())
        }
    }

    fn env(pairs: &[(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> + 'static {
        let pairs: Vec<_> = pairs.to_vec();
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }
    fn who(credentials: &Credentials) -> String {
        credentials
            .authorization("get", "/p", "", "d", "n")
            .split(':')
            .next()
            .unwrap_or_default()
            .to_owned()
    }

    #[test]
    fn environment_beats_the_credential_store_beats_the_file() {
        let system = Fake::holding("storeACCESS1234", "storeSECRET1234");
        let file = Fake::holding("fileACCESS12345", "fileSECRET12345");
        let both = [
            ("ONSHAPE_ACCESS_KEY", "envACCESS123456"),
            ("ONSHAPE_SECRET_KEY", "envSECRET123456"),
        ];
        let store = Store::new(
            env(&both),
            Some(Box::new(system.clone())),
            Some(Box::new(file.clone())),
        );
        let (found, source) = store.find().unwrap().unwrap();
        assert_eq!(source, Source::Environment);
        assert_eq!(who(&found), "On envACCESS123456");

        let store = Store::new(
            env(&[]),
            Some(Box::new(system.clone())),
            Some(Box::new(file.clone())),
        );
        let (found, source) = store.find().unwrap().unwrap();
        assert_eq!(source, Source::SystemStore);
        assert_eq!(who(&found), "On storeACCESS1234");

        system.forget().unwrap();
        let store = Store::new(env(&[]), Some(Box::new(system)), Some(Box::new(file)));
        let (found, source) = store.find().unwrap().unwrap();
        assert_eq!(source, Source::PrivateFile);
        assert_eq!(who(&found), "On fileACCESS12345");
    }

    #[test]
    fn an_unreachable_credential_store_falls_back_to_the_file() {
        let system = Fake {
            broken: true,
            ..Fake::default()
        };
        let file = Fake::default();
        let store = Store::new(
            env(&[]),
            Some(Box::new(system)),
            Some(Box::new(file.clone())),
        );
        let source = store.save(&credentials()).unwrap();
        assert_eq!(source, Source::PrivateFile);
        assert!(file.has());
        assert_eq!(store.find().unwrap().unwrap().1, Source::PrivateFile);
    }

    #[test]
    fn saving_in_the_credential_store_removes_an_older_file_copy() {
        let system = Fake::default();
        let file = Fake::holding("fileACCESS12345", "fileSECRET12345");
        let store = Store::new(
            env(&[]),
            Some(Box::new(system.clone())),
            Some(Box::new(file.clone())),
        );
        assert_eq!(store.save(&credentials()).unwrap(), Source::SystemStore);
        assert!(system.has());
        assert!(!file.has());
    }

    #[test]
    fn logout_removes_the_keys_everywhere_and_leaves_nothing_connected() {
        let system = Fake::holding("storeACCESS1234", "storeSECRET1234");
        let file = Fake::holding("fileACCESS12345", "fileSECRET12345");
        let store = Store::new(
            env(&[]),
            Some(Box::new(system.clone())),
            Some(Box::new(file.clone())),
        );
        store.forget().unwrap();
        assert!(!system.has() && !file.has());
        assert!(store.find().unwrap().is_none());
        // Forgetting again is not an error.
        store.forget().unwrap();
    }

    #[test]
    fn one_environment_variable_alone_is_refused() {
        let store = Store::new(env(&[("ONSHAPE_ACCESS_KEY", ACCESS)]), None, None);
        let error = store.find().unwrap_err();
        assert_eq!(error.kind, ErrorKind::Auth);
        assert!(error.message.contains("together"));
    }

    #[test]
    fn unconfigured_says_how_to_connect() {
        let store = Store::new(
            env(&[]),
            Some(Box::new(Fake::default())),
            Some(Box::new(Fake::default())),
        );
        let error = store.load().unwrap_err();
        assert_eq!(error.kind, ErrorKind::Auth);
        let text = error.to_string();
        assert!(text.contains("dutchman onshape login"), "{text}");
        assert!(text.contains("Connect Onshape"), "{text}");
        assert!(matches!(
            status(&store, &mut Mock::default()).unwrap(),
            Connection::NotConnected
        ));
    }

    #[test]
    fn login_tests_the_keys_before_saving_and_never_prints_them() {
        let system = Fake::default();
        let store = Store::new(env(&[]), Some(Box::new(system.clone())), None);
        let mut rejected = Mock::default();
        rejected.replies.push_back(response(401, json!({})));
        let error = login(&store, &mut rejected, ACCESS.into(), SECRET.into()).unwrap_err();
        assert_eq!(error.kind, ErrorKind::Auth);
        assert!(!system.has());
        let shown = format!("{error} {error:?}");
        assert!(!shown.contains(SECRET) && !shown.contains(ACCESS));

        let mut accepted = Mock::default();
        accepted
            .replies
            .push_back(response(200, json!({"name": "Ada Lovelace"})));
        let (user, source) = login(&store, &mut accepted, ACCESS.into(), SECRET.into()).unwrap();
        assert_eq!(
            (user.as_str(), source),
            ("Ada Lovelace", Source::SystemStore)
        );
        assert!(system.has());
        assert_eq!(accepted.sent[0].1, "/api/v17/users/sessioninfo");

        let mut again = Mock::default();
        again
            .replies
            .push_back(response(200, json!({"name": "Ada Lovelace"})));
        match status(&store, &mut again).unwrap() {
            Connection::Connected { user, source } => {
                assert_eq!(
                    (user.as_str(), source),
                    ("Ada Lovelace", Source::SystemStore)
                );
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn no_key_appears_in_debug_or_display_output() {
        let keys = credentials();
        let store = Store::new(
            env(&[]),
            Some(Box::new(Fake::holding(ACCESS, SECRET))),
            None,
        );
        let shown = format!(
            "{keys:?} {store:?} {:?} {}",
            Source::SystemStore,
            Source::SystemStore.label()
        );
        assert!(
            !shown.contains(SECRET) && !shown.contains(ACCESS),
            "{shown}"
        );
        let bad = Credentials::new(ACCESS.into(), String::new()).unwrap_err();
        let shown = format!("{bad} {bad:?}");
        assert!(!shown.contains(ACCESS), "{shown}");
    }

    #[cfg(unix)]
    #[test]
    fn the_private_file_is_created_owner_only_and_removed_on_forget() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config/dutchman/onshape.json");
        let file = FileBackend { path: path.clone() };
        assert!(file.load().unwrap().is_none());
        file.save(&credentials()).unwrap();
        let mode = |p: &std::path::Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert!(file.load().unwrap().is_some());
        file.forget().unwrap();
        assert!(!path.exists());
        file.forget().unwrap();
    }
}

#[test]
fn pasted_keys_lose_surrounding_spaces_quotes_and_line_breaks() {
    let keys = Credentials::new(" AbC123\n".to_owned(), "\"s3cr3t/+=\"\r\n".to_owned());
    assert!(keys.is_ok());
    let inner = Credentials::new("Ab C123".to_owned(), "secret".to_owned());
    let message = inner.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(message.contains("access key"), "{message}");
    assert!(
        !message.contains("Ab C123"),
        "the key must never appear: {message}"
    );
}
