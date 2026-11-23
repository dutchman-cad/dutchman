//! Fusion tool libraries converted to CamCraft's `.camtools` by the `dutchman` binary.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::Write;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{json, Value};

fn dutchman(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(args)
        .output()
        .unwrap()
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

fn tool(kind: &str, name: &str, dc: f64) -> Value {
    json!({
        "BMC": "carbide", "description": name, "type": kind, "unit": "millimeters",
        "geometry": {"DC": dc, "LCF": 12.0, "LB": 40.0, "SFDM": dc, "NOF": 2},
        "post-process": {"number": 7},
        "start-values": {"presets": [{"n": 9000.0, "v_f": 1500.0, "material": {"category": "all"}}]}
    })
}

fn library() -> Value {
    json!({"version": 22, "data": [
        tool("flat end mill", "EM 6", 6.0),
        tool("ball end mill", "BM 4", 4.0),
        tool("reamer", "Reamer 8", 8.0),
    ]})
}

fn camtools(file: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap()
}

#[test]
fn a_json_library_converts_beside_itself() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("shop.json");
    std::fs::write(&input, library().to_string()).unwrap();
    let out = dutchman(&["convert", path(&input)]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Fusion tool library (.json)"), "{stdout}");
    assert!(stdout.contains("2 tools; 1 not written"), "{stdout}");
    let written = camtools(&dir.path().join("shop.camtools"));
    assert_eq!(written["format"], "camcraft-tools");
    assert_eq!(written["units"], "mm");
    let tools = written["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools[0]["name"], "EM 6");
    assert_eq!(tools[0]["definition"]["kind"], "end_mill");
    assert_eq!(tools[1]["definition"]["kind"], "ball_nose");
    assert_eq!(tools[1]["definition"]["corner_r"], 2.0);
    assert!(String::from_utf8_lossy(&out.stderr).contains("Reamer 8"));
}

#[test]
fn a_tools_archive_converts_with_a_json_report() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("shop.tools");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&input).unwrap());
    zip.start_file("tools.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(library().to_string().as_bytes()).unwrap();
    zip.finish().unwrap();
    let output = dir.path().join("out").join("mine.camtools");
    let out = dutchman(&["convert", path(&input), "-o", path(&output), "--json"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["schema"], "dutchman.convert/1");
    assert_eq!(report["ok"], true);
    assert_eq!(report["input"]["format"], "fusion_tool_library");
    assert_eq!(report["output"]["format"], "camcraft-tools");
    assert_eq!(
        report["tools"],
        json!({"read": 3, "written": 2, "not_written": 1})
    );
    let losses = report["losses"].as_array().unwrap();
    assert!(losses
        .iter()
        .any(|l| l["level"] == "Dropped" && l["subject"].as_str().unwrap().contains("Reamer 8")));
    assert_eq!(camtools(&output)["tools"].as_array().unwrap().len(), 2);
}

#[test]
fn an_existing_default_output_is_kept_without_force() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("shop.json");
    std::fs::write(&input, library().to_string()).unwrap();
    let existing = dir.path().join("shop.camtools");
    std::fs::write(&existing, "mine").unwrap();
    let out = dutchman(&["convert", path(&input)]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(std::fs::read_to_string(&existing).unwrap(), "mine");
    let out = dutchman(&["convert", path(&input), "--force"]);
    assert!(out.status.success());
    assert_eq!(camtools(&existing)["format"], "camcraft-tools");
}

#[test]
fn a_library_with_nothing_convertible_fails_and_says_why() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("odd.json");
    std::fs::write(
        &input,
        json!({"data": [tool("reamer", "R", 8.0)]}).to_string(),
    )
    .unwrap();
    let out = dutchman(&["convert", path(&input), "--json"]);
    assert!(!out.status.success());
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert!(report["error"]["message"]
        .as_str()
        .unwrap()
        .contains("none of the 1 tools"));
    assert_eq!(report["losses"][0]["level"], "Dropped");
    assert!(!dir.path().join("odd.camtools").exists());
}

#[test]
fn a_folder_holding_a_tools_file_converts_it() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in");
    std::fs::create_dir(&input).unwrap();
    let mut zip = zip::ZipWriter::new(std::fs::File::create(input.join("shop.tools")).unwrap());
    zip.start_file("tools.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(library().to_string().as_bytes()).unwrap();
    zip.finish().unwrap();
    let out_dir = dir.path().join("out");
    let out = dutchman(&["convert", path(&input), "-o", path(&out_dir)]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        camtools(&out_dir.join("shop.camtools"))["tools"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn formats_names_the_tool_library_conversion() {
    let out = dutchman(&["formats", "--json"]);
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["tool_libraries"][0]["output"], "camtools");
    assert!(String::from_utf8_lossy(&dutchman(&["formats"]).stdout).contains(".camtools"));
}

#[test]
fn a_tool_library_result_is_presented_as_tools_not_as_a_design() {
    use dutchman::present::{FileView, SendSteps, Status, Surface, SEND_TOOLS_INTRO};
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("shop.json");
    std::fs::write(&input, library().to_string()).unwrap();
    let report = dutchman::tool_library::convert(&input, &dir.path().join("shop.camtools"), true);
    let view = FileView::from_report(&report);
    assert_eq!(view.status, Status::ConvertedWithLosses);
    assert_eq!(view.status_word, "Converted, some tools left out");
    assert_eq!(
        view.outcome,
        "2 of 3 tools written to CamCraft's tool library"
    );
    assert_eq!(
        view.capability.as_deref(),
        Some("Fusion tool library (.json): converts the tool library to CamCraft's .camtools")
    );
    let policy = dutchman::report_policy::ReportPolicy::bundled();
    let steps = SendSteps::new(policy, None, &report, Surface::Desktop);
    assert_eq!(steps.intro, SEND_TOOLS_INTRO);
    // The third card asks for the library's other export, whichever one this was.
    assert!(steps.step_card);
    assert_eq!(steps.step_title, "Please add the same library as .tools");
    assert_eq!(
        steps.step_note,
        Some("The other export of the same library lets us check every tool's sizes.")
    );
    let mut from_tools = report.clone();
    from_tools["input"]["extension"] = serde_json::json!("tools");
    let steps = SendSteps::new(policy, None, &from_tools, Surface::Desktop);
    assert_eq!(steps.step_title, "Please add the same library as .json");
    // Too big for Discord: the DM note names the tool library and the hours, not design sizes.
    let big = dutchman::report_policy::ReportPolicy::bundled();
    let linked = SendSteps::new(big, Some(u64::MAX), &report, Surface::Desktop);
    assert_eq!(
        linked.dm_text,
        "Paste the message. Dutchy replies with a private upload link for the tool library (72 hours)."
    );
}
