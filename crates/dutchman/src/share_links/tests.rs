#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use super::*;
use std::io::Write;
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

fn reply(status: u16, headers: &str, body: &str) -> String {
    format!("HTTP/1.1 {status} Response\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len())
}

struct Mock {
    address: String,
    requests: Arc<Mutex<Vec<String>>>,
    task: std::thread::JoinHandle<()>,
}

impl Mock {
    fn new(responses: Vec<String>) -> Self {
        Self::new_bytes(responses.into_iter().map(String::into_bytes).collect())
    }

    fn new_bytes(responses: Vec<Vec<u8>>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let received = requests.clone();
        let task = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            for response in responses {
                let mut stream = loop {
                    match listener.accept() {
                        Ok((stream, _)) => break stream,
                        Err(e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                && Instant::now() < deadline =>
                        {
                            std::thread::sleep(Duration::from_millis(5))
                        }
                        _ => return,
                    }
                };
                // macOS/BSD: an accepted socket inherits the listener's non-blocking mode.
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(2)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0];
                while !request.ends_with(b"\r\n\r\n") {
                    stream.read_exact(&mut byte).unwrap();
                    request.push(byte[0]);
                }
                received
                    .lock()
                    .unwrap()
                    .push(String::from_utf8(request).unwrap());
                let _ = stream.write_all(&response);
            }
        });
        Self {
            address,
            requests,
            task,
        }
    }

    fn get(&self, provider: Provider, url: &str, cap: u64) -> Result<Downloaded, String> {
        let agent = ureq::AgentBuilder::new()
            .redirects(0)
            .try_proxy_from_env(false)
            .build();
        resolve(
            provider,
            Url::parse(url).unwrap(),
            cap,
            Duration::from_secs(2),
            |url, timeout| {
                let mut local = Url::parse(&self.address).unwrap();
                local.set_path(url.path());
                local.set_query(url.query());
                agent
                    .get(local.as_str())
                    .timeout(timeout)
                    .call()
                    .map_err(|_| PRIVATE.to_owned())
            },
        )
    }

    fn finish(self, count: usize) -> Vec<String> {
        self.task.join().unwrap();
        let requests = self.requests.lock().unwrap().clone();
        assert_eq!(requests.len(), count);
        for request in &requests {
            let lower = request.to_ascii_lowercase();
            assert!(!lower.contains("referer:"));
            assert!(!lower.contains("authorization:"));
        }
        requests
    }
}

#[test]
fn accepts_only_public_provider_file_sources() {
    assert!(is_url("HTTPS://a360.co/example"));
    assert!(is_url("file:///tmp/example.f3d"));
    assert!(!is_url("/tmp/https://example/part.f3d"));
    assert!(!is_url("./https://example/part.f3d"));
    assert!(!is_url("part.f3d"));
    let (_, url) = source("https://drive.google.com/file/d/synthetic-id/view?usp=sharing&resourcekey=public-key#fragment").unwrap();
    assert_eq!(
        url.as_str(),
        "https://drive.google.com/uc?export=download&id=synthetic-id&resourcekey=public-key"
    );
    assert!(source("https://drive.google.com/open?id=synthetic-id").is_ok());
    assert!(source("https://drive.google.com/uc?export=download&id=synthetic-id").is_ok());
    for url in [
        "http://a360.co/example",
        "https://a360.co:8443/example",
        "https://user@a360.co/example",
        "https://a360.co.evil.test/example",
        "https://evilautodesk360.com/example",
        "https://accounts.google.com/",
        "https://drive.google.com/drive/folders/example",
        "https://drive.google.com/uc?id=a%2Fb",
        "file:///tmp/example.f3d",
    ] {
        assert!(source(url).is_err(), "{url}");
    }
    for url in [
        "https://a360.co/example",
        "https://example.autodesk360.com/shares/public/example",
    ] {
        assert_eq!(source(url).unwrap().0, Provider::Fusion);
    }
    assert!(source(&format!("https://a360.co/{}", "x".repeat(8192))).is_err());
}

