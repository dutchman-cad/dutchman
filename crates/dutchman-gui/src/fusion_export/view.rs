//! The "Export from Fusion" window: waiting for the exporter, the design picker, progress,
//! and how an export ended.
use std::collections::BTreeSet;
use std::path::PathBuf;

use eframe::egui::{self, RichText};

use super::inventory::{Folder, Hub, Project};
use super::job::Next;
use super::{Panel, Stage};
use crate::theme;

impl Panel {
    /// Draws the window; returns the export folder once it is ready to convert.
    pub(crate) fn window(&mut self, ctx: &egui::Context, top: f32) -> Option<PathBuf> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut convert = None;
        let height = (ctx.content_rect().height() - top - 40.0).max(240.0);
        egui::Window::new("Export from Fusion")
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, top + 8.0))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(540.0)
            .max_height(height)
            .show(ctx, |ui| {
                ui.set_max_width(540.0);
                match self.stage {
                    Stage::NoAgent => self.no_agent_ui(ui),
                    Stage::Listing => self.listing_ui(ui),
                    Stage::ListFailed => self.list_failed_ui(ui),
                    Stage::Picking => self.picking_ui(ui, height),
                    Stage::Exporting => self.exporting_ui(ui),
                    Stage::Finished => convert = self.finished_ui(ui),
                }
                self.error_ui(ui);
            });
        self.open = open && self.open;
        convert
    }

    /// No exporter has announced itself: what is missing, and the one-click install.
    fn no_agent_ui(&mut self, ui: &mut egui::Ui) {
        let cad_dir = self.env.cad_dir.clone();
        self.installer.ui(
            ui,
            self.offered,
            &self.presence,
            cad_dir,
            self.env.pid_alive,
        );
    }

    fn list_failed_ui(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui);
        let why = self
            .list_error
            .clone()
            .unwrap_or_else(|| "Your Fusion designs could not be listed.".to_owned());
        ui.label(RichText::new(why).color(p.danger));
        if ui.button("Try again").clicked() {
            self.list_designs();
        }
    }

    fn listing_ui(&mut self, ui: &mut egui::Ui) {
        let reading = self.reading_list;
        ui.horizontal(|ui| {
            ui.spinner();
            let text = match self.running.as_ref().and_then(|r| r.status.as_ref()) {
                _ if reading => "Reading the list of your designs…",
                Some(status) if status.state == "running" => "Listing your Fusion designs…",
                _ => "Asking Fusion for the list of your designs…",
            };
            ui.label(text);
        });
        if self.running.is_some() {
            self.stop_buttons(ui);
        }
    }

    /// Stop, which asks the exporter, and Stop waiting, which gives up on an answer.
    fn stop_buttons(&mut self, ui: &mut egui::Ui) {
        let stopping = self.running.as_ref().is_some_and(|r| r.stopping);
        ui.horizontal(|ui| {
            if stopping {
                ui.label("Stopping…");
            } else if ui.button("Stop").clicked() {
                self.stop();
            }
            if ui
                .button("Stop waiting")
                .on_hover_text("Leave this step without waiting for Fusion")
                .clicked()
            {
                self.stop_waiting();
            }
        });
    }

    fn picking_ui(&mut self, ui: &mut egui::Ui, height: f32) {
        let Some(inventory) = self.inventory.take() else {
            return;
        };
        let total = inventory.count();
        if total == 0 {
            ui.label("Fusion lists no designs in your account.");
        } else {
            ui.label("Choose the designs to export. Everything they insert is exported with them.");
            let search = ui.label("Search");
            ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .hint_text("Part of a hub, project, folder or design name")
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(search.id);
            if self.shown_query.as_deref() != Some(self.query.as_str()) {
                self.shown = inventory.matching_ids(&self.query);
                self.shown_query = Some(self.query.clone());
            }
            let shown = self.shown.len();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Select all shown").clicked() {
                    self.picked.extend(self.shown.iter().cloned());
                }
                if ui.button("Clear").clicked() {
                    self.picked.clear();
                }
                ui.label(format!("{} of {} designs shown", shown, total));
            });
            let query = self.query.trim().to_lowercase();
            egui::ScrollArea::vertical()
                .max_height((height - 230.0).max(120.0))
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for hub in &inventory.hubs {
                        hub_ui(ui, hub, &query, &mut self.picked);
                    }
                });
        }
        if inventory.unreadable > 0 {
            let p = theme::palette(ui);
            ui.label(
                RichText::new(format!(
                    "Fusion could not list {} places in your account; designs there are missing.",
                    inventory.unreadable
                ))
                .color(p.lantern_text),
            );
        }
        if inventory.stopped {
            let p = theme::palette(ui);
            ui.label(
                RichText::new(
                    "The list is incomplete: it was stopped before every project was listed.",
                )
                .color(p.lantern_text),
            );
        }
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            let count = self.picked.len();
            let label = match count {
                1 => "Export 1 design".to_owned(),
                n => format!("Export {n} designs"),
            };
            if ui
                .add_enabled(count > 0, egui::Button::new(label))
                .clicked()
            {
                self.export_picked();
            }
            if ui.button("List my designs again").clicked() {
                self.list_designs();
            }
        });
        if let Some(agent) = &self.agent {
            self.installer.newer_note(ui, &agent.version);
        }
        if self.inventory.is_none() {
            self.inventory = Some(inventory);
        }
    }

    fn exporting_ui(&mut self, ui: &mut egui::Ui) {
        let status = self.running.as_ref().and_then(|r| r.status.clone());
        let stopping = self.running.as_ref().is_some_and(|r| r.stopping);
        let queued = status.as_ref().is_none_or(|s| s.state == "queued");
        match &status {
            Some(status) if !queued => {
                let total = status.total.unwrap_or(0);
                let fraction = if total > 0 {
                    status.done as f32 / total as f32
                } else {
                    0.0
                };
                let text = match (status.current_number, status.total) {
                    (Some(n), Some(total)) => format!("Design {n} of {total}"),
                    (Some(n), None) => format!("Design {n}"),
                    _ => format!("{} done", status.done),
                };
                ui.add(egui::ProgressBar::new(fraction).text(text));
                if !status.message.is_empty() {
                    ui.label(&status.message);
                }
                if status.failed > 0 {
                    let p = theme::palette(ui);
                    ui.label(
                        RichText::new(format!("{} failed so far", status.failed))
                            .color(p.lantern_text),
                    );
                }
            }
            _ => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Waiting for Fusion to take your export…");
                });
                if let Some(status) = status.filter(|s| !s.message.is_empty()) {
                    ui.label(status.message);
                }
            }
        }
        if stopping {
            ui.label("Stopping after the design in progress. What is finished is kept.");
        }
        self.stop_buttons(ui);
    }

    fn finished_ui(&mut self, ui: &mut egui::Ui) -> Option<PathBuf> {
        let ended = self.ended.as_ref()?;
        let status = ended.status.clone();
        let p = theme::palette(ui);
        let kept = status.exported + status.up_to_date;
        let mut convert = false;
        let mut again = false;
        let mut choose = false;
        match status.state.as_str() {
            "done" => {
                ui.label(
                    RichText::new(format!(
                        "{} of your designs failed to export; {} exported.",
                        status.failed, kept
                    ))
                    .color(p.lantern_text),
                );
                ui.label("Try again to export the failed ones; the rest are kept.");
                ui.horizontal_wrapped(|ui| {
                    again = ui.button("Try again").clicked();
                    convert = kept > 0 && ui.button("Convert what exported").clicked();
                    choose = ui.button("Choose other designs").clicked();
                });
            }
            state => {
                let reason = status.reason.as_deref().unwrap_or(if state == "cancelled" {
                    "cancelled"
                } else {
                    "error"
                });
                let (text, next) = super::job::reason_info(reason, &status.message);
                let color = if state == "cancelled" {
                    p.tide
                } else {
                    p.danger
                };
                ui.label(RichText::new(text).color(color));
                ui.horizontal_wrapped(|ui| {
                    match next {
                        Next::TryAgain => again = ui.button("Try again").clicked(),
                        _ if state == "cancelled" => {
                            again = ui.button("Continue the export").clicked();
                        }
                        _ => {}
                    }
                    if next == Next::ChooseAgain && ui.button("List my designs again").clicked() {
                        self.back_to_picking();
                        self.list_designs();
                    }
                    convert = kept > 0 && ui.button("Convert what exported").clicked();
                    if state != "failed" || next != Next::ChooseAgain {
                        choose = ui.button("Choose other designs").clicked();
                    }
                });
            }
        }
        if again {
            self.start_export();
        }
        if choose {
            self.back_to_picking();
        }
        if convert {
            return self.convert_what_exported();
        }
        None
    }

    fn error_ui(&mut self, ui: &mut egui::Ui) {
        if let Some((text, _)) = &self.error {
            let p = theme::palette(ui);
            ui.label(RichText::new(text).color(p.danger));
        }
    }
}

