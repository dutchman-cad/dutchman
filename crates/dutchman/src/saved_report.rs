//! Local conversion reports saved beside a design. Reading never opens CAD files.

use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::Value;

/// The largest saved report accepted by the MCP reader (64 MiB).
pub const MAX_BYTES: u64 = 64 << 20;

pub fn path(output: &Path, success: bool) -> PathBuf {
    output.with_extension(if success {
        "report.json"
    } else {
        "failed.json"
    })
}

/// Adds to `report` the entry that names Dutchman and the source the report was made from,
/// which is how a later conversion knows the report is its own to replace. A report shown in a
/// window is marked the same way, so what is shown is what is saved. Marking twice changes
/// nothing.
pub fn mark(report: &mut Value) {
    let Some(input) = report
        .pointer("/input/path")
        .and_then(Value::as_str)
        .map(PathBuf::from)
    else {
        return;
    };
    if let Some(fields) = report.as_object_mut() {
        fields.insert("generator".to_owned(), crate::mark::report_mark(&input));
    }
}

/// Publish a complete report, keeping the previous report if writing fails.
pub fn save(output: &Path, report: &Value) -> Result<(), String> {
    let target = path(output, report.get("ok") == Some(&Value::Bool(true)));
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut marked = report.clone();
        mark(&mut marked);
        let mut bytes = serde_json::to_vec_pretty(&marked)?;
        bytes.push(b'\n');
        // This report belongs to the design just written in its own slot, so it replaces the
        // one an earlier attempt left there.
        dutchman_core::safe_output::write_own(&target, &bytes)?;
        Ok(())
    };
    write().map_err(|err| format!("Could not save the local report at {}: {err}. Check the folder's permissions and free space.", target.display()))
}

/// Return the original report plus a loss index. Names and paths are for local use only,
/// like `convert` and `inspect`; this is deliberately separate from anonymous diagnostics.
pub fn read(input: &Path) -> Result<Value, String> {
    let name = input
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let target = if name.ends_with(".report.json") || name.ends_with(".failed.json") {
        input.to_path_buf()
    } else if input
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            ["sc", "solidcraft", "solvecraft"]
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
    {
        path(input, true)
    } else {
        return Err("Choose a .sc design or its .report.json (or a .failed.json report).".into());
    };
    let file = std::fs::File::open(&target).map_err(|err| format!("Could not read the saved report at {}: {err}. Choose an existing .report.json saved by the desktop app or watch mode.", target.display()))?;
    if file
        .metadata()
        .is_ok_and(|metadata| metadata.len() > MAX_BYTES)
    {
        return Err("The saved report exceeds 64 MiB. Use a smaller per-file report.".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|err| {
            format!("Could not read the saved report: {err}. Check the file's permissions.")
        })?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("The saved report exceeds 64 MiB. Use a smaller per-file report.".into());
    }
    let report: Value = serde_json::from_slice(&bytes).map_err(|_| "The saved report could not be read as JSON. Choose the .report.json saved beside the design.")?;
    if report.get("schema").and_then(Value::as_str) != Some(crate::json::CONVERT)
        || !report.get("ok").is_some_and(Value::is_boolean)
        || (report.get("ok") == Some(&Value::Bool(true))
            && !report.get("losses").is_some_and(Value::is_array))
    {
        return Err("This is not a dutchman.convert/1 report. Choose a per-file report saved by the desktop app or watch mode.".into());
    }
    Ok(crate::json::with_loss_groups(report))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    #[test]
    fn reads_version_one_reports_without_output_links() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("part.report.json");
        let report = serde_json::json!({"schema": "dutchman.convert/1", "ok": true, "losses": []});
        std::fs::write(&path, serde_json::to_vec(&report).unwrap()).unwrap();
        assert_eq!(super::read(&path).unwrap()["schema"], "dutchman.convert/1");
    }
}