#[test]
fn drive_download_survives_until_owner_is_dropped() {
    let mock = Mock::new(vec![reply(
        200,
        "Content-Disposition: attachment; filename=\"../../synthetic.step\"\r\n",
        "synthetic CAD bytes",
    )]);
    let downloaded = mock
        .get(
            Provider::Drive,
            "https://drive.google.com/uc?export=download&id=example",
            100,
        )
        .unwrap();
    assert_eq!(downloaded.path().file_name().unwrap(), "synthetic.step");
    assert_eq!(
        std::fs::read(downloaded.path()).unwrap(),
        b"synthetic CAD bytes"
    );
    let folder = downloaded.folder.path().to_path_buf();
    drop(downloaded);
    assert!(!folder.exists());
    mock.finish(1);
}

#[test]
fn drive_follows_virus_scan_get_form() {
    let mock = Mock::new(vec![
        reply(
            200,
            "Content-Type: text/html; charset=utf-8\r\n",
            r#"<form id="download-form" action="https://drive.usercontent.google.com/download" method="get"><input type="hidden" name="id" value="example"><input type="hidden" name="export" value="download"><input type="hidden" name="confirm" value="t"><input type="hidden" name="uuid" value="synthetic&amp;token"></form>"#,
        ),
        reply(
            302,
            "Location: https://doc-example.googleusercontent.com/file\r\n",
            "",
        ),
        reply(
            200,
            "Content-Disposition: attachment; filename*=UTF-8''synthetic%20cube.f3d\r\n",
            "bytes",
        ),
    ]);
    let file = mock
        .get(
            Provider::Drive,
            "https://drive.google.com/uc?export=download&id=example",
            100,
        )
        .unwrap();
    assert_eq!(file.path().file_name().unwrap(), "synthetic cube.f3d");
    let requests = mock.finish(3);
    assert!(requests[1].contains("confirm=t"));
    assert!(requests[1].contains("uuid=synthetic%26token"));
    assert!(requests[1].contains("id=example"));
}

#[test]
fn drive_supports_old_confirmation_link_without_loading_other_page_urls() {
    let mock = Mock::new(vec![
        reply(
            200,
            "Content-Type: text/html\r\n",
            r#"<img src="https://unrelated.test/pixel"><a href="/uc?export=download&amp;confirm=token&amp;id=example">Download anyway</a>"#,
        ),
        reply(
            200,
            "Content-Disposition: attachment; filename=cube.ipt\r\n",
            "bytes",
        ),
    ]);
    assert!(mock
        .get(
            Provider::Drive,
            "https://drive.google.com/uc?id=example",
            100
        )
        .is_ok());
    let requests = mock.finish(2);
    assert!(requests[1].contains("confirm=token&id=example"));
}

#[test]
fn fusion_short_link_page_and_public_download() {
    let mock = Mock::new(vec![
        reply(
            302,
            "Location: https://example.autodesk360.com/shares/public/example\r\n",
            "",
        ),
        reply(
            200,
            "Content-Type: text/html\r\n",
            r#"<script type="application/json">{"data":{"downloadUrl":"https:\/\/developer.api.autodesk.com\/public-file"}}</script>"#,
        ),
        reply(
            200,
            "Content-Disposition: attachment; filename=cube.f3d\r\n",
            "bytes",
        ),
    ]);
    assert!(mock
        .get(Provider::Fusion, "https://a360.co/example", 100)
        .is_ok());
    mock.finish(3);
}

#[test]
fn fusion_download_anchor_and_missing_download_message() {
    let mock = Mock::new(vec![
        reply(
            200,
            "Content-Type: text/html\r\n",
            "<a href='https://other.test/app'>Download viewer</a><a href='/download-viewer.exe'>Download viewer</a><a href='/public/cube.f3z?token=public&amp;other=ok'><span>Download native file</span></a>",
        ),
        reply(200, "Content-Type: application/octet-stream\r\n", "bytes"),
    ]);
    assert!(mock
        .get(
            Provider::Fusion,
            "https://example.autodesk360.com/shares/public/example",
            100
        )
        .is_ok());
    mock.finish(2);
    let mock = Mock::new(vec![reply(
        200,
        "Content-Type: text/html\r\n",
        "<html>Sign in to download</html>",
    )]);
    assert_eq!(
        mock.get(Provider::Fusion, "https://a360.co/example", 100)
            .unwrap_err(),
        MANUAL
    );
    mock.finish(1);
}