fn hub_ui(ui: &mut egui::Ui, hub: &Hub, query: &str, picked: &mut BTreeSet<String>) {
    let hub_hit = !query.is_empty() && hub.name.to_lowercase().contains(query);
    let visible: Vec<&Project> = hub
        .projects
        .iter()
        .filter(|p| {
            hub_hit || query.is_empty() || {
                let mut ids = Vec::new();
                p.root
                    .collect(query, p.name.to_lowercase().contains(query), &mut ids);
                !ids.is_empty()
            }
        })
        .collect();
    if visible.is_empty() {
        return;
    }
    let force = !query.is_empty();
    let mut header = egui::CollapsingHeader::new(format!("{} ({})", hub.name, hub.count()))
        .id_salt(("fx-hub", &hub.id))
        .default_open(true);
    if force {
        header = header.open(Some(true));
    }
    header.show(ui, |ui| {
        for project in visible {
            let hit = hub_hit || project.name.to_lowercase().contains(query);
            let mut header =
                egui::CollapsingHeader::new(format!("{} ({})", project.name, project.count()))
                    .id_salt(("fx-project", &hub.id, &project.id));
            if force {
                header = header.open(Some(true));
            }
            header.show(ui, |ui| {
                folder_ui(
                    ui,
                    &project.root,
                    query,
                    hit,
                    picked,
                    &(&hub.id, &project.id),
                );
            });
        }
    });
}

