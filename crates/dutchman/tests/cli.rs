//! End-to-end checks of the `dutchman` binary.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::PathBuf;
use std::process::{Child, Command, Output, Stdio};
use std::sync::Mutex;

// A fork must not inherit a write descriptor for an executable another test will run.
// Hold this through each spawn (which waits for exec) and each executable publication.
static EXECUTABLE_PUBLICATION: Mutex<()> = Mutex::new(());

trait TestCommand {
    fn test_spawn(&mut self) -> std::io::Result<Child>;
    fn test_output(&mut self) -> std::io::Result<Output>;
}

impl TestCommand for Command {
    fn test_spawn(&mut self) -> std::io::Result<Child> {
        let mut retries = 0;
        loop {
            let result = {
                let _guard = EXECUTABLE_PUBLICATION.lock().unwrap();
                self.spawn()
            };
            match result {
                Err(error)
                    if error.kind() == std::io::ErrorKind::ExecutableFileBusy && retries < 5 =>
                {
                    retries += 1;
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                result => return result,
            }
        }
    }

    fn test_output(&mut self) -> std::io::Result<Output> {
        self.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .test_spawn()?
            .wait_with_output()
    }
}

fn dutchman(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(args)
        .test_output()
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
        .test_output()
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
        stderr.contains("reading geometry is not supported yet. Export a STEP file"),
        "{stderr}"
    );
    assert!(!output.exists(), "no output file is created on failure");
}

#[test]
fn iges_is_refused_before_decoding_in_text_and_json() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("out.solidcraft");
    let reason =
        "Dutchman doesn't convert IGES yet: open it in SolidCraft directly (File > Import).";
    for suffix in ["igs", "iges", "IGS", "IGES"] {
        let input = dir.path().join(format!("design.{suffix}"));
        std::fs::write(&input, b"not IGES data").unwrap();
        for command in ["convert", "inspect"] {
            for json in [false, true] {
                let mut args = vec![command, input.to_str().unwrap()];
                if command == "convert" {
                    args.push(output.to_str().unwrap());
                }
                if json {
                    args.push("--json");
                }
                let out = dutchman(&args);
                assert_eq!(out.status.code(), Some(1));
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                if json {
                    let report = json_of(&out);
                    assert_eq!(report["ok"], false);
                    assert_eq!(report["input"]["format"], "iges");
                    assert_eq!(report["input"]["extension"], suffix);
                    assert_eq!(report["input"]["words"], "open it in SolidCraft directly");
                    assert_eq!(report["error"]["message"], reason);
                    assert_eq!(report["error"]["stage"], "decode");
                    assert_eq!(report["error"]["upstream"], serde_json::Value::Null);
                } else {
                    assert_text_error_with_progress(&out, &input, &format!("dutchman: {reason}"));
                    if command == "convert" {
                        assert_eq!(
                            stdout,
                            format!("IGES .{suffix}: open it in SolidCraft directly\n")
                        );
                    } else {
                        assert!(stdout.is_empty());
                    }
                }
                assert!(!stdout.contains("Want to help"));
                assert!(!stderr.contains("Want to help"));
                assert!(!output.exists());
            }
        }
    }
}

#[test]
fn batch_refuses_iges_without_inviting_test_files_and_continues_other_formats() {
    let dir = tempfile::tempdir().unwrap();
    let inputs = dir.path().join("inputs");
    let outputs = dir.path().join("outputs");
    std::fs::create_dir(&inputs).unwrap();
    let first = inputs.join("first.igs");
    let second = inputs.join("second.iges");
    for path in [&first, &second] {
        std::fs::write(path, b"not IGES data").unwrap();
    }
    let reason =
        "Dutchman doesn't convert IGES yet: open it in SolidCraft directly (File > Import).";
    for directory in [false, true] {
        for json in [false, true] {
            let mut args = vec!["convert-all", "--out-dir", outputs.to_str().unwrap()];
            if directory {
                args.push(inputs.to_str().unwrap());
            } else {
                args.extend([first.to_str().unwrap(), second.to_str().unwrap()]);
            }
            if json {
                args.push("--json");
            }
            let out = dutchman(&args);
            assert_eq!(out.status.code(), Some(1));
            if json {
                let report = json_of(&out);
                assert_eq!(report["converted"], 0);
                let results = report["results"].as_array().unwrap();
                assert_eq!(results.len(), 2);
                for result in results {
                    assert_eq!(result["ok"], false);
                    assert_eq!(result["input"]["format"], "iges");
                    assert_eq!(result["error"]["message"], reason);
                }
            } else {
                let text = String::from_utf8_lossy(&out.stdout);
                assert_eq!(text.matches(reason).count(), 2);
                assert!(!text.contains("Want to help"));
            }
            assert!(!outputs.join("first.sc").exists());
            assert!(!outputs.join("second.sc").exists());
        }
    }
    std::fs::copy(fixture("generated_unit_cube.f3d"), inputs.join("cube.f3d")).unwrap();
    let out = dutchman(&[
        "convert-all",
        inputs.to_str().unwrap(),
        "--out-dir",
        outputs.to_str().unwrap(),
        "--no-verify",
        "--json",
    ]);
    let report = json_of(&out);
    assert_eq!(report["converted"], 1);
    assert_eq!(report["failed"], 2);
    assert!(outputs.join("cube.sc").is_file());
}

