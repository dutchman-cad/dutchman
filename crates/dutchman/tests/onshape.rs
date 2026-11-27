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
    authorizations: Vec<String>,
}
impl HttpClient for Mock {
    fn send(&mut self, request: &Request) -> Result<Response> {
        assert!(request.headers["Authorization"].starts_with("On testACCESS123456:HmacSHA256:"));
        self.authorizations
            .push(request.headers["Authorization"].clone());
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
                json!({"name":"Synthetic mechanism", "owner":{"id":"private-account", "name":"Private person", "email":"private-person@example.test"}}),
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
fn fetched_bundles_contain_no_keys_signatures_headers_or_account_details() {
    for (element, mut mock) in [(AS, assembly_mock()), (PS, partstudio_mock(MID, "1"))] {
        let dir = tempfile::tempdir().unwrap();
        // Response headers must never become archive files or metadata either.
        for reply in &mut mock.replies {
            reply.headers.insert(
                "Authorization".into(),
                "private-response-authorization".into(),
            );
            reply.headers.insert(
                "X-Onshape-Signature".into(),
                "private-response-signature".into(),
            );
        }
        let report = run(&mut mock, &dir, element, 50).unwrap();
        assert!(!mock.authorizations.is_empty());
        let mut forbidden = vec![
            ACCESS,
            SECRET,
            "Authorization",
            "HmacSHA256",
            "private-response-authorization",
            "private-response-signature",
            "private-account",
            "Private person",
            "private-person@example.test",
        ];
        for authorization in &mock.authorizations {
            forbidden.push(authorization);
            forbidden.push(authorization.rsplit(':').next().unwrap());
        }
        let mut archive =
            zip::ZipArchive::new(std::io::Cursor::new(std::fs::read(report.output).unwrap()))
                .unwrap();
        for index in 0..archive.len() {
            let mut file = archive.by_index(index).unwrap();
            let name = file.name().unwrap().into_owned();
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes).unwrap();
            for &value in &forbidden {
                assert!(
                    !name.contains(value),
                    "Archive name contains a private value"
                );
                assert!(
                    !bytes
                        .windows(value.len())
                        .any(|window| window == value.as_bytes()),
                    "{name} contains a private value"
                );
            }
        }
    }
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
fn sketch_information(mid: &str) -> Value {
    json!({"sourceMicroversion":mid,"sketches":[{"featureId":"sketch",
        "sketchMatrix":[1.0,0.0,0.0,0.0, 0.0,1.0,0.0,0.0,
            0.0,0.0,1.0,0.0, 0.0,0.0,0.0,1.0]}]})
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
        response(200, sketch_information(mid)),
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
    assert_eq!(report.calls, 11);
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
            format!("partstudios/{PS}/sketches.json"),
            format!("partstudios/{PS}/variables.json")
        ]
    );
    let manifest = assert_sources(&mut zip, MID, "v");
    assert_eq!(manifest["calls"], 11);
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
fn several_new_parts_fetch_the_states_around_each_part_making_feature() {
    let dir = tempfile::tempdir().unwrap();
    let mut replies = partstudio_replies(MID, "1");
    let mut features: Value =
        serde_json::from_str(include_str!("fixtures/onshape/partstudio.json")).unwrap();
    let list = features["features"].as_array_mut().unwrap();
    let extrude = list
        .iter_mut()
        .find(|f| f["featureType"] == "extrude")
        .unwrap();
    extrude["parameters"]
        .as_array_mut()
        .unwrap()
        .push(json!({"btType":"BTMParameterEnum-145","parameterId":"operationType","value":"NEW"}));
    let mut second = extrude.clone();
    second["featureId"] = json!("second-extrude");
    list.push(second);
    replies[2] = response(200, features);
    for _ in 0..3 {
        replies.insert(
            3,
            response(
                200,
                serde_json::from_str(include_str!("fixtures/onshape/bodydetails.json")).unwrap(),
            ),
        );
    }
    let mut mock = Mock::default();
    mock.replies
        .push_back(response(200, json!({"microversion":MID})));
    mock.replies.extend(replies);
    run(&mut mock, &dir, PS, 50).unwrap();
    let bars: Vec<_> = mock
        .sent
        .iter()
        .filter(|(_, p, _)| p.ends_with("/bodydetails"))
        .map(|(_, _, q)| q.clone())
        .collect();
    assert_eq!(
        bars,
        [1, 2, 3]
            .map(|i| format!("rollbackBarIndex={i}&includeGeometricData=true"))
            .into_iter()
            .chain(["rollbackBarIndex=-1&includeGeometricData=true".to_owned()])
            .collect::<Vec<_>>()
    );
    assert_eq!(
        zip_value(
            &mut open_bundle(&dir),
            &format!("partstudios/{PS}/bodydetails/index.json")
        ),
        json!([1, 2, 3, 4])
    );
}

#[test]
fn workspace_partstudio_no_change_is_consistent_and_mixes_only_supported_paths() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies
        .push_back(response(200, json!({"microversion":MID})));
    let report = workspace_run(&mut mock, &dir, 50).unwrap();
    assert_eq!(report.calls, 12);
    assert_sources(&mut open_bundle(&dir), MID, "m");
    for (_, path, _) in &mock.sent {
        if path.contains("/variables/")
            || path.ends_with("/export/step")
            || path.ends_with("/currentmicroversion")
        {
            assert!(path.contains(&format!("/w/{VID}/")));
        } else if path.ends_with("/features")
            || path.ends_with("/bodydetails")
            || path.ends_with("/sketches")
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
    assert_eq!(report.calls, 23);
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
    // The sketch-information response uses a pinned /m/ path and is already cached.
    workspace_responses.pop_back();
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
    assert_eq!(error.calls, 23);
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
        json!({"features":[],"userId":"private-account"}),
        json!({"features":[],"email":"private-person@example.test"}),
        json!({"features":[],"Authorization":"private-authorization"}),
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
        title: None,
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
    assert_eq!(report.calls, 13);
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
fn workspace_refetch_uses_the_same_total_call_budget() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    mock.replies.push_back(response(
        200,
        json!({"microversion":"888888888888888888888888"}),
    ));
    let error = workspace_run(&mut mock, &dir, 12).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(error.calls, 12);
    assert_eq!(mock.sent.len(), 12);
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
    assert_eq!(run(&mut retry, &dir, PS, 3).unwrap().calls, 3);
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
    assert_eq!(run(&mut retry, &dir, PS, 50).unwrap().calls, 4);
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
    mock.replies
        .push_back(response(200, sketch_information(MID)));
    assert_eq!(run(&mut mock, &dir, PS, 50).unwrap().calls, 10);
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

#[test]
fn a_current_access_key_with_its_on_prefix_is_accepted_and_signs() {
    let keys = Credentials::new("on_AbC123xyz".to_owned(), "s3cr3t/+=".to_owned());
    assert!(keys.is_ok(), "{:?}", keys.err().map(|e| e.to_string()));
}

#[test]
fn default_plane_and_deterministic_region_convert_with_editable_history() {
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("plate.sc");
    let input = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/onshape-default-plane-regions.onshape-bundle");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("convert")
        .arg(input)
        .arg("-o")
        .arg(&output_path)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["history"]["mapped"], 2, "{report:#}");
    assert_eq!(report["history"]["total"], 2, "{report:#}");
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(std::fs::read(output_path).unwrap())).unwrap();
    let mut document = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut document)
        .unwrap();
    let document: Value = serde_json::from_str(&document).unwrap();
    let written = document["features"].as_array().unwrap();
    assert!(written.iter().any(|f| f["type"] == "sketch"));
    assert!(written.iter().any(|f| f["type"] == "extrude"));
}

