//! The state file the Dutchman app, Dutchman Sync (the background helper) and the launcher share,
//! so they agree on which output folders are kept up to date.
//!
//! `<config folder>/sync-state.json`, schema `dutchman.sync-state/1`:
//!
//! ```json
//! {"schema": "dutchman.sync-state/1",
//!  "folders": [{"output": "/home/ada/out", "source": "/home/ada/cad", "auto_update": true,
//!               "enabled": true, "last_checked": "2026-11-22T10:00:00Z"}]}
//! ```
//!
//! `output` is the folder of converted designs (its link records say where each source is);
//! `source` is the folder of CAD files the helper was told to watch, when it was told one;
//! `enabled` is the app's "Keep this folder up to date" switch; `auto_update` off means Sync
//! only says a design changed and waits to be asked; `last_checked` is when Sync last looked.
//!
//! A missing or unreadable file is an empty state, never an error: the next write replaces it.
//! Writes replace the file atomically, so a reader never sees half a file.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// The file's schema.
pub const SCHEMA: &str = "dutchman.sync-state/1";

/// One output folder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    /// The folder of converted designs.
    pub output: PathBuf,
    /// The folder of CAD files watched, when one was chosen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    /// Whether Sync converts changed designs by itself.
    #[serde(default = "yes")]
    pub auto_update: bool,
    /// Whether the folder is kept up to date at all.
    #[serde(default)]
    pub enabled: bool,
    /// When Sync last looked at the folder (RFC 3339, UTC).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked: Option<String>,
}

fn yes() -> bool {
    true
}

impl Folder {
    /// A folder switched on, converting by itself.
    pub fn new(output: PathBuf) -> Self {
        Self {
            output,
            source: None,
            auto_update: true,
            enabled: true,
            last_checked: None,
        }
    }

    /// Notes that the folder was looked at just now.
    pub fn checked_now(&mut self) {
        self.last_checked = Some(crate::link::rfc3339(SystemTime::now()));
    }
}

/// Everything in the file.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub folders: Vec<Folder>,
}

#[derive(Serialize)]
struct OnDisk<'a> {
    schema: &'static str,
    folders: &'a [Folder],
}

/// Whether two folder paths name the same place, as written (a trailing separator or a relative
/// path does not make a folder a different one).
fn same(a: &Path, b: &Path) -> bool {
    let clean = |path: &Path| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    clean(a).components().eq(clean(b).components())
}

impl State {
    /// The entry for `output`.
    pub fn folder(&self, output: &Path) -> Option<&Folder> {
        self.folders.iter().find(|f| same(&f.output, output))
    }

    /// The entry for `output`, added switched off when there is none.
    #[allow(clippy::indexing_slicing)] // the index is of an entry found or pushed just above
    pub fn folder_mut(&mut self, output: &Path) -> &mut Folder {
        let index = self
            .folders
            .iter()
            .position(|f| same(&f.output, output))
            .unwrap_or_else(|| {
                let mut folder = Folder::new(output.to_path_buf());
                folder.enabled = false;
                self.folders.push(folder);
                self.folders.len() - 1
            });
        &mut self.folders[index]
    }

    /// Whether `output` is kept up to date.
    pub fn enabled(&self, output: &Path) -> bool {
        self.folder(output).is_some_and(|f| f.enabled)
    }

    /// Switches keeping `output` up to date on or off.
    pub fn set_enabled(&mut self, output: &Path, enabled: bool) {
        self.folder_mut(output).enabled = enabled;
    }
}

/// Where the file lives: `<config folder>/dutchman/sync-state.json`, the config folder being
/// `%APPDATA%` on Windows, `~/Library/Application Support` on macOS, and `$XDG_CONFIG_HOME` (else
/// `~/.config`) elsewhere. `DUTCHMAN_SYNC_STATE` names another file, for tests and scripts.
pub fn default_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("DUTCHMAN_SYNC_STATE").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        var("APPDATA").ok_or("cannot find APPDATA")?
    } else if cfg!(target_os = "macos") {
        var("HOME")
            .ok_or("cannot find the home folder")?
            .join("Library/Application Support")
    } else {
        var("XDG_CONFIG_HOME")
            .filter(|path| path.is_absolute())
            .map_or_else(
                || {
                    var("HOME")
                        .map(|home| home.join(".config"))
                        .ok_or("cannot find the home folder")
                },
                Ok,
            )?
    };
    Ok(base.join("dutchman").join("sync-state.json"))
}

/// The state in `path`; empty when the file is missing, unreadable, not JSON or of another schema.
pub fn load(path: &Path) -> State {
    let Ok(bytes) = std::fs::read(path) else {
        return State::default();
    };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
        return State::default();
    };
    if value
        .get("schema")
        .and_then(serde_json::Value::as_str)
        .is_none_or(|schema| !schema.starts_with("dutchman.sync-state/"))
    {
        return State::default();
    }
    // One folder that does not parse costs that folder, not the others.
    let folders = value
        .get("folders")
        .and_then(serde_json::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|item| serde_json::from_value::<Folder>(item.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    State { folders }
}

/// Writes `state` to `path` atomically, making its folder first.
pub fn save(path: &Path, state: &State) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot make {}: {e}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(&OnDisk {
        schema: SCHEMA,
        folders: &state.folders,
    })
    .map_err(|e| e.to_string())?;
    text.push('\n');
    crate::watch::atomic_write(path, text.as_bytes())
        .map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// Reads the state, lets `change` edit it, and writes it back, so one writer does not undo what
/// another wrote since it last looked.
pub fn update(path: &Path, change: impl FnOnce(&mut State)) -> Result<State, String> {
    let mut state = load(path);
    change(&mut state);
    save(path, &state)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_or_corrupt_file_is_an_empty_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        assert_eq!(load(&path), State::default());
        std::fs::write(&path, b"{not json").unwrap();
        assert_eq!(load(&path), State::default());
        std::fs::write(
            &path,
            br#"{"schema":"other/1","folders":[{"output":"/a"}]}"#,
        )
        .unwrap();
        assert_eq!(load(&path), State::default());
    }

    #[test]
    fn the_switch_round_trips_and_keeps_other_folders() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep/er/sync-state.json");
        update(&path, |state| state.set_enabled(Path::new("/out/a"), true)).unwrap();
        let state = update(&path, |state| {
            state.set_enabled(Path::new("/out/b"), true);
            state.folder_mut(Path::new("/out/a")).source = Some(PathBuf::from("/cad"));
            state.folder_mut(Path::new("/out/a")).checked_now();
        })
        .unwrap();
        let loaded = load(&path);
        assert_eq!(loaded, state);
        assert!(loaded.enabled(Path::new("/out/a")) && loaded.enabled(Path::new("/out/b")));
        assert!(loaded.folder(Path::new("/out/a")).unwrap().auto_update);
        update(&path, |state| state.set_enabled(Path::new("/out/a"), false)).unwrap();
        let loaded = load(&path);
        assert!(!loaded.enabled(Path::new("/out/a")) && loaded.enabled(Path::new("/out/b")));
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains(r#""schema": "dutchman.sync-state/1""#));
    }

    #[test]
    fn a_folder_that_does_not_parse_costs_only_itself() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sync-state.json");
        std::fs::write(
            &path,
            br#"{"schema":"dutchman.sync-state/1","folders":[{"output":5},{"output":"/ok","enabled":true}]}"#,
        )
        .unwrap();
        let state = load(&path);
        assert_eq!(state.folders.len(), 1);
        assert!(state.enabled(Path::new("/ok")));
        assert!(state.folder(Path::new("/ok")).unwrap().auto_update);
    }
}
