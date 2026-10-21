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
    assert!(String::from_utf8_lossy(&help.stdout).contains("An existing file is replaced"));

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