#[test]
fn rejects_redirects_and_confirmation_actions_before_contacting_them() {
    for provider in [Provider::Drive, Provider::Fusion] {
        for location in [
            "http://drive.google.com/file",
            "https://evil.test/file",
            "https://drive.google.com.evil.test/file",
            "https://user@a360.co/file",
            "https://127.0.0.1/file",
            "https://drive.google.com:8443/file",
        ] {
            let mock = Mock::new(vec![reply(302, &format!("Location: {location}\r\n"), "")]);
            let url = if provider == Provider::Drive {
                "https://drive.google.com/uc?id=example"
            } else {
                "https://a360.co/example"
            };
            assert_eq!(mock.get(provider, url, 100).unwrap_err(), UNSAFE);
            mock.finish(1);
        }
    }
    let mock = Mock::new(vec![reply(200, "Content-Type: text/html\r\n", "<form id=download-form action=https://evil.test/download><input type=hidden name=confirm value=t></form>")]);
    assert_eq!(
        mock.get(
            Provider::Drive,
            "https://drive.google.com/uc?id=example",
            100
        )
        .unwrap_err(),
        UNSAFE
    );
    mock.finish(1);
    assert!(!Provider::Drive.allows(&Url::parse("https://a360.co/file").unwrap()));
    assert!(!Provider::Fusion.allows(&Url::parse("https://drive.google.com/file").unwrap()));
}

#[test]
fn caps_declared_and_streamed_sizes_and_pages() {
    for response in [
        reply(200, "Content-Disposition: attachment; filename=cube.f3d\r\n", "123456"),
        "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Disposition: attachment; filename=cube.f3d\r\n\r\n123456".into(),
        reply(200, "Content-Type: text/html\r\n", &"x".repeat(MAX_PAGE_BYTES as usize + 1)),
    ] {
        let mock = Mock::new(vec![response]);
        let error = mock.get(Provider::Drive, "https://drive.google.com/uc?id=example", 5).unwrap_err();
        assert!(error.contains("200 MB download limit") || error.contains("share page is too large"), "{error}");
        mock.finish(1);
    }
}

#[test]
fn private_expired_missing_name_and_redirect_loop_are_explained() {
    for response in [
        reply(403, "", "private"),
        reply(404, "", "expired"),
        reply(200, "Content-Type: text/html\r\n", "Sign in"),
    ] {
        let mock = Mock::new(vec![response]);
        assert_eq!(
            mock.get(
                Provider::Drive,
                "https://drive.google.com/uc?id=example",
                100
            )
            .unwrap_err(),
            PRIVATE
        );
        mock.finish(1);
    }
    let mock = Mock::new(vec![reply(200, "", "unrelated")]);
    assert!(mock
        .get(
            Provider::Drive,
            "https://drive.google.com/uc?id=example",
            100
        )
        .unwrap_err()
        .contains("named CAD file"));
    mock.finish(1);
    let mock = Mock::new(vec![reply(302, "Location: /again\r\n", ""); MAX_REQUESTS]);
    assert!(mock
        .get(
            Provider::Drive,
            "https://drive.google.com/uc?id=example",
            100
        )
        .unwrap_err()
        .contains("too many times"));
    mock.finish(MAX_REQUESTS);
}

#[test]
fn deadline_and_sanitized_errors_do_not_disclose_link_tokens() {
    let error = resolve(
        Provider::Drive,
        Url::parse("https://drive.google.com/uc?id=secret-token").unwrap(),
        100,
        Duration::ZERO,
        |_, _| Err("Request started after deadline".into()),
    )
    .unwrap_err();
    assert_eq!(error, NETWORK);
    assert!(!error.contains("secret-token"));
    assert_eq!(
        disposition_name("attachment; filename*=UTF-8''cube%2Bpart.f3d"),
        Some("cube+part.f3d".into())
    );
}