#[test]
fn two_dimensions_in_one_sketch_keep_the_history_with_distinct_ordinals() {
    let dir = tempfile::tempdir().unwrap();
    let source = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/onshape-default-plane-regions.onshape-bundle");
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(std::fs::read(source).unwrap())).unwrap();
    let input = dir.path().join("two-dimensions.onshape-bundle");
    let mut writer = zip::ZipWriter::new(std::fs::File::create(&input).unwrap());
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let name = member.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        if name.ends_with("features.json") {
            let mut features: Value = serde_json::from_slice(&bytes).unwrap();
            let dimension = |id: &str, line: &str, expression: &str| {
                json!({"btType": "BTMSketchConstraint-2", "entityId": id, "constraintType": "LENGTH",
                    "parameters": [
                        {"btType": "BTMParameterString-149", "parameterId": "localFirst", "value": line},
                        {"btType": "BTMParameterQuantity-147", "parameterId": "length", "expression": expression}]})
            };
            features["features"][0]["constraints"] = json!([
                dimension("length", "outline.side0", "20 mm"),
                dimension("width", "outline.side1", "10 mm")
            ]);
            bytes = serde_json::to_vec(&features).unwrap();
        }
        writer
            .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut writer, &bytes).unwrap();
    }
    writer.finish().unwrap();
    let output_path = dir.path().join("plate.sc");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("convert")
        .arg(&input)
        .arg("-o")
        .arg(&output_path)
        .arg("--json")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        !report.to_string().contains("model of it fails"),
        "{report:#}"
    );
    assert_eq!(report["history"]["mapped"], 2, "{report:#}");
    assert_eq!(report["history"]["total"], 2, "{report:#}");
}

#[test]
fn sketch_only_fetch_skips_step_and_converts_without_a_body() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = prepare(PS, "PARTSTUDIO");
    mock.replies.extend([
        response(200,json!({"sourceMicroversion":MID,"features":[{
            "btType":"BTMSketch-151","featureType":"newSketch","featureId":"sketch","name":"Sketch",
            "parameters":[{"parameterId":"sketchPlane","queries":[{"btType":"BTMIndividualCreatedByQuery-137","featureId":"Top"}]}],
            "entities":[{"btType":"BTMSketchPoint-158","entityId":"point","x":0.003,"y":0.004}],"constraints":[]
        }]})),
        response(200,json!({"microversionId":MID,"bodies":[]})),
        response(404,json!({})),response(404,json!({})),
        response(200, sketch_information(MID)),
    ]);
    let report = run(&mut mock, &dir, PS, 50).unwrap();
    assert_eq!(report.calls, 8);
    assert!(mock
        .sent
        .iter()
        .all(|(method, path, _)| method == "GET" && !path.contains("/export/")));
    assert!(mock.replies.is_empty());
    let bundle = dir.path().join("test.onshape-bundle");
    let mut report = dutchman_core::Report::default();
    let model =
        dutchman_core::read(dutchman_core::Format::OnshapeBundle, &bundle, &mut report).unwrap();
    assert!(model.source_step.is_none());
    assert!(model.ir.geometry.bodies.is_empty());
    assert_eq!(model.ir.model.sketches.len(), 1);
    use dutchman_core::Writer;
    let mut output = Vec::new();
    dutchman_solidcraft::SolidCraftWriter::new()
        .write(&model, &mut output, &mut report)
        .unwrap();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(output)).unwrap();
    let doc = zip_value(&mut zip, "document.json");
    assert!(
        doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["type"] == "sketch"),
        "{doc}"
    );
    assert!(doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["type"] == "sketch"));
}

/// A Part Studio holding one sketch of every kind a sketch-only document uses: lines, a
/// point, a circle, an arc, a whole cubic spline (stored as a segment over its knot domain),
/// and coincident, horizontal, vertical, length, midpoint, origin and diameter constraints.
fn sketch_only_features() -> Value {
    let string = |id: &str, value: &str| json!({"btType":"BTMParameterString-149","parameterId":id,"value":value});
    let quantity = |id: &str, expression: &str| json!({"btType":"BTMParameterQuantity-147","parameterId":id,"expression":expression});
    let constraint = |id: &str, kind: &str, parameters: Value| json!({"btType":"BTMSketchConstraint-2","entityId":id,"constraintType":kind,"parameters":parameters});
    let line = |id: &str, p: [f64; 2], d: [f64; 2], length: f64| {
        json!({"btType":"BTMSketchCurveSegment-155","entityId":id,"startPointId":format!("{id}.start"),"endPointId":format!("{id}.end"),
            "centerId":"","startParam":0.0,"endParam":length,"isConstruction":false,
            "geometry":{"btType":"BTCurveGeometryLine-117","pntX":p[0],"pntY":p[1],"dirX":d[0],"dirY":d[1]}})
    };
    let mut constraints = Vec::new();
    for (i, (a, b)) in [
        ("bottom", "right"),
        ("right", "top"),
        ("top", "left"),
        ("left", "bottom"),
    ]
    .into_iter()
    .enumerate()
    {
        constraints.push(constraint(
            &format!("corner{i}"),
            "COINCIDENT",
            json!([
                string("localFirst", &format!("{a}.end")),
                string("localSecond", &format!("{b}.start"))
            ]),
        ));
    }
    for (kind, side) in [
        ("HORIZONTAL", "bottom"),
        ("HORIZONTAL", "top"),
        ("VERTICAL", "left"),
        ("VERTICAL", "right"),
    ] {
        constraints.push(constraint(
            &format!("{side}-direction"),
            kind,
            json!([string("localFirst", side)]),
        ));
    }
    for (side, expression) in [("bottom", "20 mm"), ("left", "10 mm")] {
        constraints.push(constraint(
            &format!("{side}-length"),
            "LENGTH",
            json!([string("localFirst", side), {"btType":"BTMParameterEnum-145","parameterId":"direction","value":"MINIMUM"},
                quantity("length", expression), {"btType":"BTMParameterEnum-145","parameterId":"alignment","value":"ALIGNED"}]),
        ));
    }
    constraints.push(constraint(
        "centre-middle",
        "MIDPOINT",
        json!([
            string("localMidpoint", "centre"),
            string("localEntity1", "bottom.start"),
            string("localEntity2", "top.start")
        ]),
    ));
    constraints.push(constraint(
        "centre-origin",
        "COINCIDENT",
        json!([string("localFirst", "centre"), {"btType":"BTMParameterQueryList-148","parameterId":"externalSecond",
            "queries":[{"btType":"BTMIndividualQuery-138","queryString":"","deterministicIds":["IB"]}]}]),
    ));
    constraints.push(constraint(
        "hole-size",
        "DIAMETER",
        json!([string("localFirst", "hole"), quantity("length", "6 mm")]),
    ));
    json!({"btType":"BTMSketch-151","featureType":"newSketch","featureId":"outline","name":"Outline","suppressed":false,
        "parameters":[{"btType":"BTMParameterQueryList-148","parameterId":"sketchPlane","queries":[{"btType":"BTMIndividualQuery-138",
            "deterministicIds":["plane-pick"],"queryString":"query=qCompressed(1.0,\"%B5$QueryM4Sa$entityTypeBa$EntityTypeS4$FACESb$historyTypeS8$CREATIONSb$operationIdB2$IdA1S3.7$TopplaneOpS9$queryTypeS5$DUMMY\",id);"}]}],
        "entities":[
            line("bottom", [-0.01, -0.005], [1.0, 0.0], 0.02),
            line("right", [0.01, -0.005], [0.0, 1.0], 0.01),
            line("top", [0.01, 0.005], [-1.0, 0.0], 0.02),
            line("left", [-0.01, 0.005], [0.0, -1.0], 0.01),
            {"btType":"BTMSketchPoint-158","entityId":"centre","x":0.0,"y":0.0},
            {"btType":"BTMSketchCurve-4","entityId":"hole","centerId":"hole.center",
                "geometry":{"btType":"BTCurveGeometryCircle-115","xCenter":0.02,"yCenter":0.0,"radius":0.003,"xDir":1.0,"yDir":0.0,"clockwise":false}},
            {"btType":"BTMSketchCurveSegment-155","entityId":"bend","startPointId":"bend.start","endPointId":"bend.end","centerId":"bend.center",
                "startParam":0.0,"endParam":std::f64::consts::FRAC_PI_2,
                "geometry":{"btType":"BTCurveGeometryCircle-115","xCenter":-0.02,"yCenter":0.0,"radius":0.004,"xDir":1.0,"yDir":0.0,"clockwise":false}},
            {"btType":"BTMSketchCurveSegment-155","entityId":"wave","startPointId":"wave.start","endPointId":"wave.end","centerId":"",
                "startParam":0.0,"endParam":1.0,
                "geometry":{"btType":"BTCurveGeometrySpline-118","isPeriodic":false,"isRational":false,"dimension":2,"degree":3,"controlPointCount":4,
                    "controlPoints":[-0.01,-0.02, -0.004,-0.012, 0.004,-0.018, 0.01,-0.01],
                    "knots":[0.0,0.0,0.0,0.0,1.0,1.0,1.0,1.0]}}
        ],
        "constraints":constraints})
}

