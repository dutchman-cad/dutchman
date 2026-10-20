//! End-to-end checks of the `dutchman` binary.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

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
    let input = scratch("part.ipt");
    std::fs::write(&input, b"not really a part").unwrap();
    let output = scratch("part.solidcraft");

    let out = dutchman(&["convert", input.to_str().unwrap(), output.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("Inventor part (.ipt) files are not supported yet"),
        "{stderr}"
    );
    assert!(!output.exists(), "no output file is created on failure");
}

#[test]
fn convert_rejects_wrong_output_extension() {
    let out = dutchman(&["convert", "part.ipt", "part.step"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("output must be a .solidcraft file"));
}

#[test]
fn convert_rejects_unknown_input() {
    let input = scratch("notes.txt");
    std::fs::write(&input, b"hello").unwrap();
    let out = dutchman(&["convert", input.to_str().unwrap(), "x.solidcraft"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("unrecognised input format"));
}

#[test]
fn convert_reports_missing_input() {
    let out = dutchman(&["convert", "/nonexistent/part.ipt", "x.solidcraft"]);
    assert_eq!(out.status.code(), Some(1));
}

#[test]
fn missing_arguments_is_a_usage_error() {
    let out = dutchman(&["convert"]);
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn convert_decodes_f3d_and_leaves_no_file_when_writing_fails() {
    let input = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../dutchman-core/tests/fixtures/generated_unit_cube.f3d"
    );
    let output = scratch("cube.solidcraft");

    let out = dutchman(&["convert", input, output.to_str().unwrap()]);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("cannot encode output"), "{stderr}");
    assert!(!output.exists(), "no output file is created on failure");
}
