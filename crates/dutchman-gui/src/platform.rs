//! Handing files to the rest of the desktop: opening a design in SolidCraft, and showing a file
//! in the system's file manager. Each starts a process and returns at once; the process is
//! waited on in the background, so it never holds up the window.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The environment variable that names the SolidCraft app, when it is not on `PATH`.
pub const APP_ENV: &str = "SOLIDCRAFT_APP";

/// Opens `design` in SolidCraft: the app `SOLIDCRAFT_APP` names, else `solidcraft` on `PATH`,
/// else whatever the system opens `.solidcraft` files with.
pub fn open_in_solidcraft(design: &Path) -> Result<(), String> {
    match solidcraft_app() {
        Some(app) => spawn(Command::new(app).arg(design)),
        None => open(design),
    }
}

/// Whether a SolidCraft app was found, to say so beside the button.
pub fn has_solidcraft_app() -> bool {
    solidcraft_app().is_some()
}

fn solidcraft_app() -> Option<PathBuf> {
    if let Some(app) = std::env::var_os(APP_ENV).map(PathBuf::from) {
        return app.is_file().then_some(app);
    }
    on_path(&OsString::from(if cfg!(windows) {
        "solidcraft.exe"
    } else {
        "solidcraft"
    }))
}

fn on_path(name: &OsString) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// Opens `path` with the system's default app for it.
pub fn open(path: &Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        spawn(Command::new("open").arg(path))
    } else if cfg!(windows) {
        spawn(Command::new("explorer").arg(path))
    } else {
        spawn(Command::new("xdg-open").arg(path))
    }
}

/// Shows `path` in the system's file manager: selected where the file manager can select it,
/// else its folder.
pub fn reveal(path: &Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        spawn(Command::new("open").arg("-R").arg(path))
    } else if cfg!(windows) {
        let mut select = OsString::from("/select,");
        select.push(path);
        spawn(Command::new("explorer").arg(select))
    } else {
        let folder = path.parent().unwrap_or(path);
        spawn(Command::new("xdg-open").arg(folder))
    }
}

/// Starts `command` and reaps it in the background.
fn spawn(command: &mut Command) -> Result<(), String> {
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .spawn()
        .map_err(|err| format!("could not start {program}: {err}"))?;
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}