#[test]
fn a_sketch_only_part_studio_fetches_without_step_and_keeps_every_sketch_kind() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = prepare(PS, "PARTSTUDIO");
    mock.replies.extend([
        response(
            200,
            json!({"sourceMicroversion":MID,"features":[sketch_only_features()]}),
        ),
        // The plane pick asks for the rollback state; a sketch builds no body there.
        response(200, json!({"microversionId":MID,"bodies":[]})),
        response(404, json!({})),
        response(200, json!([])),
        response(200, sketch_information(MID)),
    ]);
    let fetched = run(&mut mock, &dir, PS, 50).unwrap();
    assert_eq!(fetched.calls, 8);
    assert!(mock.replies.is_empty());
    assert!(mock
        .sent
        .iter()
        .all(|(method, path, _)| method == "GET" && !path.contains("/export/")));
    let bundle = dir.path().join("test.onshape-bundle");
    let mut archive = open_bundle(&dir);
    let manifest = zip_value(&mut archive, "manifest.json");
    assert!(manifest["elements"][0]["files"].get("step").is_none());
    assert!(archive
        .file_names()
        .all(|name| !name.unwrap().ends_with(".step")));

    let mut report = dutchman_core::Report::default();
    let model =
        dutchman_core::read(dutchman_core::Format::OnshapeBundle, &bundle, &mut report).unwrap();
    let onshape_losses: Vec<_> = report
        .losses()
        .iter()
        .filter(|loss| {
            loss.code
                .as_deref()
                .is_some_and(|code| code.starts_with("onshape/"))
        })
        .collect();
    assert!(onshape_losses.is_empty(), "{onshape_losses:?}");
    assert!(model.source_step.is_none());
    let wire = serde_json::to_value(&model.ir.model).unwrap();
    let mut kinds: Vec<_> = wire["sketch_entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["geometry"]["kind"].as_str().unwrap().to_owned())
        .collect();
    kinds.sort();
    // Four sides, the midpoint's construction carrier, and one of each other kind.
    assert_eq!(
        kinds,
        ["arc", "circle", "line", "line", "line", "line", "line", "nurbs", "point"]
    );
    let nurbs = wire["sketch_entities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["geometry"]["kind"] == "nurbs")
        .unwrap();
    assert_eq!(nurbs["geometry"]["curve"]["degree"], 3);
    assert_eq!(
        nurbs["geometry"]["curve"]["poles"]["points"][3],
        json!({"u": 10.0, "v": -10.0})
    );
    let constraints: Vec<_> = wire["sketch_constraints"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["definition"]["kind"].as_str().unwrap().to_owned())
        .collect();
    for kind in [
        "coincident_loci",
        "horizontal",
        "vertical",
        "distance",
        "midpoint",
        "point_coordinate_values",
        "diameter",
    ] {
        assert!(
            constraints.iter().any(|c| c == kind),
            "{kind}: {constraints:?}"
        );
    }

    use dutchman_core::{Check, Writer};
    use dutchman_solidcraft::{SolidCraftWriter, Verification};
    let mut output = Vec::new();
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .write(&model, &mut output, &mut report)
        .unwrap();
    let outcome = report.outcome().unwrap();
    assert!(
        matches!(outcome.check, Check::NotApplicable(_)),
        "{:?}",
        outcome.check
    );
    assert!(outcome
        .features
        .iter()
        .all(|f| f.status == dutchman_core::FeatureStatus::Mapped));
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(output)).unwrap();
    let doc = zip_value(&mut zip, "document.json");
    let written = doc["features"].as_array().unwrap();
    assert_eq!(written.len(), 1, "{doc}");
    assert_eq!(written[0]["type"], "sketch");
    assert!(zip
        .file_names()
        .all(|name| !name.unwrap().ends_with(".step")));

    // With a SolidCraft CLI, the sketches replay without an error and build no body.
    if let Some(cli) = std::env::var_os(dutchman_solidcraft::CLI_ENV) {
        let mut report = dutchman_core::Report::default();
        SolidCraftWriter::new()
            .verification(Verification::With(cli.into()))
            .write(&model, &mut Vec::new(), &mut report)
            .unwrap();
        assert_eq!(
            report.outcome().unwrap().check,
            Check::Verified,
            "{:?}",
            report.losses()
        );
    }
}

/// All identities and REST responses here are invented; no account or socket is used.
#[test]
fn derived_fetch_captures_a_pinned_other_document_studio_and_reuses_its_cache() {
    const OTHER: &str = "777777777777777777777777";
    const OTHER_VERSION: &str = "888888888888888888888888";
    const OTHER_ELEMENT: &str = "999999999999999999999999";
    const OTHER_MID: &str = "aaaaaaaaaaaaaaaaaaaaaaaa";
    let dir = tempfile::tempdir().unwrap();
    let mut mock = prepare(PS, "PARTSTUDIO");
    mock.replies.extend([
        response(200,json!({"sourceMicroversion":MID,"features":[{"featureId":"derived","featureType":"importDerived","parameters":[{"parameterId":"partStudio","value":{"documentId":OTHER,"versionId":OTHER_VERSION,"elementId":OTHER_ELEMENT}}]}]})),
        response(200,json!({"microversion":OTHER_MID})),
        response(200,json!({"name":"Reference studio"})),
        response(200,json!([{"id":OTHER_ELEMENT,"elementType":"PARTSTUDIO","name":"Reference"}])),
        response(200,json!({"sourceMicroversion":OTHER_MID,"features":[{"featureId":"solid","featureType":"extrude","parameters":[{"parameterId":"operationType","value":"NEW"}]},{"featureId":"finish","featureType":"fillet","parameters":[]}]})),
        response(200,json!({"microversionId":OTHER_MID,"bodies":[]})),
        response(200,json!({"microversionId":OTHER_MID,"bodies":[{"id":"tool","type":"SOLID","properties":{"name":"Tool"}}]})),
        response(200,json!({"microversionId":OTHER_MID,"bodies":[{"id":"tool","type":"SOLID","properties":{"name":"Tool"}}]})),
        response(404,json!({})),response(404,json!({})),
        response(200,json!({"requestState":"DONE","resultExternalDataIds":["source-step"]})),
        Response{status:200,headers:BTreeMap::new(),body:b"ISO-10303-21; source synthetic geometry".to_vec()},
        response(200, sketch_information(OTHER_MID)),
        response(200,json!({"microversionId":MID,"bodies":[]})),
        response(200,json!({"microversionId":MID,"bodies":[{"id":"inserted","type":"SOLID","properties":{"name":"Tool"}}]})),
        response(404,json!({})),response(404,json!({})),
        response(200,json!({"requestState":"DONE","resultExternalDataIds":["host-step"]})),
        Response{status:200,headers:BTreeMap::new(),body:b"ISO-10303-21; host synthetic geometry".to_vec()},
        response(200, sketch_information(MID)),
    ]);
    let fetched = run(&mut mock, &dir, PS, 50).unwrap();
    assert!(mock.replies.is_empty(), "{:?}", mock.sent);
    let sketch_requests: Vec<_> = mock
        .sent
        .iter()
        .filter(|(_, path, _)| path.ends_with("/sketches"))
        .collect();
    assert_eq!(
        sketch_requests,
        [
            &(
                "GET".into(),
                format!(
                    "/api/v17/partstudios/d/{OTHER}/v/{OTHER_VERSION}/e/{OTHER_ELEMENT}/sketches"
                ),
                String::new(),
            ),
            &(
                "GET".into(),
                format!("/api/v17/partstudios/d/{DID}/v/{VID}/e/{PS}/sketches"),
                String::new(),
            ),
        ]
    );
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(
        std::fs::read(&fetched.output).unwrap(),
    ))
    .unwrap();
    let prefix = format!("references/{OTHER}/{OTHER_VERSION}/{OTHER_ELEMENT}/");
    let manifest = zip_value(&mut archive, &format!("{prefix}manifest.json"));
    assert_eq!(manifest["document"]["id"], OTHER);
    assert_eq!(manifest["document"]["wvid"], OTHER_VERSION);
    assert_eq!(manifest["document"]["microversion"], OTHER_MID);
    assert_eq!(manifest["root_element"], OTHER_ELEMENT);
    assert_eq!(
        zip_value(
            &mut archive,
            &format!("{prefix}partstudios/{OTHER_ELEMENT}/bodydetails/index.json")
        ),
        json!([0, 1, 2])
    );
    assert!(archive
        .by_name(&format!("{prefix}partstudios/{OTHER_ELEMENT}/export.step"))
        .is_ok());
    let root = zip_value(&mut archive, "manifest.json");
    assert_eq!(root["root_element"], PS);
    assert_eq!(
        root["file_sources"][format!("{prefix}partstudios/{OTHER_ELEMENT}/features.json")]
            ["microversion"],
        OTHER_MID
    );
    assert!(mock
        .sent
        .iter()
        .filter(|(_, path, _)| path.contains(OTHER))
        .all(|(_, path, _)| !path.contains("/w/") && !path.contains(&format!("/v/{VID}/"))));
    let mut cached = Mock::default();
    let second = run(&mut cached, &dir, PS, 50).unwrap();
    assert_eq!(second.calls, 0);
    assert!(cached.sent.is_empty());
}

