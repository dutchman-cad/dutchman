//! The one small window: *Choose a folder of CAD files to keep converted*. The source folder,
//! the output folder, **Start**, and the Start at login checkbox. Starting writes the folder
//! into the shared state, where the running tray program picks it up, and closes the window.
//!
//! The window is a program of its own (`dutchman-sync --window`) started by the tray, so a
//! window that is closed or crashes never takes the sync down with it. The window with a status
//! row for every design is a later change.

use std::path::{Path, PathBuf};

use dutchman::sync_state::{self, State};
use eframe::egui;

use super::login;
use crate::theme;

/// The window's title.
pub const TITLE: &str = "Dutchman Sync";
/// The heading.
pub const HEADING: &str = "Choose a folder of CAD files to keep converted";
const SIZE: [f32; 2] = [560.0, 380.0];

/// Why a choice of folders cannot start.
pub fn check(source: &str, output: &str) -> Result<(PathBuf, PathBuf), String> {
    let (source, output) = (source.trim(), output.trim());
    if source.is_empty() {
        return Err("Choose the folder your CAD files are in.".to_owned());
    }
    if output.is_empty() {
        return Err("Choose the folder the converted designs go in.".to_owned());
    }
    let (source, output) = (PathBuf::from(source), PathBuf::from(output));
    if !source.is_dir() {
        return Err(format!("{} is not a folder.", source.display()));
    }
    if output.exists() && !output.is_dir() {
        return Err(format!("{} is not a folder.", output.display()));
    }
    if output.starts_with(&source) || source.starts_with(&output) {
        return Err("Choose an output folder that is not inside the source folder, nor the other way round.".to_owned());
    }
    Ok((source, output))
}

/// Adds the chosen folders to `state`, turned on.
pub fn start(state_file: &Path, source: PathBuf, output: PathBuf) -> Result<(), String> {
    std::fs::create_dir_all(&output)
        .map_err(|e| format!("Cannot create {}: {e}", output.display()))?;
    let mut state = sync_state::read_from(state_file);
    state.add_folder(output, Some(source));
    state.paused = false;
    sync_state::write_to(state_file, &state)
}

struct Window {
    state_file: PathBuf,
    state: State,
    source: String,
    output: String,
    error: Option<String>,
}

impl Window {
    fn new(cc: &eframe::CreationContext<'_>, state_file: PathBuf) -> Self {
        theme::install(&cc.egui_ctx);
        let state = sync_state::read_from(&state_file);
        Self {
            state_file,
            state,
            source: String::new(),
            output: String::new(),
            error: None,
        }
    }

    fn folder_row(ui: &mut egui::Ui, label: &str, value: &mut String) {
        ui.label(label);
        theme::control_row(ui, |ui| {
            let width = (ui.available_width() - 100.0).max(120.0);
            ui.add_sized(
                [width, theme::control_height(ui)],
                egui::TextEdit::singleline(&mut *value),
            );
            if ui.button("Choose\u{2026}").clicked() {
                let mut dialog = rfd::FileDialog::new().set_title(label);
                if Path::new(value.as_str()).is_dir() {
                    dialog = dialog.set_directory(value.as_str());
                }
                if let Some(folder) = dialog.pick_folder() {
                    *value = folder.to_string_lossy().into_owned();
                }
            }
        });
    }
}

impl eframe::App for Window {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(theme::SPACE_2);
            ui.label(egui::RichText::new(HEADING).text_style(theme::title()));
            ui.add_space(theme::SPACE_4);
            Self::folder_row(ui, "Source folder", &mut self.source);
            ui.add_space(theme::SPACE_2);
            Self::folder_row(ui, "Output folder", &mut self.output);
            ui.add_space(theme::SPACE_4);
            if let Some(error) = &self.error {
                ui.colored_label(theme::palette(ui).danger, error);
                ui.add_space(theme::SPACE_2);
            }
            ui.horizontal(|ui| {
                let start_button =
                    ui.add(egui::Button::new("Start").fill(ui.visuals().selection.bg_fill));
                if start_button.clicked() {
                    match check(&self.source, &self.output)
                        .and_then(|(source, output)| start(&self.state_file, source, output))
                    {
                        Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                        Err(why) => self.error = Some(why),
                    }
                }
            });
            ui.add_space(theme::SPACE_4);
            let mut login_on = self.state.start_at_login;
            if ui
                .checkbox(&mut login_on, "Start Dutchman Sync when I log in")
                .changed()
            {
                match login::set(login_on) {
                    Ok(()) => {
                        self.state.start_at_login = login_on;
                        let _ = sync_state::update(|state| state.start_at_login = login_on);
                        self.error = None;
                    }
                    Err(why) => self.error = Some(why),
                }
            }
            if !self.state.folders.is_empty() {
                ui.add_space(theme::SPACE_4);
                ui.label(egui::RichText::new("Folders").text_style(theme::label()));
                let mut changed = None;
                for (index, folder) in self.state.folders.iter().enumerate() {
                    let mut enabled = folder.enabled;
                    if ui
                        .checkbox(&mut enabled, folder.output.display().to_string())
                        .changed()
                    {
                        changed = Some((index, enabled));
                    }
                }
                if let Some((index, enabled)) = changed {
                    if let Some(folder) = self.state.folders.get_mut(index) {
                        folder.enabled = enabled;
                        let output = folder.output.clone();
                        let _ = sync_state::update(|state| {
                            if let Some(folder) = state.folder_mut(&output) {
                                folder.enabled = enabled;
                            }
                        });
                    }
                }
            }
        });
    }
}

