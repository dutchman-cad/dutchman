//! Public front ends use the same allow-listed profile; no native files leave this process.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn cli_counts_profile_is_opt_in_and_rejects_private_keys() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("private-design.f3d");
    std::fs::write(&source, b"synthetic unreadable file").unwrap();
    let destination = folder.path().join("counts.json");
    let output = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(["convert", "--no-verify", "--report-counts", "--report-anon"])
        .arg(&destination)
        .arg(&source)
        .arg(folder.path().join("output.sc"))
        .output()
        .unwrap();
    assert!(!output.status.success());
    let text = std::fs::read_to_string(&destination).unwrap();
    let report: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "dutchman.report-counts/1");
    assert_eq!(report["files"][0]["converted"], false);
    assert!(!text.contains("private-design"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("salt"));
    let output = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(["convert", "--report-counts", "--report-anon"])
        .arg(&destination)
        .arg("--report-key")
        .arg(folder.path().join("key.json"))
        .arg(&source)
        .arg(folder.path().join("output.sc"))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!folder.path().join("key.json").exists());
}

#[test]
fn mcp_counts_profile_writes_only_after_an_explicit_request() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("private-design.f3d");
    std::fs::write(&source, b"synthetic unreadable file").unwrap();
    let destination = folder.path().join("counts.json");
    let mut child = Command::new(env!("CARGO_BIN_EXE_dutchman-mcp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let requests = [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"counts-test","version":"1"}}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"convert_all","arguments":{"inputs":[source],"out_dir":folder.path().join("output"),"report_anon":destination,"report_counts":true,"verify":false}}}),
    ];
    for request in requests {
        writeln!(stdin, "{request}").unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        let reply: Value = serde_json::from_str(&line).unwrap();
        assert!(reply.get("error").is_none(), "{reply}");
        if reply["id"] == 2 {
            assert_eq!(
                reply["result"]["structuredContent"]["report_anon"]["profile"], "counts",
                "{reply}"
            );
            assert!(reply["result"]["structuredContent"]["report_anon"]
                .get("salt")
                .is_none());
        }
    }
    drop(stdin);
    assert!(child.wait().unwrap().success());
    let text = std::fs::read_to_string(destination).unwrap();
    let report: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "dutchman.report-counts/1");
    assert!(!text.contains("private-design"));
}

#[test]
fn cli_counts_profile_on_a_converted_fixture_has_no_source_measurements() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("private-design.f3d");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d"),
        &source,
    )
    .unwrap();
    let destination = folder.path().join("counts.json");
    let output = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(["convert", "--no-verify", "--report-counts", "--report-anon"])
        .arg(&destination)
        .arg(&source)
        .arg(folder.path().join("output.sc"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(destination).unwrap();
    let report: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["files"][0]["source_format"], "fusion_archive");
    assert_eq!(report["files"][0]["converted"], true);
    assert!(!text.contains("private-design"));
    assert!(!text.contains("geometry"));
    assert!(!text.contains("deviation"));
    assert!(!text.contains("volume"));
}

#[test]
fn convert_all_makes_the_folders_its_journal_and_report_go_in() {
    let folder = tempfile::tempdir().unwrap();
    let designs = folder.path().join("designs");
    std::fs::create_dir_all(&designs).unwrap();
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d"),
        designs.join("cube.f3d"),
    )
    .unwrap();
    // Neither the output folder nor the report's folder exists yet.
    let out = folder.path().join("out");
    let journal = out.join("journal.jsonl");
    let report = folder.path().join("reports/summary-counts.json");
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_dutchman"))
            .args(["convert-all", "--no-verify", "--jobs", "1", "--out-dir"])
            .arg(&out)
            .arg("--journal")
            .arg(&journal)
            .args(["--report-counts", "--report-anon"])
            .arg(&report)
            .arg(&designs)
            .output()
            .unwrap()
    };
    for _ in 0..2 {
        let output = run();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = std::fs::read_to_string(&report).unwrap();
        let counts: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(counts["schema"], "dutchman.report-counts/1");
        assert_eq!(counts["files"][0]["converted"], true);
    }
    assert!(std::fs::read_to_string(&journal).unwrap().lines().count() >= 1);
}

#[test]
fn convert_makes_the_folders_of_a_full_anonymous_report_and_its_key() {
    let folder = tempfile::tempdir().unwrap();
    let source = folder.path().join("private-design.f3d");
    std::fs::copy(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d"),
        &source,
    )
    .unwrap();
    let report = folder.path().join("shared/report.json");
    let key = folder.path().join("private/key.json");
    let output = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(["convert", "--no-verify", "--report-anon"])
        .arg(&report)
        .arg("--report-key")
        .arg(&key)
        .arg(&source)
        .arg(folder.path().join("output.sc"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_str(&std::fs::read_to_string(report).unwrap()).unwrap();
    assert_eq!(report["schema"], "dutchman.report-anon/1");
    let key: Value = serde_json::from_str(&std::fs::read_to_string(key).unwrap()).unwrap();
    assert!(key["salt"].is_string());
}