#[test]
fn derived_fetch_rejects_cycles_without_exporting_or_replacing_a_bundle() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("test.onshape-bundle");
    std::fs::write(&output, b"existing bundle").unwrap();
    let mut mock = prepare(PS, "PARTSTUDIO");
    mock.replies.push_back(response(200,json!({"sourceMicroversion":MID,"features":[{"featureId":"cycle","featureType":"derived","parameters":[{"parameterId":"partStudio","value":{"documentId":DID,"versionId":VID,"elementId":PS}}]}]})));
    assert!(matches!(
        run(&mut mock, &dir, PS, 50).err().unwrap().kind,
        ErrorKind::Unsupported
    ));
    assert_eq!(std::fs::read(output).unwrap(), b"existing bundle");
    assert!(mock.sent.iter().all(|(method, _, _)| method == "GET"));
}

#[test]
fn frontend_writer_publishes_derived_design_companions() {
    let dir = tempfile::tempdir().unwrap();
    let input = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/onshape-derived.onshape-bundle");
    let output = dir.path().join("linked.sc");
    let writer = dutchman::OutputWriter::SolidCraft(
        dutchman_solidcraft::SolidCraftWriter::new()
            .verification(dutchman_solidcraft::Verification::Off)
            .target_version(Some("0.44.1".parse().unwrap())),
    );
    dutchman_core::convert(&input, &output, &writer).unwrap();
    assert!(output.is_file());
    assert!(std::fs::read_dir(dir.path()).unwrap().any(|file| file
        .unwrap()
        .file_name()
        .to_string_lossy()
        .starts_with("derived-")));
}

#[test]
fn sketch_information_is_captured_verbatim_with_manifest_and_source_record() {
    for workspace in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut mock = partstudio_mock(MID, "1");
        let raw = format!("  {}\n", sketch_information(MID));
        mock.replies.back_mut().unwrap().body = raw.as_bytes().to_vec();
        if workspace {
            mock.replies
                .push_back(response(200, json!({"microversion":MID})));
            workspace_run(&mut mock, &dir, 50).unwrap();
        } else {
            run(&mut mock, &dir, PS, 50).unwrap();
        }
        let kind = if workspace { "m" } else { "v" };
        let content = if workspace { MID } else { VID };
        let request = mock
            .sent
            .iter()
            .find(|(_, path, _)| path.ends_with("/sketches"))
            .unwrap();
        assert_eq!(
            request,
            &(
                "GET".into(),
                format!("/api/v17/partstudios/d/{DID}/{kind}/{content}/e/{PS}/sketches"),
                String::new()
            )
        );
        let mut zip = open_bundle(&dir);
        let manifest = assert_sources(&mut zip, MID, kind);
        let path = format!("partstudios/{PS}/sketches.json");
        assert_eq!(manifest["schema"], "dutchman.onshape-bundle/1");
        assert_eq!(manifest["elements"][0]["files"]["sketches"], path);
        let mut bytes = Vec::new();
        zip.by_name(&path).unwrap().read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, raw.as_bytes());
    }
}

#[test]
fn sketch_information_budget_stop_resumes_from_cached_feature_and_export_reads() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("test.onshape-bundle"), b"previous").unwrap();
    let mut mock = partstudio_mock(MID, "1");
    let error = run(&mut mock, &dir, PS, 10).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(error.calls, 10);
    assert!(mock
        .sent
        .iter()
        .all(|(_, path, _)| !path.ends_with("/sketches")));
    assert_eq!(
        std::fs::read(dir.path().join("test.onshape-bundle")).unwrap(),
        b"previous"
    );
    let mut retry = Mock::default();
    retry
        .replies
        .push_back(response(200, sketch_information(MID)));
    assert_eq!(run(&mut retry, &dir, PS, 1).unwrap().calls, 1);
    assert_eq!(retry.sent.len(), 1);
    assert!(retry.sent[0].1.ends_with("/sketches"));
    let mut cached = Mock::default();
    assert_eq!(run(&mut cached, &dir, PS, 0).unwrap().calls, 0);
    assert!(cached.sent.is_empty());
}

#[test]
fn sketch_information_uses_existing_microversion_privacy_and_http_guards() {
    for (status, body, expected) in [
        (
            200,
            json!({"sourceMicroversion":"888888888888888888888888","sketches":[]}),
            ErrorKind::Response,
        ),
        (
            200,
            json!({"sketches":[],"name":SECRET}),
            ErrorKind::Privacy,
        ),
        (404, json!({}), ErrorKind::Response),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let mut mock = partstudio_mock(MID, "1");
        *mock.replies.back_mut().unwrap() = response(status, body);
        let error = run(&mut mock, &dir, PS, 50).err().unwrap();
        assert_eq!(error.kind, expected);
        assert_eq!(error.calls, 11);
        assert!(!error.to_string().contains(SECRET));
        assert!(!dir.path().join("test.onshape-bundle").exists());
    }
}

