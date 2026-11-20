//! CLI share downloads need cleanup even when an intentional exit bypasses destructors.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

static ACTIVE: Mutex<Option<PathBuf>> = Mutex::new(None);

pub(super) struct Scratch(tempfile::TempDir);

impl Scratch {
    pub(super) fn new() -> Result<Self, String> {
        // ctrlc runs this closure on its own ordinary thread, never in a raw signal handler.
        ctrlc::set_handler(|| exit(130)).map_err(|_| {
            "Cannot listen for cancellation. Download the file yourself and convert it locally."
                .to_owned()
        })?;
        let mut active = ACTIVE.lock().unwrap_or_else(|error| error.into_inner());
        let folder = tempfile::Builder::new()
            .prefix("dutchman-share-cli-")
            .tempdir()
            .map_err(|_| {
                "Cannot create a temporary download folder. Check free disk space.".to_owned()
            })?;
        *active = Some(folder.path().to_path_buf());
        Ok(Self(folder))
    }

    pub(super) fn path(&self) -> &Path {
        self.0.path()
    }
}

/// Also used before broken-pipe and memory-refusal exits. Only the private scratch root
/// is registered here; converted outputs are never owned by this cleanup path.
pub(super) fn cleanup() {
    let mut active = ACTIVE.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(path) = active.as_ref() {
        // Keep the path registered if removal fails, so Drop can retry it.
        if std::fs::remove_dir_all(path).is_ok() || !path.exists() {
            *active = None;
        }
    }
}

pub(super) fn exit(code: i32) -> ! {
    cleanup();
    std::process::exit(code)
}

impl Drop for Scratch {
    fn drop(&mut self) {
        cleanup();
        // TempDir's destructor is the final backstop, including after a failed removal.
    }
}

#[cfg(test)]
mod tests;