#[test]
fn downloaded_synthetic_design_converts_like_a_local_file() {
    // Existing synthetic cadmpeg-generated fixture; its origin is recorded in the fixtures README.
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
    let bytes = std::fs::read(&fixture).unwrap();
    let mut response = format!("HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: {}\r\nContent-Disposition: attachment; filename=generated_unit_cube.f3d\r\n\r\n", bytes.len()).into_bytes();
    response.extend_from_slice(&bytes);
    let mock = Mock::new_bytes(vec![response]);
    let file = mock
        .get(
            Provider::Drive,
            "https://drive.google.com/uc?id=example",
            MAX_DOWNLOAD_BYTES,
        )
        .unwrap();
    let output = tempfile::tempdir().unwrap();
    let options = crate::ConvertOptions {
        no_verify: true,
        ..Default::default()
    };
    let settings = crate::batch::Settings::default();
    let downloaded = crate::batch::convert_all(
        &[file.path().to_path_buf()],
        &output.path().join("downloaded"),
        &options,
        &settings,
    )
    .unwrap();
    let local = crate::batch::convert_all(
        &[fixture],
        &output.path().join("local"),
        &options,
        &settings,
    )
    .unwrap();
    assert_eq!(downloaded["ok"], true, "{downloaded}");
    assert_eq!(local["ok"], true, "{local}");
    let extension = options.output_extension();
    let design = |folder: &str| {
        let archive = std::fs::File::open(
            output
                .path()
                .join(folder)
                .join(format!("generated_unit_cube.{extension}")),
        )
        .unwrap();
        let mut archive = zip::ZipArchive::new(archive).unwrap();
        let mut document = String::new();
        archive
            .by_name("document.json")
            .unwrap()
            .read_to_string(&mut document)
            .unwrap();
        serde_json::from_str::<serde_json::Value>(&document).unwrap()
    };
    assert_eq!(design("downloaded"), design("local"));
    let scratch = file.folder.path().to_path_buf();
    drop(file);
    assert!(!scratch.exists());
    assert!(output.path().join("downloaded").exists());
    mock.finish(1);
}

#[test]
fn actual_http_timeout_is_bounded() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    listener.set_nonblocking(true).unwrap();
    let task = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error)
                    if error.kind() == std::io::ErrorKind::WouldBlock
                        && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(2))
                }
                _ => return,
            }
        };
        // macOS/BSD: an accepted socket inherits the listener's non-blocking mode.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut request = [0; 4096];
        let _ = stream.read(&mut request);
        std::thread::sleep(Duration::from_millis(100));
        let _ = stream.write_all(
            reply(
                200,
                "Content-Disposition: attachment; filename=cube.f3d\r\n",
                "bytes",
            )
            .as_bytes(),
        );
    });
    let agent = ureq::AgentBuilder::new()
        .redirects(0)
        .try_proxy_from_env(false)
        .build();
    let error = resolve(
        Provider::Drive,
        Url::parse("https://drive.google.com/uc?id=example").unwrap(),
        100,
        Duration::from_millis(20),
        |_, remaining| {
            agent
                .get(&address)
                .timeout(remaining)
                .call()
                .map_err(|_| NETWORK.to_owned())
        },
    )
    .unwrap_err();
    assert_eq!(error, NETWORK);
    task.join().unwrap();
}

#[test]
fn truncated_download_removes_partial_file_and_folder() {
    let mock = Mock::new(vec!["HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 20\r\nContent-Disposition: attachment; filename=cube.f3d\r\n\r\nshort".into()]);
    let folder = tempfile::tempdir().unwrap();
    let scratch = folder.path().to_path_buf();
    let agent = ureq::AgentBuilder::new().try_proxy_from_env(false).build();
    let result = resolve_to(
        Provider::Drive,
        Url::parse("https://drive.google.com/uc?id=example").unwrap(),
        100,
        Duration::from_secs(2),
        folder,
        |_, remaining| {
            agent
                .get(&mock.address)
                .timeout(remaining)
                .call()
                .map_err(|_| NETWORK.to_owned())
        },
    );
    assert!(result.is_err());
    assert!(!scratch.exists());
    mock.finish(1);
}