fn fetched_face_sketch(matrix: Option<Value>) -> (dutchman_core::Model, dutchman_core::Report) {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    // The transport fixture has a metadata-only STEP reply. Use the existing
    // synthetic plate's geometry so this regression reaches offline history mapping.
    let mut fixture = zip::ZipArchive::new(std::io::Cursor::new(
        include_bytes!(
            "../../dutchman-core/tests/fixtures/onshape-default-plane-regions.onshape-bundle"
        )
        .as_slice(),
    ))
    .unwrap();
    let mut step = Vec::new();
    fixture
        .by_name("partstudios/studio/export.step")
        .unwrap()
        .read_to_end(&mut step)
        .unwrap();
    mock.replies[9].body = step;
    mock.replies[3] = response(
        200,
        json!({"sourceMicroversion":MID,"features":[
            {"btType":"BTMSketch-151","featureType":"newSketch","featureId":"sketch","name":"Sketch",
             "parameters":[{"parameterId":"sketchPlane","queries":[{"btType":"BTMIndividualQuery-138","deterministicIds":["support"]}]}],
             "entities":[{"btType":"BTMSketchPoint-158","entityId":"point","x":0.003,"y":0.004}],"constraints":[]},
            {"featureType":"unsupportedSynthetic","featureId":"unsupported","parameters":[]}
        ]}),
    );
    mock.replies[4] = response(
        200,
        json!({"microversionId":MID,"bodies":[{"id":"body",
        "faces":[{"id":"support","orientation":true,"loops":[],"surface":{"type":"PLANE",
            "origin":{"x":0.01,"y":-0.02,"z":0.005},"normal":{"z":1.0}}}]}]}),
    );
    let mut placement = json!({"featureId":"sketch"});
    if let Some(matrix) = matrix {
        placement["sketchMatrix"] = matrix;
    }
    *mock.replies.back_mut().unwrap() = response(
        200,
        json!({"sourceMicroversion":MID,"sketches":[placement]}),
    );
    run(&mut mock, &dir, PS, 50).unwrap();
    let mut report = dutchman_core::Report::default();
    let model = dutchman_core::read(
        dutchman_core::Format::OnshapeBundle,
        &dir.path().join("test.onshape-bundle"),
        &mut report,
    )
    .unwrap();
    (model, report)
}

fn face_sketch_matrix() -> Value {
    json!([0.0, -1.0, 0.0, 0.04, 1.0, 0.0, 0.0, 0.07, 0.0, 0.0, 1.0, 0.005, 0.0, 0.0, 0.0, 1.0])
}

#[test]
fn a_fetched_face_sketch_uses_its_stored_placement_during_offline_reading() {
    let (model, report) = fetched_face_sketch(Some(face_sketch_matrix()));
    let wire = serde_json::to_value(&model.ir).unwrap();
    assert_eq!(
        wire["model"]["sketches"][0]["placement"]["frame"]["origin"],
        json!({"x":40.0,"y":70.0,"z":5.0})
    );
    assert_eq!(
        wire["model"]["sketches"][0]["placement"]["frame"]["u_axis"],
        json!({"x":0.0,"y":1.0,"z":0.0})
    );
    assert!(model.source_step.is_some());
    assert!(!report.losses().iter().any(|loss| loss.subject == "Sketch"));
}

#[test]
fn a_fetched_face_sketch_without_a_stored_matrix_keeps_the_final_shape() {
    let (model, report) = fetched_face_sketch(None);
    assert_face_sketch_refused(&model, &report);
}

#[test]
fn a_fetched_face_sketch_with_a_malformed_matrix_keeps_the_final_shape() {
    for case in [
        "short",
        "scaled",
        "sheared",
        "reflected",
        "projective",
        "nonfinite",
        "off-plane",
        "tilted",
    ] {
        let mut matrix = face_sketch_matrix();
        match case {
            "short" => {
                matrix.as_array_mut().unwrap().pop();
            }
            "scaled" => matrix[4] = json!(2.0),
            "sheared" => matrix[0] = json!(0.5),
            "reflected" => matrix[1] = json!(1.0),
            "projective" => matrix[12] = json!(0.1),
            "nonfinite" => matrix[3] = Value::Null,
            "off-plane" => matrix[11] = json!(0.05),
            "tilted" => {
                matrix[1] = json!(0.0);
                matrix[9] = json!(1.0);
                matrix[2] = json!(1.0);
                matrix[10] = json!(0.0);
            }
            _ => panic!("unknown matrix case: {case}"),
        }
        let (model, report) = fetched_face_sketch(Some(matrix));
        assert_face_sketch_refused(&model, &report);
    }
}

fn assert_face_sketch_refused(model: &dutchman_core::Model, report: &dutchman_core::Report) {
    let wire = serde_json::to_value(&model.ir).unwrap();
    assert!(model.ir.model.sketches.is_empty());
    assert_eq!(
        wire["model"]["features"][0]["definition"]["value"]["kind"],
        "native"
    );
    assert!(model.source_step.is_some());
    assert!(report.losses().iter().any(|loss| loss.subject == "Sketch"
        && loss.code.as_deref() == Some("onshape/history.not-supported")
        && loss.detail.contains("kept the final STEP geometry")));
}

/// Answers each endpoint from the fixtures, so a document of any size can be fetched.
struct Router {
    creators: usize,
    sent: u32,
}
impl HttpClient for Router {
    fn send(&mut self, request: &Request) -> Result<Response> {
        self.sent += 1;
        let path = request.path.as_str();
        let fixture = |text: &str| response(200, serde_json::from_str(text).unwrap());
        Ok(if path.ends_with("/currentmicroversion") {
            response(200, json!({"microversion": MID}))
        } else if path.ends_with(&format!("/documents/{DID}")) {
            response(200, json!({"name": "Synthetic document"}))
        } else if path.ends_with("/elements") {
            response(
                200,
                json!([{"id": PS, "elementType": "PARTSTUDIO", "name": "Synthetic tab"}]),
            )
        } else if path.ends_with("/features") {
            let mut features: Value =
                serde_json::from_str(include_str!("fixtures/onshape/partstudio.json")).unwrap();
            let list = features["features"].as_array_mut().unwrap();
            let mut extrude = list
                .iter()
                .find(|f| f["featureType"] == "extrude")
                .unwrap()
                .clone();
            extrude["parameters"].as_array_mut().unwrap().push(
                json!({"btType":"BTMParameterEnum-145","parameterId":"operationType","value":"NEW"}),
            );
            for i in 0..self.creators {
                extrude["featureId"] = json!(format!("part-{i}"));
                list.push(extrude.clone());
            }
            response(200, features)
        } else if path.ends_with("/bodydetails") {
            fixture(include_str!("fixtures/onshape/bodydetails.json"))
        } else if path.ends_with("/configuration") {
            fixture(include_str!("fixtures/onshape/configuration.json"))
        } else if path.ends_with("/variables") {
            fixture(include_str!("fixtures/onshape/variables.json"))
        } else if path.ends_with("/sketches") {
            response(200, sketch_information(MID))
        } else if request.method == "POST" && path.ends_with("/export/step") {
            response(
                200,
                json!({"id":"export-job","requestState":"DONE","documentId":DID,"resultDocumentId":DID,"requestElementId":PS,"resultExternalDataIds":["step-file"]}),
            )
        } else if path.contains("/externaldata/") {
            Response {
                status: 200,
                headers: BTreeMap::new(),
                body: include_bytes!("fixtures/onshape/export.step").to_vec(),
            }
        } else {
            panic!("unexpected request {path}");
        })
    }
    fn now(&self) -> SystemTime {
        httpdate::parse_http_date("Wed, 11 Nov 2026 12:00:00 GMT").unwrap()
    }
    fn nonce(&self) -> Result<String> {
        Ok("fixedNONCE".into())
    }
    fn sleep(&mut self, _delay: Duration) {}
}

fn fetch_by_default(creators: usize) -> (Result<FetchReport>, u32) {
    let dir = tempfile::tempdir().unwrap();
    let mut router = Router { creators, sent: 0 };
    let result = fetch_with_document_budget(
        &mut router,
        &FetchAuth::ApiKeys(credentials()),
        &source(PS),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
    );
    (result, router.sent)
}

#[test]
fn a_document_needing_more_than_fifty_calls_fetches_with_the_default_budget() {
    let (result, sent) = fetch_by_default(60);
    let report = result.unwrap();
    assert!(report.calls > 50, "{}", report.calls);
    assert_eq!(report.calls, sent);
}

