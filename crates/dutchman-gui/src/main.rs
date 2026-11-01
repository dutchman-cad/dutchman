//! `dutchman-gui`: the desktop app. See the crate's library for how it is built.

// No console window behind the app on Windows. `--version` and `--help` still print when the
// output is redirected; `dutchman --version` is the way to read the version from a terminal.
#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::process::ExitCode;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::time::Duration;

const GRAPHICS_HELP: &str =
    "Check that a desktop session and working graphics drivers are available \
    (X11 or Wayland with OpenGL on Linux). Run dutchman-gui from a terminal and include its \
    diagnostics when reporting startup trouble. You can still convert files with dutchman convert.";

const HELP: &str = "Convert native CAD designs into SolidCraft designs in a desktop window.

Usage: dutchman-gui [OPTIONS]

Options:
  -h, --help     Print help and exit
  -V, --version  Print version and exit

Run without options to open the window, then add designs or folders to convert.";

#[derive(Debug, PartialEq, Eq)]
enum Startup {
    Window,
    Help,
    Version,
}

/// Parses arguments after the program name without starting graphics or reading the environment.
fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Startup, String> {
    let mut startup = Startup::Window;
    for arg in args {
        match arg.to_str() {
            Some("--help" | "-h") => startup = Startup::Help,
            Some("--version" | "-V") if startup != Startup::Help => startup = Startup::Version,
            Some("--version" | "-V") => {}
            _ => {
                return Err(format!(
                    "unknown argument '{}'. Run dutchman-gui --help for usage.",
                    arg.to_string_lossy()
                ));
            }
        }
    }
    Ok(startup)
}

/// Stop a driver that stalls before eframe can return a startup error.
fn graphics_timed_out(ready: Receiver<()>, timeout: Duration) -> bool {
    matches!(ready.recv_timeout(timeout), Err(RecvTimeoutError::Timeout))
}

fn main() -> ExitCode {
    if let Some(code) = dutchman::worker::run_if_worker() {
        return code;
    }
    match parse_args(std::env::args_os().skip(1)) {
        Ok(Startup::Help) => {
            println!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(Startup::Version) => {
            println!("dutchman-gui {}", dutchman::BUILD_VERSION);
            return ExitCode::SUCCESS;
        }
        Err(err) => {
            eprintln!("dutchman-gui: {err}");
            return ExitCode::from(2);
        }
        Ok(Startup::Window) => {}
    }
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
        dutchman_gui::SETTINGS_NAME,
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
    fn startup_arguments_choose_text_or_a_window() {
        assert_eq!(parse_args([]), Ok(Startup::Window));
        for arg in ["--version", "-V"] {
            assert_eq!(parse_args([OsString::from(arg)]), Ok(Startup::Version));
        }
        for arg in ["--help", "-h"] {
            assert_eq!(parse_args([OsString::from(arg)]), Ok(Startup::Help));
        }
        for arg in ["--unknown", "part.f3d"] {
            let message = parse_args([OsString::from(arg)]).unwrap_err();
            assert!(message.contains(arg));
            assert!(message.contains("--help"));
        }
        assert_eq!(
            parse_args(["--version", "--help"].map(OsString::from)),
            Ok(Startup::Help)
        );
        assert!(parse_args(["--help", "--unknown"].map(OsString::from)).is_err());
    }

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
