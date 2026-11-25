//! Fusion tool libraries (`.tools`, `.json`) converted to CamCraft's `.camtools`, as a
//! `dutchman.convert/1` report. The reading and mapping live in `dutchman_core::camtools`.

use std::path::Path;

use dutchman_core::camtools;
use serde_json::{json, Value};

/// The `output.format` of a written `.camtools`.
pub const FORMAT: &str = camtools::FORMAT;

/// The program the input is from, as the reports name it.
pub const PROGRAM: &str = "Fusion";

/// What was decided about a file: its path, size and modification time, and the answer.
type Verdicts =
    std::collections::HashMap<(std::path::PathBuf, u64, Option<std::time::SystemTime>), bool>;

/// Whether `path` is a Fusion tool library: a `.tools` file, or a `.json` export of one. A `.json`
/// is looked at once per size and modification time, however many times the batch asks.
pub fn is_library(path: &Path) -> bool {
    static VERDICTS: std::sync::Mutex<Option<Verdicts>> = std::sync::Mutex::new(None);
    let is_json = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("json"));
    if !is_json {
        return camtools::is_library(path);
    }
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    let key = (path.to_path_buf(), meta.len(), meta.modified().ok());
    if let Some(known) = VERDICTS
        .lock()
        .ok()
        .and_then(|v| v.as_ref().and_then(|m| m.get(&key).copied()))
    {
        return known;
    }
    let verdict = camtools::is_library(path);
    if let Ok(mut guard) = VERDICTS.lock() {
        guard
            .get_or_insert_with(Default::default)
            .insert(key, verdict);
    }
    verdict
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
                "message": no_tools_message(&converted),
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

/// Why nothing was written: no tool in the library, or none that CamCraft can hold.
fn no_tools_message(converted: &camtools::Converted) -> String {
    match (converted.tools_read, converted.holders) {
        (0, 0) => "this library holds no tools".to_owned(),
        (0, holders) => {
            format!("this library holds only holders ({holders}), no tools to convert")
        }
        (read, _) => format!(
            "none of the {read} tools in the library can be written to a .camtools (CamCraft refuses an empty library); the losses say why"
        ),
    }
}
