#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use dutchman::{batch_report, saved_report};
use serde_json::{json, Value};

fn report(name: &str, mode: &str) -> Value {
    json!({"schema": "dutchman.convert/1", "ok": true,
        "input": {"path": format!("sources/{name}.ipt")},
        "output": {"path": format!("converted/{name}.sc")},
        "mode": mode, "history": {"rebuilt": 2, "needing": 3}, "losses": []})
}

#[test]
fn each_outcome_has_a_row_with_the_first_cause() {
    let mut partial = report("partial", "partial_history");
    partial["history"]["features"] = json!([
        {"status": "not_needed", "reason": "tree node"},
        {"status": "left_out", "reason": "downstream of a blocker"},
        {"status": "blocked", "reason": "edge, \"round\" | <missing>\nNext line"}]);
    let failed = json!({"schema": "dutchman.convert/1", "ok": false,
        "input": {"path": "sources/failed.ipt"}, "error": {"message": "Cannot read geometry."}});
    let (md, csv) = batch_report::render(
        &[
            report("full", "history"),
            partial,
            report("geometry", "import"),
            failed,
        ],
        std::path::Path::new("converted"),
    );
    assert_eq!(md, "# Batch report\n\n4 files: 1 full history, 1 partial, 1 geometry only, 1 failed\n\n| Input path | Output path | Outcome | First reason |\n|---|---|---|---|\n| failed.ipt |  | Failed | Cannot read geometry. |\n| full.ipt | full.sc | Full history |  |\n| geometry.ipt | geometry.sc | Geometry only |  |\n| partial.ipt | partial.sc | Partial 2 of 3 | edge, \"round\" &#124; &lt;missing&gt; Next line |\n");
    assert_eq!(csv, "Input path,Output path,Outcome,First reason\r\n\"failed.ipt\",\"\",\"Failed\",\"Cannot read geometry.\"\r\n\"full.ipt\",\"full.sc\",\"Full history\",\"\"\r\n\"geometry.ipt\",\"geometry.sc\",\"Geometry only\",\"\"\r\n\"partial.ipt\",\"partial.sc\",\"Partial 2 of 3\",\"edge, \"\"round\"\" | <missing>\nNext line\"\r\n");
}

#[test]
fn earlier_run_reads_nested_reports_and_writes_both_files() {
    let folder = tempfile::tempdir().unwrap();
    for (name, mode) in [
        ("full", "history"),
        ("partial", "partial_history"),
        ("geometry", "import"),
    ] {
        saved_report::save(
            &folder.path().join(format!("nested/{name}.sc")),
            &report(name, mode),
        )
        .unwrap();
    }
    saved_report::save(
        &folder.path().join("failed.sc"),
        &json!({
            "schema": "dutchman.convert/1", "ok": false,
            "input": {"path": "sources/failed.ipt"}, "error": {"message": "No geometry."}
        }),
    )
    .unwrap();
    assert_eq!(batch_report::write_folder(folder.path()).unwrap(), 4);
    let md = std::fs::read_to_string(folder.path().join("dutchman-batch-report.md")).unwrap();
    for outcome in ["Full history", "Partial 2 of 3", "Geometry only", "Failed"] {
        assert!(md.contains(outcome));
    }
    assert!(folder.path().join("dutchman-batch-report.csv").is_file());
    assert_eq!(batch_report::write_folder(folder.path()).unwrap(), 4);
}

#[test]
fn empty_or_unreadable_report_does_not_publish_a_summary() {
    let folder = tempfile::tempdir().unwrap();
    assert!(batch_report::write_folder(folder.path()).is_err());
    std::fs::write(folder.path().join("synthetic.report.json"), b"{}").unwrap();
    assert!(batch_report::write_folder(folder.path()).is_err());
    assert!(!folder.path().join("dutchman-batch-report.md").exists());
}

#[test]
fn flattened_history_uses_geometry_words_and_older_counts_still_read() {
    let mut old = report("older", "partial_history");
    old["history"] = json!({"mapped": 1, "total": 4});
    assert_eq!(dutchman::present::batch_outcome(&old), "Partial 1 of 4");
    old["mode"] = json!("history");
    old["verification"] = json!({"status": "flattened", "reason": "Replay differs."});
    assert_eq!(dutchman::present::batch_outcome(&old), "Geometry only");
    assert_eq!(batch_report::first_reason(&old), "Replay differs.");
}