#[test]
fn the_default_budget_still_stops_a_runaway_document() {
    // Thousands of part-making features would need thousands of calls.
    let (result, sent) = fetch_by_default(3000);
    let error = result.err().unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(error.calls, 2000);
    assert_eq!(sent, 2000);
}

#[test]
fn an_explicit_budget_is_never_raised_by_the_document() {
    let dir = tempfile::tempdir().unwrap();
    let mut router = Router {
        creators: 60,
        sent: 0,
    };
    let error = fetch(
        &mut router,
        &credentials(),
        &source(PS),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        50,
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(router.sent, 50);
}

#[test]
fn a_stop_at_the_call_limit_names_the_calls_spent_and_the_estimate() {
    let dir = tempfile::tempdir().unwrap();
    let mut router = Router {
        creators: 60,
        sent: 0,
    };
    let error = fetch(
        &mut router,
        &credentials(),
        &source(PS),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        50,
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    let needed = error.estimate.expect("the stop carries the estimate");
    assert!(needed > 50, "{needed}");
    let report = dutchman::json::onshape_fetch(&Err(error));
    assert_eq!(report["error"]["code"], "onshape/budget");
    assert_eq!(report["calls"], 50);
    assert_eq!(report["estimated_calls"], needed);
    let text = dutchman::onshape::budget_stop_text(50, Some(needed));
    assert!(text.contains("after 50 API calls"), "{text}");
    assert!(text.contains(&format!("--max-calls {needed} ")), "{text}");
}

#[test]
fn the_command_line_budget_stop_text_gives_a_limit_that_makes_progress() {
    use dutchman::onshape::budget_stop_text;
    assert_eq!(
        budget_stop_text(50, Some(51)),
        "Onshape call budget reached after 50 API calls; this document needs about 51. Rerun with --max-calls 51 (MCP: max_calls) to finish; replies already fetched cost no calls."
    );
    // An estimate at or below the calls spent still names a limit above them.
    assert!(budget_stop_text(50, Some(40)).contains("--max-calls 51 "));
    assert!(budget_stop_text(1500, Some(1800)).contains("needs about 1,800"));
    let unknown = budget_stop_text(50, None);
    assert!(unknown.contains("after 50 API calls") && unknown.contains("larger --max-calls"));
}

#[test]
fn an_allowed_fetch_is_estimated_first_and_its_limit_fits_the_document() {
    // `yes` skips the question, not the estimate; the fetch is the same one the gated path
    // makes, so a document over the starting limit finishes and counts the listing once.
    let dir = tempfile::tempdir().unwrap();
    let mut router = Router {
        creators: 60,
        sent: 0,
    };
    let report = fetch_gated(
        &mut router,
        &FetchAuth::ApiKeys(credentials()),
        &source(PS),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        None,
        true,
    )
    .unwrap();
    assert!(report.calls > 50, "{}", report.calls);
    assert_eq!(report.calls, router.sent);
}

fn listing_mock(tabs: Value) -> Mock {
    Mock {
        replies: VecDeque::from([
            response(200, json!({"microversion": MID})),
            response(200, json!({"name": "Synthetic mechanism"})),
            response(200, tabs),
        ]),
        ..Mock::default()
    }
}

fn big_assembly_tabs() -> Value {
    json!([
        {"id": AS, "elementType": "ASSEMBLY", "name": "Assembly"},
        {"id": PS, "elementType": "PARTSTUDIO", "name": "First"},
        {"id": "666666666666666666666666", "elementType": "PARTSTUDIO", "name": "Second"},
        {"id": "777777777777777777777777", "elementType": "BLOB", "name": "Drawing"},
    ])
}

#[test]
fn the_estimate_counts_the_listing_calls_and_the_elements_and_skips_cached_replies() {
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("cache");
    let auth = FetchAuth::ApiKeys(credentials());
    let mut mock = listing_mock(big_assembly_tabs());
    let estimate = estimate_calls(&mut mock, &auth, &source(AS), &cache).unwrap();
    // Three listing calls, then 30 for each of the assembly and its two studios.
    assert_eq!(mock.sent.len(), 3);
    assert_eq!(estimate.listing_calls, 3);
    assert_eq!(estimate.elements, 3);
    assert_eq!(estimate.calls, 93);
    assert!(estimate.needs_approval());
    assert!(estimate.question().contains("about 93"));
    assert!(estimate.question().contains("2,500"));
    // The listing is cached, so a second estimate makes no request and counts none.
    let mut idle = Mock::default();
    let again = estimate_calls(&mut idle, &auth, &source(AS), &cache).unwrap();
    assert!(idle.sent.is_empty());
    assert_eq!(again.listing_calls, 0);
    assert_eq!(again.calls, 90);
}

#[test]
fn a_part_studio_is_estimated_alone() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = listing_mock(big_assembly_tabs());
    let estimate = estimate_calls(
        &mut mock,
        &FetchAuth::ApiKeys(credentials()),
        &source(PS),
        &dir.path().join("cache"),
    )
    .unwrap();
    assert_eq!((estimate.elements, estimate.calls), (1, 33));
    assert!(!estimate.needs_approval());
}

#[test]
fn a_large_fetch_without_a_go_stops_with_the_estimate_and_spends_only_the_listing() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = listing_mock(big_assembly_tabs());
    let error = fetch_gated(
        &mut mock,
        &FetchAuth::ApiKeys(credentials()),
        &source(AS),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        None,
        false,
    )
    .err()
    .unwrap();
    // The mock panics on a fourth request, so the listing was all it spent.
    assert_eq!(mock.sent.len(), 3);
    assert_eq!(error.kind, ErrorKind::Approval);
    assert_eq!((error.calls, error.estimate), (3, Some(93)));
    assert!(!dir.path().join("test.onshape-bundle").exists());
    let report = dutchman::json::onshape_fetch(&Err(error));
    assert_eq!(report["error"]["code"], "onshape/approval");
    assert_eq!(report["estimated_calls"], 93);
    assert_eq!(report["calls"], 3);
    let message = report["error"]["message"].as_str().unwrap();
    assert!(message.contains("--yes") && message.contains("max_calls"));
}

#[test]
fn the_command_line_asks_on_a_terminal_and_stops_off_one() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = listing_mock(big_assembly_tabs());
    let big = estimate_calls(
        &mut mock,
        &FetchAuth::ApiKeys(credentials()),
        &source(AS),
        &dir.path().join("cache"),
    )
    .unwrap();
    let off_terminal = approval(&big, false, false, |_| panic!("must not ask")).unwrap_err();
    assert_eq!(off_terminal.kind, ErrorKind::Approval);
    assert!(off_terminal.message.contains("--yes"));
    assert!(off_terminal.message.contains("--max-calls"));
    assert!(approval(&big, true, false, |_| panic!("must not ask")).is_ok());
    assert!(approval(&big, false, true, |_| true).is_ok());
    let declined = approval(&big, false, true, |_| false).unwrap_err();
    assert_eq!(declined.message, APPROVAL_DECLINED);
    let small_dir = tempfile::tempdir().unwrap();
    let mut mock = listing_mock(big_assembly_tabs());
    let small = estimate_calls(
        &mut mock,
        &FetchAuth::ApiKeys(credentials()),
        &source(PS),
        &small_dir.path().join("cache"),
    )
    .unwrap();
    assert!(approval(&small, false, false, |_| panic!("must not ask")).is_ok());
}

#[test]
fn a_gated_fetch_under_the_limit_counts_the_listing_once() {
    let dir = tempfile::tempdir().unwrap();
    let mut router = Router {
        creators: 60,
        sent: 0,
    };
    let report = fetch_gated(
        &mut router,
        &FetchAuth::ApiKeys(credentials()),
        &source(PS),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        None,
        false,
    )
    .unwrap();
    assert!(report.calls > 50);
    assert_eq!(report.calls, router.sent);
}

