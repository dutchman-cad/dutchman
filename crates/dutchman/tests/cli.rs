//! End-to-end checks of the `dutchman` binary.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;
use std::process::{Command, Output};

fn dutchman(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(args)
        .output()
        .unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dutchman-cli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[test]
fn convert_reports_unsupported_format_cleanly() {
    let input = scratch("assembly.sldasm");
    std::fs::write(&input, b"not really an assembly").unwrap();
    let output = scratch("assembly.solidcraft");

    let out = dutchman(&["convert", input.to_str().unwrap(), output.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(
            "SolidWorks assembly (.sldasm) files are not supported yet. Export a STEP file"
        ),
        "{stderr}"
    );
    assert!(!output.exists(), "no output file is created on failure");
}

#[test]
fn convert_rejects_wrong_output_extension() {
    let out = dutchman(&["convert", "part.f3d", "part.step"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("output must be a .solidcraft file"));
}

#[test]
fn convert_rejects_unknown_input() {
    let input = scratch("notes.txt");
    std::fs::write(&input, b"hello").unwrap();
    let out = dutchman(&["convert", input.to_str().unwrap(), "x.solidcraft"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains(
        "Dutchman does not convert this kind of file. It converts Inventor part (.ipt), Inventor assembly (.iam), Fusion archive (.f3d), Fusion multi-document archive (.f3z) and SolidWorks part (.sldprt) files."
    ));
}

#[test]
fn convert_reports_missing_input() {
    let out = dutchman(&["convert", "/nonexistent/part.f3d", "x.solidcraft"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn missing_arguments_is_a_usage_error() {
    let out = dutchman(&["convert"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn convert_writes_a_solidcraft_design_from_f3d() {
    let input = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../dutchman-core/tests/fixtures/generated_unit_cube.f3d"
    );
    let output = scratch("cube.solidcraft");

    let out = dutchman(&["convert", input, output.to_str().unwrap()]);

    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(std::fs::read(&output).unwrap().starts_with(b"PK\x03\x04"));
}

#[test]
fn convert_leaves_no_file_when_the_input_has_no_geometry() {
    let input = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../dutchman-core/tests/fixtures/container_metadata_only.f3d"
    );
    let output = scratch("empty.solidcraft");

    let out = dutchman(&["convert", input, output.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("no geometry could be read"), "{stderr}");
    assert!(!output.exists(), "no output file is created on failure");
}

#[test]
fn convert_reports_what_became_of_the_history() {
    let input = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../dutchman-core/tests/fixtures/generated_unit_cube.f3d"
    );

    let out = dutchman(&[
        "convert",
        input,
        scratch("flat.solidcraft").to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("history: flattened: the design carries no feature history"),
        "{stderr}"
    );

    let out = dutchman(&[
        "convert",
        "--import-only",
        input,
        scratch("import.solidcraft").to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Info: history: not written (--import-only)"),
        "{stderr}"
    );
}

#[test]
fn solidcraft_and_no_verify_conflict() {
    let out = dutchman(&[
        "convert",
        "--no-verify",
        "--solidcraft",
        "/x",
        "a.f3d",
        "b.solidcraft",
    ]);
    assert_eq!(out.status.code(), Some(2));
}

fn fixture(name: &str) -> String {
    format!(
        "{}/../dutchman-core/tests/fixtures/{name}",
        env!("CARGO_MANIFEST_DIR")
    )
}

fn json_of(out: &Output) -> serde_json::Value {
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn convert_json_reports_what_was_written() {
    let output = scratch("json.solidcraft");
    let out = dutchman(&[
        "convert",
        "--json",
        "--no-verify",
        &fixture("synthetic_cube.ipt"),
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0));
    assert!(out.stderr.is_empty(), "nothing but JSON is printed");
    let report = json_of(&out);
    assert_eq!(report["schema"], "dutchman.convert/1");
    assert_eq!(report["ok"], true);
    assert_eq!(report["input"]["format"], "inventor_part");
    assert_eq!(report["output"]["path"], output.to_str().unwrap());
    assert_eq!(report["output"]["format"], "solidcraft/23");
    assert_eq!(report["mode"], "import");
    assert_eq!(report["verification"]["status"], "not_applicable");
    assert_eq!(report["history"]["total"], 0);
    let loss = &report["losses"][0];
    for key in ["level", "code", "subject", "message"] {
        assert!(loss.get(key).is_some(), "{key}: {loss}");
    }
}

#[test]
fn convert_json_reports_a_failure() {
    let out = dutchman(&[
        "convert",
        "--json",
        &fixture("container_metadata_only.f3d"),
        scratch("never.solidcraft").to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let report = json_of(&out);
    assert_eq!(report["ok"], false);
    assert!(report["error"]["message"]
        .as_str()
        .unwrap()
        .contains("no geometry could be read"));
}

#[test]
fn inspect_json_shows_what_would_map_without_writing() {
    let out = dutchman(&[
        "inspect",
        "--json",
        &fixture("design_sketch_constraints.f3d"),
    ]);
    assert_eq!(out.status.code(), Some(0));
    let report = json_of(&out);
    assert_eq!(report["schema"], "dutchman.inspect/1");
    assert_eq!(report["geometry"]["bodies"], 1);
    assert_eq!(report["mode"], "import");
    assert_eq!(report["verification"]["status"], "flattened");
    assert_eq!(report["history"]["mapped"], 1);
    assert_eq!(report["history"]["total"], 1);
    let feature = &report["history"]["features"][0];
    assert_eq!(feature["kind"], "sketch");
    assert_eq!(feature["status"], "mapped");
}

#[test]
fn inspect_json_names_what_blocks_a_solidworks_history() {
    let out = dutchman(&["inspect", "--json", &fixture("body_history.sldprt")]);
    assert_eq!(out.status.code(), Some(0));
    let report = json_of(&out);
    assert_eq!(report["input"]["format"], "solidworks_part");
    assert_eq!(report["mode"], "import");
    assert_eq!(report["verification"]["status"], "flattened");
    assert_eq!(
        report["verification"]["reason"],
        "feature \"Boss\": unresolved profiles are not mapped yet"
    );
    let features: Vec<_> = report["history"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| (f["kind"].as_str().unwrap(), f["status"].as_str().unwrap()))
        .collect();
    assert_eq!(features, [("extrude", "blocked"), ("native", "blocked")]);
}

#[test]
fn inspect_prints_a_text_summary() {
    let out = dutchman(&["inspect", &fixture("generated_unit_cube.f3d")]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.starts_with("Fusion archive (.f3d): 1 body, 6 faces\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("would write: geometry without history"),
        "{stdout}"
    );
}

#[test]
fn formats_lists_what_converts() {
    let out = dutchman(&["formats", "--json"]);
    assert_eq!(out.status.code(), Some(0));
    let report = json_of(&out);
    assert_eq!(report["schema"], "dutchman.formats/1");
    let fusion = report["formats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "fusion_archive")
        .unwrap();
    assert_eq!(fusion["status"], "converts");
    assert_eq!(fusion["extension"], "f3d");
    let solidworks = report["formats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "solidworks_part")
        .unwrap();
    assert_eq!(solidworks["status"], "converts");

    let text = dutchman(&["formats"]);
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.contains("Inventor assembly (.iam)"), "{stdout}");
}

#[test]
fn a_multi_document_archive_converts_like_a_fusion_archive() {
    let input = scratch("design.f3z");
    std::fs::copy(fixture("generated_unit_cube.f3d"), &input).unwrap();
    let output = scratch("from_f3z.solidcraft");
    let out = dutchman(&[
        "convert",
        "--json",
        "--no-verify",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
    ]);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    assert_eq!(json_of(&out)["input"]["format"], "fusion_multi_document");

    let noise = scratch("noise.f3z");
    std::fs::write(&noise, [0xffu8; 512]).unwrap();
    let out = dutchman(&["convert", noise.to_str().unwrap(), output.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot read this design"));
}

#[test]
fn convert_all_converts_a_folder_and_reports_each_file() {
    let root = std::env::temp_dir().join(format!("dutchman-batch-{}", std::process::id()));
    let project = root.join("project");
    let nested = project.join("parts");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::copy(fixture("generated_unit_cube.f3d"), project.join("cube.f3d")).unwrap();
    std::fs::copy(fixture("synthetic_cube.ipt"), nested.join("block.ipt")).unwrap();
    std::fs::write(nested.join("broken.f3d"), [0xffu8; 256]).unwrap();
    std::fs::write(project.join("notes.txt"), b"not a design").unwrap();
    let out_dir = root.join("out");
    let missing = root.join("missing.f3d");

    let out = dutchman(&[
        "convert-all",
        "--json",
        "--no-verify",
        "--jobs",
        "2",
        "--out-dir",
        out_dir.to_str().unwrap(),
        project.to_str().unwrap(),
        missing.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1), "a failure makes the run fail");
    let report = json_of(&out);
    assert_eq!(report["schema"], "dutchman.convert-all/1");
    assert_eq!(report["ok"], false);
    assert_eq!(report["converted"], 2);
    assert_eq!(report["failed"], 2);
    let results = report["results"].as_array().unwrap();
    assert_eq!(
        results.len(),
        4,
        "the text file is not a design and is skipped"
    );
    for result in results {
        assert_eq!(result["schema"], "dutchman.convert/1");
    }
    assert!(out_dir.join("cube.solidcraft").is_file());
    assert!(out_dir.join("parts").join("block.solidcraft").is_file());
    assert!(!out_dir.join("parts").join("broken.solidcraft").exists());

    let text = dutchman(&[
        "convert-all",
        "--no-verify",
        "--out-dir",
        out_dir.to_str().unwrap(),
        project.join("cube.f3d").to_str().unwrap(),
    ]);
    assert_eq!(text.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.ends_with("1 converted, 0 failed\n"), "{stdout}");
}
