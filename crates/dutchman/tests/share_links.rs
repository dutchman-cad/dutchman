//! CLI checks never contact a public service; resolution itself uses loopback mocks in the library.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::process::Command;

#[test]
fn convert_url_help_and_rejected_links_keep_json_private() {
    let help = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(["convert", "--help"])
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8_lossy(&help.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(help.contains("public Fusion or Google Drive"));
    for link in [
        "http://a360.co/synthetic-secret",
        "https://drive.google.com/drive/folders/synthetic-secret",
        "https://unrelated.test/synthetic-secret",
    ] {
        let scratch = tempfile::tempdir().unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_dutchman"))
            .env("TMPDIR", scratch.path())
            .env("TMP", scratch.path())
            .env("TEMP", scratch.path())
            .args(["convert", link, "--json"])
            .output()
            .unwrap();
        assert_eq!(result.status.code(), Some(1));
        assert_eq!(std::fs::read_dir(scratch.path()).unwrap().count(), 0);
        let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(report["schema"], "dutchman.convert/1");
        assert_eq!(report["ok"], false);
        assert_eq!(report["input"]["path"], "Shared file");
        assert_eq!(report["error"]["stage"], "input");
        assert!(!String::from_utf8_lossy(&result.stdout).contains("synthetic-secret"));
        assert!(!String::from_utf8_lossy(&result.stderr).contains("synthetic-secret"));
    }
}
