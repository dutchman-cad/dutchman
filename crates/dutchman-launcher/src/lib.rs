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

/// Per-user module folder; resolving it never creates it.
pub fn modules_folder() -> Option<PathBuf> {
    module_folder_for(
        std::env::consts::OS,
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from),
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
    )
}

fn module_folder_for(
    os: &str,
    home: Option<PathBuf>,
    local: Option<PathBuf>,
    data: Option<PathBuf>,
) -> Option<PathBuf> {
    let nonempty = |path: &PathBuf| !path.as_os_str().is_empty();
    match os {
        "windows" => Some(local.filter(nonempty)?.join("Dutchman/modules")),
        "macos" => Some(
            home.filter(nonempty)?
                .join("Library/Application Support/Dutchman/modules"),
        ),
        _ => Some(
            data.filter(nonempty)
                .or_else(|| home.filter(nonempty).map(|h| h.join(".local/share")))?
                .join("dutchman/modules"),
        ),
    }
}

/// Search locations in priority order: program folder, user modules, then PATH.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe().and_then(std::fs::canonicalize) {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_owned());
        }
    }
    dirs.extend(modules_folder());
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

/// Find a command beside the running executable, then in the user modules folder, then PATH.
/// Names contain at most 64 lowercase ASCII letters, digits or hyphens and start with a letter or digit.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    if !valid_name(name) {
        return None;
    }
    find_in_dirs(name, search_dirs())
}

fn find_in_dirs(name: &str, dirs: Vec<PathBuf>) -> Option<PathBuf> {
    let filename = format!("dutchman-{name}{}", std::env::consts::EXE_SUFFIX);
    dirs.into_iter()
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

#[cfg(test)]
mod folder_tests {
    use super::*;

    #[test]
    fn priority_and_missing_folder() {
        let root = std::env::temp_dir().join(format!("dutchman-search-{}", std::process::id()));
        let dirs: Vec<_> = ["beside", "modules", "path"]
            .iter()
            .map(|p| root.join(p))
            .collect();
        for dir in &dirs {
            std::fs::create_dir_all(dir).unwrap();
            let file = dir.join(format!("dutchman-demo{}", std::env::consts::EXE_SUFFIX));
            std::fs::write(&file, "").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        for dir in &dirs {
            assert_eq!(
                find_in_dirs("demo", dirs.clone()),
                Some(dir.join(format!("dutchman-demo{}", std::env::consts::EXE_SUFFIX)))
            );
            std::fs::remove_dir_all(dir).unwrap();
        }
        assert_eq!(find_in_dirs("demo", dirs), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn platform_folders() {
        let home = || Some(PathBuf::from("/home/demo"));
        assert_eq!(
            module_folder_for("macos", home(), None, None),
            Some(PathBuf::from(
                "/home/demo/Library/Application Support/Dutchman/modules"
            ))
        );
        assert_eq!(
            module_folder_for(
                "windows",
                None,
                Some(PathBuf::from("C:/Users/demo/AppData/Local")),
                None
            ),
            Some(PathBuf::from(
                "C:/Users/demo/AppData/Local/Dutchman/modules"
            ))
        );
        for data in [None, Some(PathBuf::new())] {
            assert_eq!(
                module_folder_for("linux", home(), None, data),
                Some(PathBuf::from("/home/demo/.local/share/dutchman/modules"))
            );
        }
        assert_eq!(
            module_folder_for("linux", home(), None, Some(PathBuf::from("/data"))),
            Some(PathBuf::from("/data/dutchman/modules"))
        );
    }
}
