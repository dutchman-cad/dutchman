//! Dutchman Sync as a program: the tray process, and the window it opens.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::mpsc::channel;
use std::sync::Arc;

use dutchman::sync::WatchSettings;
use dutchman::sync_state;

use super::controller::{self, Msg, Shell};
use super::engine::{RealEngine, Supervisor};
use super::instance;
use super::notify::Desktop;
use super::tray::{self, TrayControl};

/// The window's argument: open the folder window and nothing else.
pub const WINDOW_ARG: &str = "--window";

/// The converter program: the `dutchman` command beside this program (or in the macOS app's
/// Helpers folder), else this program itself, which converts one file in a child of its own
/// as the desktop app does.
pub fn converter(exe: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "dutchman.exe"
    } else {
        "dutchman"
    };
    let beside = exe.parent().map(|dir| dir.join(name));
    let helpers = exe
        .parent()
        .and_then(Path::parent)
        .map(|contents| contents.join("Helpers").join(name));
    [beside, helpers]
        .into_iter()
        .flatten()
        .find(|path| path.is_file())
        .unwrap_or_else(|| exe.to_path_buf())
}

/// The Dutchman app's program: beside this one, as the archives and the AppImage lay them out.
pub fn dutchman_app(exe: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) {
        "dutchman-gui.exe"
    } else {
        "dutchman-gui"
    };
    exe.parent()
        .map(|dir| dir.join(name))
        .filter(|path| path.is_file())
}

struct TrayShell {
    exe: PathBuf,
    control: Arc<dyn TrayControl>,
    window: Option<Child>,
}

impl Shell for TrayShell {
    fn show_window(&mut self) {
        // A window that is still open is left to the user; there is nothing to raise it with.
        if let Some(child) = &mut self.window {
            if matches!(child.try_wait(), Ok(None)) {
                return;
            }
        }
        self.window = Command::new(&self.exe)
            .arg(WINDOW_ARG)
            .spawn()
            .map_err(|e| eprintln!("dutchman-sync: cannot open the window: {e}"))
            .ok();
    }

    fn open_app(&mut self) {
        let started = if let Some(app) = dutchman_app(&self.exe) {
            Command::new(app).spawn().map(|_| ())
        } else if cfg!(target_os = "macos") {
            Command::new("open")
                .args(["-a", "Dutchman Convert"])
                .spawn()
                .map(|_| ())
        } else {
            Err(std::io::Error::other(
                "the Dutchman app is not installed beside Dutchman Sync",
            ))
        };
        if let Err(e) = started {
            eprintln!("dutchman-sync: cannot open Dutchman: {e}");
        }
    }

    fn show_paused(&mut self, paused: bool) {
        self.control.set_paused(paused);
    }

    fn finished(&mut self) {
        self.control.exit();
    }
}

/// Runs Dutchman Sync in the tray. `background` is set when the system started it at login.
pub fn run_tray(background: bool) -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find the program: {e}"))?;
    let state_file = sync_state::path().ok_or("cannot find the per-user config folder")?;
    let dir = state_file
        .parent()
        .ok_or("the state file has no folder")?
        .to_path_buf();
    let guard = match instance::launch(&dir) {
        Ok(instance::Launch::First(guard)) => guard,
        // The running copy was asked to open its window.
        Ok(instance::Launch::Running) => return Ok(()),
        Err(e) => {
            return Err(format!(
                "cannot tell whether Dutchman Sync is already running: {e}"
            ))
        }
    };
    let (messages, receiver) = channel();
    let (show, shown) = channel();
    guard
        .serve(show)
        .map_err(|e| format!("cannot listen for a second launch: {e}"))?;
    let forward = messages.clone();
    std::thread::spawn(move || {
        while shown.recv().is_ok() {
            if forward.send(Msg::ShowWindow).is_err() {
                break;
            }
        }
    });
    let engine = Arc::new(RealEngine {
        settings: WatchSettings::new(converter(&exe)),
    });
    let result = tray::run(messages, move |control| {
        let mut supervisor = Supervisor::new(engine, Arc::new(Desktop), Some(state_file.clone()));
        let mut shell = TrayShell {
            exe,
            control,
            window: None,
        };
        controller::run(
            &receiver,
            &mut supervisor,
            &mut shell,
            &state_file,
            background,
        );
    });
    drop(guard);
    result
}
