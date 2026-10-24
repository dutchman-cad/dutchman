//! Discover and run installed Dutchman CAD commands.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::{self, BufRead, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// An installed command and the executable selected for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    pub name: String,
    pub path: PathBuf,
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && !name.starts_with(['-', '.'])
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe().and_then(std::fs::canonicalize) {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_owned());
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    let current = std::env::current_dir().ok();
    dirs.into_iter()
        .filter_map(|dir| {
            if dir.is_absolute() {
                Some(dir)
            } else {
                current.as_ref().map(|base| base.join(dir))
            }
        })
        .collect()
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(windows)]
    {
        path.extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
    }
    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

/// Find a command beside the running executable, then in PATH order.
/// Names contain at most 64 lowercase ASCII letters, digits or hyphens and start with a letter or digit.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    if !valid_name(name) {
        return None;
    }
    let filename = format!("dutchman-{name}{}", std::env::consts::EXE_SUFFIX);
    search_dirs()
        .into_iter()
        .map(|dir| dir.join(&filename))
        .find(|path| executable(path))
}

/// List executable commands in search order, keeping the first of each name, sorted by name.
pub fn installed_tools() -> Vec<Tool> {
    let mut tools = BTreeMap::new();
    for dir in search_dirs() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let filename = entry.file_name();
            let Some(filename) = filename.to_str() else {
                continue;
            };
            #[cfg(windows)]
            let filename = match filename.rsplit_once('.') {
                Some((stem, ext)) if ext.eq_ignore_ascii_case("exe") => stem,
                _ => continue,
            };
            let Some(name) = filename.strip_prefix("dutchman-") else {
                continue;
            };
            if name != "launcher" && valid_name(name) && executable(&entry.path()) {
                tools.entry(name.to_owned()).or_insert_with(|| entry.path());
            }
        }
    }
    tools
        .into_iter()
        .map(|(name, path)| Tool { name, path })
        .collect()
}

/// Read the first stdout line of `--version`, allowing three seconds and at most 4096 bytes.
pub fn tool_version(path: &Path) -> Option<String> {
    let mut child = Command::new(path)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let stdout = child.stdout.take()?;
    let (sender, receiver) = std::sync::mpsc::channel();
    let reader = std::thread::Builder::new().spawn(move || {
        let mut line = Vec::new();
        let mut stdout = io::BufReader::new(stdout);
        let result = (&mut stdout).take(4096).read_until(b'\n', &mut line);
        // Drain remaining output so a second line cannot terminate the child with SIGPIPE.
        if result.is_ok() {
            let _ = io::copy(&mut stdout, &mut io::sink());
        }
        let _ = sender.send(result.ok().map(|_| line));
    });
    if reader.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return None;
    }
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) => return None,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let line = receiver
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .ok()??;
    let line = String::from_utf8(line).ok()?.trim().to_owned();
    (!line.is_empty()).then_some(line)
}

/// Replace this process on Unix; elsewhere wait with inherited standard streams.
/// Exit codes outside 0 through 255 become 1.
pub fn exec(path: &Path, args: &[OsString]) -> io::Result<i32> {
    let mut command = Command::new(path);
    command.args(args);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        Err(command.exec())
    }
    #[cfg(not(unix))]
    {
        let status = command.status()?;
        Ok(status
            .code()
            .filter(|code| (0..=255).contains(code))
            .unwrap_or(1))
    }
}

/// Explain where to install an absent command.
pub fn not_installed_message(name: &str) -> String {
    format!("dutchman: `dutchman {name}` is not installed; get it at https://github.com/dutchman-cad/{name}")
}

/// Format installed commands and their available version strings for help.
pub fn tools_listing() -> String {
    let tools = installed_tools();
    if tools.is_empty() {
        return String::new();
    }
    let mut text = String::from("Installed Dutchman CAD tools:\n");
    for tool in tools {
        text.push_str(&format!("  {}", tool.name));
        if let Some(version) = tool_version(&tool.path) {
            text.push_str(&format!("  {version}"));
        }
        text.push('\n');
    }
    text
}
