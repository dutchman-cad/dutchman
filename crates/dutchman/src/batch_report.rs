//! Local, deterministic folder summaries. No CAD files are opened or sent anywhere.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// Find saved reports recursively, without following symbolic links.
pub fn report_paths(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut pending = vec![(folder.to_path_buf(), 0)];
    let mut paths = Vec::new();
    let mut entries = 0_usize;
    while let Some((dir, depth)) = pending.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|err| {
            format!(
                "Could not read {}: {err}. Choose an accessible output folder.",
                dir.display()
            )
        })? {
            let entry = entry.map_err(|err| format!("Could not read the output folder: {err}."))?;
            entries += 1;
            if entries > 100_000 {
                return Err(
                    "The output folder exceeds 100,000 entries. Choose a smaller folder.".into(),
                );
            }
            let kind = entry
                .file_type()
                .map_err(|err| format!("Could not check an output entry: {err}."))?;
            if kind.is_dir() {
                if depth >= 32 {
                    return Err(
                        "The output folder exceeds 32 levels. Choose a smaller folder.".into(),
                    );
                }
                pending.push((entry.path(), depth + 1));
            } else if kind.is_file() {
                let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
                if name.ends_with(".report.json") || name.ends_with(".failed.json") {
                    paths.push(entry.path());
                    if paths.len() > 10_000 {
                        return Err(
                            "The output folder exceeds 10,000 reports. Choose a smaller folder."
                                .into(),
                        );
                    }
                }
            }
        }
    }
    paths.sort();
    Ok(paths)
}

/// The first cause, excluding notes that only describe features downstream of it.
pub fn first_reason(report: &Value) -> String {
    let text = |pointer| report.pointer(pointer).and_then(Value::as_str);
    let feature_reason = report
        .pointer("/history/features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|feature| {
            matches!(
                feature.get("status").and_then(Value::as_str),
                Some("blocked" | "left_out")
            )
        })
        .filter_map(|feature| feature.get("reason").and_then(Value::as_str))
        .filter(|reason| !reason.trim().is_empty())
        .find(|reason| {
            let lower = reason.to_lowercase();
            !lower.contains("after the first")
                && !lower.contains("downstream")
                && !(lower.contains("follows") && lower.contains("blocker"))
        });
    text("/error/message")
        .or(feature_reason)
        .or_else(|| text("/verification/reason"))
        .or_else(|| {
            report
                .get("losses")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find_map(|loss| loss.get("message").and_then(Value::as_str))
        })
        .unwrap_or("")
        .to_owned()
}

fn field<'a>(report: &'a Value, pointer: &str) -> &'a str {
    report
        .pointer(pointer)
        .and_then(Value::as_str)
        .unwrap_or("")
}

