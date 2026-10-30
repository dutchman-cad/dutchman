//! Fixed, name-free log locations and retention for automatically placed logs.

use std::collections::BTreeMap;
use std::io::{BufRead, Read};
use std::path::{Path, PathBuf};

/// The platform's diagnostic folder. Resolving it does not create it.
pub fn default_folder() -> Result<PathBuf, String> {
    let home = || {
        std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| "cannot find the home folder; use --diagnostics PATH".to_owned())
    };
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA")
            .filter(|value| !value.is_empty())
            .map(|base| PathBuf::from(base).join("Dutchman/diagnostics"))
            .ok_or_else(|| "cannot find LOCALAPPDATA; use --diagnostics PATH".to_owned())
    } else if cfg!(target_os = "macos") {
        Ok(home()?.join("Library/Logs/Dutchman"))
    } else {
        let base = std::env::var_os("XDG_STATE_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .map_or_else(|| home().map(|path| path.join(".local/state")), Ok)?;
        Ok(base.join("dutchman/diagnostics"))
    }
}

/// Create a uniquely named log at local start time and retain the newest ten logs.
pub fn new_default_log() -> Result<PathBuf, String> {
    let folder = default_folder()?;
    let stamp = local_timestamp()?;
    create_log(&folder, &stamp)
}

fn local_timestamp() -> Result<String, String> {
    let mut command = if cfg!(windows) {
        let mut command = std::process::Command::new("powershell");
        command.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-Date -Format 'yyyy-MM-dd_HHmmss'",
        ]);
        command
    } else {
        let mut command = std::process::Command::new("date");
        command.arg("+%Y-%m-%d_%H%M%S");
        command
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|err| format!("cannot read local start time: {err}; use --diagnostics PATH"))?;
    let stamp = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || !log_name(&format!("{stamp}.jsonl")) {
        return Err("cannot read local start time; use --diagnostics PATH".into());
    }
    Ok(stamp)
}

fn create_log(folder: &Path, stamp: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(folder).map_err(|err| {
        format!(
            "cannot create diagnostic folder {}: {err}",
            folder.display()
        )
    })?;
    let latest = logs(folder)
        .into_iter()
        .filter_map(|path| {
            let name = path.file_stem()?.to_str()?;
            if name.get(..17)? != stamp {
                return None;
            }
            name.get(18..)
                .map_or(Some(0_u64), |suffix| suffix.parse().ok())
        })
        .max();
    let first = match latest {
        Some(counter) => counter
            .checked_add(1)
            .ok_or("cannot choose a unique diagnostic log name; use --diagnostics PATH")?,
        None => 0,
    };
    for offset in 0..10_000 {
        let counter = first
            .checked_add(offset)
            .ok_or("cannot choose a unique diagnostic log name; use --diagnostics PATH")?;
        let name = if counter == 0 {
            format!("{stamp}.jsonl")
        } else {
            format!("{stamp}-{counter}.jsonl")
        };
        let path = folder.join(name);
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => {
                retain(folder, &path)?;
                return Ok(path);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(format!(
                    "cannot create diagnostic log {}: {err}",
                    path.display()
                ))
            }
        }
    }
    Err("cannot choose a unique diagnostic log name; use --diagnostics PATH".into())
}

fn log_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".jsonl") else {
        return false;
    };
    let Some(date) = stem.get(..17) else {
        return false;
    };
    if !date.bytes().enumerate().all(|(i, byte)| match i {
        4 | 7 => byte == b'-',
        10 => byte == b'_',
        _ => byte.is_ascii_digit(),
    }) {
        return false;
    }
    let suffix = stem.get(17..).unwrap_or_default();
    suffix.is_empty()
        || suffix
            .strip_prefix('-')
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
}

