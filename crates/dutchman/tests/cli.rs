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

#[cfg(feature = "pilot")]
#[test]
fn pilot_help_names_the_embedded_command() {
    for args in [vec!["pilot", "--help"], vec!["pilot"]] {
        let out = dutchman(&args);
        assert!(out.status.success());
        let help = String::from_utf8_lossy(&out.stdout);
        assert!(help.contains("Usage:\n  dutchman pilot how"), "{help}");
        assert!(!help.contains("dutchman-pilot how"), "{help}");
    }
    let out = dutchman(&["pilot", "model", "--help"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Usage: dutchman pilot model"));
}

#[cfg(feature = "pilot")]
#[test]
fn pilot_dispatches_and_reports_errors() {
    let out = dutchman(&["pilot", "how", "round all edges 3 mm"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("solid.fillet"));

    let dir = tempfile::tempdir().unwrap();
    let missing_log = dir.path().join("missing.jsonl");
    let out = dutchman(&["pilot", "log-summary", missing_log.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).starts_with("dutchman: "));

    let out = dutchman(&["pilot", "model", "--upstream-timeout", "0"]);
    assert_eq!(out.status.code(), Some(2));
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(error.contains("--upstream-timeout"), "{error}");
    assert!(error.contains("1..=3600"), "{error}");
}

#[cfg(not(feature = "pilot"))]
#[test]
fn pilot_is_absent_without_the_feature() {
    let out = dutchman(&["--help"]);
    assert!(out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("pilot"));
    let out = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(["pilot", "--help"])
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8(out.stderr).unwrap(), "dutchman: `dutchman pilot` is not installed; get it at https://github.com/dutchman-cad/pilot\n");
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
    let converts = if cfg!(feature = "freecad") {
        "Inventor part (.ipt), Inventor assembly (.iam), Fusion archive (.f3d), Fusion \
         multi-document archive (.f3z), SolidWorks part (.sldprt) and FreeCAD document (.FCStd)"
    } else {
        "Inventor part (.ipt), Inventor assembly (.iam), Fusion archive (.f3d), Fusion \
         multi-document archive (.f3z) and SolidWorks part (.sldprt)"
    };
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&format!(
            "Dutchman does not convert this kind of file. It converts {converts} files."
        )),
        "{stderr}"
    );
}