fn relative(path: &str, root: &Path) -> String {
    let normalized = path.replace('\\', "/");
    let path = Path::new(&normalized);
    let cwd = std::env::current_dir().ok();
    let absolute_path = cwd.as_ref().map(|cwd| cwd.join(path));
    let absolute_root = cwd.as_ref().map(|cwd| cwd.join(root));
    path.strip_prefix(root)
        .ok()
        .or_else(|| {
            absolute_path
                .as_ref()?
                .strip_prefix(absolute_root.as_ref()?)
                .ok()
        })
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn markdown(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\\', "&#92;")
        .replace('|', "&#124;")
        .replace('`', "&#96;")
        .replace('*', "&#42;")
        .replace('_', "&#95;")
        .replace('[', "&#91;")
        .replace(']', "&#93;")
        .replace('~', "&#126;")
        .replace(['\r', '\n'], " ")
}

fn csv(text: &str) -> String {
    format!("\"{}\"", text.replace('"', "\"\""))
}

/// Render one row per report. Paths are relative to the common input parent and output folder.
pub fn render(reports: &[Value], folder: &Path) -> (String, String) {
    let inputs: Vec<_> = reports
        .iter()
        .map(|r| field(r, "/input/path").replace('\\', "/"))
        .collect();
    let mut root = inputs.iter().filter_map(|input| Path::new(input).parent());
    let mut common = root.next().unwrap_or(Path::new("")).to_path_buf();
    for parent in root {
        while !parent.starts_with(&common) && common.pop() {}
    }
    let mut rows: Vec<_> = reports
        .iter()
        .map(|report| {
            [
                relative(field(report, "/input/path"), &common),
                relative(field(report, "/output/path"), folder),
                crate::present::batch_outcome(report),
                first_reason(report),
            ]
        })
        .collect();
    rows.sort();
    let mut full = 0;
    let mut partial = 0;
    let mut geometry = 0;
    let mut failed = 0;
    for row in &rows {
        match row.get(2).map(String::as_str) {
            Some("Full history") => full += 1,
            Some("Geometry only") => geometry += 1,
            Some(outcome) if outcome.starts_with("Partial ") => partial += 1,
            _ => failed += 1,
        }
    }
    let count = reports.len();
    let files = if count == 1 { "file" } else { "files" };
    let mut md = format!("# Batch report\n\n{count} {files}: {full} full history, {partial} partial, {geometry} geometry only, {failed} failed\n\n| Input path | Output path | Outcome | First reason |\n|---|---|---|---|\n");
    let mut csv_text = String::from("Input path,Output path,Outcome,First reason\r\n");
    for row in rows {
        md.push_str(&format!(
            "| {} |\n",
            row.iter()
                .map(|s| markdown(s))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
        csv_text.push_str(&row.iter().map(|s| csv(s)).collect::<Vec<_>>().join(","));
        csv_text.push_str("\r\n");
    }
    (md, csv_text)
}

/// Summarise an earlier run. If both success and failure exist, the newest report wins.
pub fn write_folder(folder: &Path) -> Result<usize, String> {
    let paths = report_paths(folder)?;
    let mut reports = BTreeMap::new();
    let mut bytes = 0_u64;
    for path in paths {
        let metadata =
            std::fs::metadata(&path).map_err(|err| format!("Could not read the report: {err}."))?;
        bytes = bytes.saturating_add(metadata.len());
        if bytes > 256 << 20 {
            return Err("The reports exceed 256 MiB. Choose a smaller output folder.".into());
        }
        let report = crate::saved_report::read(&path)?;
        let key = field(&report, "/input/path").to_owned();
        if key.is_empty() {
            return Err(format!(
                "The report at {} has no input path. Choose reports with input paths.",
                path.display()
            ));
        }
        let modified = metadata.modified().ok();
        let entry = reports
            .entry(key)
            .or_insert_with(|| (modified, report.clone()));
        if modified >= entry.0 {
            *entry = (modified, report);
        }
    }
    if reports.is_empty() {
        return Err(
            "No per-file reports were found. Choose the output folder of a completed run.".into(),
        );
    }
    let reports: Vec<_> = reports.into_values().map(|(_, report)| report).collect();
    write_reports(&reports, folder)
}

/// Write a summary of freshly completed per-file reports.
pub fn write_reports(reports: &[Value], folder: &Path) -> Result<usize, String> {
    std::fs::create_dir_all(folder).map_err(|err| {
        format!("Could not create the output folder: {err}. Choose a writable folder.")
    })?;
    let (md, csv) = render(reports, folder);
    for (name, content) in [
        ("dutchman-batch-report.md", md),
        ("dutchman-batch-report.csv", csv),
    ] {
        let save = || -> Result<(), Box<dyn std::error::Error>> {
            let mut file = tempfile::NamedTempFile::new_in(folder)?;
            file.write_all(content.as_bytes())?;
            file.persist(folder.join(name))?;
            Ok(())
        };
        save().map_err(|err| format!("Could not write {name}: {err}. Check the output folder's permissions and free space."))?;
    }
    Ok(reports.len())
}