fn logs(folder: &Path) -> Vec<PathBuf> {
    let mut logs: Vec<_> = std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter(|entry| entry.file_name().to_str().is_some_and(log_name))
        .map(|entry| entry.path())
        .collect();
    logs.sort_by_cached_key(|path| {
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let stamp = name.get(..17).unwrap_or_default().to_owned();
        let counter = name
            .get(18..)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        (
            std::fs::metadata(path).and_then(|m| m.modified()).ok(),
            stamp,
            counter,
        )
    });
    logs.reverse();
    logs
}

fn retain(folder: &Path, current: &Path) -> Result<(), String> {
    // Keep the new file even on a filesystem with coarse modification timestamps.
    let older: Vec<_> = logs(folder)
        .into_iter()
        .filter(|path| path != current)
        .collect();
    for path in older.into_iter().skip(9) {
        std::fs::remove_file(&path)
            .map_err(|err| format!("cannot remove old diagnostic log {}: {err}", path.display()))?;
    }
    Ok(())
}

/// Newest default log with an unfinished conversion, for the app's launch notice.
pub fn unfinished_default_log() -> Option<PathBuf> {
    let path = logs(&default_folder().ok()?).into_iter().next()?;
    unfinished(&path).then_some(path)
}

fn unfinished(path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut active = BTreeMap::<String, u64>::new();
    let mut ended = false;
    let mut reader = std::io::BufReader::new(file);
    loop {
        // A bounded start record can list 10,000 members. Refuse a tampered oversized
        // line rather than allocating without limit while opening the desktop app.
        let mut line = String::new();
        match reader.by_ref().take(4 << 20).read_line(&mut line) {
            Ok(0) => break,
            Ok(_) if line.ends_with('\n') => {}
            _ => return true,
        }
        let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) else {
            return true;
        };
        let Some(id) = record.get("file").and_then(serde_json::Value::as_str) else {
            continue;
        };
        match record.get("type").and_then(serde_json::Value::as_str) {
            Some("start") => {
                if active.len() >= 100_000 && !active.contains_key(id) {
                    return true;
                }
                *active.entry(id.to_owned()).or_default() += 1;
            }
            Some("end") => {
                ended = true;
                if let Some(count) = active.get_mut(id) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        active.remove(id);
                    }
                }
            }
            _ => {}
        }
    }
    !ended || active.values().any(|count| *count > 0)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn unique_names_retention_and_unrelated_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("notes.jsonl"), "keep").unwrap();
        std::fs::create_dir(dir.path().join("2026-10-29_201500-99.jsonl")).unwrap();
        let mut paths = Vec::new();
        for _ in 0..12 {
            paths.push(create_log(dir.path(), "2026-10-29_201500").unwrap());
        }
        assert_eq!(logs(dir.path()).len(), 10);
        let unique: std::collections::BTreeSet<_> = paths.iter().collect();
        assert_eq!(unique.len(), paths.len());
        assert!(paths.last().unwrap().is_file());
        assert!(paths.iter().take(2).all(|path| !path.exists()));
        assert!(paths.iter().skip(2).all(|path| path.is_file()));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("notes.jsonl")).unwrap(),
            "keep"
        );
        assert!(dir.path().join("2026-10-29_201500-99.jsonl").is_dir());
        for name in [
            "report.jsonl",
            "2026-10-29_201500-private.jsonl",
            "2026-10-29_201500.jsonl.bak",
        ] {
            assert!(!log_name(name));
        }
    }

    #[test]
    fn recovery_tracks_all_conversions_and_truncated_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log");
        std::fs::write(&path, "").unwrap();
        assert!(unfinished(&path));
        std::fs::write(&path, "{\"type\":\"start\",\"file\":\"f1\"}\n{\"type\":\"start\",\"file\":\"f2\"}\n{\"type\":\"end\",\"file\":\"f2\"}\n").unwrap();
        assert!(unfinished(&path));
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{{\"type\":\"end\",\"file\":\"f1\"}}").unwrap();
        assert!(!unfinished(&path));
        write!(file, "{{\"type\":").unwrap();
        assert!(unfinished(&path));
    }
}