#[test]
fn convert_reports_missing_input() {
    let out = dutchman(&["convert", "/nonexistent/part.f3d", "x.solidcraft"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn an_unreadable_archive_suggests_recovery_in_text_and_json() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("incomplete.f3d");
    let output = dir.path().join("out.solidcraft");
    std::fs::write(&input, b"not an archive").unwrap();
    let args = [input.to_str().unwrap(), output.to_str().unwrap()];
    let text = dutchman(&["convert", args[0], args[1]]);
    assert_eq!(text.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&text.stderr);
    assert!(stderr.contains("download or copy it again"), "{stderr}");
    assert!(stderr.contains("check its extension"), "{stderr}");
    assert!(
        stderr.contains("template=file-did-not-convert.yml"),
        "{stderr}"
    );
    let json = dutchman(&["convert", "--json", args[0], args[1]]);
    assert_eq!(json.status.code(), Some(1));
    assert!(json.stderr.is_empty());
    let report = json_of(&json);
    assert_eq!(report["error"]["stage"], "decode");
    assert_eq!(report["error"]["upstream"], "cadmpeg");
    let message = report["error"]["message"].as_str().unwrap();
    assert_eq!(stderr.trim(), format!("dutchman: {message}"));
    assert!(!output.exists());
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
    // Its one body is a sketch's, which no conversion can write; inspect says so.
    assert_eq!(
        report["conversion"],
        serde_json::json!({"possible": false,
            "reason": "the model has no solids or surfaces SolidCraft can import"})
    );
    let text = dutchman(&["inspect", &fixture("design_sketch_constraints.f3d")]);
    assert!(String::from_utf8_lossy(&text.stdout).contains(
        "would write: nothing; converting it fails: the model has no solids or surfaces"
    ));
    let cube = json_of(&dutchman(&[
        "inspect",
        "--json",
        &fixture("generated_unit_cube.f3d"),
    ]));
    assert_eq!(
        cube["conversion"],
        serde_json::json!({"possible": true, "reason": null})
    );
}

#[test]
fn inspect_json_names_what_blocks_a_solidworks_history() {
    let out = dutchman(&["inspect", "--json", &fixture("body_history.sldprt")]);
    assert_eq!(out.status.code(), Some(0));
    let report = json_of(&out);
    assert_eq!(report["input"]["format"], "solidworks_part");
    assert_eq!(
        report["geometry"],
        serde_json::json!({"bodies": 1, "faces": 1})
    );
    assert_eq!(
        report["conversion"],
        serde_json::json!({"possible": false,
            "reason": "the model has no solids or surfaces SolidCraft can import"})
    );
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
        .map(|f| {
            (
                f["name"].as_str().unwrap(),
                f["kind"].as_str().unwrap(),
                f["status"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        features,
        [
            ("Boss", "extrude", "blocked"),
            ("Profile", "native", "blocked")
        ]
    );
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
fn formats_text_matches_guide() {
    let out = dutchman(&["formats"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    let guide = include_str!("../../../docs/guide.md");
    let section = guide
        .split_once("## Formats")
        .unwrap()
        .1
        .split("\n## ")
        .next()
        .unwrap();
    let listing = section
        .split_once("```text")
        .unwrap()
        .1
        .split_once("```")
        .unwrap()
        .0
        .trim();
    // Ignore column padding, and the FreeCAD line in a build without FreeCAD: the guide shows
    // the default build.
    let lines = |listing: &str| {
        listing
            .lines()
            .filter(|line| cfg!(feature = "freecad") || !line.starts_with("FreeCAD document "))
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        lines(&text),
        lines(listing),
        "Update the Formats listing in docs/guide.md to match dutchman formats"
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
    assert_eq!(report["skipped"], 1, "notes.txt");
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
    // The whole text report, every loss in full, beside the designs.
    let log = std::fs::read_to_string(out_dir.join("dutchman.log")).unwrap();
    assert!(
        log.contains("converted") && log.contains("cube.f3d"),
        "{log}"
    );
    assert!(log.contains("missing.f3d: no such file or folder"), "{log}");
    assert!(log.ends_with("2 converted, 2 failed, 1 skipped\n"), "{log}");
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
    // Each converted file's losses are listed beneath it, as `convert` prints them.
    let lines: Vec<&str> = stdout.lines().collect();
    assert!(lines[0].starts_with("converted  "), "{stdout}");
    assert!(
        lines[1..lines.len() - 1]
            .iter()
            .all(|line| line.starts_with("           ") && line.contains(": ")),
        "{stdout}"
    );
    assert!(
        stdout.contains("Degraded: f3d/material.not-transferred"),
        "{stdout}"
    );

    // A folder with a file Dutchman does not know says so in the counts.
    let text = dutchman(&[
        "convert-all",
        "--no-verify",
        "--out-dir",
        out_dir.to_str().unwrap(),
        nested.to_str().unwrap(),
        project
            .join("notes.txt")
            .parent()
            .unwrap()
            .join("cube.f3d")
            .to_str()
            .unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.ends_with("2 converted, 1 failed\n"), "{stdout}");
    let text = dutchman(&[
        "convert-all",
        "--no-verify",
        "--out-dir",
        out_dir.to_str().unwrap(),
        project.to_str().unwrap(),
    ]);
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(
        stdout.ends_with("2 converted, 1 failed, 1 skipped (not in a format Dutchman knows)\n"),
        "{stdout}"
    );
}

#[test]
fn convert_says_what_it_wrote_and_cuts_long_notes_short() {
    let output = scratch("plain.solidcraft");
    let cube = fixture("solidworks_cube.sldprt");
    let out = dutchman(&["convert", "--no-verify", &cube, output.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        stdout.trim_end(),
        format!(
            "converted  {cube} -> {} (geometry without history)",
            output.display()
        )
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(
            "step/native.passthrough-record-omitted: 2 uninterpreted passthrough record(s)"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains("step/native.source-record-omitted: 1 source-native record(s)"),
        "{stderr}"
    );
    assert!(
        stderr.contains("so no declared identity was verified. …\n"),
        "{stderr}"
    );
    assert!(!stderr.contains("Agreement is consistency"), "{stderr}");
    assert!(
        stderr.ends_with("--verbose prints them in full)\n"),
        "{stderr}"
    );

    let out = dutchman(&[
        "convert",
        "--no-verify",
        "--verbose",
        &cube,
        output.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Agreement is consistency, not a declaration."),
        "{stderr}"
    );
    assert!(!stderr.contains("--verbose prints"), "{stderr}");
}

#[test]
fn no_clobber_keeps_an_existing_design() {
    let dir = std::env::temp_dir().join(format!("dutchman-noclobber-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let output = dir.join("cube.solidcraft");
    std::fs::write(&output, b"SENTINEL").unwrap();
    let cube = fixture("synthetic_cube.ipt");

    let out = dutchman(&[
        "convert",
        "--no-verify",
        "--no-clobber",
        &cube,
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("already exists"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(std::fs::read(&output).unwrap(), b"SENTINEL");

    // Without it, the existing file is replaced, as `--help` says.
    let out = dutchman(&["convert", "--no-verify", &cube, output.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(0));
    assert_ne!(std::fs::read(&output).unwrap(), b"SENTINEL");
    let help = dutchman(&["convert", "--help"]);
    assert!(String::from_utf8_lossy(&help.stdout).contains("An explicit output is replaced"));

    // convert-all fails that one file and converts the rest.
    let out_dir = dir.join("all");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join("synthetic_cube.solidcraft"), b"SENTINEL").unwrap();
    let out = dutchman(&[
        "convert-all",
        "--json",
        "--no-verify",
        "--no-clobber",
        "--out-dir",
        out_dir.to_str().unwrap(),
        &cube,
        &fixture("generated_unit_cube.f3d"),
    ]);
    let report = json_of(&out);
    assert_eq!(report["converted"], 1);
    assert_eq!(report["failed"], 1);
    assert_eq!(
        std::fs::read(out_dir.join("synthetic_cube.solidcraft")).unwrap(),
        b"SENTINEL"
    );
    assert!(out_dir.join("generated_unit_cube.solidcraft").is_file());
}

/// A library whose folder, files and parts carry names that must not reach a shared report.
fn private_library(root: &std::path::Path) -> PathBuf {
    let library = root.join("AcmeClient_Confidential");
    let nested = library.join("Prototype_Gearbox_X93");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::copy(
        fixture("generated_unit_cube.f3d"),
        library.join("Housing_Rev7_Secret.f3d"),
    )
    .unwrap();
    std::fs::copy(
        fixture("synthetic_cube.ipt"),
        nested.join("Bracket_K42.ipt"),
    )
    .unwrap();
    std::fs::write(nested.join("Broken_Widget_Q8.f3d"), [0xffu8; 256]).unwrap();
    std::fs::write(library.join("pricing_notes.txt"), b"not a design").unwrap();
    library
}

#[test]
fn the_anonymous_report_names_no_file_and_matches_its_ids_to_the_files() {
    let root = std::env::temp_dir().join(format!("dutchman-anon-{}", std::process::id()));
    let library = private_library(&root);
    let out_dir = root.join("Converted_For_AcmeClient");
    let anon = root.join("report-anon.json");
    let key = root.join("report-key.json");
    let missing = library.join("Missing_Part_Z5.f3d");

    let out = dutchman(&[
        "convert-all",
        "--no-verify",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--report-anon",
        anon.to_str().unwrap(),
        "--report-key",
        key.to_str().unwrap(),
        library.to_str().unwrap(),
        missing.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    let salt = stderr
        .split("the salt ")
        .nth(1)
        .and_then(|rest| rest.get(..32))
        .unwrap()
        .to_owned();

    let text = std::fs::read_to_string(&anon).unwrap();
    for secret in [
        root.to_str().unwrap(),
        "AcmeClient",
        "Confidential",
        "Prototype_Gearbox",
        "Gearbox",
        "Housing_Rev7",
        "Secret",
        "Bracket_K42",
        "Broken_Widget",
        "Missing_Part",
        "pricing",
        "Converted_For",
        &salt,
    ] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    let report: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(report["schema"], "dutchman.report-anon/1");
    assert_eq!(report["totals"]["files"], 4);
    assert_eq!(report["totals"]["converted"], 2);
    assert_eq!(report["totals"]["failed"], 2);
    assert_eq!(report["totals"]["skipped"], 1);
    assert_eq!(report["skipped_by_extension"]["txt"], 1);
    assert_eq!(report["formats"]["fusion_archive"]["files"], 3);
    assert_eq!(report["formats"]["inventor_part"]["converted"], 1);
    assert_eq!(report["stages"]["decode"]["failed"], 1, "{report}");
    assert_eq!(report["stages"]["input"]["failed"], 1, "{report}");
    assert!(report["versions"]["dutchman"].is_string());
    assert!(report["versions"]["cadmpeg"]
        .as_str()
        .unwrap()
        .starts_with("0."));
    assert_eq!(
        report["timing"]["count"], 3,
        "the missing file was never timed"
    );
    assert!(report["losses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|loss| loss["code"] == "f3d/material.not-transferred" && loss["stage"] == "decode"));

    // The key, kept by whoever ran the batch, says which file each id is; the same salt
    // gives the same ids again.
    let key: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&key).unwrap()).unwrap();
    assert_eq!(key["salt"], salt.as_str());
    let failed = report["errors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|error| error["stage"] == "decode")
        .unwrap();
    let id = failed["file_ids"][0].as_str().unwrap();
    assert!(key["files"][id]
        .as_str()
        .unwrap()
        .ends_with("Broken_Widget_Q8.f3d"));
    let again = root.join("again.json");
    dutchman(&[
        "convert-all",
        "--no-verify",
        "--out-dir",
        out_dir.to_str().unwrap(),
        "--report-anon",
        again.to_str().unwrap(),
        "--report-salt",
        &salt,
        library.to_str().unwrap(),
    ]);
    let again = std::fs::read_to_string(&again).unwrap();
    assert!(again.contains(id), "{again}");
}

#[test]
fn a_journal_lets_a_stopped_run_carry_on_without_converting_files_again() {
    let root = std::env::temp_dir().join(format!("dutchman-journal-{}", std::process::id()));
    let library = private_library(&root);
    let out_dir = root.join("out");
    let journal = root.join("journal.jsonl");
    let run = || {
        json_of(&dutchman(&[
            "convert-all",
            "--json",
            "--no-verify",
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--journal",
            journal.to_str().unwrap(),
            library.to_str().unwrap(),
        ]))
    };
    let first = run();
    assert_eq!(first["resumed"], 0);
    assert_eq!(first["converted"], 2);
    assert_eq!(
        std::fs::read_to_string(&journal).unwrap().lines().count(),
        3
    );
    let second = run();
    assert_eq!(second["resumed"], 3, "{second}");
    assert_eq!(second["converted"], 2);
    assert_eq!(second["failed"], 1);

    // A design removed since is made again; a changed file is converted again.
    std::fs::remove_file(out_dir.join("Housing_Rev7_Secret.solidcraft")).unwrap();
    let third = run();
    assert_eq!(third["resumed"], 2, "{third}");
    assert!(out_dir.join("Housing_Rev7_Secret.solidcraft").is_file());
}

#[test]
fn a_file_over_its_time_or_memory_is_stopped_and_the_batch_carries_on() {
    let root = std::env::temp_dir().join(format!("dutchman-limits-{}", std::process::id()));
    let library = private_library(&root);
    let out = dutchman(&[
        "convert-all",
        "--json",
        "--no-verify",
        "--max-memory",
        "1",
        "--out-dir",
        root.join("out").to_str().unwrap(),
        library.to_str().unwrap(),
    ]);
    let report = json_of(&out);
    // Every file still has a report: a converter stopped for its memory fails alone.
    assert_eq!(report["results"].as_array().unwrap().len(), 3, "{report}");
    for result in report["results"].as_array().unwrap() {
        if let Some(message) = result["error"]["message"].as_str() {
            if message.starts_with("stopped when it held") {
                assert_ne!(result["error"]["stage"], serde_json::Value::Null);
            }
        }
    }
}

#[cfg(feature = "freecad")]
#[test]
fn a_freecad_document_converts() {
    let out_dir = tempfile::tempdir().unwrap();
    let output = out_dir.path().join("product.solidcraft");
    let out = dutchman(&[
        "convert",
        "--json",
        "--no-verify",
        &fixture("core_design_product.FCStd"),
        output.to_str().unwrap(),
    ]);
    let report = json_of(&out);
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["input"]["format"], "freecad_document");
    assert!(output.is_file());
    let formats = json_of(&dutchman(&["formats", "--json"]));
    let freecad = formats["formats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "freecad_document")
        .unwrap();
    assert_eq!(freecad["status"], "converts");
}

#[cfg(not(feature = "freecad"))]
#[test]
fn a_build_without_freecad_says_where_freecad_files_convert() {
    let out_dir = tempfile::tempdir().unwrap();
    let output = out_dir.path().join("product.solidcraft");
    let out = dutchman(&[
        "convert",
        "--json",
        &fixture("core_design_product.FCStd"),
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let report = json_of(&out);
    assert_eq!(
        report["error"]["message"],
        "this build leaves FreeCAD files out (built without the `freecad` feature); \
         Dutchman's release builds convert them: https://github.com/dutchman-cad/dutchman/releases"
    );
    assert_eq!(report["error"]["upstream"], serde_json::Value::Null);
    assert!(!output.exists());
    let text = String::from_utf8(dutchman(&["formats"]).stdout).unwrap();
    let line = text.lines().find(|l| l.starts_with("FreeCAD")).unwrap();
    assert!(
        line.contains("not yet") && line.contains("left out of this build"),
        "{line}"
    );
}

#[test]
fn freecad_macro_output_is_available_without_freecad_input_support() {
    let dir = tempfile::tempdir().unwrap();
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
    let output = dir.path().join("out.FCMacro");
    let result = dutchman(&[
        "convert",
        input.to_str().unwrap(),
        "--to",
        "freecad",
        "-o",
        output.to_str().unwrap(),
        "--json",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["output"]["format"], "freecad-macro/1");
    let script = std::fs::read_to_string(output).unwrap();
    assert!(script.contains("original_step("));
    assert!(script.contains("ISO-10303-21"));
}

#[test]
fn unknown_tool_explains_where_to_install_it() {
    let out = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("unknown-tool-for-test")
        .env("PATH", "")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8(out.stderr).unwrap(), "dutchman: `dutchman unknown-tool-for-test` is not installed; get it at https://github.com/dutchman-cad/unknown-tool-for-test\n");
}

#[test]
fn builtin_typos_keep_clap_suggestions() {
    let out = dutchman(&["conver"]);
    assert_eq!(out.status.code(), Some(2));
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("similar subcommand"), "{text}");
    assert!(text.contains("convert"), "{text}");
}

#[cfg(unix)]
mod external_tools {
    use super::*;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Stdio;

    fn script(dir: &std::path::Path, name: &str, body: &str) {
        let path = dir.join(format!("dutchman-{name}"));
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn dispatch_preserves_stdin_arguments_and_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        script(
            dir.path(),
            "foo",
            "printf '<%s>\\n' \"$@\"; IFS= read -r line; printf 'stdin:%s\\n' \"$line\"; exit 7",
        );
        let mut child = Command::new(env!("CARGO_BIN_EXE_dutchman"))
            .args(["foo", "a", "b c"])
            .env("PATH", dir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"hello world\n")
            .unwrap();
        let out = child.wait_with_output().unwrap();
        assert_eq!(out.status.code(), Some(7));
        assert_eq!(out.stdout, b"<a>\n<b c>\nstdin:hello world\n");
    }

    #[test]
    fn installed_pilot_accepts_its_own_flags_before_clap() {
        let dir = tempfile::tempdir().unwrap();
        script(
            dir.path(),
            "pilot",
            "printf 'installed Pilot'; printf ' <%s>' \"$@\"; printf '\\n'",
        );
        for args in [
            vec!["pilot", "--version"],
            vec!["--verbose", "pilot", "--own-flag", "b c"],
        ] {
            let out = Command::new(env!("CARGO_BIN_EXE_dutchman"))
                .args(&args)
                .env("PATH", dir.path())
                .output()
                .unwrap();
            assert!(out.status.success());
            let expected = if args.len() == 2 {
                "installed Pilot <--version>\n"
            } else {
                "installed Pilot <--own-flag> <b c>\n"
            };
            assert_eq!(out.stdout, expected.as_bytes());
        }
    }

    #[test]
    fn adjacent_tool_is_preferred_and_help_lists_it() {
        let adjacent = tempfile::tempdir().unwrap();
        let path = tempfile::tempdir().unwrap();
        let binary = adjacent.path().join("dutchman");
        std::fs::copy(env!("CARGO_BIN_EXE_dutchman"), &binary).unwrap();
        script(adjacent.path(), "foo", "echo adjacent-1.0");
        script(path.path(), "foo", "echo path-2.0");
        let out = Command::new(&binary)
            .arg("foo")
            .env("PATH", path.path())
            .output()
            .unwrap();
        assert_eq!(out.stdout, b"adjacent-1.0\n");
        for args in [vec![], vec!["--help"]] {
            let out = Command::new(&binary)
                .args(args)
                .env("PATH", path.path())
                .output()
                .unwrap();
            assert!(out.status.success());
            let text = String::from_utf8_lossy(&out.stdout);
            assert!(
                text.contains("Installed Dutchman CAD tools:\n  foo  adjacent-1.0"),
                "{text}"
            );
            assert!(!text.contains("path-2.0"), "{text}");
        }
    }

    #[test]
    fn non_utf8_arguments_pass_through() {
        use std::os::unix::ffi::OsStringExt;
        let dir = tempfile::tempdir().unwrap();
        script(dir.path(), "foo", "printf '%s' \"$1\"");
        let out = Command::new(env!("CARGO_BIN_EXE_dutchman"))
            .arg("foo")
            .arg(std::ffi::OsString::from_vec(vec![0xff, b'a']))
            .env("PATH", dir.path())
            .output()
            .unwrap();
        assert_eq!(out.stdout, [0xff, b'a']);
    }

    #[test]
    fn resolution_prefers_builtins_then_installed_tools_then_files() {
        let dir = tempfile::tempdir().unwrap();
        script(dir.path(), "formats", "echo external-formats");
        script(dir.path(), "foo", "echo external-foo");
        std::fs::write(dir.path().join("foo"), b"sentinel").unwrap();
        let run = |args: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_dutchman"))
                .current_dir(dir.path())
                .env("PATH", dir.path())
                .args(args)
                .output()
                .unwrap()
        };
        let out = run(&["formats"]);
        assert!(out.status.success());
        assert!(!String::from_utf8_lossy(&out.stdout).contains("external-formats"));
        assert_eq!(run(&["foo"]).stdout, b"external-foo\n");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../dutchman-core/tests/fixtures/generated_unit_cube.f3d"
            ),
            dir.path().join("pilot"),
        )
        .unwrap();
        let out = run(&["convert", "pilot", "pilot.solidcraft", "--no-verify"]);
        assert_eq!(out.status.code(), Some(1));
        assert!(String::from_utf8_lossy(&out.stderr).contains("does not convert this kind of file"));
    }

    #[test]
    fn installed_tool_with_builtin_typo_name_runs() {
        let dir = tempfile::tempdir().unwrap();
        script(dir.path(), "conver", "echo installed");
        let out = Command::new(env!("CARGO_BIN_EXE_dutchman"))
            .arg("conver")
            .env("PATH", dir.path())
            .output()
            .unwrap();
        assert!(out.status.success());
        assert_eq!(out.stdout, b"installed\n");
    }
}

#[cfg(feature = "pilot")]
#[test]
fn pilot_version_falls_back_to_builtin() {
    let out = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(["pilot", "--version"])
        .env("PATH", "")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("0.1.0-beta.1"));
}

#[test]
fn bare_and_explicit_conversion_derive_and_protect_default_outputs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../dutchman-core/tests/fixtures/generated_unit_cube.f3d"
        ),
        dir.path().join("part.f3d"),
    )
    .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_dutchman"))
            .current_dir(dir.path())
            .args(args)
            .env("PATH", "")
            .output()
            .unwrap()
    };
    for prefix in [vec![], vec!["convert"], vec!["--verbose"]] {
        let mut args = prefix;
        args.extend(["part.f3d", "--no-verify"]);
        let out = run(&args);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let output = dir.path().join("part.solidcraft");
        assert!(std::fs::read(&output).unwrap().starts_with(b"PK"));
        std::fs::remove_file(output).unwrap();
    }
    let output = dir.path().join("part.solidcraft");
    std::fs::write(&output, b"sentinel").unwrap();
    let modified = output.metadata().unwrap().modified().unwrap();
    let out = run(&["part.f3d"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(out.stderr, b"dutchman: part.solidcraft already exists; choose another output with -o PATH, or replace it with --force\n");
    assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
    assert_eq!(output.metadata().unwrap().modified().unwrap(), modified);
    assert!(run(&["part.f3d", "--force", "--no-verify"])
        .status
        .success());
    assert_ne!(std::fs::read(&output).unwrap(), b"sentinel");
    std::fs::write(&output, b"sentinel").unwrap();
    assert!(run(&["part.f3d", "part.solidcraft", "--no-verify"])
        .status
        .success());
    assert_ne!(std::fs::read(&output).unwrap(), b"sentinel");
    assert_eq!(
        run(&["part.f3d", "--force", "--no-clobber"]).status.code(),
        Some(2)
    );
    assert!(run(&["part.f3d", "-o", "other.solidcraft", "--no-verify"])
        .status
        .success());
}

#[test]
fn default_output_extensions_and_input_protection() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_dutchman"))
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap()
    };
    std::fs::write(dir.path().join("part.FCStd"), b"sentinel").unwrap();
    for force in [vec![], vec!["--force"]] {
        let mut args = vec!["convert", "part.FCStd", "--to", "fcstd"];
        args.extend(force);
        let out = run(&args);
        assert_eq!(out.status.code(), Some(2));
        assert!(String::from_utf8_lossy(&out.stderr).contains("is the input file"));
        assert_eq!(
            std::fs::read(dir.path().join("part.FCStd")).unwrap(),
            b"sentinel"
        );
    }
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../dutchman-core/tests/fixtures/generated_unit_cube.f3d"
        ),
        dir.path().join("part.F3D"),
    )
    .unwrap();
    assert!(run(&["part.F3D", "--to", "freecad"]).status.success());
    assert!(dir.path().join("part.FCMacro").is_file());
    for ext in ["step", "StP", "IPT", "f3z", "sldprt"] {
        let out = run(&[&format!("missing.{ext}")]);
        assert_eq!(out.status.code(), Some(1));
        assert!(!String::from_utf8_lossy(&out.stderr).contains("is not installed"));
    }
}

#[test]
fn builtin_command_wins_over_a_same_named_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("formats"), b"sentinel").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .current_dir(dir.path())
        .arg("formats")
        .output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Fusion"));
}
