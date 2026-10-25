//! The web server end to end, on synthetic fixtures: upload, convert, read the results page and
//! the API's report, download designs, and the limits and checks around them.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::Write as _;
use std::path::{Path, PathBuf};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum::Router;
use dutchman_web::Config;
use http_body_util::BodyExt as _;
use serde_json::Value;
use tower::ServiceExt as _;

const BOUNDARY: &str = "dutchman-test-boundary";

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures")
        .join(name)
}

/// A server keeping its jobs in `data`, converting in this process.
fn server(data: &Path) -> Router {
    dutchman_web::router(Config {
        data_dir: data.to_path_buf(),
        ..Config::default()
    })
}

/// A multipart body with `fields` (name, value) and `files` (field name, file name, bytes).
fn form(fields: &[(&str, &str)], files: &[(&str, &str, Vec<u8>)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in fields {
        write!(
            body,
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        )
        .unwrap();
    }
    for (name, file, bytes) in files {
        write!(
            body,
            "--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{file}\"\r\n\
             Content-Type: application/octet-stream\r\n\r\n"
        )
        .unwrap();
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    write!(body, "--{BOUNDARY}--\r\n").unwrap();
    body
}

fn post(uri: &str, body: Vec<u8>) -> Request<Body> {
    Request::post(uri)
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

async fn send(
    app: &Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = response
        .into_body()
        .collect()
        .await
        .unwrap()
        .to_bytes()
        .to_vec();
    (status, headers, body)
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

#[tokio::test]
async fn the_page_offers_the_formats_that_convert() {
    let data = tempfile::tempdir().unwrap();
    let (status, headers, body) = send(&server(data.path()), get("/")).await;
    assert_eq!(status, StatusCode::OK);
    let page = text(&body);
    assert!(page.contains("action=\"convert\""), "{page}");
    assert!(page.contains(".f3d"));
    assert!(page.contains("Inventor part"));
    assert!(headers.get(header::CONTENT_SECURITY_POLICY).is_some());
}

#[tokio::test]
async fn an_upload_converts_and_its_designs_download() {
    let data = tempfile::tempdir().unwrap();
    let app = server(data.path());
    let cube = std::fs::read(fixture("generated_unit_cube.f3d")).unwrap();
    let (status, _, body) = send(
        &app,
        post(
            "/api/convert",
            form(
                &[("to", "solidcraft")],
                &[
                    ("files", "Bracket v2.f3d", cube.clone()),
                    ("files", "notes.txt", b"not a design".to_vec()),
                ],
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", text(&body));
    let report: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(report["schema"], "dutchman.convert-all/1");
    assert_eq!(report["converted"], 1);
    assert_eq!(report["skipped"], 1);
    let result = &report["results"][0];
    assert_eq!(result["input"]["path"], "Bracket v2.f3d");
    assert_eq!(result["output"]["path"], "Bracket v2.solidcraft");
    let job = report["job"].as_str().unwrap();
    let link = result["output"]["download"].as_str().unwrap();
    assert_eq!(link, format!("jobs/{job}/files/Bracket%20v2.solidcraft"));
    let shown = text(&body);
    assert!(
        !shown.contains(&data.path().display().to_string()),
        "the report shows where the server keeps uploads: {shown}"
    );

    let (status, headers, design) = send(&app, get(&format!("/{link}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(design.starts_with(b"PK"), "a design is a zip container");
    let disposition = headers[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(
        disposition.contains("Bracket v2.solidcraft"),
        "{disposition}"
    );

    let (status, _, all) = send(&app, get(&format!("/jobs/{job}/designs.zip"))).await;
    assert_eq!(status, StatusCode::OK);
    let zip = zip::ZipArchive::new(std::io::Cursor::new(all)).unwrap();
    let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    assert_eq!(names, ["Bracket v2.solidcraft"]);

    let (status, _, page) = send(&app, get(&format!("/jobs/{job}"))).await;
    assert_eq!(status, StatusCode::OK);
    let page = text(&page);
    assert!(page.contains("1 converted, 0 failed, 1 skipped"), "{page}");
    assert!(page.contains("Geometry without history"), "{page}");
    assert!(page.contains(&format!("href=\"../{link}\"")), "{page}");
}

#[tokio::test]
async fn the_form_redirects_to_its_results_and_failures_are_shown() {
    let data = tempfile::tempdir().unwrap();
    let app = server(data.path());
    let empty = std::fs::read(fixture("container_metadata_only.f3d")).unwrap();
    let (status, headers, _) = send(
        &app,
        post("/convert", form(&[], &[("files", "empty.f3d", empty)])),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    assert!(location.starts_with("jobs/"), "{location}");
    let (status, _, page) = send(&app, get(&format!("/{location}"))).await;
    assert_eq!(status, StatusCode::OK);
    let page = text(&page);
    assert!(page.contains("0 converted, 1 failed"), "{page}");
    assert!(page.contains("Failed"));
    assert!(!page.contains("designs.zip"), "nothing to download: {page}");
}

#[tokio::test]
async fn a_project_zip_keeps_its_folders() {
    let data = tempfile::tempdir().unwrap();
    let app = server(data.path());
    let mut archive = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut archive);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("project/parts/cube.f3d", options).unwrap();
        zip.write_all(&std::fs::read(fixture("generated_unit_cube.f3d")).unwrap())
            .unwrap();
        zip.start_file("project/cube.sldprt", options).unwrap();
        zip.write_all(&std::fs::read(fixture("solidworks_cube.sldprt")).unwrap())
            .unwrap();
        zip.finish().unwrap();
    }
    let (status, _, body) = send(
        &app,
        post(
            "/api/convert",
            form(&[], &[("files", "project.zip", archive.into_inner())]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", text(&body));
    let report: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(report["converted"], 2);
    let mut outputs: Vec<&str> = report["results"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["output"]["path"].as_str().unwrap())
        .collect();
    outputs.sort_unstable();
    assert_eq!(
        outputs,
        ["project/cube.solidcraft", "project/parts/cube.solidcraft"]
    );
}

#[tokio::test]
async fn unsafe_zips_and_empty_uploads_are_refused() {
    let data = tempfile::tempdir().unwrap();
    let app = server(data.path());
    let mut archive = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut archive);
        zip.start_file("../escape.f3d", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"x").unwrap();
        zip.finish().unwrap();
    }
    let (status, _, body) = send(
        &app,
        post(
            "/api/convert",
            form(&[], &[("files", "evil.zip", archive.into_inner())]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(text(&body).contains("outside its own folder"));

    let (status, _, body) = send(&app, post("/api/convert", form(&[], &[]))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(text(&body).contains("Choose at least one design file"));

    // A refused upload leaves no job behind.
    assert_eq!(std::fs::read_dir(data.path()).unwrap().count(), 0);
}

#[tokio::test]
async fn uploads_over_the_limit_are_refused() {
    let data = tempfile::tempdir().unwrap();
    let app = dutchman_web::router(Config {
        data_dir: data.path().to_path_buf(),
        max_upload: 1024,
        ..Config::default()
    });
    let (status, _, body) = send(
        &app,
        post(
            "/api/convert",
            form(&[], &[("files", "big.f3d", vec![0u8; 4096])]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE, "{}", text(&body));
    assert!(text(&body).contains("Uploads are limited to"));
}

#[tokio::test]
async fn downloads_stay_inside_their_job() {
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("secret.txt"), "secret").unwrap();
    let app = server(data.path());
    for uri in [
        "/jobs/../secret.txt",
        "/jobs/0123456789abcdef0123456789abcdef/files/../../secret.txt",
        "/jobs/0123456789abcdef0123456789abcdef/files/%2E%2E/%2E%2E/secret.txt",
        "/jobs/not-a-job/designs.zip",
    ] {
        let (status, _, body) = send(&app, get(uri)).await;
        assert_ne!(status, StatusCode::OK, "{uri}");
        assert!(!text(&body).contains("secret"), "{uri}");
    }
}

#[tokio::test]
async fn a_password_is_asked_for_when_set() {
    let data = tempfile::tempdir().unwrap();
    let app = dutchman_web::router(Config {
        data_dir: data.path().to_path_buf(),
        login: Some(("me".to_owned(), "s3cret".to_owned())),
        ..Config::default()
    });
    let (status, headers, _) = send(&app, get("/")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(headers.contains_key(header::WWW_AUTHENTICATE));

    let wrong = Request::get("/")
        .header(header::AUTHORIZATION, "Basic bWU6d3Jvbmc=") // me:wrong
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, wrong).await.0, StatusCode::UNAUTHORIZED);

    let right = Request::get("/")
        .header(header::AUTHORIZATION, "Basic bWU6czNjcmV0") // me:s3cret
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(&app, right).await.0, StatusCode::OK);

    // A health check needs no password.
    assert_eq!(send(&app, get("/healthz")).await.0, StatusCode::OK);
}