#[test]
fn confirmation_preserves_resource_key_and_original_file() {
    let base =
        Url::parse("https://drive.google.com/uc?export=download&id=example&resourcekey=public-key")
            .unwrap();
    let page = "<form id=download-form method=GET action='https://drive.usercontent.google.com/download'><input type=hidden name=confirm value=t><input type=hidden name=id value=different></form>";
    let result = drive_confirmation(page, &base).unwrap();
    assert_eq!(
        result.query_pairs().find(|(key, _)| key == "id").unwrap().1,
        "example"
    );
    assert_eq!(
        result
            .query_pairs()
            .find(|(key, _)| key == "resourcekey")
            .unwrap()
            .1,
        "public-key"
    );
    assert!(supported_name(Path::new("part.prt.7")));
    assert!(!supported_name(Path::new("program.exe")));
}

#[test]
fn frontend_shutdown_removes_active_scratch_and_prevents_a_late_write() {
    let mut parent = Some(tempfile::tempdir().unwrap());
    let root = parent.as_ref().unwrap().path().to_path_buf();
    let folder = scratch(Some(&root)).unwrap();
    let mock = Mock::new(vec![reply(
        200,
        "Content-Disposition: attachment; filename=cube.f3d\r\n",
        "bytes",
    )]);
    let agent = ureq::AgentBuilder::new().try_proxy_from_env(false).build();
    let result = resolve_to(
        Provider::Drive,
        Url::parse("https://drive.google.com/uc?id=example").unwrap(),
        100,
        Duration::from_secs(2),
        folder,
        |_, remaining| {
            let response = agent
                .get(&mock.address)
                .timeout(remaining)
                .call()
                .map_err(|_| NETWORK.to_owned())?;
            // The window closes while the background request is in flight. Its owner
            // removes the parent before this late response can create an output file.
            drop(parent.take());
            Ok(response)
        },
    );
    assert!(result.is_err());
    assert!(!root.exists());
    mock.finish(1);
}

#[test]
fn sign_in_redirects_explain_private_links_without_contacting_login_hosts() {
    for (provider, source, login, message) in [
        (
            Provider::Drive,
            "https://drive.google.com/uc?id=example",
            "https://accounts.google.com/signin",
            PRIVATE,
        ),
        (
            Provider::Fusion,
            "https://a360.co/example",
            "https://accounts.autodesk.com/",
            MANUAL,
        ),
        (
            Provider::Fusion,
            "https://a360.co/example",
            "https://signin.autodesk.com/",
            MANUAL,
        ),
    ] {
        let mock = Mock::new(vec![reply(302, &format!("Location: {login}\r\n"), "")]);
        assert_eq!(mock.get(provider, source, 100).unwrap_err(), message);
        mock.finish(1);
    }
}

#[test]
fn total_deadline_removes_scratch_while_body_is_still_streaming() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let (headers, received) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut stream = loop {
            match listener.accept() {
                Ok((stream, _)) => break stream,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    assert!(Instant::now() < deadline);
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(error) => panic!("Mock accept failed: {error}"),
            }
        };
        // macOS/BSD: an accepted socket inherits the listener's non-blocking mode.
        stream.set_nonblocking(false).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let _ = stream.read(&mut [0; 4096]);
        stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\nContent-Disposition: attachment; filename=synthetic.f3d\r\n\r\nx").unwrap();
        headers.send(()).unwrap();
        let _ = wait.recv_timeout(Duration::from_secs(5));
        let _ = stream.write_all(b"xxxx");
    });
    let root = tempfile::tempdir().unwrap();
    let started = Instant::now();
    let result = download_using(
        Provider::Drive,
        Url::parse("https://drive.google.com/uc?id=synthetic").unwrap(),
        Some(root.path()),
        Duration::from_millis(200),
        move |_, _| {
            // Deliberately supply a transport whose body ignores the download deadline.
            ureq::AgentBuilder::new()
                .try_proxy_from_env(false)
                .build()
                .get(&address)
                .timeout(Duration::from_secs(5))
                .call()
                .map_err(|_| NETWORK.to_owned())
        },
    );
    received.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(result.unwrap_err(), NETWORK);
    assert!(started.elapsed() < Duration::from_secs(1));
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    release.send(()).unwrap();
    server.join().unwrap();
    assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
}