#[test]
fn convert_rejects_wrong_output_extension() {
    let out = dutchman(&["convert", "part.f3d", "part.step"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("output must be a .sc file"));
}

#[test]
fn convert_rejects_unknown_input() {
    let input = scratch("notes.txt");
    std::fs::write(&input, b"hello").unwrap();
    let out = dutchman(&["convert", input.to_str().unwrap(), "x.solidcraft"]);
    assert_eq!(out.status.code(), Some(1));
    let converts = dutchman_core::Format::supported_names();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains(&format!(
            "Dutchman does not convert this kind of file. It converts {converts}."
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
    assert_progress_json(&json);
    let report = json_of(&json);
    assert_eq!(report["error"]["stage"], "decode");
    assert_eq!(report["error"]["upstream"], "cadmpeg");
    let message = report["error"]["message"].as_str().unwrap();
    assert_text_error_with_progress(&text, &input, &format!("dutchman: {message}"));
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
    assert_progress_json(&out);
    let report = json_of(&out);
    assert_eq!(report["schema"], "dutchman.convert/1");
    assert_eq!(report["ok"], true);
    assert_eq!(report["input"]["format"], "inventor_part");
    assert_eq!(
        report["input"]["words"],
        dutchman_core::Format::InventorPart.words()
    );
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
fn formats_markdown_matches_the_formats_page() {
    let out = dutchman(&["formats", "--markdown"]);
    assert!(out.status.success(), "{out:?}");
    let table = String::from_utf8(out.stdout).unwrap();
    let page = include_str!("../../../docs/formats.md");
    let block = page
        .split_once("<!-- formats: generated from dutchman formats --markdown -->")
        .unwrap()
        .1
        .split_once("<!-- /formats -->")
        .unwrap()
        .0;
    // Ignore padding, and the FreeCAD row in a build without FreeCAD: the page shows the
    // default build.
    let rows = |text: &str| {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .filter(|line| cfg!(feature = "freecad") || !line.starts_with("| FreeCAD document |"))
            .map(|line| line.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        rows(&table),
        rows(block),
        "Update the table in docs/formats.md: dutchman formats --markdown"
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
    assert_eq!(fusion["level"], "L5");
    let unclassified = report["formats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "unclassified_part")
        .unwrap();
    assert_eq!(unclassified.get("level"), Some(&serde_json::Value::Null));
    assert_eq!(fusion["extension"], "f3d");
    let solidworks = report["formats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "solidworks_part")
        .unwrap();
    assert_eq!(solidworks["status"], "converts");

    let iges = report["formats"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == "iges")
        .unwrap();
    assert_eq!(iges["status"], "not_supported");
    assert_eq!(iges["words"], "open it in SolidCraft directly");
    assert_eq!(
        iges["converts"],
        "SolidCraft imports IGES itself (File > Import)"
    );

    let text = dutchman(&["formats"]);
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.contains("Inventor assembly (.iam)"), "{stdout}");
    assert!(stdout.contains("IGES (.igs, .iges)  open it in SolidCraft directly  L9  not yet: SolidCraft imports IGES itself (File > Import)"), "{stdout}");
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
    assert!(out_dir.join("cube.sc").is_file());
    assert!(out_dir.join("parts").join("block.sc").is_file());
    // The whole text report, every loss in full, beside the designs.
    let log = std::fs::read_to_string(out_dir.join("dutchman.log")).unwrap();
    assert!(
        log.contains("converted") && log.contains("cube.f3d"),
        "{log}"
    );
    assert!(log.contains("missing.f3d: no such file or folder"), "{log}");
    assert!(log.ends_with("2 converted, 2 failed, 1 skipped\n"), "{log}");
    assert!(!out_dir.join("parts").join("broken.sc").exists());

    let text = dutchman(&[
        "convert-all",
        "--quiet",
        "--no-verify",
        "--out-dir",
        out_dir.to_str().unwrap(),
        project.join("cube.f3d").to_str().unwrap(),
    ]);
    assert_eq!(text.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&text.stdout);
    assert!(stdout.ends_with("1 converted, 0 failed\n"), "{stdout}");
    // Each converted file's losses are listed beneath it, as `convert` prints them, after the
    // SolidCraft version it targets.
    let lines = without_target_line(&stdout);
    assert_eq!(lines[0], "Fusion .f3d: reads geometry");
    assert!(lines[1].starts_with("converted  "), "{stdout}");
    assert!(
        lines[2..lines.len() - 1]
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
        "--quiet",
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
        "--quiet",
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
    let out = dutchman(&[
        "convert",
        "--quiet",
        "--no-verify",
        &cube,
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        without_target_line(&stdout).join("\n"),
        format!(
            "SolidWorks .sldprt: reads geometry\nconverted  {cube} -> {} (geometry without history)",
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
    let output = dir.join("cube.sc");
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
    std::fs::write(out_dir.join("synthetic_cube.sc"), b"SENTINEL").unwrap();
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
        std::fs::read(out_dir.join("synthetic_cube.sc")).unwrap(),
        b"SENTINEL"
    );
    assert!(out_dir.join("generated_unit_cube.sc").is_file());
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
    std::fs::remove_file(out_dir.join("Housing_Rev7_Secret.sc")).unwrap();
    let third = run();
    assert_eq!(third["resumed"], 2, "{third}");
    assert!(out_dir.join("Housing_Rev7_Secret.sc").is_file());
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
        .test_output()
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

    fn script(dir: &std::path::Path, name: &str, body: &str) {
        let path = dir.join(format!("dutchman-{name}"));
        executable(&path, |file| {
            write!(file, "#!/bin/sh\n{body}\n").unwrap();
        });
    }

    fn executable(path: &std::path::Path, write: impl FnOnce(&mut std::fs::File)) {
        let _guard = EXECUTABLE_PUBLICATION.lock().unwrap();
        let mut temporary = tempfile::NamedTempFile::new_in(path.parent().unwrap()).unwrap();
        write(temporary.as_file_mut());
        temporary.as_file().sync_all().unwrap();
        // Close the writer before chmod and rename. Publication alone cannot prevent
        // inherited writers; the lock also excludes forks until the writer is closed.
        let temporary = temporary.into_temp_path();
        std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::rename(&temporary, path).unwrap();
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
            .test_spawn()
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
    fn printer_commands_dispatch_to_the_print_companion() {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("dutchman");
        executable(&binary, |file| {
            let mut source = std::fs::File::open(env!("CARGO_BIN_EXE_dutchman")).unwrap();
            std::io::copy(&mut source, file).unwrap();
        });
        script(dir.path(), "print", "printf '<%s>\\n' \"$@\"; exit 7");
        for (args, expected) in [
            (
                vec!["printers", "--json", "--config", "a b.toml"],
                "<printers>\n<--json>\n<--config>\n<a b.toml>\n",
            ),
            (
                vec!["--verbose", "printers", "--json"],
                "<printers>\n<--json>\n",
            ),
            (
                vec!["print", "a b.3mf", "--printer", "demo", "--yes"],
                "<print>\n<a b.3mf>\n<--printer>\n<demo>\n<--yes>\n",
            ),
        ] {
            let out = Command::new(&binary)
                .args(args)
                .env("PATH", dir.path())
                .test_output()
                .unwrap();
            assert_eq!(out.status.code(), Some(7));
            assert_eq!(out.stdout, expected.as_bytes());
        }
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
                .test_output()
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
        executable(&binary, |file| {
            let mut source = std::fs::File::open(env!("CARGO_BIN_EXE_dutchman")).unwrap();
            std::io::copy(&mut source, file).unwrap();
        });
        script(adjacent.path(), "foo", "echo adjacent-1.0");
        script(path.path(), "foo", "echo path-2.0");
        let out = Command::new(&binary)
            .arg("foo")
            .env("PATH", path.path())
            .test_output()
            .unwrap();
        assert_eq!(out.stdout, b"adjacent-1.0\n");
        for args in [vec![], vec!["--help"]] {
            let out = Command::new(&binary)
                .args(args)
                .env("PATH", path.path())
                .test_output()
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
            .test_output()
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
                .test_output()
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
            .test_output()
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
        .test_output()
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
            .test_output()
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
        let output = dir.path().join("part.sc");
        assert!(std::fs::read(&output).unwrap().starts_with(b"PK"));
        std::fs::remove_file(output).unwrap();
    }
    let output = dir.path().join("part.sc");
    std::fs::write(&output, b"sentinel").unwrap();
    let modified = output.metadata().unwrap().modified().unwrap();
    let out = run(&["part.f3d"]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(out.stderr, b"dutchman: part.sc already exists; choose another output with -o PATH, or replace it with --force\n");
    assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
    assert_eq!(output.metadata().unwrap().modified().unwrap(), modified);
    assert!(run(&["part.f3d", "--force", "--no-verify"])
        .status
        .success());
    assert_ne!(std::fs::read(&output).unwrap(), b"sentinel");
    std::fs::write(&output, b"sentinel").unwrap();
    assert!(run(&["part.f3d", "part.sc", "--no-verify"])
        .status
        .success());
    assert_ne!(std::fs::read(&output).unwrap(), b"sentinel");
    assert_eq!(
        run(&["part.f3d", "--force", "--no-clobber"]).status.code(),
        Some(2)
    );
    assert!(run(&["part.f3d", "-o", "other.sc", "--no-verify"])
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
            .test_output()
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
        .test_output()
        .unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Fusion"));
}

#[test]
fn format_status_and_volunteer_invitation_follow_run_options() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("first.prt");
    let second = dir.path().join("second.prt");
    for path in [&first, &second] {
        std::fs::write(path, b"#UGC:2").unwrap();
    }
    let output = dir.path().join("out.sc");
    for option in [None, Some("--quiet"), Some("--json")] {
        let mut args = vec!["convert", first.to_str().unwrap(), output.to_str().unwrap()];
        args.extend(option);
        let out = dutchman(&args);
        assert_eq!(out.status.code(), Some(1));
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        if option == Some("--json") {
            let report = json_of(&out);
            assert_eq!(report["input"]["format"], "creo_part");
            assert_eq!(report["input"]["words"], "opens, no geometry yet");
            assert!(!stderr.contains("Want to help"));
        } else {
            assert!(stdout.starts_with("Creo .prt: opens, no geometry yet\n"));
        }
        assert_eq!(
            stdout.matches("Want to help").count(),
            usize::from(option.is_none())
        );
        assert!(!stdout.contains("unknown format"));
        assert!(!output.exists());
        let mut args = vec![
            "convert-all",
            first.to_str().unwrap(),
            second.to_str().unwrap(),
            "--out-dir",
            dir.path().to_str().unwrap(),
        ];
        args.extend(option);
        let out = dutchman(&args);
        assert_eq!(out.status.code(), Some(1));
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            stdout.matches("Want to help").count(),
            usize::from(option.is_none())
        );
        if option.is_none() {
            let pitch = stdout
                .lines()
                .find(|l| l.starts_with("Want to help"))
                .unwrap();
            assert!(pitch.starts_with("Want to help Dutchman read Creo better?"));
            assert!(!pitch.contains("first") && !pitch.contains("second"));
        }
    }
}

#[test]
fn every_listed_format_has_reader_status() {
    let report = json_of(&dutchman(&["formats", "--json"]));
    let formats = report["formats"].as_array().unwrap();
    assert_eq!(formats.len(), dutchman_core::Format::ALL.len());
    for format in dutchman_core::Format::ALL {
        let row = formats.iter().find(|row| row["id"] == format.id()).unwrap();
        assert_eq!(row["words"], format.words());
        assert_eq!(
            row.get("level"),
            Some(&serde_json::json!(format.headline_level()))
        );
    }
}

#[test]
fn an_inconclusive_part_is_recognised_and_refused() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("inconclusive.prt");
    std::fs::write(&input, b"no recognised signature").unwrap();
    let output = dir.path().join("out.solidcraft");
    let out = dutchman(&[
        "convert",
        "--json",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
    ]);
    assert_eq!(out.status.code(), Some(1));
    let report = json_of(&out);
    assert_eq!(report["input"]["format"], "unclassified_part");
    assert_eq!(report["input"]["words"], "recognised, not read yet");
    assert!(report["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not supported yet"));
    assert!(!output.exists());
}

#[test]
fn numbered_saves_select_the_newest_and_report_the_older_revision() {
    let dir = tempfile::tempdir().unwrap();
    let inputs = dir.path().join("inputs");
    let outputs = dir.path().join("outputs");
    std::fs::create_dir_all(&inputs).unwrap();
    for name in ["bracket.prt.3", "bracket.prt.7"] {
        std::fs::write(inputs.join(name), b"#UGC:2").unwrap();
    }
    for json in [false, true] {
        let mut args = vec![
            "convert-all",
            inputs.to_str().unwrap(),
            "--out-dir",
            outputs.to_str().unwrap(),
        ];
        if json {
            args.push("--json");
        }
        let out = dutchman(&args);
        // Creo is recognised but does not supply geometry yet; only the newest is attempted.
        assert_eq!(out.status.code(), Some(1));
        if json {
            let report = json_of(&out);
            assert_eq!(report["results"].as_array().unwrap().len(), 1);
            assert_eq!(
                report["results"][0]["input"]["path"],
                inputs.join("bracket.prt.7").to_str().unwrap()
            );
            assert_eq!(report["results"][0]["input"]["extension"], "prt.7");
            assert_eq!(report["skipped"], 1);
            assert_eq!(
                report["skipped_revisions"][0]["path"],
                inputs.join("bracket.prt.3").to_str().unwrap()
            );
        } else {
            let text = String::from_utf8(out.stdout).unwrap();
            assert!(text.contains("Creo .prt.7: opens, no geometry yet"));
            assert!(text.contains("bracket.prt.3: older revision; using "));
            assert!(text.contains("1 skipped (older revisions)"));
            assert!(!text.contains("Creo .prt.3:"));
        }
    }
}

#[test]
fn numbered_default_outputs_use_the_base_name_and_keep_existing_files() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("bracket.prt.7");
    let output = dir.path().join("bracket.sc");
    std::fs::write(&input, b"#UGC:2").unwrap();
    std::fs::write(&output, b"keep this design").unwrap();
    let out = dutchman(&[input.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("bracket.sc already exists"));
    assert_eq!(std::fs::read(output).unwrap(), b"keep this design");
    assert!(!dir.path().join("bracket.prt.sc").exists());
}

fn assert_text_error_with_progress(out: &Output, input: &std::path::Path, expected: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let mut lines: Vec<_> = stderr.lines().collect();
    assert_eq!(lines.pop(), Some(expected));
    assert!(!lines.is_empty());
    let prefix = format!("{}: ", input.display());
    for line in lines {
        assert!(line.starts_with(&prefix), "unexpected stderr line: {line}");
        assert!(
            line.contains(" s elapsed"),
            "unexpected stderr line: {line}"
        );
    }
}

fn assert_progress_json(out: &Output) -> Vec<serde_json::Value> {
    let text = std::str::from_utf8(&out.stderr).unwrap();
    let events: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(!events.is_empty());
    for event in &events {
        assert_eq!(event["schema"], "dutchman.progress/1");
        assert!(event["elapsed_ms"].is_u64());
        assert!(event["designs_read"].is_u64());
        assert!(event["heartbeat"].is_boolean());
    }
    assert!(events
        .windows(2)
        .all(|pair| pair[0]["elapsed_ms"].as_u64() <= pair[1]["elapsed_ms"].as_u64()));
    events
}

#[test]
fn conversion_progress_stays_on_stderr_and_quiet_suppresses_it() {
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("progress.sc");
    for batch in [false, true] {
        for json in [false, true] {
            for quiet in [false, true] {
                let mut args = vec![if batch { "convert-all" } else { "convert" }, "--no-verify"];
                if json {
                    args.push("--json");
                }
                if quiet {
                    args.push("--quiet");
                }
                let input = fixture("generated_unit_cube.f3d");
                args.push(&input);
                if batch {
                    args.push("--out-dir");
                }
                args.push(if batch {
                    dir.path().to_str().unwrap()
                } else {
                    output.to_str().unwrap()
                });
                let out = dutchman(&args);
                assert_eq!(
                    out.status.code(),
                    Some(0),
                    "{}",
                    String::from_utf8_lossy(&out.stderr)
                );
                let stdout = String::from_utf8_lossy(&out.stdout);
                assert!(!stdout.contains("s elapsed"));
                assert!(!stdout.contains("dutchman.progress/1"));
                if quiet {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    assert!(!stderr.contains("s elapsed"));
                    assert!(!stderr.contains("dutchman.progress/1"));
                    if json {
                        assert!(out.stderr.is_empty());
                    }
                } else if json {
                    let events = assert_progress_json(&out);
                    assert!(events.iter().any(|event| event["stage"] == "decode"));
                    assert!(events.iter().any(|event| event["stage"] == "project"));
                    let last = events.last().unwrap();
                    assert_eq!(last["stage"], "write");
                    assert_eq!(last["designs_read"], 1);
                    assert_eq!(last["designs_total"], 1);
                } else {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    assert!(stderr.contains("1 of 1 designs read"));
                    assert!(stderr.contains("s elapsed"));
                    assert!(!stderr.contains('\r'));
                    assert!(!stderr.contains('\x1b'));
                }
                if json {
                    let report = json_of(&out);
                    assert_eq!(report["ok"], true);
                    assert_eq!(
                        report["schema"],
                        if batch {
                            "dutchman.convert-all/1"
                        } else {
                            "dutchman.convert/1"
                        }
                    );
                    assert!(report.get("heartbeat").is_none());
                }
            }
        }
    }
}

#[test]
fn inspection_progress_leaves_the_final_json_report_on_stdout() {
    for quiet in [false, true] {
        let input = fixture("generated_unit_cube.f3d");
        let mut args = vec!["inspect", "--json", &input];
        if quiet {
            args.push("--quiet");
        }
        let out = dutchman(&args);
        assert_eq!(out.status.code(), Some(0));
        let report = json_of(&out);
        assert_eq!(report["schema"], "dutchman.inspect/1");
        assert_eq!(report["ok"], true);
        assert!(report.get("heartbeat").is_none());
        if quiet {
            assert!(out.stderr.is_empty());
        } else {
            let events = assert_progress_json(&out);
            assert_eq!(events.last().unwrap()["designs_read"], 1);
            assert_eq!(events.last().unwrap()["stage"], "map");
        }
    }
}

#[cfg(feature = "freecad")]
#[test]
fn techdraw_dxf_exports_page_text_and_reports_missing_ir_geometry() {
    let formats = json_of(&dutchman(&["formats", "--json"]));
    assert_eq!(formats["drawings"][0]["input"], "freecad_document");
    assert_eq!(formats["drawings"][0]["output"], "dxf");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/techdraw.FCStd");
    let folder = tempfile::tempdir().unwrap();
    let input = folder.path().join("box.FCStd");
    std::fs::copy(fixture, &input).unwrap();
    std::fs::create_dir(folder.path().join("box.dxf")).unwrap();
    let result = dutchman(&["convert", input.to_str().unwrap(), "--to", "dxf", "--json"]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["pages"][0]["name"], "Page");
    assert_eq!(
        report["pages"][0]["dimension_representation"],
        "lines_and_text"
    );
    assert_eq!(report["pages"][0]["entities_per_layer"]["DIMENSIONS"], 8);
    assert_eq!(report["pages"][0]["entities_per_layer"]["FRAME"], 6);
    assert_eq!(report["pages"][0]["entities_per_layer"]["TITLE_BLOCK"], 1);
    assert_eq!(report["pages"][0]["entities_per_layer"]["ANNOTATIONS"], 2);
    assert_eq!(report["pages"][0]["entities_per_layer"]["VIEW_2_Front"], 0);
    let losses = report["pages"][0]["losses"].as_array().unwrap();
    for subject in ["Front", "Top"] {
        assert!(
            losses.iter().any(|l| l["subject"] == subject
                && l["level"] == "Dropped"
                && l["upstream"] == "cadmpeg"),
            "{losses:?}"
        );
    }
    let output = folder.path().join("box-Page.dxf");
    let text = std::fs::read_to_string(&output).unwrap();
    let lines: Vec<_> = text.lines().collect();
    let (records, remainder) = lines.as_chunks::<2>();
    assert!(remainder.is_empty());
    let pairs: Vec<_> = records
        .iter()
        .map(|[code, value]| (*code, *value))
        .collect();
    assert_eq!(pairs.iter().filter(|p| **p == ("0", "TEXT")).count(), 4);
    assert_eq!(
        pairs.iter().filter(|p| **p == ("8", "ANNOTATIONS")).count(),
        2
    );
    let annotation_entities: Vec<_> = text
        .split("0\nTEXT\n")
        .filter(|entity| entity.contains("8\nANNOTATIONS\n"))
        .collect();
    assert_eq!(annotation_entities.len(), 2);
    assert!(annotation_entities[0].contains("20\n100\n40\n5\n"));
    assert!(annotation_entities[1].contains("20\n93\n40\n5\n"));
    assert!(pairs.contains(&("10", "120")));
    assert!(text.contains("Synthetic box"));
    assert!(!text.contains("%.2w"));
    let retry = dutchman(&["convert", input.to_str().unwrap(), "--to", "dxf", "--json"]);
    assert!(!retry.status.success());
    assert_eq!(std::fs::read_to_string(&output).unwrap(), text);
    let forced = dutchman(&[
        "convert",
        input.to_str().unwrap(),
        "--to",
        "dxf",
        "--force",
        "--page",
        "Page",
        "--json",
    ]);
    assert!(forced.status.success());
    assert_eq!(std::fs::read_to_string(&output).unwrap(), text);
    let mut model =
        dutchman_core::read_drawings(&input, &mut dutchman_core::Report::default()).unwrap();
    let note = model
        .ir
        .model
        .drawings
        .iter()
        .find(|drawing| drawing.object.ends_with("#Note"))
        .unwrap()
        .object
        .clone();
    model.ir.model.view_presentations.push(
        serde_json::from_value(serde_json::json!({
            "id": "test:dxf:presentation#note", "object": note, "order": 0, "visible": false
        }))
        .unwrap(),
    );
    let pages = dutchman_dxf::pages(&model, None, &mut dutchman_core::Report::default()).unwrap();
    assert_eq!(pages[0].entities_per_layer["ANNOTATIONS"], 0);
    assert!(pages[0]
        .report
        .losses()
        .iter()
        .any(|loss| loss.subject == "Note" && loss.detail.contains("hidden")));
    model.ir.model.view_presentations[0].visible = Some(true);
    model
        .ir
        .model
        .semantic_annotations
        .iter_mut()
        .find(|annotation| annotation.object.ends_with("#Note"))
        .unwrap()
        .text = vec!["first".into(), "".into(), "third".into()];
    let pages = dutchman_dxf::pages(&model, None, &mut dutchman_core::Report::default()).unwrap();
    assert_eq!(pages[0].entities_per_layer["ANNOTATIONS"], 2);
    let text = String::from_utf8(pages[0].bytes.clone()).unwrap();
    assert!(text.contains("20\n86\n"));
    assert!(!text.contains("20\n93\n"));
}

#[cfg(feature = "freecad")]
#[test]
fn drawing_only_pages_support_selection_and_output_rules() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/techdraw-pages.FCStd");
    let folder = tempfile::tempdir().unwrap();
    let result = dutchman(&[
        "convert",
        fixture.to_str().unwrap(),
        "--to",
        "dxf",
        "-o",
        folder.path().to_str().unwrap(),
        "--json",
    ]);
    assert!(
        result.status.success(),
        "{} {}",
        String::from_utf8_lossy(&result.stderr),
        String::from_utf8_lossy(&result.stdout)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["pages"].as_array().unwrap().len(), 2);
    assert!(report["pages"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["dimension_representation"] == "none"));
    for name in ["First", "Second"] {
        assert!(folder
            .path()
            .join(format!("techdraw-pages-{name}.dxf"))
            .exists());
    }
    let selected = folder.path().join("selected.dxf");
    let result = dutchman(&[
        "convert",
        fixture.to_str().unwrap(),
        "--to",
        "dxf",
        "--page",
        "Second",
        "-o",
        selected.to_str().unwrap(),
        "--json",
    ]);
    assert!(result.status.success());
    let text = std::fs::read_to_string(&selected).unwrap();
    assert!(text.contains("Second"));
    assert!(!text.contains("First"));
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["pages"].as_array().unwrap().len(), 1);
    let missing = dutchman(&[
        "convert",
        fixture.to_str().unwrap(),
        "--to",
        "dxf",
        "--page",
        "Absent",
        "-o",
        selected.to_str().unwrap(),
        "--json",
    ]);
    assert!(!missing.status.success());
    assert_eq!(std::fs::read_to_string(&selected).unwrap(), text);
    let part = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/design_history.FCStd");
    let no_page = dutchman(&[
        "convert",
        part.to_str().unwrap(),
        "--to",
        "dxf",
        "-o",
        selected.to_str().unwrap(),
        "--json",
    ]);
    assert!(!no_page.status.success());
    let report: serde_json::Value = serde_json::from_slice(&no_page.stdout).unwrap();
    assert!(report["error"]["message"]
        .as_str()
        .unwrap()
        .contains("no TechDraw page"));
}

#[cfg(feature = "freecad")]
#[test]
fn neutral_techdraw_fixtures_keep_templates_dimensions_and_units() {
    for (fixture, value, dimension_entities, frame_entities) in [
        ("techdraw", Some("30"), 8, 6),
        ("techdraw-pages", None, 0, 6),
        ("cadmpeg-techdraw-views", Some("30"), 24, 8),
        ("cadmpeg-techdraw-dimensions", Some("R7"), 35, 8),
        ("cadmpeg-techdraw-variants", Some("R11"), 43, 8),
    ] {
        let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../dutchman-core/tests/fixtures/{fixture}.FCStd"));
        let model =
            dutchman_core::read_drawings(&input, &mut dutchman_core::Report::default()).unwrap();
        let pages =
            dutchman_dxf::pages(&model, None, &mut dutchman_core::Report::default()).unwrap();
        assert_eq!(
            pages.len(),
            if value.is_none() { 2 } else { 1 },
            "{fixture}"
        );
        for page in &pages {
            let text = std::str::from_utf8(&page.bytes).unwrap();
            assert!(text.contains("$INSUNITS\n70\n4\n"), "{fixture}");
            assert!(text.contains("$LIMMAX\n10\n210\n20\n297\n"), "{fixture}");
            assert_eq!(
                page.entities_per_layer["FRAME"], frame_entities,
                "{fixture}"
            );
            assert_eq!(
                page.entities_per_layer["DIMENSIONS"], dimension_entities,
                "{fixture}"
            );
            assert_eq!(page.entities_per_layer["TITLE_BLOCK"], 1, "{fixture}");
            assert!(page.entities_per_layer["ANNOTATIONS"] > 0, "{fixture}");
            assert!(text.contains("100\nAcDbText\n"), "{fixture}");
            assert!(text.contains("40\n4\n"), "template text height: {fixture}");
            if let Some(value) = value {
                assert!(text.contains(&format!("1\n{value}\n")), "{fixture}: {text}");
                assert!(page.entities_per_layer["DIMENSIONS"] > 1, "{fixture}");
                assert!(
                    text.contains("100\nAcDbEntity\n8\nDIMENSIONS\n100\nAcDbLine\n"),
                    "dimension lines: {fixture}"
                );
                assert!(
                    page.report
                        .losses()
                        .iter()
                        .any(|loss| loss.subject == "Front"
                            && loss.detail.contains("view curves are unavailable")),
                    "{fixture}"
                );
            }
            if fixture.contains("variants") {
                for label in ["42", "\\U+230022", "40"] {
                    assert!(text.contains(&format!("1\n{label}\n")));
                }
            }
            if fixture.contains("views")
                || fixture.contains("dimensions")
                || fixture.contains("variants")
            {
                assert!(text.contains("1\nEdited title\n"));
                assert!(text.contains("0\nCIRCLE\n"));
                assert!(text.contains("0\nARC\n"));
            }
            if fixture.contains("dimensions") || fixture.contains("variants") {
                assert_eq!(page.dimension_representation, "partial_lines_and_text");
                assert!(text.contains("1\n90\\U+00B0\n"));
                assert!(page
                    .report
                    .losses()
                    .iter()
                    .any(|loss| loss.detail.contains("text only")));
            }
        }
    }
}

#[cfg(feature = "freecad")]
#[test]
fn neutral_view_curves_keep_placement_visibility_and_source_layers() {
    let input = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/techdraw.FCStd");
    let mut model =
        dutchman_core::read_drawings(&input, &mut dutchman_core::Report::default()).unwrap();
    let view = model
        .ir
        .model
        .drawings
        .iter_mut()
        .find(|d| d.object.ends_with("#Front"))
        .unwrap();
    for (source, visibility, displayed) in [
        ("cosmetic", "hidden", true),
        ("center_line", "visible", true),
        ("projected_model_edge", "visible", false),
    ] {
        view.curves.push(
            serde_json::from_value(serde_json::json!({
                "geometry": {"kind": "line", "start": [1,2], "end": [3,4]},
                "source": source, "visibility": visibility, "displayed": displayed
            }))
            .unwrap(),
        );
    }
    let pages = dutchman_dxf::pages(&model, None, &mut dutchman_core::Report::default()).unwrap();
    let page = &pages[0];
    assert_eq!(page.entities_per_layer["VIEW_2_Front_COSMETIC_HIDDEN"], 1);
    assert_eq!(page.entities_per_layer["VIEW_2_Front_CENTER"], 1);
    assert_eq!(page.entities_per_layer["VIEW_2_Front"], 0);
    let text = std::str::from_utf8(&page.bytes).unwrap();
    assert!(text.contains(
        "8\nVIEW_2_Front_COSMETIC_HIDDEN\n100\nAcDbLine\n10\n67\n20\n184\n11\n71\n21\n188\n"
    ));
    assert!(text.contains("2\nVIEW_2_Front_COSMETIC_HIDDEN\n70\n0\n62\n7\n6\nHIDDEN\n"));
    assert!(text.contains("2\nVIEW_2_Front_CENTER\n70\n0\n62\n7\n6\nCENTER\n"));
}

#[test]
fn memory_refusal_precedes_decode_and_returns_exit_two() {
    let Some(available) = dutchman::choices::available_memory() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("synthetic.step");
    let output = dir.path().join("never-written.sc");
    // Sparse metadata is enough for the estimator; no design bytes are read or allocated.
    std::fs::File::create(&input)
        .unwrap()
        .set_len(available.saturating_add(2_000_000_000))
        .unwrap();
    for flags in [vec!["--yes"], vec!["--json"], vec![]] {
        let mut args = vec![
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--preset",
            "full",
        ];
        args.extend(flags.iter().copied());
        let out = dutchman(&args);
        assert_eq!(out.status.code(), Some(2), "{out:?}");
        if flags.contains(&"--json") {
            let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
            assert_eq!(report["ok"], false);
            assert!(report["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with("this conversion needs about "));
            assert_eq!(report["choices"]["preset"], "full");
            assert!(report["estimate"]["peak_bytes"].as_u64().unwrap() > available);
        } else {
            assert!(String::from_utf8_lossy(&out.stderr)
                .contains("try --preset standard (or --no-memory-check to run anyway)"));
        }
        assert!(!output.exists());
    }
    let batch = dutchman(&[
        "convert-all",
        input.to_str().unwrap(),
        "--out-dir",
        dir.path().join("never-created").to_str().unwrap(),
        "--preset",
        "full",
        "--json",
    ]);
    assert_eq!(batch.status.code(), Some(2));
    let report: serde_json::Value = serde_json::from_slice(&batch.stdout).unwrap();
    assert_eq!(report["schema"], "dutchman.convert-all/1");
    assert!(!dir.path().join("never-created").exists());
    let out = dutchman(&[
        "convert",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
        "--preset",
        "full",
        "--no-memory-check",
        "--json",
    ]);
    assert_eq!(
        out.status.code(),
        Some(1),
        "the decoder's file-size guard takes over: {out:?}"
    );
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!report["error"]["message"]
        .as_str()
        .unwrap()
        .contains("try --preset"));
}

#[test]
fn estimates_are_text_only_and_respect_quiet() {
    let dir = tempfile::tempdir().unwrap();
    let input = PathBuf::from(fixture("synthetic_cube.ipt"));
    let output = dir.path().join("cube.solidcraft");
    for flags in [vec![], vec!["--quiet"], vec!["--json"]] {
        let mut args = vec![
            "convert",
            input.to_str().unwrap(),
            output.to_str().unwrap(),
            "--no-verify",
            "--preset",
            "fast",
        ];
        args.extend(flags.iter().copied());
        let out = dutchman(&args);
        assert!(out.status.success(), "{out:?}");
        let text = String::from_utf8_lossy(&out.stdout);
        assert_eq!(
            text.contains("Estimate: under 0.1 GB, under a minute"),
            flags.is_empty(),
            "{text}"
        );
        if flags.contains(&"--json") {
            let report: serde_json::Value = serde_json::from_str(&text).unwrap();
            assert!(report["estimate"]["peak_bytes"].is_u64());
            assert!(report["estimate"]["seconds"].is_u64());
            assert_eq!(
                report["choices"],
                serde_json::json!({"preset": "fast", "history": false, "sketches": false, "joints": false})
            );
        }
    }
}

#[test]
fn narrowing_choices_reach_batch_workers_and_invalidate_journal_reuse() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture("synthetic_cube.ipt");
    let out_dir = dir.path().join("out");
    let journal = dir.path().join("journal.jsonl");
    let run = |sketches: bool| {
        let mut args = vec![
            "convert-all",
            &input,
            "--out-dir",
            out_dir.to_str().unwrap(),
            "--journal",
            journal.to_str().unwrap(),
            "--preset",
            "full",
            "--no-history",
            "--no-joints",
            "--no-memory-check",
            "--no-verify",
            "--json",
        ];
        if !sketches {
            args.push("--no-sketches");
        }
        json_of(&dutchman(&args))
    };
    let first = run(true);
    assert_eq!(first["converted"], 1, "{first}");
    assert_eq!(first["resumed"], 0);
    assert_eq!(
        first["results"][0]["choices"],
        serde_json::json!({"preset": "full", "history": false, "sketches": true, "joints": false})
    );
    assert_eq!(run(true)["resumed"], 1);
    let changed = run(false);
    assert_eq!(
        changed["resumed"], 0,
        "changing a choice must reconvert: {changed}"
    );
    assert_eq!(changed["results"][0]["choices"]["sketches"], false);
}

#[test]
fn threads_accepts_serial_and_auto_on_conversion_commands() {
    let dir = tempfile::tempdir().unwrap();
    let input = fixture("generated_unit_cube.f3d");
    for threads in ["0", "1", "4"] {
        let output = dir.path().join(format!("threads-{threads}.sc"));
        let out = dutchman(&[
            "convert",
            input.as_str(),
            output.to_str().unwrap(),
            "--threads",
            threads,
            "--no-verify",
            "--json",
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(
            report["threads"]["requested"],
            threads.parse::<u64>().unwrap()
        );
        assert_eq!(report["threads"]["used"], 1);
        for args in [
            vec![
                "convert-all",
                input.as_str(),
                "--out-dir",
                dir.path().to_str().unwrap(),
                "--threads",
                threads,
                "--help",
            ],
            vec![
                "watch",
                dir.path().to_str().unwrap(),
                "--out-dir",
                dir.path().to_str().unwrap(),
                "--threads",
                threads,
                "--help",
            ],
        ] {
            assert!(dutchman(&args).status.success());
        }
    }
}

#[test]
fn part_threads_keep_assembly_json_and_design_bytes_identical() {
    if !dutchman_core::Format::FreeCadDocument.can_convert() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let input = fixture("freecad-1.1.4-A01/A01a.FCStd");
    let output = dir.path().join("assembly.solidcraft");
    let mut previous = None;
    for threads in ["1", "4"] {
        let out = dutchman(&[
            "convert",
            &input,
            output.to_str().unwrap(),
            "--threads",
            threads,
            "--no-verify",
            "--solidcraft-version",
            "0.26.0",
            "--preset",
            "full",
            "--no-memory-check",
            "--json",
            "--quiet",
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let mut report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        report.as_object_mut().unwrap().remove("threads");
        let design = std::fs::read(&output).unwrap();
        if let Some((before_report, before_design)) = previous {
            assert_eq!(before_report, report);
            assert_eq!(before_design, design);
        }
        previous = Some((report, design));
    }
}

/// Output lines without the one "Target SolidCraft x.y.z (...)" line, which names whatever
/// SolidCraft the machine has.
fn without_target_line(stdout: &str) -> Vec<&str> {
    let (target, rest): (Vec<&str>, Vec<&str>) = stdout
        .lines()
        .partition(|line| line.starts_with("Target SolidCraft "));
    assert_eq!(target.len(), 1, "{stdout}");
    rest
}

#[test]
fn solidcraft_output_extensions_follow_the_target_and_accept_explicit_names() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::copy(
        fixture("generated_unit_cube.f3d"),
        dir.path().join("part.f3d"),
    )
    .unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_dutchman"))
            .current_dir(dir.path())
            .args(args)
            .env("PATH", "")
            .env_remove("SOLIDCRAFT_CLI")
            .test_output()
            .unwrap()
    };
    for (target, extension) in [("0.25.0", "solidcraft"), ("0.25.1", "sc"), ("0.27.0", "sc")] {
        let out = run(&[
            "convert",
            "part.f3d",
            "--solidcraft-version",
            target,
            "--no-verify",
            "--json",
        ]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let output = format!("part.{extension}");
        assert_eq!(json_of(&out)["output"]["path"], output);
        assert!(std::fs::read(dir.path().join(&output))
            .unwrap()
            .starts_with(b"PK"));
        std::fs::remove_file(dir.path().join(output)).unwrap();
    }
    let newest = run(&["part.f3d", "--no-verify", "--json"]);
    assert!(newest.status.success());
    assert_eq!(json_of(&newest)["output"]["path"], "part.sc");
    for extension in ["sc", "solidcraft", "solvecraft", "SC"] {
        for target in ["0.25.0", "0.25.1"] {
            let output = format!("explicit.{extension}");
            let out = run(&[
                "convert",
                "part.f3d",
                "-o",
                &output,
                "--solidcraft-version",
                target,
                "--no-verify",
                "--json",
            ]);
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
            let report = json_of(&out);
            assert_eq!(report["output"]["path"], output);
            assert_eq!(report["output"]["format"], "solidcraft/23");
            assert!(std::fs::read(dir.path().join(output))
                .unwrap()
                .starts_with(b"PK"));
        }
    }
}

#[test]
fn repro_cli_writes_a_private_kit_and_reports_success_or_failure() {
    for (name, status) in [
        ("generated_unit_cube.f3d", "no_failure_to_reproduce"),
        ("container_metadata_only.f3d", "failure"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let output = dutchman(&[
            "repro",
            &fixture(name),
            "--out",
            dir.path().to_str().unwrap(),
            "--json",
        ]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(result["schema"], "dutchman.repro/1");
        assert_eq!(result["anonymous"]["status"], status);
        assert!(result.get("local_key").is_none());
        assert!(
            result["summary"]
                .as_str()
                .unwrap()
                .contains("Do not share")
                .then_some(())
                .is_some()
                || result["summary"].as_str().unwrap().contains("Never share")
        );
        let kit = dir.path().join("kit");
        assert!(kit.join("anonymous.json").is_file());
        assert!(kit.join("local-key.json").is_file());
        assert!(kit.join("next-steps.md").is_file());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        let again = dutchman(&[
            "repro",
            &fixture(name),
            "--out",
            dir.path().to_str().unwrap(),
        ]);
        assert_eq!(again.status.code(), Some(1));
    }
}
