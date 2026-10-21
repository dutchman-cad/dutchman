//! `dutchman-gui`: the desktop app. See the crate's library for how it is built.

// No console window behind the app on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;

fn main() -> ExitCode {
    match eframe::run_native(
        "Dutchman",
        dutchman_gui::options(),
        Box::new(|cc| Ok(Box::new(dutchman_gui::App::new(cc)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("dutchman-gui: {err}");
            ExitCode::FAILURE
        }
    }
}
