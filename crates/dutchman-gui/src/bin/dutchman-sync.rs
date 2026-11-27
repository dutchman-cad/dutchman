//! `dutchman-sync`: Dutchman Sync, a small background helper that keeps converted designs up to
//! date with their source files. It lives in the system tray (the menu bar on macOS).

// No console window behind the program on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::process::ExitCode;

use dutchman_gui::sync_helper::{app, login, window};

const HELP: &str = "Keep converted designs up to date with their source files.

Usage: dutchman-sync [OPTIONS]

Options:
      --window       Open only the window where a folder is chosen
      --background   Started at login: open no window
  -h, --help         Print help and exit
  -V, --version      Print version and exit

Run without options to start Dutchman Sync in the system tray. Starting it again opens its
window. Folders and settings are shared with the Dutchman app (docs/keep-up-to-date.md).";

fn main() -> ExitCode {
    if let Some(code) = dutchman::worker::run_if_worker() {
        return code;
    }
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let first = args.first().map(|arg| arg.to_string_lossy().into_owned());
    let result = match first.as_deref() {
        Some("--help" | "-h") => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Some("--version" | "-V") => {
            println!("dutchman-sync {}", dutchman::BUILD_VERSION);
            return ExitCode::SUCCESS;
        }
        Some(app::WINDOW_ARG) => window::run(),
        Some(login::BACKGROUND_ARG) => app::run_tray(true),
        None => app::run_tray(false),
        Some(other) => {
            eprintln!(
                "dutchman-sync: unknown argument '{other}'. Run dutchman-sync --help for usage."
            );
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("dutchman-sync: {why}");
            ExitCode::FAILURE
        }
    }
}