#[test]
fn latest_report_wins_and_cli_can_summarise_saved_results() {
    let folder = tempfile::tempdir().unwrap();
    let output = folder.path().join("synthetic.sc");
    let mut failed = report("synthetic", "import");
    failed["ok"] = json!(false);
    failed["error"] = json!({"message": "Earlier failure."});
    saved_report::save(&output, &failed).unwrap();
    let failed_path = saved_report::path(&output, false);
    let times = std::fs::FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH);
    std::fs::File::options()
        .write(true)
        .open(failed_path)
        .unwrap()
        .set_times(times)
        .unwrap();
    saved_report::save(&output, &report("synthetic", "history")).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("batch-report")
        .arg(folder.path())
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let md = std::fs::read_to_string(folder.path().join("dutchman-batch-report.md")).unwrap();
    assert!(md.contains("Full history"));
    assert!(!md.contains("Earlier failure."));
    assert_eq!(md.lines().count(), 7);
    assert!(md.contains("1 file: 1 full history, 0 partial, 0 geometry only, 0 failed"));
}

#[test]
fn input_paths_accept_both_platform_separators() {
    let reports = [
        json!({"ok": false, "input": {"path": "C:\\sources\\nested\\a.ipt"}}),
        json!({"ok": false, "input": {"path": "C:\\sources\\b.ipt"}}),
    ];
    let (md, _) = batch_report::render(&reports, std::path::Path::new("converted"));
    assert!(md.contains("| nested/a.ipt |"));
    assert!(md.contains("| b.ipt |"));
}

#[test]
fn fresh_cli_run_can_write_a_batch_report_with_failed_inputs() {
    let folder = tempfile::tempdir().unwrap();
    let input = folder.path().join("synthetic.ipt");
    std::fs::write(&input, b"synthetic undecodable input").unwrap();
    let output = folder.path().join("converted");
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args([
            "convert-all",
            "--batch-report",
            "--yes",
            "--no-verify",
            "--out-dir",
        ])
        .arg(&output)
        .arg(&input)
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let md = std::fs::read_to_string(output.join("dutchman-batch-report.md")).unwrap();
    assert!(md.contains("1 file: 0 full history, 0 partial, 0 geometry only, 1 failed"));
    assert!(md.contains("| synthetic.ipt |"));
    assert!(md.contains("| Failed |"));
    assert!(output.join("dutchman-batch-report.csv").is_file());
}

#[test]
fn absolute_outputs_are_relative_to_a_relative_output_folder() {
    let mut report = report("synthetic", "history");
    report["output"]["path"] = json!(std::env::current_dir()
        .unwrap()
        .join("converted/synthetic.sc"));
    let (md, _) = batch_report::render(&[report], std::path::Path::new("converted"));
    assert!(md.contains("| synthetic.ipt | synthetic.sc | Full history |"));
}

#[test]
fn markdown_markup_in_names_and_reasons_is_literal() {
    let mut report = report("[synthetic]", "import");
    report["verification"] = json!({"reason": "*edge* _blocked_ ~~later~~ [reason](target)"});
    let (md, csv) = batch_report::render(&[report], std::path::Path::new("converted"));
    assert!(md.contains("&#91;synthetic&#93;.ipt"));
    assert!(md.contains(
        "&#42;edge&#42; &#95;blocked&#95; &#126;&#126;later&#126;&#126; &#91;reason&#93;(target)"
    ));
    assert!(csv.contains("*edge* _blocked_ ~~later~~ [reason](target)"));
}

#[test]
fn totals_keep_zero_categories_and_count_retained_outcomes() {
    let mut flattened = report("flattened", "history");
    flattened["verification"] = json!({"status": "flattened"});
    for (reports, totals) in [
        (
            vec![],
            "0 files: 0 full history, 0 partial, 0 geometry only, 0 failed",
        ),
        (
            vec![report("full", "history")],
            "1 file: 1 full history, 0 partial, 0 geometry only, 0 failed",
        ),
        (
            vec![report("partial", "partial_history"), flattened],
            "2 files: 0 full history, 1 partial, 1 geometry only, 0 failed",
        ),
    ] {
        let (md, csv) = batch_report::render(&reports, std::path::Path::new("converted"));
        assert!(md.starts_with(&format!("# Batch report\n\n{totals}\n\n")));
        assert_eq!(csv.lines().count(), reports.len() + 1);
        assert!(csv.starts_with("Input path,Output path,Outcome,First reason\r\n"));
    }
}
