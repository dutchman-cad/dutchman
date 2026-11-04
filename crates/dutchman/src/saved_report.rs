//! Local conversion reports saved beside a design. Reading never opens CAD files.

use std::io::{Read, Write};
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

/// Publish a complete report, keeping the previous report if writing fails.
pub fn save(output: &Path, report: &Value) -> Result<(), String> {
    let target = path(output, report.get("ok") == Some(&Value::Bool(true)));
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let write = || -> Result<(), Box<dyn std::error::Error>> {
        std::fs::create_dir_all(parent)?;
        let mut file = tempfile::NamedTempFile::new_in(parent)?;
        serde_json::to_writer_pretty(&mut file, report)?;
        file.write_all(b"\n")?;
        file.persist(&target)?;
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
