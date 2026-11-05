//! Handing files to the rest of the desktop: opening a design in SolidCraft, and showing a file
//! in the system's file manager. Each starts a process and returns at once; the process is
//! waited on in the background, so it never holds up the window.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The environment variable that names the SolidCraft app, when it is not on `PATH`.
pub const APP_ENV: &str = "SOLIDCRAFT_APP";

/// Opens `design` in SolidCraft: the app `SOLIDCRAFT_APP` names, else `solidcraft` on `PATH` or
/// beside this program,
/// else whatever the system opens the design's extension with.
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
    let name = OsString::from(if cfg!(windows) {
        "solidcraft.exe"
    } else {
        "solidcraft"
    });
    on_path(&name).or_else(|| {
        let app = std::env::current_exe().ok()?.parent()?.join(&name);
        app.is_file().then_some(app)
    })
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
        explorer(path, false)
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
        explorer(path, true)
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

/// Explorer requires native separators, including when an app built a mixed path.
pub fn windows_path(path: &str) -> String {
    path.replace('/', "\\")
}

pub fn display_path(path: &Path) -> String {
    let path = path.display().to_string();
    if cfg!(windows) {
        windows_path(&path)
    } else {
        path
    }
}

/// Explorer parses /select itself. Quote only the path, keeping /select,<path> together.
pub fn explorer_argument(path: &str, select: bool) -> String {
    let path = windows_path(path);
    format!("{}\"{path}\"", if select { "/select," } else { "" })
}

fn explorer(path: &Path, select: bool) -> Result<(), String> {
    let mut command = Command::new("explorer");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.raw_arg(explorer_argument(&path.to_string_lossy(), select));
    }
    #[cfg(not(windows))]
    command.arg(explorer_argument(&path.to_string_lossy(), select));
    spawn(&mut command)
}

/// Copy the complete run log; report a filesystem error without changing the conversion.
pub fn copy_log(source: &Path, destination: &Path) -> Result<(), String> {
    if source == destination
        || std::fs::canonicalize(source)
            .ok()
            .zip(std::fs::canonicalize(destination).ok())
            .is_some_and(|(a, b)| a == b)
    {
        return Ok(());
    }
    std::fs::copy(source, destination)
        .map(|_| ())
        .map_err(|e| format!("Could not copy the log: {e}. Choose another location."))
}

pub fn save_log(source: PathBuf) {
    std::thread::spawn(move || {
        let picked = pollster::block_on(
            rfd::AsyncFileDialog::new()
                .set_title("Copy log to…")
                .set_file_name(source.file_name().unwrap_or_default().to_string_lossy())
                .save_file(),
        );
        if let Some(file) = picked {
            if let Err(message) = copy_log(&source, file.path()) {
                pollster::block_on(
                    rfd::AsyncMessageDialog::new()
                        .set_title("Copy log")
                        .set_description(message)
                        .set_level(rfd::MessageLevel::Error)
                        .show(),
                );
            }
        }
    });
}

/// Shared by result and report cards: always acts on their own run's log.
pub fn log_controls(ui: &mut eframe::egui::Ui, path: &Path, ready: bool) -> Result<(), String> {
    // Named as the run log, so the path doesn't read as stray text; long paths are cut and
    // shown whole on hover.
    let shown = display_path(path);
    ui.add(
        eframe::egui::Label::new(
            eframe::egui::RichText::new(format!("Run log: {shown}"))
                .small()
                .weak(),
        )
        .truncate(),
    )
    .on_hover_text(&shown);
    // The first row is immutable. A batch can show hundreds of cards without rereading it
    // on every frame (a start row may list thousands of members).
    let key = eframe::egui::Id::new(("diagnostic version", path));
    let notice = ui
        .ctx()
        .data(|data| data.get_temp::<Option<String>>(key))
        .unwrap_or_else(|| {
            let notice = dutchman::diagnostics::version_notice(path);
            ui.ctx()
                .data_mut(|data| data.insert_temp(key, notice.clone()));
            notice
        });
    if let Some(notice) = notice {
        ui.label(notice);
    }
    let mut result = Ok(());
    ui.horizontal_wrapped(|ui| {
        if ui
            .add_enabled(ready, eframe::egui::Button::new("Show this run's log"))
            .on_disabled_hover_text("Available when this run ends.")
            .clicked()
        {
            result = reveal(path);
        }
        if ui
            .add_enabled(ready, eframe::egui::Button::new("Copy log to…"))
            .on_disabled_hover_text("Available when this run ends.")
            .clicked()
        {
            save_log(path.into());
        }
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explorer_paths_and_selection_cover_mixed_spaces_and_unc() {
        for (input, expected) in [
            (
                "C:/Users/u/AppData/Local/Dutchman/diagnostics",
                r"C:\Users\u\AppData\Local\Dutchman\diagnostics",
            ),
            (
                r"C:\Users\A B/Dutchman/diagnostics\run.jsonl",
                r"C:\Users\A B\Dutchman\diagnostics\run.jsonl",
            ),
            (
                r"\\server/share name/diagnostics\run.jsonl",
                r"\\server\share name\diagnostics\run.jsonl",
            ),
        ] {
            assert_eq!(windows_path(input), expected);
            assert_eq!(explorer_argument(input, false), format!("\"{expected}\""));
            assert_eq!(
                explorer_argument(input, true),
                format!("/select,\"{expected}\"")
            );
        }
    }
    #[cfg(windows)]
    #[test]
    fn explorer_raw_selection_is_not_requoted_by_command() {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new("explorer");
        command.raw_arg(explorer_argument(
            r"C:\Users\A B/Dutchman/diagnostics\run.jsonl",
            true,
        ));
        // std's Windows command debug output includes the actual raw command line.
        assert!(format!("{command:?}").contains("/select,"));
        assert_eq!(command.get_args().count(), 1);
        let argument = explorer_argument(r"C:\Users\A B/Dutchman/diagnostics\run.jsonl", true);
        let output = Command::new("cmd.exe")
            .args(["/D", "/C", "echo"])
            .raw_arg(&argument)
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), argument);
    }
    #[test]
    fn copying_log_preserves_name_contents_and_source() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir
            .path()
            .join("2026-11-02_163710_dutchman-0.2.6_failed.jsonl");
        let target = dir.path().join("copy.jsonl");
        std::fs::write(&source, "synthetic diagnostic\n").unwrap();
        copy_log(&source, &target).unwrap();
        assert_eq!(
            std::fs::read(&source).unwrap(),
            std::fs::read(&target).unwrap()
        );
        copy_log(&source, &source).unwrap();
        assert!(copy_log(&source, &dir.path().join("missing/copy")).is_err());
    }
}
