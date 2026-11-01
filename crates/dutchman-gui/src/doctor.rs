//! Local folder checks and matching STEP controls, using the CLI's shared engine.
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui;
use serde_json::Value;

enum Message {
    Input(PathBuf),
    Step(PathBuf),
    Progress(usize, usize),
    Finished(Result<Value, String>),
    Preview(String, egui::ColorImage),
    Plan(String),
}

pub(crate) struct Page {
    input: Option<PathBuf>,
    step: Option<PathBuf>,
    jobs: usize,
    memory_mb: u64,
    running: bool,
    status: String,
    report: Option<Value>,
    report_folder: Option<PathBuf>,
    target: usize,
    previews: Vec<(String, egui::TextureHandle)>,
    plan: Option<String>,
    sender: Sender<Message>,
    receiver: Receiver<Message>,
}

impl Page {
    pub(crate) fn new(input: Option<PathBuf>) -> Self {
        let step = input
            .as_deref()
            .and_then(dutchman::diagnose::step::matching);
        let (sender, receiver) = channel();
        Self {
            input,
            step,
            jobs: 1,
            memory_mb: 8192,
            running: false,
            status: String::new(),
            report: None,
            report_folder: None,
            target: 1,
            previews: Vec::new(),
            plan: None,
            sender,
            receiver,
        }
    }

