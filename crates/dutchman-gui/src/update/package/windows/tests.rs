use super::*;
use std::io::Cursor;
use std::time::Duration;

fn package(missing: bool) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for name in PROGRAMS.into_iter().take(if missing { 2 } else { 3 }) {
        zip.start_file(
            format!("dutchman-1.2.3-x86_64-pc-windows-msvc/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(format!("new {name}").as_bytes()).unwrap();
    }
    for name in [
        "AGENTS.md",
        "docs/guide.md",
        "skills/dutchman-converting/SKILL.md",
    ] {
        zip.start_file(
            format!("dutchman-1.2.3-x86_64-pc-windows-msvc/{name}"),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(b"new guidance").unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn fixture(root: &Path) -> PathBuf {
    let installed = root.join("Portable copy");
    std::fs::create_dir(&installed).unwrap();
    for name in PROGRAMS {
        std::fs::write(installed.join(name), format!("old {name}")).unwrap();
    }
    std::fs::write(installed.join("settings.txt"), b"keep").unwrap();
    installed
}

fn fetcher<'a>(
    bytes: &'a [u8],
    correct: bool,
) -> impl Fn(&str, u64, &mut dyn Write) -> Result<(), String> + 'a {
    move |url, _, out| {
        if url.ends_with(".sha256") {
            let name = url.rsplit('/').next().unwrap().trim_end_matches(".sha256");
            let hash = if correct {
                Sha256::digest(bytes)
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect()
            } else {
                "0".repeat(64)
            };
            writeln!(out, "{hash}  {name}").map_err(|e| e.to_string())
        } else {
            out.write_all(bytes).map_err(|e| e.to_string())
        }
    }
}

#[test]
fn checksum_failure_deletes_each_download_without_changing_the_install() {
    for msi in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let installed = fixture(root.path());
        let logs = root.path().join("logs");
        let kind = if msi {
            Install::Msi(installed.clone())
        } else {
            Install::Zip(installed.clone())
        };
        let bytes = package(false);
        let error = prepare_in(&kind, "v1.2.3", &logs, &fetcher(&bytes, false))
            .err()
            .unwrap();
        assert!(error.contains("checksum does not match"));
        assert_eq!(std::fs::read_dir(&logs).unwrap().count(), 0);
        for name in PROGRAMS {
            assert_eq!(
                std::fs::read(installed.join(name)).unwrap(),
                format!("old {name}").as_bytes()
            );
            assert!(!installed.join(format!("{name}.dutchman-old")).exists());
        }
        assert_eq!(
            std::fs::read(installed.join("settings.txt")).unwrap(),
            b"keep"
        );
    }
}

#[test]
fn verified_msi_waits_for_close_and_portable_zip_swaps_all_three_programs() {
    let root = tempfile::tempdir().unwrap();
    let installed = fixture(root.path());
    let logs = root.path().join("logs");
    let bytes = package(false);
    let msi = prepare_in(
        &Install::Msi(installed.clone()),
        "v1.2.3",
        &logs,
        &fetcher(&bytes, true),
    )
    .unwrap()
    .unwrap();
    assert_eq!(std::fs::read(&msi.path).unwrap(), bytes);
    assert_eq!(
        std::fs::read(installed.join("dutchman-gui.exe")).unwrap(),
        b"old dutchman-gui.exe"
    );
    drop(msi);
    let kind = Install::Zip(installed.clone());
    assert!(prepare_in(&kind, "v1.2.3", &logs, &fetcher(&bytes, true))
        .unwrap()
        .is_none());
    for name in PROGRAMS {
        assert_eq!(
            std::fs::read(installed.join(name)).unwrap(),
            format!("new {name}").as_bytes()
        );
        assert_eq!(
            std::fs::read(installed.join(format!("{name}.dutchman-old"))).unwrap(),
            format!("old {name}").as_bytes()
        );
    }
    assert_eq!(
        std::fs::read(installed.join("docs/guide.md")).unwrap(),
        b"new guidance"
    );
    assert!(installed.join("AGENTS.md").is_file());
    cleanup(&kind);
    for name in PROGRAMS {
        assert!(!installed.join(format!("{name}.dutchman-old")).exists());
    }
    assert_eq!(
        std::fs::read(installed.join("settings.txt")).unwrap(),
        b"keep"
    );
}

#[test]
fn a_users_own_old_files_and_folders_survive_an_update_and_the_next_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let installed = fixture(root.path());
    std::fs::write(installed.join("notes.old"), b"mine").unwrap();
    std::fs::write(installed.join("dutchman.exe.old"), b"mine too").unwrap();
    std::fs::create_dir_all(installed.join("docs")).unwrap();
    std::fs::write(installed.join("docs/my-notes.md"), b"my docs").unwrap();
    std::fs::create_dir_all(installed.join("skills/mine")).unwrap();
    std::fs::write(installed.join("skills/mine/SKILL.md"), b"my skill").unwrap();
    std::fs::write(installed.join("AGENTS.md.old"), b"my agents").unwrap();
    let bytes = package(false);
    let kind = Install::Zip(installed.clone());
    assert!(prepare_in(
        &kind,
        "v1.2.3",
        &root.path().join("logs"),
        &fetcher(&bytes, true)
    )
    .unwrap()
    .is_none());
    cleanup(&kind);
    cleanup(&kind);
    for (name, content) in [
        ("notes.old", "mine"),
        ("dutchman.exe.old", "mine too"),
        ("docs/my-notes.md", "my docs"),
        ("skills/mine/SKILL.md", "my skill"),
        ("AGENTS.md.old", "my agents"),
        ("settings.txt", "keep"),
    ] {
        assert_eq!(
            std::fs::read(installed.join(name)).unwrap(),
            content.as_bytes(),
            "{name}"
        );
    }
    assert_eq!(
        std::fs::read(installed.join("docs/guide.md")).unwrap(),
        b"new guidance"
    );
    // The update's own backups are gone, and a second update works.
    assert!(!installed.join("dutchman.exe.dutchman-old").exists());
    assert!(!installed.join(STATE).exists());
    assert!(prepare_in(
        &kind,
        "v1.2.3",
        &root.path().join("logs"),
        &fetcher(&bytes, true)
    )
    .unwrap()
    .is_none());
}

