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
    assert!(page.contains(&format!(
        "<span class=\"version\">v{}</span>",
        dutchman::BUILD_VERSION
    )));
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
    assert_eq!(result["output"]["path"], "Bracket v2.sc");
    let job = report["job"].as_str().unwrap();
    let link = result["output"]["download"].as_str().unwrap();
    assert_eq!(link, format!("jobs/{job}/files/Bracket%20v2.sc"));
    let shown = text(&body);
    assert!(
        !shown.contains(&data.path().display().to_string()),
        "the report shows where the server keeps uploads: {shown}"
    );

    let (status, headers, design) = send(&app, get(&format!("/{link}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(design.starts_with(b"PK"), "a design is a zip container");
    let disposition = headers[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(disposition.contains("Bracket v2.sc"), "{disposition}");

    let (status, _, all) = send(&app, get(&format!("/jobs/{job}/designs.zip"))).await;
    assert_eq!(status, StatusCode::OK);
    let zip = zip::ZipArchive::new(std::io::Cursor::new(all)).unwrap();
    let names: Vec<String> = zip.file_names().map(|n| n.unwrap().into_owned()).collect();
    assert_eq!(names, ["Bracket v2.sc"]);

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
    assert_eq!(outputs, ["project/cube.sc", "project/parts/cube.sc"]);
}

#[tokio::test]
async fn a_folder_upload_preserves_project_paths_and_rejects_traversal() {
    let data = tempfile::tempdir().unwrap();
    let app = server(data.path());
    let bytes = std::fs::read(fixture("generated_unit_cube.f3d")).unwrap();
    let (status, _, body) = send(
        &app,
        post(
            "/api/convert",
            form(&[], &[("folder", "project/parts/cube.f3d", bytes.clone())]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{}", text(&body));
    let report: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(report["converted"], 1);
    assert_eq!(
        report["results"][0]["output"]["path"],
        "project/parts/cube.sc"
    );
    assert!(report["plan"].as_str().unwrap().contains("1 part"));
    let (status, _, _) = send(
        &app,
        post(
            "/api/convert",
            form(&[], &[("folder", "../escape.f3d", bytes)]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(!data.path().join("escape.f3d").exists());
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

#[tokio::test]
async fn zip_budgets_are_shared_by_the_upload() {
    let mut archive = std::io::Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut archive);
        zip.start_file("part.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"1234").unwrap();
        zip.finish().unwrap();
    }
    for (entries, bytes, message) in [
        (10, 7, "unpacks to more than"),
        (1, 100, "entries"),
        (2, 8, ""),
    ] {
        let data = tempfile::tempdir().unwrap();
        let app = dutchman_web::router(Config {
            data_dir: data.path().to_path_buf(),
            zip: dutchman_web::upload::ZipLimits { entries, bytes },
            ..Config::default()
        });
        let (status, _, body) = send(
            &app,
            post(
                "/api/convert",
                form(
                    &[],
                    &[
                        ("files", "first.zip", archive.get_ref().clone()),
                        ("files", "second.zip", archive.get_ref().clone()),
                    ],
                ),
            ),
        )
        .await;
        if message.is_empty() {
            assert_eq!(status, StatusCode::OK, "{}", text(&body));
            let report: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(report["skipped"], 2);
        } else {
            assert_eq!(status, StatusCode::BAD_REQUEST, "{}", text(&body));
            assert!(text(&body).contains(message));
            assert_eq!(std::fs::read_dir(data.path()).unwrap().count(), 0);
        }
    }
}

#[tokio::test]
async fn saved_data_above_the_limit_refuses_new_jobs() {
    let data = tempfile::tempdir().unwrap();
    let nested = data.path().join("saved/out");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(nested.join("design"), b"12345").unwrap();
    let app = dutchman_web::router(Config {
        data_dir: data.path().to_path_buf(),
        max_data: 4,
        ..Config::default()
    });
    for uri in ["/convert", "/api/convert"] {
        let (status, _, body) = send(&app, post(uri, form(&[], &[]))).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert!(text(&body).contains("too little storage"));
        assert_eq!(std::fs::read_dir(data.path()).unwrap().count(), 1);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn too_little_free_space_refuses_new_jobs() {
    let data = tempfile::tempdir().unwrap();
    let app = dutchman_web::router(Config {
        data_dir: data.path().to_path_buf(),
        min_free: u64::MAX,
        ..Config::default()
    });
    let (status, _, body) = send(&app, post("/api/convert", form(&[], &[]))).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(text(&body).contains("too little storage"));
    assert_eq!(std::fs::read_dir(data.path()).unwrap().count(), 0);
}

struct SlowUpload(Option<tokio::sync::oneshot::Sender<()>>);

impl axum::body::HttpBody for SlowUpload {
    type Data = axum::body::Bytes;
    type Error = std::io::Error;

    fn poll_frame(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        if let Some(started) = self.0.take() {
            let _ = started.send(());
        }
        std::task::Poll::Pending
    }
}

#[tokio::test]
async fn excess_uploads_are_refused_without_waiting_for_the_body() {
    let data = tempfile::tempdir().unwrap();
    let app = dutchman_web::router(Config {
        data_dir: data.path().to_path_buf(),
        uploads: 1,
        ..Config::default()
    });
    let (started, receiving) = tokio::sync::oneshot::channel();
    let mut request = post("/api/convert", Vec::new());
    *request.body_mut() = Body::new(SlowUpload(Some(started)));
    let first = tokio::spawn(app.clone().oneshot(request));
    tokio::time::timeout(std::time::Duration::from_secs(5), receiving)
        .await
        .unwrap()
        .unwrap();
    let mut excess = post("/api/convert", Vec::new());
    *excess.body_mut() = Body::new(SlowUpload(None));
    let (status, _, body) =
        tokio::time::timeout(std::time::Duration::from_secs(5), send(&app, excess))
            .await
            .unwrap();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(text(&body).contains("too many uploads"));
    first.abort();
    let _ = first.await;
    let (status, _, _) = send(&app, post("/api/convert", form(&[], &[]))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "the permit is released");
}

#[tokio::test]
async fn browser_origins_are_checked_on_both_upload_routes() {
    let data = tempfile::tempdir().unwrap();
    let app = dutchman_web::router(Config {
        data_dir: data.path().to_path_buf(),
        public_origin: vec!["https://public.example".to_owned()],
        ..Config::default()
    });
    for uri in ["/convert", "/api/convert"] {
        for (origin, fetch, allowed) in [
            (None, None, true),
            (Some("http://server:8080"), Some("same-origin"), true),
            (Some("http://SERVER:8080"), None, true),
            (Some("https://public.example"), None, true),
            (Some("http://server:8080"), Some("cross-site"), false),
            (Some("https://public.example"), Some("cross-site"), false),
            (None, Some("cross-site"), false),
            (Some("https://other.example"), None, false),
            (Some("http://server:9999"), None, false),
            (Some("null"), None, false),
            (Some("http://server:8080@other.example"), None, false),
            (Some("http://server:8080/path"), None, false),
        ] {
            let mut request = post(uri, form(&[], &[]));
            request
                .headers_mut()
                .insert(header::HOST, "server:8080".parse().unwrap());
            if let Some(origin) = origin {
                request
                    .headers_mut()
                    .insert(header::ORIGIN, origin.parse().unwrap());
            }
            if let Some(fetch) = fetch {
                request
                    .headers_mut()
                    .insert("sec-fetch-site", fetch.parse().unwrap());
            }
            let (status, _, body) = send(&app, request).await;
            assert_eq!(
                status,
                if allowed {
                    StatusCode::BAD_REQUEST
                } else {
                    StatusCode::FORBIDDEN
                },
                "{uri} {origin:?} {fetch:?}: {}",
                text(&body)
            );
            if !allowed {
                assert!(text(&body).contains("Submit uploads"));
            }
        }
    }
    assert_eq!(std::fs::read_dir(data.path()).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn data_folders_are_private_and_checked_before_use() {
    use std::os::unix::fs::{symlink, PermissionsExt as _};
    let data = tempfile::tempdir().unwrap();
    let fresh = data.path().join("fresh");
    dutchman_web::prepare_data_dir(&fresh).unwrap();
    assert_eq!(
        std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777,
        0o700
    );
    dutchman_web::prepare_data_dir(&fresh).unwrap();
    let link = data.path().join("link");
    symlink(&fresh, &link).unwrap();
    assert!(dutchman_web::prepare_data_dir(&link).is_err());
    std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(dutchman_web::prepare_data_dir(&fresh).is_err());
    std::fs::set_permissions(&fresh, std::fs::Permissions::from_mode(0o720)).unwrap();
    assert!(dutchman_web::prepare_data_dir(&fresh).is_err());
    let file = data.path().join("file");
    std::fs::write(&file, b"").unwrap();
    assert!(dutchman_web::prepare_data_dir(&file).is_err());
}

#[tokio::test]
async fn a_base_path_serves_every_page_under_it() {
    let data = tempfile::tempdir().unwrap();
    let app = dutchman_web::router(Config {
        data_dir: data.path().to_path_buf(),
        base_path: "/dutchman".to_owned(),
        ..Config::default()
    });
    let (status, _, page) = send(&app, get("/dutchman/")).await;
    assert_eq!(status, StatusCode::OK);
    assert!(text(&page).contains("action=\"convert\""));
    // The page's links are relative to `/dutchman/`, so the bare path moves there.
    let (status, headers, _) = send(&app, get("/dutchman")).await;
    assert_eq!(status, StatusCode::PERMANENT_REDIRECT);
    assert_eq!(headers[header::LOCATION], "/dutchman/");
    assert_eq!(send(&app, get("/")).await.0, StatusCode::NOT_FOUND);
    assert_eq!(send(&app, get("/healthz")).await.0, StatusCode::OK);
    assert_eq!(send(&app, get("/dutchman/healthz")).await.0, StatusCode::OK);

    let cube = std::fs::read(fixture("generated_unit_cube.f3d")).unwrap();
    let (status, headers, _) = send(
        &app,
        post(
            "/dutchman/convert",
            form(&[], &[("files", "cube.f3d", cube.clone())]),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    assert!(location.starts_with("jobs/"), "{location}");
    let (status, _, page) = send(&app, get(&format!("/dutchman/{location}"))).await;
    assert_eq!(status, StatusCode::OK);
    let page = text(&page);
    let link = page
        .split("class=\"download\" href=\"../")
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .unwrap();
    let (status, _, design) = send(&app, get(&format!("/dutchman/{link}"))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(design.starts_with(b"PK"));

    // Problem pages lead back to the base path, whatever their depth.
    let (status, _, page) = send(
        &app,
        get("/dutchman/jobs/0123456789abcdef0123456789abcdef/files/a/b.sc"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(text(&page).contains("href=\"/dutchman/\""));

    // The cross-site check still guards the upload routes under the base path.
    let request = Request::post("/dutchman/api/convert")
        .header(
            header::CONTENT_TYPE,
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .header("sec-fetch-site", "cross-site")
        .body(Body::from(form(&[], &[("files", "cube.f3d", cube)])))
        .unwrap();
    let (status, _, body) = send(&app, request).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(text(&body).starts_with("{\"error\""), "{}", text(&body));
}

#[tokio::test]
async fn designs_are_also_saved_in_the_output_folder() {
    let data = tempfile::tempdir().unwrap();
    let share = tempfile::tempdir().unwrap();
    let app = dutchman_web::router(Config {
        data_dir: data.path().to_path_buf(),
        output_dir: Some(share.path().to_path_buf()),
        ..Config::default()
    });
    let (_, _, page) = send(&app, get("/")).await;
    assert!(text(&page).contains("also saved in the server's output folder"));

    let cube = std::fs::read(fixture("generated_unit_cube.f3d")).unwrap();
    let mut saved = Vec::new();
    for _ in 0..2 {
        let (status, _, body) = send(
            &app,
            post(
                "/api/convert",
                form(&[], &[("files", "Bracket.f3d", cube.clone())]),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", text(&body));
        let report: Value = serde_json::from_slice(&body).unwrap();
        saved.push(report["results"][0]["output"]["saved"].clone());
        assert!(
            !text(&body).contains(&share.path().display().to_string()),
            "the report shows where the server keeps designs"
        );
        let job = report["job"].as_str().unwrap();
        let (_, _, page) = send(&app, get(&format!("/jobs/{job}"))).await;
        assert!(text(&page).contains("Saved in the output folder as Bracket"));
    }
    // A second upload of the same name never replaces the first one's design.
    assert_eq!(saved, ["Bracket.sc", "Bracket (1).sc"]);
    for name in ["Bracket.sc", "Bracket (1).sc"] {
        let design = std::fs::read(share.path().join(name)).unwrap();
        assert!(design.starts_with(b"PK"), "{name}");
    }
}

#[tokio::test]
async fn designs_open_in_solidcraft_when_configured() {
    for (address, to, expected) in [
        (
            Some("/"),
            "solidcraft",
            Some("/?open=%2Fdutchman%2Fjobs%2F"),
        ),
        (None, "solidcraft", None),
        (Some("/"), "freecad", None),
    ] {
        let data = tempfile::tempdir().unwrap();
        let mut config = Config {
            data_dir: data.path().to_path_buf(),
            base_path: "/dutchman".to_owned(),
            solidcraft: address.map(str::to_owned),
            ..Config::default()
        };
        let app = dutchman_web::router(config.clone());
        let cube = std::fs::read(fixture("generated_unit_cube.f3d")).unwrap();
        let (status, _, body) = send(
            &app,
            post(
                "/dutchman/api/convert",
                form(&[("to", to)], &[("files", "Bracket & v2.f3d", cube)]),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{}", text(&body));
        let report: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(report["converted"], 1, "{report}");
        let job = report["job"].as_str().unwrap();
        let (_, _, page) = send(&app, get(&format!("/dutchman/jobs/{job}"))).await;
        let (_, _, saved) = send(&app, get(&format!("/dutchman/jobs/{job}/report.json"))).await;
        let saved: Value = serde_json::from_slice(&saved).unwrap();
        assert_eq!(
            saved["results"][0]["output"],
            report["results"][0]["output"]
        );
        let output = &report["results"][0]["output"];
        if to == "freecad" {
            assert_eq!(output["format"], "freecad-macro/1");
        }
        if let Some(prefix) = expected {
            let link = format!("{prefix}{job}%2Ffiles%2FBracket%2520%2526%2520v2.sc");
            assert_eq!(output["open"], link);
            assert!(
                text(&page).contains(&format!(
                    "href=\"{link}\" target=\"_blank\" rel=\"noopener\">Open in SolidCraft</a>"
                )),
                "{}",
                text(&page)
            );
            let download = output["download"].as_str().unwrap();
            assert_eq!(
                send(&app, get(&format!("/dutchman/{download}"))).await.0,
                StatusCode::OK
            );
            let stored: Value = serde_json::from_slice(
                &std::fs::read(data.path().join(job).join("report.json")).unwrap(),
            )
            .unwrap();
            assert!(stored["results"][0]["output"].get("open").is_none());
            config.solidcraft = Some("/cad/".to_owned());
            let restarted = dutchman_web::router(config);
            let (_, _, body) = send(
                &restarted,
                get(&format!("/dutchman/jobs/{job}/report.json")),
            )
            .await;
            let report: Value = serde_json::from_slice(&body).unwrap();
            assert!(report["results"][0]["output"]["open"]
                .as_str()
                .unwrap()
                .starts_with("/cad/?open=%2Fdutchman%2F"));
        } else {
            assert!(output.get("open").is_none(), "{report}");
            assert!(!text(&page).contains("Open in SolidCraft"));
        }
    }
}

#[tokio::test]
async fn a_failed_file_offers_the_private_steps_with_its_message() {
    let data = tempfile::tempdir().unwrap();
    let app = server(data.path());
    let upload = form(&[], &[("files", "Secret.f3d", b"not a design".to_vec())]);
    let (_, headers, _) = send(&app, post("/convert", upload)).await;
    let location = headers[header::LOCATION].to_str().unwrap().to_owned();
    let (_, _, page) = send(&app, get(&format!("/{location}"))).await;
    let page = text(&page);
    for wanted in [
        "Join our Discord server",
        "https://discord.gg/yTcseBEdeu",
        "https://discord.com/users/",
        "Drag the file in (the file you picked).",
        "Have a STEP export?",
        "Report anonymously on GitHub",
    ] {
        assert!(page.contains(wanted), "{wanted}: {page}");
    }
    assert!(page.contains("Report id: f"), "{page}");
    assert!(page.contains("STEP: unknown."), "{page}");
    let message = page.split("data-message=\"").nth(1).unwrap();
    let message = message.split('"').next().unwrap();
    assert!(!message.contains("Secret"), "{message}");
}