    fn pick(&self, step: bool, ctx: &egui::Context) {
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let picked = if step {
                rfd::FileDialog::new()
                    .set_title("Add matching STEP")
                    .add_filter("STEP export", &["step", "stp"])
                    .pick_file()
            } else {
                rfd::FileDialog::new()
                    .set_title("Check a folder")
                    .pick_folder()
            };
            if let Some(path) = picked {
                let _ = sender.send(if step {
                    Message::Step(path)
                } else {
                    Message::Input(path)
                });
            }
            ctx.request_repaint();
        });
    }

    fn start(&mut self, ctx: &egui::Context) {
        let Some(input) = self.input.clone() else {
            return;
        };
        let folder = input.is_dir() || dutchman::batch::extension_of(&input) == "zip";
        let out = dutchman::doctor::default_output(&input);
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        let settings = dutchman::doctor::Settings {
            jobs: std::num::NonZeroUsize::new(self.jobs),
            memory: self.memory_mb.saturating_mul(1 << 20),
            diagnose: dutchman::diagnose::Settings {
                step_path: self.step.clone(),
                target_part: Some(self.target),
                memory_limit: Some(self.memory_mb.saturating_mul(1 << 20)),
                ..dutchman::diagnose::Settings::default()
            },
        };
        self.running = true;
        self.status = "Checking locally…".into();
        self.report = None;
        self.previews.clear();
        std::thread::spawn(move || {
            let report = if folder {
                dutchman::doctor::run(&input, &out, &settings, &|done, total| {
                    let _ = sender.send(Message::Progress(done, total));
                    ctx.request_repaint();
                })
            } else {
                // Keep each explicit matching-STEP check separate from previous diagnose runs.
                let next = dutchman::batch::default_output(&out);
                dutchman::diagnose::run(&input, &next, &settings.diagnose, &|_| {})
            };
            if let Ok(report) = &report {
                if folder {
                    if let Ok(bytes) = std::fs::read(out.join("local-key.json")) {
                        if let Ok(key) = serde_json::from_slice::<Value>(&bytes) {
                            for file in key
                                .get("files")
                                .and_then(Value::as_array)
                                .into_iter()
                                .flatten()
                                .take(16)
                            {
                                if let Some(checkpoint) =
                                    file.get("checkpoint").and_then(Value::as_str)
                                {
                                    send_previews(
                                        &PathBuf::from(checkpoint).join("diagnose/preview"),
                                        &format!(
                                            "File {}",
                                            file.get("file").and_then(Value::as_u64).unwrap_or(0)
                                        ),
                                        &sender,
                                    );
                                }
                            }
                        }
                    }
                } else if let Some(path) = report.pointer("/paths/report").and_then(Value::as_str) {
                    if let Some(parent) = std::path::Path::new(path).parent() {
                        send_previews(&parent.join("preview"), "Design", &sender);
                    }
                }
            }
            let _ = sender.send(Message::Finished(report));
            ctx.request_repaint();
        });
    }

    /// Returns true after the user chooses to leave the page.
    pub(crate) fn ui(&mut self, ctx: &egui::Context) -> bool {
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                Message::Input(input) => {
                    self.step = None;
                    self.input = Some(input.clone());
                    self.plan = None;
                    let sender = self.sender.clone();
                    let ctx = ctx.clone();
                    std::thread::spawn(move || {
                        let plan = dutchman::batch::plan(
                            std::slice::from_ref(&input),
                            &dutchman::doctor::default_output(&input),
                        );
                        let mut summary = plan.description;
                        for job in plan.jobs.iter().take(100) {
                            if let Some(step) = &job.fallback {
                                summary.push_str(&format!("\nMatching STEP: {}", step.display()));
                            }
                        }
                        let _ = sender.send(Message::Plan(summary));
                        ctx.request_repaint();
                    });
                    self.report = None;
                }
                Message::Plan(text) => self.plan = Some(text),
                Message::Preview(label, image) => {
                    let texture =
                        ctx.load_texture(label.clone(), image, egui::TextureOptions::LINEAR);
                    self.previews.push((label, texture));
                }
                Message::Step(step) => {
                    self.step = Some(step);
                    self.report = None;
                }
                Message::Progress(done, total) => {
                    self.status = format!("Checked {done} of {total} designs")
                }
                Message::Finished(result) => {
                    self.running = false;
                    match result {
                        Ok(report) => {
                            self.status =
                                "Checks finished. Review findings; nothing was sent.".into();
                            self.report_folder = report
                                .pointer("/paths/report")
                                .and_then(Value::as_str)
                                .and_then(|p| std::path::Path::new(p).parent())
                                .map(std::path::Path::to_path_buf);
                            self.report = Some(report);
                        }
                        Err(why) => self.status = why,
                    }
                }
            }
        }
        let mut close = false;
        let mut selected = None;
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Check a folder");
            ui.label("Convert and check locally. Completed folder checks resume when you run them again.");
            ui.label("Only report.json is anonymous. Keep converted designs and all other output private.");
            ui.add_enabled_ui(!self.running, |ui| {
                if ui.button("Choose folder…").clicked() { self.pick(false,ctx); }
                if let Some(input) = &self.input { ui.label(input.display().to_string()); }
                if let Some(plan) = &self.plan { ui.label(plan); }
                ui.horizontal(|ui| {
                    ui.label("Concurrent designs"); ui.add(egui::DragValue::new(&mut self.jobs).range(1..=64));
                    ui.label("Total memory (MiB)"); ui.add(egui::DragValue::new(&mut self.memory_mb).range(256..=1_048_576));
                });
                let file = self.input.as_ref().is_some_and(|p| p.is_file() && dutchman::batch::extension_of(p) != "zip");
                if file {
                    if ui.button("Add matching STEP").clicked() { self.pick(true,ctx); }
                    if let Some(step) = &self.step { ui.label(format!("Matching STEP: {}",step.display())); }
                    ui.horizontal(|ui| { ui.label("Report target: part"); ui.add(egui::DragValue::new(&mut self.target).range(1..=100_000)); });
                }
                if ui.add_enabled(self.input.is_some(),egui::Button::new("Start local checks")).clicked() { self.start(ctx); }
                if ui.button("Back to conversion").clicked() { close = true; }
            });
            ui.label(&self.status);
            for pair in self.previews.chunks(2) {
                ui.columns(2,|columns| {
                    for ((label,texture),column) in pair.iter().zip(columns.iter_mut()) {
                        column.label(label);
                        column.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(300.0,225.0)));
                    }
                });
            }
            if let Some(report) = &self.report {
                for part in report.pointer("/report/parts").and_then(Value::as_array).into_iter().flatten() {
                    if let Some(number) = part.get("part").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()) {
                        if ui.selectable_label(number == self.target,format!("Target part {number}")).clicked() { selected = Some(number); }
                    }
                }
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(files) = report.get("files").and_then(Value::as_array) {
                        for file in files {
                            let number = file.get("file").and_then(Value::as_u64).unwrap_or(0);
                            let status = if file.get("converted") == Some(&Value::Bool(true)) { "Converted" } else { "Needs review" };
                            ui.label(format!("File {number}: {status}"));
                        }
                    }
                    let anonymous = report.get("report").unwrap_or(report);
                    comparison_ui(ui,"Matching STEP",anonymous.get("step_comparison"));
                    for file in anonymous.get("files").and_then(Value::as_array).into_iter().flatten() {
                        comparison_ui(ui,"Matching STEP",file.get("step_comparison"));
                        comparison_ui(ui,"Stored geometry versus conversion",file.get("geometry_checks"));
                    }
                    for finding in anonymous.get("findings").and_then(Value::as_array).into_iter().flatten() {
                        let count = finding.get("count").and_then(Value::as_u64).unwrap_or(0);
                        let codes = finding.pointer("/signature/error/codes").and_then(Value::as_array)
                            .into_iter().flatten().filter_map(Value::as_str).collect::<Vec<_>>().join(", ");
                        ui.label(format!("{count} occurrence(s): {}",if codes.is_empty() { "Conversion stopped" } else { &codes }));
                    }
                    ui.collapsing("Anonymous report details",|ui| {
                        ui.monospace(serde_json::to_string_pretty(anonymous).unwrap_or_default());
                    });
                });
            }
        });
        if let (Some(number), Some(folder)) = (selected, self.report_folder.as_ref()) {
            match dutchman::diagnose::select_target(folder, number) {
                Ok(report) => {
                    self.target = number;
                    if let Some(field) = self.report.as_mut().and_then(|r| r.get_mut("report")) {
                        *field = report;
                    }
                }
                Err(why) => self.status = why,
            }
        }
        close
    }
}