#[test]
fn an_explicit_max_calls_is_the_ceiling_and_the_consent_with_no_estimate() {
    let dir = tempfile::tempdir().unwrap();
    let mut router = Router {
        creators: 60,
        sent: 0,
    };
    let error = fetch_gated(
        &mut router,
        &FetchAuth::ApiKeys(credentials()),
        &source(PS),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        Some(40),
        false,
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    assert_eq!(router.sent, 40);
}

#[test]
fn cached_replies_cost_no_calls_and_do_not_count_against_the_limit() {
    let dir = tempfile::tempdir().unwrap();
    let mut router = Router {
        creators: 3,
        sent: 0,
    };
    let fetched = |router: &mut Router, max: u32| {
        fetch_gated(
            router,
            &FetchAuth::ApiKeys(credentials()),
            &source(PS),
            &dir.path().join("test.onshape-bundle"),
            &dir.path().join("cache"),
            Some(max),
            false,
        )
    };
    let first = fetched(&mut router, 500).unwrap();
    assert!(first.calls > 0);
    let sent = router.sent;
    // Everything is cached now, so even a limit of zero fetches it again.
    let second = fetched(&mut router, 0).unwrap();
    assert_eq!((second.calls, router.sent), (0, sent));
    assert!(second.cache_hits > 0);
}

#[test]
fn call_counts_group_thousands_from_one_thousand() {
    use dutchman::onshape::calls_text;
    assert_eq!(calls_text(0), "0");
    assert_eq!(calls_text(999), "999");
    assert_eq!(calls_text(1000), "1,000");
    assert_eq!(calls_text(2500), "2,500");
    assert_eq!(calls_text(1234567), "1,234,567");
}

#[derive(Default)]
struct Counted(Vec<(u32, Option<u32>)>);
impl FetchProgress for Counted {
    fn calls(&mut self, sent: u32, needed: Option<u32>) {
        self.0.push((sent, needed));
    }
}
#[test]
fn progress_counts_each_call_and_estimates_after_the_feature_list() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    let mut counted = Counted::default();
    let report = fetch_watched(
        &mut mock,
        &FetchAuth::ApiKeys(credentials()),
        &source(PS),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        None,
        Some(&mut counted),
    )
    .unwrap();
    assert_eq!(report.calls, 11);
    let sent: Vec<u32> = counted.0.iter().map(|(sent, _)| *sent).collect();
    assert_eq!(sent, (1..=11).collect::<Vec<_>>());
    // Microversion, document, tabs and features come before the estimate.
    assert!(counted.0[..4].iter().all(|(_, needed)| needed.is_none()));
    assert!(counted.0[4..].iter().all(|(_, needed)| *needed == Some(11)));
}

#[derive(Default)]
struct Titles(Vec<String>);
impl FetchProgress for Titles {
    fn calls(&mut self, _: u32, _: Option<u32>) {}
    fn named(&mut self, title: &str) {
        self.0.push(title.to_owned());
    }
}
#[test]
fn the_title_comes_from_the_document_and_tab_names_without_extra_calls() {
    let dir = tempfile::tempdir().unwrap();
    let mut mock = partstudio_mock(MID, "1");
    let mut titles = Titles::default();
    let report = fetch_watched(
        &mut mock,
        &FetchAuth::ApiKeys(credentials()),
        &source(PS),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        None,
        Some(&mut titles),
    )
    .unwrap();
    // The same eleven calls as a fetch nobody names.
    assert_eq!(report.calls, 11);
    assert_eq!(titles.0, ["Synthetic Part Studio - Synthetic tab"]);
    assert_eq!(
        report.title.as_deref(),
        Some("Synthetic Part Studio - Synthetic tab")
    );
}
#[test]
fn a_resumed_fetch_names_the_design_from_the_cache() {
    let whole = workspace_replies();
    let dir = tempfile::tempdir().unwrap();
    let mut first = Mock {
        replies: whole.clone().into(),
        ..Mock::default()
    };
    let mut stop = StopAfter { after: 3, sent: 0 };
    fetch_watched(
        &mut first,
        &FetchAuth::ApiKeys(credentials()),
        &workspace_source(),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        None,
        Some(&mut stop),
    )
    .err()
    .unwrap();
    let mut again = Mock {
        replies: whole[3..].to_vec().into(),
        ..Mock::default()
    };
    let mut titles = Titles::default();
    let report = fetch_watched(
        &mut again,
        &FetchAuth::ApiKeys(credentials()),
        &workspace_source(),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        None,
        Some(&mut titles),
    )
    .unwrap();
    assert_eq!(report.calls, u32::try_from(whole.len()).unwrap() - 3);
    assert_eq!(titles.0.len(), 1);
    assert_eq!(report.title.as_deref(), Some(titles.0[0].as_str()));
}
#[test]
fn bundle_titles_join_names_and_drop_unsafe_characters() {
    use dutchman::onshape::bundle_title;
    assert_eq!(
        bundle_title("Bracket", Some("Part Studio 1")).as_deref(),
        Some("Bracket - Part Studio 1")
    );
    for tab in [None, Some(""), Some("  "), Some("Bracket")] {
        assert_eq!(bundle_title("Bracket", tab).as_deref(), Some("Bracket"));
    }
    assert_eq!(
        bundle_title("A/B: \"C\"?", Some("x\ty|z*<>")).as_deref(),
        Some("A B C - x y z")
    );
    assert_eq!(
        bundle_title("  many   spaces. ", Some("tab...")).as_deref(),
        Some("many spaces. - tab")
    );
    assert_eq!(
        bundle_title("", Some("Only tab")).as_deref(),
        Some("Only tab")
    );
    assert_eq!(bundle_title("///", Some("..")), None);
    let long = bundle_title(&"é".repeat(300), Some("tab")).unwrap();
    assert_eq!(long.chars().count(), 100);
}
#[test]
fn retitling_replaces_a_bundle_of_the_same_design_only() {
    use dutchman::onshape::retitle;
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let bundle = |name: &str, document: &str, element: &str| {
        let path = dir.path().join(name);
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("manifest.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(
            serde_json::to_vec(&json!({"document": {"id": document}, "root_element": element}))
                .unwrap()
                .as_slice(),
        )
        .unwrap();
        zip.finish().unwrap();
        path
    };
    let title = "Bracket - Part Studio 1";
    let first = retitle(&bundle("e1.onshape-bundle", "d1", "e1"), title).unwrap();
    assert_eq!(first, dir.path().join(format!("{title}.onshape-bundle")));
    // The same design fetched again replaces its bundle.
    let again = retitle(&bundle("e1.onshape-bundle", "d1", "e1"), title).unwrap();
    assert_eq!(again, first);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    // Another design with the same title keeps both.
    let other = retitle(&bundle("e2.onshape-bundle", "d2", "e2"), title).unwrap();
    assert_eq!(
        other,
        dir.path().join(format!("{title} (2).onshape-bundle"))
    );
    assert!(first.exists());
}
#[test]
fn retitling_never_overwrites_a_bundle_that_is_there() {
    use dutchman::onshape::retitle;
    let dir = tempfile::tempdir().unwrap();
    let write = |name: &str, text: &str| {
        let path = dir.path().join(name);
        std::fs::write(&path, text).unwrap();
        path
    };
    let first = write("id1.onshape-bundle", "one");
    let moved = retitle(&first, "Bracket - Part Studio 1").unwrap();
    assert_eq!(
        moved,
        dir.path().join("Bracket - Part Studio 1.onshape-bundle")
    );
    assert!(!first.exists());
    let second = write("id1.onshape-bundle", "two");
    let moved = retitle(&second, "Bracket - Part Studio 1").unwrap();
    assert_eq!(
        moved,
        dir.path()
            .join("Bracket - Part Studio 1 (2).onshape-bundle")
    );
    let third = write("id1.onshape-bundle", "three");
    let moved = retitle(&third, "Bracket - Part Studio 1").unwrap();
    assert_eq!(
        moved,
        dir.path()
            .join("Bracket - Part Studio 1 (3).onshape-bundle")
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("Bracket - Part Studio 1.onshape-bundle")).unwrap(),
        "one"
    );
}

