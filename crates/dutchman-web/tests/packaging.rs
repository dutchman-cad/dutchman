//! Checks the archive's programs with synthetic executables, without a release build.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]

use std::fs;
use std::os::unix::fs::PermissionsExt as _;
use std::process::Command;

fn archive(target: &str, web: bool) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path();
    fs::create_dir_all(dir.join("scripts")).unwrap();
    fs::write(
        dir.join("scripts/package-archive.sh"),
        include_str!("../../../scripts/package-archive.sh"),
    )
    .unwrap();
    fs::write(
        dir.join("scripts/archive-readme.py"),
        include_str!("../../../scripts/archive-readme.py"),
    )
    .unwrap();
    for name in ["LICENSE", "NOTICE", "README.md"] {
        fs::write(dir.join(name), "Archive fixture\n").unwrap();
    }
    let fonts = dir.join("crates/dutchman-gui/assets/fonts");
    fs::create_dir_all(&fonts).unwrap();
    fs::write(fonts.join("licence.txt"), "Font licence fixture\n").unwrap();
    let licences = dir.join("licences");
    fs::create_dir_all(&licences).unwrap();
    fs::write(
        licences.join("dependency-MIT.txt"),
        "Dependency licence fixture\n",
    )
    .unwrap();
    let release = dir.join("target").join(target).join("release");
    fs::create_dir_all(&release).unwrap();
    for name in ["dutchman", "dutchman-mcp", "dutchman-gui", "dutchman-web"] {
        // Non-Linux fixtures deliberately omit the web program.
        if name == "dutchman-web" && !web {
            continue;
        }
        let program = release.join(name);
        let input = if name == "dutchman-mcp" {
            "read -r request\n"
        } else {
            ""
        };
        fs::write(
            &program,
            format!("#!/bin/sh\n{input}printf '%s\\n' '{{\"result\":{{}}}}'\n"),
        )
        .unwrap();
        fs::set_permissions(program, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = Command::new("bash")
        .arg("scripts/package-archive.sh")
        .args([target, "fixture", "test-ref"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let listing = Command::new("tar")
        .args(["tzf", "dist/fixture.tar.gz"])
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(listing.status.success());
    let files = String::from_utf8(listing.stdout).unwrap();
    for name in ["dutchman", "dutchman-mcp", "dutchman-gui"] {
        assert!(files.lines().any(|path| path == format!("fixture/{name}")));
    }
    assert_eq!(
        files.lines().any(|path| path == "fixture/dutchman-web"),
        web
    );
    assert!(files
        .lines()
        .any(|path| path == "fixture/licences/dependency-MIT.txt"));
    assert!(dir.join("dist/fixture.tar.gz.sha256").is_file());
}

#[test]
fn linux_archives_include_the_web_server() {
    for target in ["x86_64-unknown-linux-gnu", "aarch64-unknown-linux-gnu"] {
        archive(target, true);
    }
}

#[test]
fn macos_archives_keep_their_existing_programs() {
    archive("aarch64-apple-darwin", false);
}
