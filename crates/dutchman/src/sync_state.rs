//! The state file Dutchman Sync shares with the Dutchman app: which folders are kept up to
//! date, whether syncing is paused, and whether Sync starts at login.
//!
//! One file, `<platform config dir>/dutchman/sync-state.json`, read and written by the app's
//! switch, the Sync tray menu and the launcher, so they always agree. It is written
//! atomically; a missing or unreadable file reads as the empty state, and is replaced whole the
//! next time something is saved.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The file's schema.
pub const SCHEMA: &str = "dutchman.sync-state/1";

/// The file's name inside the Dutchman config folder.
pub const FILE_NAME: &str = "sync-state.json";

/// One folder of converted designs that Sync watches.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Folder {
    /// The output folder: the one that holds the designs and their link records.
    #[serde(default)]
    pub output: PathBuf,
    /// The folder of CAD files the designs were converted from, when it was chosen as one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PathBuf>,
    /// Whether a changed source is converted again. Off: Sync only tells you.
    #[serde(default = "yes")]
    pub auto_update: bool,
    /// Whether Sync watches the folder. Quitting Dutchman Sync turns this off for every folder.
    #[serde(default)]
    pub enabled: bool,
    /// When Sync last looked at the folder, as RFC 3339 UTC (`2026-11-22T09:30:00Z`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_checked: Option<String>,
}

/// What the state file holds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default = "schema")]
    pub schema: String,
    #[serde(default)]
    pub folders: Vec<Folder>,
    /// Sync is paused: nothing is checked or converted.
    #[serde(default)]
    pub paused: bool,
    /// Dutchman Sync starts when the user logs in.
    #[serde(default)]
    pub start_at_login: bool,
}

fn yes() -> bool {
    true
}

fn schema() -> String {
    SCHEMA.to_owned()
}

impl Default for State {
    fn default() -> Self {
        Self {
            schema: schema(),
            folders: Vec::new(),
            paused: false,
            start_at_login: false,
        }
    }
}

impl State {
    /// The folder whose output is `output`.
    pub fn folder_mut(&mut self, output: &Path) -> Option<&mut Folder> {
        self.folders.iter_mut().find(|f| f.output == output)
    }

    /// Adds the folder, or turns it on (keeping its other settings) when it is already there.
    pub fn add_folder(&mut self, output: PathBuf, source: Option<PathBuf>) {
        if let Some(folder) = self.folder_mut(&output) {
            folder.enabled = true;
            if source.is_some() {
                folder.source = source;
            }
        } else {
            self.folders.push(Folder {
                output,
                source,
                auto_update: true,
                enabled: true,
                last_checked: None,
            });
        }
    }

    /// Turns every folder off, as quitting Dutchman Sync does.
    pub fn disable_all(&mut self) {
        for folder in &mut self.folders {
            folder.enabled = false;
        }
    }

    /// The folders Sync is to watch.
    pub fn enabled(&self) -> impl Iterator<Item = &Folder> {
        self.folders.iter().filter(|f| f.enabled)
    }
}

/// The folder the state file lives in: `dutchman` inside the platform's per-user config
/// folder (`%APPDATA%` on Windows, `~/Library/Application Support` on macOS,
/// `$XDG_CONFIG_HOME` or `~/.config` elsewhere). `None` when the platform names none.
pub fn config_dir() -> Option<PathBuf> {
    let var = |name: &str| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let base = if cfg!(windows) {
        var("APPDATA")?
    } else if cfg!(target_os = "macos") {
        var("HOME")?.join("Library").join("Application Support")
    } else {
        var("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| var("HOME").map(|home| home.join(".config")))?
    };
    Some(base.join("dutchman"))
}

/// The state file's path.
pub fn path() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join(FILE_NAME))
}

/// Reads the state at `path`. A missing, unreadable or corrupt file, or one with another
/// schema's major version, is the empty state.
pub fn read_from(path: &Path) -> State {
    let Ok(bytes) = std::fs::read(path) else {
        return State::default();
    };
    match serde_json::from_slice::<State>(&bytes) {
        Ok(state) if state.schema.starts_with("dutchman.sync-state/1") => state,
        _ => State::default(),
    }
}

