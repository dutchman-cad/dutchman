//! `dutchman-gui`: the desktop app. See the crate's library for how it is built.

// No console window behind the app on Windows.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::process::ExitCode;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::Duration;

const GRAPHICS_HELP: &str =
    "Check that a desktop session and working graphics drivers are available \
    (X11 or Wayland with OpenGL on Linux). Run dutchman-gui from a terminal and include its \
    diagnostics when reporting startup trouble. You can still convert files with dutchman convert.";

/// Stop a driver that stalls before eframe can return a startup error.
fn graphics_timed_out(ready: Receiver<()>, timeout: Duration) -> bool {
    matches!(ready.recv_timeout(timeout), Err(RecvTimeoutError::Timeout))
}

fn main() -> ExitCode {
    let (ready, receiver) = channel();
    let watchdog = match std::thread::Builder::new()
        .name("graphics-startup".to_owned())
        .spawn(move || {
            if graphics_timed_out(receiver, Duration::from_secs(30)) {
                eprintln!(
                    "dutchman-gui: graphics did not initialize within 30 seconds. {GRAPHICS_HELP}"
                );
                std::process::exit(1);
            }
        }) {
        Ok(watchdog) => watchdog,
        Err(err) => {
            eprintln!("dutchman-gui: cannot monitor graphics startup: {err}");
            return ExitCode::FAILURE;
        }
    };
    let initialized = ready.clone();
    let result = eframe::run_native(
        "Dutchman",
        dutchman_gui::options(),
        Box::new(move |cc| {
            // eframe creates the window and renderer before calling the app creator.
            let _ = initialized.send(());
            Ok(Box::new(dutchman_gui::App::new(cc)))
        }),
    );
    // An early error (or a normal close) also cancels the startup deadline.
    drop(ready);
    let _ = watchdog.join();
    finish(result)
}

fn finish(result: eframe::Result) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("dutchman-gui: cannot run the desktop window: {err}. {GRAPHICS_HELP}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphics_startup_has_a_deadline_until_ready_or_cancelled() {
        let (ready, receiver) = channel();
        assert!(graphics_timed_out(receiver, Duration::ZERO));
        let (initialized, receiver) = channel();
        assert!(initialized.send(()).is_ok());
        assert!(!graphics_timed_out(receiver, Duration::ZERO));
        let (cancelled, receiver) = channel();
        drop(cancelled);
        assert!(!graphics_timed_out(receiver, Duration::ZERO));
        drop(ready);
    }

    #[test]
    fn a_window_error_returns_failure() {
        assert_eq!(finish(Ok(())), ExitCode::SUCCESS);
        let error = eframe::Error::AppCreation(Box::new(std::io::Error::other("no renderer")));
        assert_eq!(finish(Err(error)), ExitCode::FAILURE);
        assert!(GRAPHICS_HELP.contains("OpenGL"));
        assert!(GRAPHICS_HELP.contains("dutchman convert"));
    }
}
