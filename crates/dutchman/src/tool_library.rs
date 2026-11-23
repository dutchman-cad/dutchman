//! Fusion tool libraries (`.tools`, `.json`) converted to CamCraft's `.camtools`, as a
//! `dutchman.convert/1` report. The reading and mapping live in `dutchman_core::camtools`.

use std::path::Path;

use dutchman_core::camtools;
use serde_json::{json, Value};

/// The `output.format` of a written `.camtools`.
pub const FORMAT: &str = camtools::FORMAT;

/// The program the input is from, as the reports name it.
pub const PROGRAM: &str = "Fusion";

/// Whether `path` is a Fusion tool library: a `.tools` file, or a `.json` export of one.
pub fn is_library(path: &Path) -> bool {
    camtools::is_library(path)
}

/// The reader's status line for a tool library, as a design format's `status_for` says it.
pub fn status(path: &Path) -> Option<String> {
    is_library(path).then(|| {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("tools")
            .to_owned();
        status_text(&ext)
    })
}

/// The status line for a tool library with extension `ext`.
pub fn status_text(ext: &str) -> String {
    format!("{PROGRAM} tool library (.{ext}): {WORDS}")
}

/// Whether `report` is about a tool library.
pub fn is_report(report: &Value) -> bool {
    report.pointer("/input/format").and_then(Value::as_str) == Some("fusion_tool_library")
}

/// The title of the third card for the library of `report`: the same library exported the other
/// way, `.tools` for a `.json` and `.json` for a `.tools`.
pub fn other_export_title(report: &Value) -> String {
    let ext = report
        .pointer("/input/extension")
        .and_then(Value::as_str)
        .unwrap_or("json");
    let other = if ext.eq_ignore_ascii_case("tools") {
        "json"
    } else {
        "tools"
    };
    format!("Please add the same library as .{other}")
}

/// What was written, in a sentence: the tools written out of the tools read.
pub fn outcome(report: &Value) -> String {
    let count = |key: &str| {
        report
            .pointer(&format!("/tools/{key}"))
            .and_then(Value::as_u64)
            .unwrap_or(0)
    };
    format!(
        "{} of {} tools written to CamCraft's tool library",
        count("written"),
        count("read")
    )
}

/// What Dutchman does with a tool library, in the words the reports use.
pub const WORDS: &str = "converts the tool library to CamCraft's .camtools";

/// The `input` object of a report about a tool library.
pub fn input(path: &Path) -> Value {
    json!({
        "path": path.display().to_string(),
        "format": "fusion_tool_library",
        "format_name": "Fusion tool library (.tools, .json)",
        "program": PROGRAM,
        "extension": path.extension().and_then(|e| e.to_str()),
        "words": WORDS,
    })
}

/// Converts the library `input` to a `.camtools` at `output` and returns the report. An
/// existing `output` is replaced only when `replace` is set.
pub fn convert(input_path: &Path, output: &Path, replace: bool) -> Value {
    let input = input(input_path);
    if !replace && output.try_exists().unwrap_or(true) {
        return crate::json::failure(
            crate::json::CONVERT,
            input,
            &format!(
                "{} already exists; choose another output, or replace it with --force",
                output.display()
            ),
        );
    }
    let converted = match camtools::convert_path(input_path) {
        Ok(converted) => converted,
        Err(message) => return crate::json::failure(crate::json::CONVERT, input, &message),
    };
    let mut report = crate::json::success(crate::json::CONVERT, input, &converted.report);
    let skipped = converted.tools_read.saturating_sub(converted.tools_written);
    report.insert(
        "tools".to_owned(),
        json!({"read": converted.tools_read, "written": converted.tools_written, "not_written": skipped}),
    );
    let Some(text) = converted.text else {
        report.insert("ok".to_owned(), json!(false));
        report.insert(
            "error".to_owned(),
            json!({
                "message": format!(
                    "none of the {} tools in the library can be written to a .camtools (CamCraft refuses an empty library); the losses say why",
                    converted.tools_read
                ),
                "stage": dutchman_core::Stage::Map.id(),
                "upstream": Value::Null,
            }),
        );
        return Value::Object(report);
    };
    let written = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(output, text));
    if let Err(err) = written {
        return crate::json::failure(
            crate::json::CONVERT,
            input_of(&report),
            &format!("cannot write {}: {err}", output.display()),
        );
    }
    report.insert(
        "output".to_owned(),
        json!({"path": output.display().to_string(), "format": FORMAT}),
    );
    Value::Object(report)
}

fn input_of(report: &serde_json::Map<String, Value>) -> Value {
    report.get("input").cloned().unwrap_or(Value::Null)
}