/// Opens the window and returns when it is closed.
pub fn run() -> Result<(), String> {
    let state_file = sync_state::path().ok_or("cannot find the per-user config folder")?;
    let viewport = egui::ViewportBuilder::default()
        .with_title(TITLE)
        .with_app_id("dutchman-sync")
        .with_inner_size(SIZE)
        .with_min_inner_size([480.0, 300.0]);
    eframe::run_native(
        TITLE,
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(move |cc| Ok(Box::new(Window::new(cc, state_file)))),
    )
    .map_err(|e| format!("cannot open the window: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_needs_two_distinct_folders_and_the_source_must_exist() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("cad");
        std::fs::create_dir(&src).unwrap();
        let src_text = src.to_string_lossy().into_owned();
        assert!(check("", "x").unwrap_err().contains("CAD files"));
        assert!(check(&src_text, " ")
            .unwrap_err()
            .contains("converted designs"));
        assert!(check("/no/such/folder", "/o")
            .unwrap_err()
            .contains("not a folder"));
        let inside = src.join("out").to_string_lossy().into_owned();
        assert!(check(&src_text, &inside)
            .unwrap_err()
            .contains("not inside"));
        let out = dir.path().join("sc").to_string_lossy().into_owned();
        assert_eq!(check(&src_text, &out).unwrap(), (src, PathBuf::from(out)));
    }

    #[test]
    fn start_writes_the_folder_into_the_shared_state_turned_on() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cfg").join(sync_state::FILE_NAME);
        let output = dir.path().join("out");
        start(&file, dir.path().join("cad"), output.clone()).unwrap();
        assert!(output.is_dir());
        let state = sync_state::read_from(&file);
        let folder = state.enabled().next().unwrap();
        assert_eq!(folder.output, output);
        assert_eq!(
            folder.source.as_deref(),
            Some(dir.path().join("cad").as_path())
        );
        assert!(folder.auto_update && !state.paused);
    }

    /// The window with two folders listed and Start at login on, Day and Night, at its own size.
    /// Needs a GPU renderer; set DUTCHMAN_GUI_SCREENSHOTS.
    #[test]
    #[ignore = "renders GPU screenshots of the Sync window; set DUTCHMAN_GUI_SCREENSHOTS"]
    fn sync_window_screenshots() {
        let out = PathBuf::from(std::env::var_os("DUTCHMAN_GUI_SCREENSHOTS").unwrap());
        std::fs::create_dir_all(&out).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(sync_state::FILE_NAME);
        let mut state = State::default();
        state.add_folder(
            PathBuf::from("/Users/demo/Documents/Converted"),
            Some(PathBuf::from("/Users/demo/Documents/CAD")),
        );
        state.add_folder(
            PathBuf::from("/Users/demo/Projects/Converted"),
            Some(PathBuf::from("/Users/demo/Projects/CAD")),
        );
        state.folders[1].enabled = false;
        state.start_at_login = true;
        sync_state::write_to(&file, &state).unwrap();
        for (theme, name) in [
            (egui::ThemePreference::Light, "day"),
            (egui::ThemePreference::Dark, "night"),
        ] {
            let state_file = file.clone();
            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(SIZE[0], SIZE[1]))
                .build_eframe(move |cc| Window::new(cc, state_file));
            harness.ctx.set_theme(theme);
            harness.run_steps(8);
            let image = harness.render().unwrap();
            assert!(image.height() <= image.width());
            image
                .save(out.join(format!("sync-window-{name}.png")))
                .unwrap();
        }
    }
}