#[test]
fn incomplete_verified_zip_leaves_all_original_programs_in_place() {
    let root = tempfile::tempdir().unwrap();
    let installed = fixture(root.path());
    let bytes = package(true);
    assert!(prepare_in(
        &Install::Zip(installed.clone()),
        "v1.2.3",
        &root.path().join("logs"),
        &fetcher(&bytes, true)
    )
    .is_err());
    assert_eq!(std::fs::read_dir(&installed).unwrap().count(), 4);
    for name in PROGRAMS {
        assert_eq!(
            std::fs::read(installed.join(name)).unwrap(),
            format!("old {name}").as_bytes()
        );
    }
}

#[test]
fn urls_and_checksums_require_the_stable_release_and_allowed_https_hosts() {
    assert!(asset_url("v1.2.3", "msi")
        .unwrap()
        .ends_with("/v1.2.3/dutchman-1.2.3-x86_64-pc-windows-msvc.msi"));
    for tag in ["nightly", "v1.2.3/../../other", "v1.2.3-beta"] {
        assert!(asset_url(tag, "zip").is_err());
    }
    for url in [
        "http://github.com/dutchman-cad/dutchman/releases/download/x",
        "https://github.com/other/repo",
        "https://release-assets.githubusercontent.com.evil.test/a",
        "https://user@release-assets.githubusercontent.com/a",
    ] {
        assert!(!allowed_url(url));
    }
    assert!(allowed_url(
        "https://release-assets.githubusercontent.com/a?token=b"
    ));
    let text = format!("{}  app.msi\n", "ab".repeat(32));
    assert_eq!(
        checksum(text.as_bytes(), "app.msi").unwrap(),
        "ab".repeat(32)
    );
    assert!(checksum(text.as_bytes(), "other.msi").is_err());
    assert!(checksum(b"abcd  app.msi", "app.msi").is_err());
}