fn comparison_ui(ui: &mut egui::Ui, title: &str, comparison: Option<&Value>) {
    let Some(comparison) = comparison.filter(|v| !v.is_null()) else {
        return;
    };
    ui.separator();
    ui.label(title);
    ui.label("Advisory: confirm differences before making a report. Body numbers may differ from assembly part numbers.");
    if comparison.get("status").and_then(Value::as_str) != Some("measured") {
        ui.label(
            "Measurements unavailable. Check the matching export and installed SolidCraft CLI.",
        );
    }
    for row in comparison
        .get("parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .take(100)
    {
        let body = row
            .get("body")
            .and_then(Value::as_u64)
            .map_or("Unmatched reference".to_owned(), |n| format!("Body {n}"));
        let status = if row.get("looks_wrong") == Some(&Value::Bool(true)) {
            "Review difference"
        } else if row.get("status").and_then(Value::as_str) == Some("incomplete") {
            "Measurements incomplete"
        } else {
            "Within thresholds"
        };
        ui.label(format!("{body}: {status}"));
        let volume = row
            .pointer("/differences/volume_relative")
            .and_then(Value::as_f64);
        let placement = row
            .pointer("/differences/placement_mm")
            .and_then(Value::as_f64);
        if let (Some(volume), Some(placement)) = (volume, placement) {
            ui.label(format!(
                "Volume difference {:.2}%; position difference {placement:.3} mm",
                volume * 100.0
            ));
        }
    }
}

fn send_previews(folder: &std::path::Path, label: &str, sender: &Sender<Message>) {
    for (kind, caption) in [("step", "Matching STEP"), ("geometry", "Converted")] {
        for (name, side) in [("original", "Stored source"), ("converted", caption)] {
            let path = folder.join(kind).join(format!("{name}.png"));
            if !std::fs::metadata(&path).is_ok_and(|m| m.len() <= (4 << 20)) {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            let Ok(mut reader) =
                image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()
            else {
                continue;
            };
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(640);
            limits.max_image_height = Some(480);
            limits.max_alloc = Some(16 << 20);
            reader.limits(limits);
            let Ok(image) = reader.decode() else { continue };
            let image = image.to_rgba8();
            let size = [image.width() as usize, image.height() as usize];
            let _ = sender.send(Message::Preview(
                format!("{label}: {side}"),
                egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()),
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    #[test]
    fn page_exposes_local_controls_without_starting_work() {
        let mut harness = egui_kittest::Harness::new_state(
            |ctx, page: &mut Page| {
                page.ui(ctx);
            },
            Page::new(None),
        );
        harness.run_steps(2);
        harness.get_by_label("Check a folder");
        harness.get_by_label("Start local checks");
        assert!(!harness.state().running);
    }
    #[test]
    fn clicking_a_part_saves_the_local_report_target() {
        let dir = tempfile::tempdir().unwrap();
        let report = serde_json::json!({"schema": dutchman::diagnose::SCHEMA,
            "parts": [{"part":1,"outcome":"ok"},{"part":2,"outcome":"ok"}], "summary": {"parts":2}});
        std::fs::write(dir.path().join("report.json"), report.to_string()).unwrap();
        let mut page = Page::new(None);
        page.report = Some(serde_json::json!({"report":report}));
        page.report_folder = Some(dir.path().to_path_buf());
        let mut harness = egui_kittest::Harness::new_state(
            |ctx, page: &mut Page| {
                page.ui(ctx);
            },
            page,
        );
        harness.run_steps(2);
        harness.get_by_label("Target part 2").click();
        harness.step();
        assert_eq!(harness.state().target, 2);
        assert_eq!(
            dutchman::diagnose::report_from(dir.path()).unwrap()["target_part"],
            2
        );
    }
}