fn folder_ui(
    ui: &mut egui::Ui,
    folder: &Folder,
    query: &str,
    parent_hit: bool,
    picked: &mut BTreeSet<String>,
    path: &(&String, &String),
) {
    let hit = parent_hit || (!query.is_empty() && folder.name.to_lowercase().contains(query));
    if !folder.any_match(query, parent_hit) {
        return;
    }
    if !folder.name.is_empty() {
        let shown = if query.is_empty() {
            folder.count()
        } else {
            let mut here = Vec::new();
            folder.collect(query, parent_hit, &mut here);
            here.len()
        };
        let mut header = egui::CollapsingHeader::new(format!("{} ({})", folder.name, shown))
            .id_salt(("fx-folder", path.0, path.1, &folder.name));
        if !query.is_empty() {
            header = header.open(Some(true));
        }
        header.show(ui, |ui| body(ui, folder, query, hit, picked, path));
    } else {
        body(ui, folder, query, hit, picked, path);
    }
}

fn body(
    ui: &mut egui::Ui,
    folder: &Folder,
    query: &str,
    hit: bool,
    picked: &mut BTreeSet<String>,
    path: &(&String, &String),
) {
    for sub in &folder.folders {
        folder_ui(ui, sub, query, hit, picked, path);
    }
    for design in &folder.designs {
        if !(hit || query.is_empty() || design.name.to_lowercase().contains(query)) {
            continue;
        }
        let mut on = picked.contains(&design.id);
        if ui.checkbox(&mut on, &design.name).changed() {
            if on {
                picked.insert(design.id.clone());
            } else {
                picked.remove(&design.id);
            }
        }
    }
}
