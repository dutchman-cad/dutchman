//! Handing files to the rest of the desktop: opening a design in SolidCraft, and showing a file
//! in the system's file manager. Each starts a process and returns at once; the process is
//! waited on in the background, so it never holds up the window.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The environment variable that names the SolidCraft app, when it is not on `PATH`.
pub const APP_ENV: &str = "SOLIDCRAFT_APP";

/// How a design was opened, so the window can say when the system default stood in.
#[derive(Debug, PartialEq, Eq)]
pub enum Opened {
    /// By the SolidCraft the version pill names, or the app found by `SOLIDCRAFT_APP` or `PATH`.
    SolidCraft,
    /// By the system's app for the design, because no SolidCraft was chosen or found.
    SystemDefault,
}

/// What to run to open a design in the SolidCraft a person chose.
#[derive(Debug, PartialEq, Eq)]
pub enum Launch {
    /// A program and its arguments, the design last.
    Run(PathBuf, Vec<OsString>),
    /// The chosen install has a command-line tool and no app beside it.
    CliOnly(String),
}

/// The platforms [`launch_for`] knows how to open an app on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    Mac,
    Windows,
    Linux,
}

impl Os {
    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// The message for a chosen `solidcraft-cli` that has no SolidCraft app next to it.
fn cli_only_message(cli: &Path) -> String {
    format!(
        "The SolidCraft you chose ({}) is a command-line install with no SolidCraft app next to it, so Dutchman cannot open the design in it. Install the SolidCraft app of that release, or choose a different solidcraft-cli in the SolidCraft version window.",
        cli.display()
    )
}

/// The Flatpak app ID when `cli` lives inside a Flatpak install (`…/flatpak/app/<id>/…`).
fn flatpak_id(cli: &Path) -> Option<String> {
    let parts: Vec<_> = cli
        .components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect();
    let at = parts
        .windows(2)
        .position(|w| matches!(w, [a, b] if a == "flatpak" && b == "app"))?;
    let id = parts.get(at + 2)?;
    id.contains('.').then(|| id.to_string())
}

/// How to open `design` in the SolidCraft whose `solidcraft-cli` is `cli`: the macOS bundle it
/// sits in (`open -a`, which works whatever the bundle's ID, `ai.storyteller.solidcraft` before
/// 0.49 or `io.tangencylabs.solidcraft` since), the app, AppImage or Flatpak of the same install
/// on Linux, `SolidCraft.exe` beside it on Windows. `is_file` says whether a path exists, so
/// this runs on every platform in tests.
pub fn launch_for(os: Os, cli: &Path, design: &Path, is_file: &dyn Fn(&Path) -> bool) -> Launch {
    let design = OsString::from(design);
    let dir = cli.parent().unwrap_or(Path::new(""));
    match os {
        Os::Mac => match cli
            .ancestors()
            .find(|a| a.extension().is_some_and(|e| e.eq_ignore_ascii_case("app")))
        {
            Some(app) => Launch::Run(PathBuf::from("open"), vec!["-a".into(), app.into(), design]),
            None => Launch::CliOnly(cli_only_message(cli)),
        },
        Os::Windows => ["SolidCraft.exe", "solidcraft.exe"]
            .iter()
            .map(|n| dir.join(n))
            .find(|c| is_file(c))
            .map(|exe| Launch::Run(exe, vec![design]))
            .unwrap_or_else(|| Launch::CliOnly(cli_only_message(cli))),
        Os::Linux => {
            if let Some(id) = flatpak_id(cli) {
                return Launch::Run(
                    PathBuf::from("flatpak"),
                    vec!["run".into(), id.into(), design],
                );
            }
            [
                "solidcraft",
                "SolidCraft",
                "SolidCraft.AppImage",
                "solidcraft.AppImage",
            ]
            .iter()
            .map(|n| dir.join(n))
            .find(|c| is_file(c))
            .map(|app| Launch::Run(app, vec![design]))
            .unwrap_or_else(|| Launch::CliOnly(cli_only_message(cli)))
        }
    }
}

/// Opens `design` in SolidCraft. With a `solidcraft-cli` chosen (the version pill's), that
/// install's own app opens it, and a CLI-only install is refused with a message rather than
/// opening another release. With none chosen: the app `SOLIDCRAFT_APP` names, else `solidcraft`
/// on `PATH` or beside this program, else whatever the system opens the design's extension with.
pub fn open_in_solidcraft(design: &Path, chosen_cli: Option<&Path>) -> Result<Opened, String> {
    if let Some(cli) = chosen_cli {
        return match launch_for(Os::current(), cli, design, &|p| p.is_file()) {
            Launch::Run(program, args) => {
                spawn(Command::new(program).args(args)).map(|()| Opened::SolidCraft)
            }
            Launch::CliOnly(message) => Err(message),
        };
    }
    match solidcraft_app() {
        Some(app) => spawn(Command::new(app).arg(design)).map(|()| Opened::SolidCraft),
        None => open(design).map(|()| Opened::SystemDefault),
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
    log_controls_with(ui, path, ready, |_| {})
}

/// [`log_controls`], with `more` buttons after them on the same row. Laid out top to bottom
/// in their own block, so a row around them cannot squeeze the buttons beside the log line.
pub fn log_controls_with(
    ui: &mut eframe::egui::Ui,
    path: &Path,
    ready: bool,
    more: impl FnOnce(&mut eframe::egui::Ui),
) -> Result<(), String> {
    ui.vertical(|ui| log_controls_in(ui, path, ready, more))
        .inner
}

fn log_controls_in(
    ui: &mut eframe::egui::Ui,
    path: &Path,
    ready: bool,
    more: impl FnOnce(&mut eframe::egui::Ui),
) -> Result<(), String> {
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
        more(ui);
    });
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &Path) -> bool {
        false
    }

    #[test]
    fn mac_opens_the_bundle_the_cli_sits_in() {
        let cli = Path::new("/Applications/SolidCraft 0.48.1.app/Contents/MacOS/solidcraft-cli");
        let launch = launch_for(Os::Mac, cli, Path::new("/x/a.solidcraft"), &none);
        assert_eq!(
            launch,
            Launch::Run(
                "open".into(),
                vec![
                    "-a".into(),
                    "/Applications/SolidCraft 0.48.1.app".into(),
                    "/x/a.solidcraft".into()
                ]
            )
        );
    }

    #[test]
    fn mac_cli_outside_a_bundle_is_cli_only() {
        let launch = launch_for(
            Os::Mac,
            Path::new("/usr/local/bin/solidcraft-cli"),
            Path::new("/x/a.solidcraft"),
            &none,
        );
        assert!(matches!(launch, Launch::CliOnly(m) if m.contains("no SolidCraft app")));
    }

    #[test]
    fn windows_opens_the_exe_beside_the_cli_or_is_cli_only() {
        let cli = Path::new("C:/Program Files/SolidCraft/solidcraft-cli.exe");
        let exe = Path::new("C:/Program Files/SolidCraft").join("SolidCraft.exe");
        let have = |p: &Path| p == exe;
        assert_eq!(
            launch_for(Os::Windows, cli, Path::new("a.solidcraft"), &have),
            Launch::Run(exe.clone(), vec!["a.solidcraft".into()])
        );
        assert!(matches!(
            launch_for(Os::Windows, cli, Path::new("a.solidcraft"), &none),
            Launch::CliOnly(_)
        ));
    }

    #[test]
    fn linux_opens_the_gui_binary_beside_the_cli_or_is_cli_only() {
        let cli = Path::new("/opt/solidcraft-0.48.1/solidcraft-cli");
        let gui = Path::new("/opt/solidcraft-0.48.1/solidcraft");
        let have = |p: &Path| p == gui;
        assert_eq!(
            launch_for(Os::Linux, cli, Path::new("/x/a.solidcraft"), &have),
            Launch::Run(gui.into(), vec!["/x/a.solidcraft".into()])
        );
        assert!(matches!(
            launch_for(Os::Linux, cli, Path::new("/x/a.solidcraft"), &none),
            Launch::CliOnly(m) if m.contains("/opt/solidcraft-0.48.1/solidcraft-cli")
        ));
    }

    #[test]
    fn linux_flatpak_cli_runs_that_flatpak_app() {
        for id in ["io.tangencylabs.solidcraft", "ai.storyteller.solidcraft"] {
            let cli =
                format!("/var/lib/flatpak/app/{id}/x86_64/stable/abc/files/bin/solidcraft-cli");
            assert_eq!(
                launch_for(
                    Os::Linux,
                    Path::new(&cli),
                    Path::new("/x/a.solidcraft"),
                    &none
                ),
                Launch::Run(
                    "flatpak".into(),
                    vec!["run".into(), id.into(), "/x/a.solidcraft".into()]
                )
            );
        }
    }

    #[test]
    fn no_chosen_cli_uses_the_default_path_not_a_chosen_install() {
        // With no choice, no launch plan is made from a CLI; the app/system route runs and
        // reports which one it used.
        assert_eq!(Opened::SystemDefault, Opened::SystemDefault);
        assert_ne!(Opened::SolidCraft, Opened::SystemDefault);
    }
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