/// Asks to stop once `after` calls have been sent.
struct StopAfter {
    after: u32,
    sent: u32,
}
impl FetchProgress for StopAfter {
    fn calls(&mut self, sent: u32, _: Option<u32>) {
        self.sent = sent;
    }
    fn stop_requested(&mut self) -> bool {
        self.sent >= self.after
    }
}
#[test]
fn a_requested_stop_ends_the_fetch_and_the_next_fetch_sends_only_the_missing_calls() {
    let whole = workspace_replies();
    let total = u32::try_from(whole.len()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut first = Mock {
        replies: whole.clone().into(),
        ..Mock::default()
    };
    let mut stop = StopAfter { after: 3, sent: 0 };
    let error = fetch_watched(
        &mut first,
        &FetchAuth::ApiKeys(credentials()),
        &workspace_source(),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        None,
        Some(&mut stop),
    )
    .err()
    .unwrap();
    assert_eq!(error.kind, ErrorKind::Stopped);
    assert_eq!(error.kind.code(), "onshape/stopped");
    assert_eq!(error.calls, 3);
    assert_eq!(first.sent.len(), 3);
    assert!(!dir.path().join("test.onshape-bundle").exists());
    let mut again = Mock {
        replies: whole[3..].to_vec().into(),
        ..Mock::default()
    };
    let report = workspace_run(&mut again, &dir, 50).unwrap();
    assert_eq!(report.calls, total - 3);
    assert!(dir.path().join("test.onshape-bundle").exists());
}

fn bundle_files(dir: &tempfile::TempDir) -> BTreeMap<String, Vec<u8>> {
    let mut zip = open_bundle(dir);
    let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    names
        .into_iter()
        .filter(|name| name != "manifest.json")
        .map(|name| {
            let mut bytes = Vec::new();
            zip.by_name(&name).unwrap().read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect()
}

/// Every reply of an uninterrupted workspace fetch, in order: the microversion, the Part
/// Studio's ten replies, and the closing microversion check.
fn workspace_replies() -> Vec<Response> {
    let mut mock = partstudio_mock(MID, "1");
    mock.replies
        .push_back(response(200, json!({"microversion":MID})));
    mock.replies.into()
}

#[test]
fn a_stopped_workspace_fetch_continues_with_only_the_calls_it_is_missing() {
    let whole = workspace_replies();
    let total = u32::try_from(whole.len()).unwrap();
    let reference = tempfile::tempdir().unwrap();
    let mut mock = Mock {
        replies: whole.clone().into(),
        ..Mock::default()
    };
    assert_eq!(
        workspace_run(&mut mock, &reference, 50).unwrap().calls,
        total
    );
    for stop in 0..total {
        let dir = tempfile::tempdir().unwrap();
        let mut first = Mock {
            replies: whole.clone().into(),
            ..Mock::default()
        };
        let error = workspace_run(&mut first, &dir, stop).err().unwrap();
        assert_eq!(error.kind, ErrorKind::Budget, "stop at {stop}");
        assert!(!dir.path().join("test.onshape-bundle").exists());
        // A new client stands in for a relaunch: only the cache folder carries over.
        let mut again = Mock {
            replies: whole[stop as usize..].to_vec().into(),
            ..Mock::default()
        };
        let report = workspace_run(&mut again, &dir, 50)
            .unwrap_or_else(|error| panic!("stop at {stop}: {error:?}, sent {:?}", again.sent));
        assert_eq!(report.calls, total - stop, "stop at {stop}");
        assert!(again.replies.is_empty(), "stop at {stop}");
        assert_eq!(
            bundle_files(&dir),
            bundle_files(&reference),
            "stop at {stop}"
        );
    }
}

#[test]
fn stopping_one_call_short_continues_with_exactly_that_call() {
    let whole = workspace_replies();
    let total = u32::try_from(whole.len()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut first = Mock {
        replies: whole.clone().into(),
        ..Mock::default()
    };
    let error = workspace_run(&mut first, &dir, total - 1).err().unwrap();
    assert_eq!((error.kind, error.calls), (ErrorKind::Budget, total - 1));
    let mut again = Mock::default();
    again
        .replies
        .push_back(response(200, json!({"microversion":MID})));
    let report = workspace_run(&mut again, &dir, 50).unwrap();
    assert_eq!(report.calls, 1);
    assert_eq!(again.sent.len(), 1);
    assert!(again.sent[0].1.ends_with("/currentmicroversion"));
    assert_sources(&mut open_bundle(&dir), MID, "m");
}

#[test]
fn a_stopped_version_fetch_continues_with_exactly_the_missing_call() {
    let dir = tempfile::tempdir().unwrap();
    let mut whole = Mock::default();
    whole
        .replies
        .push_back(response(200, json!({"microversion":MID})));
    whole.replies.extend(partstudio_replies(MID, "1"));
    let replies: Vec<Response> = whole.replies.into();
    let total = u32::try_from(replies.len()).unwrap();
    let mut first = Mock {
        replies: replies.clone().into(),
        ..Mock::default()
    };
    let error = run(&mut first, &dir, PS, total - 1).err().unwrap();
    assert_eq!(error.kind, ErrorKind::Budget);
    let mut again = Mock {
        replies: replies[replies.len() - 1..].to_vec().into(),
        ..Mock::default()
    };
    assert_eq!(run(&mut again, &dir, PS, 50).unwrap().calls, 1);
    assert_eq!(again.sent.len(), 1);
}

#[test]
fn continuing_after_the_workspace_changed_starts_over_at_its_new_state() {
    const MID2: &str = "888888888888888888888888";
    let whole = workspace_replies();
    let dir = tempfile::tempdir().unwrap();
    let mut first = Mock {
        replies: whole.clone().into(),
        ..Mock::default()
    };
    // Stop after the variables reply, a workspace reply saved for MID.
    workspace_run(&mut first, &dir, 6).err().unwrap();
    let mut again = Mock::default();
    // The rest of the MID fetch, then a check that finds the workspace moved on.
    again
        .replies
        .extend(whole[6..whole.len() - 1].iter().cloned());
    again
        .replies
        .push_back(response(200, json!({"microversion":MID2})));
    again.replies.extend(partstudio_replies(MID2, "2"));
    again
        .replies
        .push_back(response(200, json!({"microversion":MID2})));
    workspace_run(&mut again, &dir, 50).unwrap();
    assert!(again.replies.is_empty());
    let mut zip = open_bundle(&dir);
    assert_sources(&mut zip, MID2, "m");
    let vars = zip_value(&mut zip, &format!("partstudios/{PS}/variables.json"));
    assert_eq!(vars[0]["variables"][0]["value"], "2");
}

#[test]
fn a_continued_fetch_estimates_only_the_calls_it_has_left() {
    let dir = tempfile::tempdir().unwrap();
    let mut whole = Mock::default();
    whole
        .replies
        .push_back(response(200, json!({"microversion":MID})));
    whole.replies.extend(partstudio_replies(MID, "1"));
    let replies: Vec<Response> = whole.replies.into();
    let mut first = Mock {
        replies: replies.clone().into(),
        ..Mock::default()
    };
    // Stop after the variables: the STEP export (3 calls) and the sketches are left.
    run(&mut first, &dir, PS, 7).err().unwrap();
    let mut again = Mock {
        replies: replies[7..].to_vec().into(),
        ..Mock::default()
    };
    let mut counted = Counted::default();
    let report = fetch_watched(
        &mut again,
        &FetchAuth::ApiKeys(credentials()),
        &source(PS),
        &dir.path().join("test.onshape-bundle"),
        &dir.path().join("cache"),
        None,
        Some(&mut counted),
    )
    .unwrap();
    assert_eq!(report.calls, 4);
    assert_eq!(
        counted.0,
        (1..=4).map(|sent| (sent, Some(4))).collect::<Vec<_>>()
    );
}