#[cfg(windows)]
#[test]
#[ignore = "Needs the packaged release ZIP; run by the Windows installer job"]
fn windows_portable_swap_while_gui_is_running() {
    use std::process::{Command, Stdio};
    let archive = PathBuf::from(
        std::env::var_os("DUTCHMAN_PORTABLE_TEST_ARCHIVE").expect("Release ZIP path"),
    );
    let root = tempfile::tempdir().unwrap();
    let installed = fixture(root.path());
    let system = PathBuf::from(std::env::var_os("SystemRoot").unwrap());
    // A renamed cmd.exe blocked on stdin holds an executable image open without a GPU.
    std::fs::copy(
        system.join("System32/cmd.exe"),
        installed.join("dutchman-gui.exe"),
    )
    .unwrap();
    let mut child = Command::new(installed.join("dutchman-gui.exe"))
        .args(["/c", "set /p hold="])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert!(child.try_wait().unwrap().is_none());
    let name = archive.file_stem().unwrap().to_str().unwrap();
    let result = apply_zip(&archive, name, &installed);
    let _ = child.kill();
    child.wait().unwrap();
    result.unwrap();
    for name in PROGRAMS {
        assert!(installed.join(format!("{name}.dutchman-old")).exists());
    }
    for guide in [
        "AGENTS.md",
        "docs/install.md",
        "skills/dutchman-converting/SKILL.md",
    ] {
        assert!(installed.join(guide).is_file());
    }
    let version = Command::new(installed.join("dutchman.exe"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(version.status.success());
    assert!(String::from_utf8_lossy(&version.stdout).contains(env!("CARGO_PKG_VERSION")));
    cleanup(&Install::Zip(installed.clone()));
    for name in PROGRAMS {
        assert!(!installed.join(format!("{name}.dutchman-old")).exists());
    }
}

#[test]
fn a_verified_appimage_replaces_the_running_file_and_a_bad_checksum_does_not() {
    if super::super::appimage::asset_name("v1.2.3", std::env::consts::ARCH).is_err() {
        return; // No AppImage exists for this processor.
    }
    let root = tempfile::tempdir().unwrap();
    let logs = root.path().join("logs");
    let target = root.path().join("Dutchman.AppImage");
    std::fs::write(&target, b"old appimage").unwrap();
    let kind = Install::AppImage(target.clone());
    let bytes = b"new appimage".to_vec();
    let error = prepare_in(&kind, "v1.2.3", &logs, &fetcher(&bytes, false))
        .err()
        .unwrap();
    assert!(error.contains("checksum does not match"));
    assert_eq!(std::fs::read(&target).unwrap(), b"old appimage");
    assert_eq!(std::fs::read_dir(&logs).unwrap().count(), 0);
    assert!(prepare_in(&kind, "v1.2.3", &logs, &fetcher(&bytes, true))
        .unwrap()
        .is_none());
    assert_eq!(std::fs::read(&target).unwrap(), bytes);
}

mod blocked_network {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    const TIMEOUTS: Timeouts = Timeouts {
        connect: Duration::from_secs(1),
        stall: Duration::from_secs(1),
        total: None,
    };

    /// How a local server treats the package request.
    #[derive(Clone, Copy)]
    enum Mode {
        /// Accepts the connection and never answers.
        Silent,
        /// Sends headers and part of the body, then goes quiet.
        Stalls,
        /// Nothing listens.
        Refuses,
        /// Sends part of the body and closes the connection.
        Truncates,
    }

    /// Serves a valid checksum, then treats the package as `mode` says. Returns the port.
    fn serve(mode: Mode, bytes: Vec<u8>) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        if matches!(mode, Mode::Refuses) {
            drop(listener);
            return port;
        }
        std::thread::spawn(move || {
            let mut held = Vec::new();
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut request = [0; 2048];
                let n = if matches!(mode, Mode::Silent) {
                    0
                } else {
                    stream.read(&mut request).unwrap_or(0)
                };
                let head = String::from_utf8_lossy(&request[..n]).to_string();
                let path = head.split_whitespace().nth(1).unwrap_or("").to_owned();
                if let Some(name) = path
                    .strip_suffix(".sha256")
                    .map(|p| p.rsplit('/').next().unwrap_or(""))
                {
                    let body = format!(
                        "{}  {name}\n",
                        Sha256::digest(&bytes)
                            .iter()
                            .map(|b| format!("{b:02x}"))
                            .collect::<String>()
                    );
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                } else if matches!(mode, Mode::Truncates) {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\npartial"
                    );
                } else if matches!(mode, Mode::Stalls) {
                    let _ = write!(
                        stream,
                        "HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\npartial"
                    );
                    let _ = stream.flush();
                    held.push(stream);
                } else {
                    held.push(stream);
                }
            }
        });
        port
    }

    /// Runs a whole update against the local server and checks it ends clearly and in time.
    fn run(mode: Mode) {
        let error = run_failing(mode);
        assert_eq!(error, UNREACHABLE);
        assert!(error.contains("unchanged"));
    }

    /// Fails an update against the local server and returns the error; checks time and files.
    fn run_failing(mode: Mode) -> String {
        let bytes = package(false);
        let port = serve(mode, bytes);
        let root = tempfile::tempdir().unwrap();
        let installed = fixture(root.path());
        let logs = root.path().join("logs");
        let fetch = |url: &str, limit: u64, out: &mut dyn Write| {
            let path = url.split("/download/").nth(1).unwrap();
            download_with(
                &format!("http://127.0.0.1:{port}/{path}"),
                limit,
                out,
                TIMEOUTS,
            )
        };
        let started = Instant::now();
        let error = prepare_in(&Install::Zip(installed.clone()), "v1.2.3", &logs, &fetch)
            .err()
            .expect("a blocked network must fail");
        assert!(
            started.elapsed() < Duration::from_secs(8),
            "took {:?}",
            started.elapsed()
        );
        for name in PROGRAMS {
            assert_eq!(
                std::fs::read_to_string(installed.join(name)).unwrap(),
                format!("old {name}")
            );
        }
        assert_eq!(
            std::fs::read(installed.join("settings.txt")).unwrap(),
            b"keep"
        );
        error
    }

    #[test]
    fn a_server_that_never_answers_ends_in_the_clear_message() {
        run(Mode::Silent);
    }

    #[test]
    fn a_download_that_stalls_midway_ends_in_the_clear_message() {
        run(Mode::Stalls);
    }

    #[test]
    fn a_refused_connection_ends_in_the_clear_message() {
        run(Mode::Refuses);
    }

    #[test]
    fn a_connection_that_drops_midway_says_so_not_firewall() {
        let error = run_failing(Mode::Truncates);
        assert_ne!(error, UNREACHABLE);
        assert!(error.contains("interrupted"), "{error}");
        assert!(error.contains("unchanged"));
    }
}