/// Writes `state` to `path` atomically: a temporary file in the same folder, renamed over the
/// old one, so a reader never sees half a file.
pub fn write_to(path: &Path, state: &State) -> Result<(), String> {
    let dir = path.parent().ok_or("the state file has no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let mut json = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    json.push(b'\n');
    let mut temp = tempfile::Builder::new()
        .prefix(".sync-state-")
        .suffix(".tmp")
        .tempfile_in(dir)
        .map_err(|e| format!("cannot write in {}: {e}", dir.display()))?;
    std::io::Write::write_all(&mut temp, &json)
        .and_then(|()| temp.as_file().sync_all())
        .map_err(|e| format!("cannot write the state: {e}"))?;
    temp.persist(path)
        .map_err(|e| format!("cannot replace {}: {}", path.display(), e.error))?;
    Ok(())
}

/// Reads the shared state file.
pub fn read() -> State {
    path().map_or_else(State::default, |path| read_from(&path))
}

/// Held while this process reads, edits and writes the state back, so two of its threads never
/// lose each other's change.
static UPDATING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Reads the state at `path`, lets `change` edit it, and writes it back.
pub fn update_at(path: &Path, change: impl FnOnce(&mut State)) -> Result<(), String> {
    let _guard = UPDATING.lock().unwrap_or_else(|e| e.into_inner());
    let mut state = read_from(path);
    change(&mut state);
    write_to(path, &state)
}

/// Reads the shared state file, lets `change` edit it, and writes it back.
pub fn update(change: impl FnOnce(&mut State)) -> Result<(), String> {
    let path = path().ok_or("cannot find the per-user config folder")?;
    update_at(&path, change)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_every_field() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("nested").join(FILE_NAME);
        let state = State {
            schema: SCHEMA.to_owned(),
            folders: vec![Folder {
                output: PathBuf::from("/out"),
                source: Some(PathBuf::from("/src")),
                auto_update: false,
                enabled: true,
                last_checked: Some("2026-11-22T09:30:00Z".to_owned()),
            }],
            paused: true,
            start_at_login: true,
        };
        write_to(&file, &state).unwrap();
        assert_eq!(read_from(&file), state);
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("\"schema\": \"dutchman.sync-state/1\""));
        // No temporary file is left beside it.
        assert_eq!(
            std::fs::read_dir(file.parent().unwrap()).unwrap().count(),
            1
        );
    }

    #[test]
    fn missing_or_corrupt_reads_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        assert_eq!(read_from(&file), State::default());
        std::fs::write(&file, b"{ not json").unwrap();
        assert_eq!(read_from(&file), State::default());
        std::fs::write(
            &file,
            b"{\"schema\":\"dutchman.sync-state/2\",\"paused\":true}",
        )
        .unwrap();
        assert_eq!(read_from(&file), State::default());
        // A corrupt file is replaced by the next write.
        write_to(&file, &State::default()).unwrap();
        assert_eq!(read_from(&file), State::default());
    }

    #[test]
    fn absent_fields_take_their_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(FILE_NAME);
        std::fs::write(
            &file,
            br#"{"schema":"dutchman.sync-state/1","folders":[{"output":"/o"}]}"#,
        )
        .unwrap();
        let state = read_from(&file);
        assert!(!state.paused && !state.start_at_login);
        let folder = state.folders.first().unwrap();
        assert!(folder.auto_update && !folder.enabled);
        assert_eq!(folder.source, None);
    }

    #[test]
    fn adding_a_known_folder_turns_it_on_and_quitting_turns_all_off() {
        let mut state = State::default();
        state.add_folder("/o".into(), Some("/s".into()));
        state.folder_mut(Path::new("/o")).unwrap().auto_update = false;
        state.disable_all();
        assert_eq!(state.enabled().count(), 0);
        state.add_folder("/o".into(), None);
        assert_eq!(state.folders.len(), 1);
        let folder = state.enabled().next().unwrap();
        assert!(!folder.auto_update);
        assert_eq!(folder.source.as_deref(), Some(Path::new("/s")));
    }
}
