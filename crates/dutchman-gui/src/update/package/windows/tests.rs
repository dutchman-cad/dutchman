use super::*;
use std::io::Cursor;

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
            assert!(!installed.join(format!("{name}.old")).exists());
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
            std::fs::read(installed.join(format!("{name}.old"))).unwrap(),
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
        assert!(!installed.join(format!("{name}.old")).exists());
    }
    assert_eq!(
        std::fs::read(installed.join("settings.txt")).unwrap(),
        b"keep"
    );
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
        assert!(installed.join(format!("{name}.old")).exists());
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
        assert!(!installed.join(format!("{name}.old")).exists());
    }
}
