//! The window: designs in, an output folder, Convert, and what became of each file.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use dutchman::batch::{Event, Job};
use dutchman::ConvertOptions;
use eframe::egui::{
    self, Align, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke, TextStyle,
    ThemePreference,
};
use serde_json::Value;

use crate::platform;
use crate::theme::{self, Palette};
use crate::worker::{self, Message, Pick, Run};

/// What became of one file in a run.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Waiting,
    Converting,
    Converted,
    Failed,
    /// The run was stopped before this file started.
    NotStarted,
}

/// One file of a run.
pub struct Row {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub status: Status,
    /// Its `dutchman.convert/1` report, once it has one.
    pub report: Option<Value>,
    /// The job that converts it; none for an input that could not be planned.
    pub job: Option<Job>,
}

/// Dutchman's window.
pub struct App {
    inputs: Vec<PathBuf>,
    out_dir: Option<PathBuf>,
    options: ConvertOptions,
    rows: Vec<Row>,
    /// Rows before the run's first job: inputs that could not be planned.
    offset: usize,
    run: Option<Run>,
    messages: Receiver<Message>,
    sender: Sender<Message>,
    /// A one-line notice, such as an app that could not be opened.
    notice: Option<String>,
    has_app: bool,
    /// The SolidCraft CLI that checks histories when nothing else is chosen, if one is found.
    checker: Option<PathBuf>,
    /// What to scroll into view on the next frame, so the results are seen without looking
    /// for them.
    reveal: Option<Reveal>,
}

/// What to scroll into view.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Reveal {
    /// The results' heading, as a run starts.
    Results,
    /// The first file that needs attention, as a run ends.
    FirstIssue,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let (sender, messages) = channel();
        App {
            inputs: Vec::new(),
            out_dir: None,
            options: ConvertOptions::default(),
            rows: Vec::new(),
            offset: 0,
            run: None,
            messages,
            sender,
            notice: None,
            has_app: platform::has_solidcraft_app(),
            checker: dutchman_solidcraft::find_cli(&dutchman_solidcraft::Verification::Auto).ok(),
            reveal: None,
        }
    }

    /// Adds design files and folders to convert, each once.
    pub fn add_inputs(&mut self, paths: impl IntoIterator<Item = PathBuf>) {
        for path in paths {
            if !self.inputs.contains(&path) {
                self.inputs.push(path);
            }
        }
    }

    /// Sets the folder designs are written to.
    pub fn set_out_dir(&mut self, dir: PathBuf) {
        self.out_dir = Some(dir);
    }

    /// Whether written histories are checked in SolidCraft.
    pub fn set_verify(&mut self, verify: bool) {
        self.options.no_verify = !verify;
    }

    /// The rows of the current or last run.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Sets the project folder assemblies' part files are searched in; `None` finds it.
    pub fn set_project(&mut self, project: Option<PathBuf>) {
        self.options.project = project;
    }

    /// Converts the row at `index` again, searching `folder` too for its assemblies' part
    /// files.
    pub fn search_for_row(&mut self, index: usize, folder: PathBuf, ctx: &egui::Context) {
        if self.running() {
            return;
        }
        let Some(job_index) = index.checked_sub(self.offset) else {
            return;
        };
        let Some(row) = self.rows.get_mut(index) else {
            return;
        };
        let Some(job) = row.job.as_mut() else {
            return;
        };
        if !job.search.also.contains(&folder) {
            job.search.also.push(folder);
        }
        let job = job.clone();
        row.status = Status::Waiting;
        row.report = None;
        self.notice = None;
        self.run = Some(Run::retry(
            job,
            job_index,
            &self.options,
            self.sender.clone(),
            ctx.clone(),
        ));
    }

    /// Whether a run is in progress.
    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    fn start(&mut self, ctx: &egui::Context) {
        let Some(out_dir) = self.out_dir.clone() else {
            return;
        };
        self.rows.clear();
        self.offset = 0;
        self.notice = None;
        self.run = Some(Run::start(
            self.inputs.clone(),
            out_dir,
            &self.options,
            self.sender.clone(),
            ctx.clone(),
        ));
    }

    fn picked(&mut self, pick: Pick, paths: Vec<PathBuf>, ctx: &egui::Context) {
        let Some(first) = paths.first().cloned() else {
            return;
        };
        match pick {
            Pick::Files | Pick::Folder => self.add_inputs(paths),
            Pick::Output => self.out_dir = Some(first),
            Pick::Project => self.options.project = Some(first),
            Pick::SearchFolder(row) => self.search_for_row(row, first, ctx),
            Pick::SearchFile(row) => {
                if let Some(folder) = first.parent() {
                    self.search_for_row(row, folder.to_path_buf(), ctx);
                }
            }
            Pick::SaveLog(row) => {
                if let Some(report) = self.rows.get(row).and_then(|row| row.report.as_ref()) {
                    let text = dutchman::json::full_log(report, &dutchman::batch::line(report));
                    if let Err(err) = std::fs::write(&first, text) {
                        self.notice = Some(format!("Cannot save {}: {err}", display(&first)));
                    }
                }
            }
        }
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(message) = self.messages.try_recv() {
            match message {
                Message::Picked { pick, paths } => self.picked(pick, paths, ctx),
                Message::Planned { problems, jobs } => {
                    self.reveal = Some(Reveal::Results);
                    self.offset = problems.len();
                    self.rows = problems
                        .into_iter()
                        .map(|report| Row {
                            input: report
                                .pointer("/input/path")
                                .and_then(Value::as_str)
                                .map(PathBuf::from)
                                .unwrap_or_default(),
                            output: None,
                            status: Status::Failed,
                            report: Some(report),
                            job: None,
                        })
                        .chain(jobs.into_iter().map(|job| Row {
                            input: job.input.clone(),
                            output: Some(job.output.clone()),
                            status: Status::Waiting,
                            report: None,
                            job: Some(job),
                        }))
                        .collect();
                }
                Message::Progress(Event::Started(index)) => {
                    if let Some(row) = self.rows.get_mut(self.offset + index) {
                        row.status = Status::Converting;
                    }
                }
                Message::Progress(Event::Finished(index, report)) => {
                    if let Some(row) = self.rows.get_mut(self.offset + index) {
                        row.status = if report.get("ok") == Some(&Value::Bool(true)) {
                            Status::Converted
                        } else {
                            Status::Failed
                        };
                        row.report = Some(report);
                    }
                }
                Message::Done => {
                    self.run = None;
                    self.reveal = Some(Reveal::FirstIssue);
                    for row in &mut self.rows {
                        if matches!(row.status, Status::Waiting | Status::Converting) {
                            row.status = Status::NotStarted;
                        }
                    }
                }
            }
        }
    }

    fn take_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|file| file.path.clone())
                .collect()
        });
        if !dropped.is_empty() && !self.running() {
            self.add_inputs(dropped);
        }
    }

    /// Draws the window, for eframe and for tests.
    pub fn ui(&mut self, ctx: &egui::Context) {
        self.receive(ctx);
        self.take_dropped_files(ctx);

        egui::TopBottomPanel::top("title")
            .frame(
                Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(Margin::symmetric(
                        theme::SPACE_8 as i8,
                        theme::SPACE_4 as i8,
                    )),
            )
            .show_separator_line(false)
            .show(ctx, |ui| self.title(ui));

        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(Margin {
                        left: theme::SPACE_8 as i8,
                        right: theme::SPACE_8 as i8,
                        top: 0,
                        bottom: theme::SPACE_4 as i8,
                    }),
            )
            .show(ctx, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.inputs_panel(ui);
                        ui.add_space(theme::SPACE_4);
                        self.output_panel(ui, ctx);
                        if !self.rows.is_empty() {
                            ui.add_space(theme::SPACE_8);
                            self.results(ui);
                        }
                    });
            });
    }

    fn title(&mut self, ui: &mut egui::Ui) {
        let p = theme::palette(ui);
        ui.horizontal(|ui| {
            let mark = if ui.visuals().dark_mode {
                egui::include_image!("../assets/dutchman-mark-night.svg")
            } else {
                egui::include_image!("../assets/dutchman-mark.svg")
            };
            ui.add(egui::Image::new(mark).fit_to_exact_size(egui::vec2(40.0, 40.0)));
            ui.add_space(theme::SPACE_2);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("Dutchman").text_style(theme::title()));
                ui.label(
                    RichText::new("Converts native CAD designs into SolidCraft designs.")
                        .small()
                        .color(p.tide),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let current = ui.ctx().options(|o| o.theme_preference);
                for (preference, name) in [
                    (ThemePreference::Dark, "Night"),
                    (ThemePreference::Light, "Day"),
                    (ThemePreference::System, "System"),
                ] {
                    if ui.selectable_label(current == preference, name).clicked() {
                        ui.ctx().set_theme(preference);
                    }
                }
            });
        });
    }

    fn inputs_panel(&mut self, ui: &mut egui::Ui) {
        let p = *theme::palette(ui);
        let hovering = ui.ctx().input(|i| !i.raw.hovered_files.is_empty());
        section_label(ui, &p, "Designs");
        panel(ui, &p, hovering, |ui| {
            if self.inputs.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(theme::SPACE_4);
                    ui.label(RichText::new("Drop design files or folders here").heading());
                    ui.label(
                        RichText::new(if cfg!(feature = "freecad") {
                            "Fusion (.f3d, .f3z), Inventor (.ipt, .iam), SolidWorks (.sldprt) and FreeCAD (.FCStd)"
                        } else {
                            "Fusion (.f3d, .f3z), Inventor (.ipt, .iam) and SolidWorks (.sldprt)"
                        })
                        .small()
                        .color(p.tide),
                    );
                    ui.add_space(theme::SPACE_4);
                });
            } else {
                let mut remove = None;
                let running = self.running();
                for (index, input) in self.inputs.iter().enumerate() {
                    egui::Sides::new().shrink_left().truncate().show(
                        ui,
                        |ui| path_label(ui, &p, input),
                        |ui| {
                            let button = egui::Button::new(RichText::new("Remove").small());
                            if ui.add_enabled(!running, button).clicked() {
                                remove = Some(index);
                            }
                        },
                    );
                }
                if let Some(index) = remove {
                    self.inputs.remove(index);
                }
            }
            ui.add_space(theme::SPACE_2);
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!self.running(), |ui| {
                    if ui.button("Add files…").clicked() {
                        worker::pick(Pick::Files, self.sender.clone(), ui.ctx().clone());
                    }
                    if ui.button("Add folder…").clicked() {
                        worker::pick(Pick::Folder, self.sender.clone(), ui.ctx().clone());
                    }
                    if !self.inputs.is_empty() && ui.button("Clear").clicked() {
                        self.inputs.clear();
                    }
                });
            });
        });
    }

    fn output_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = *theme::palette(ui);
        section_label(ui, &p, "Output folder");
        panel(ui, &p, false, |ui| {
            let running = self.running();
            let sender = self.sender.clone();
            egui::Sides::new().shrink_left().truncate().show(
                ui,
                |ui| match &self.out_dir {
                    Some(dir) => {
                        ui.add(
                            egui::Label::new(RichText::new(display(dir)).monospace()).truncate(),
                        )
                        .on_hover_text(display(dir));
                    }
                    None => {
                        ui.label(RichText::new("Not chosen yet").color(p.tide));
                    }
                },
                |ui| {
                    if ui
                        .add_enabled(!running, egui::Button::new("Choose…"))
                        .clicked()
                    {
                        worker::pick(Pick::Output, sender, ctx.clone());
                    }
                },
            );
            ui.add_space(theme::SPACE_2);
            ui.add_enabled_ui(!self.running(), |ui| {
                ui.checkbox(
                    &mut self.options.import_only,
                    "Write geometry only, without feature history",
                );
                let mut replace = !self.options.no_clobber;
                if ui
                    .checkbox(&mut replace, "Replace designs already in the output folder")
                    .changed()
                {
                    self.options.no_clobber = !replace;
                }
                let mut verify = !self.options.no_verify;
                if ui
                    .checkbox(&mut verify, "Check feature history in SolidCraft")
                    .changed()
                {
                    self.options.no_verify = !verify;
                }
                let checker = match &self.checker {
                    Some(cli) => format!("Checks with {}.", display(cli)),
                    None => format!(
                        "No SolidCraft found, so history is written unchecked: install SolidCraft, \
                         or set {} to its solidcraft-cli.",
                        dutchman_solidcraft::CLI_ENV
                    ),
                };
                ui.add(egui::Label::new(RichText::new(checker).small().color(p.tide)).wrap());
                ui.add_space(theme::SPACE_2);
                let chosen = self.options.project.clone();
                let mut find = false;
                egui::Sides::new().shrink_left().truncate().show(
                    ui,
                    |ui| {
                        let project = match &chosen {
                            Some(dir) => display(dir),
                            None => "found from each assembly".to_owned(),
                        };
                        ui.add(
                            egui::Label::new(
                                RichText::new(format!("Inventor project folder: {project}"))
                                    .small()
                                    .color(p.tide),
                            )
                            .truncate(),
                        )
                        .on_hover_text(
                            "Where an assembly's part files are searched for when they are not \
                             at the paths it stores. Found from each assembly: the nearest folder \
                             above it holding an .ipj, else two folders up; a folder you add is \
                             its files' project.",
                        );
                    },
                    |ui| {
                        if chosen.is_some()
                            && ui.button(RichText::new("Automatic").small()).clicked()
                        {
                            find = true;
                        }
                        if ui.button(RichText::new("Choose…").small()).clicked() {
                            worker::pick(Pick::Project, self.sender.clone(), ctx.clone());
                        }
                    },
                );
                if find {
                    self.options.project = None;
                }
            });
        });
        ui.add_space(theme::SPACE_4);
        ui.horizontal(|ui| {
            if self.running() {
                if ui.button("Stop").clicked() {
                    if let Some(run) = &self.run {
                        run.stop();
                    }
                }
            } else {
                let ready = !self.inputs.is_empty() && self.out_dir.is_some();
                let convert = egui::Button::new(
                    RichText::new("Convert")
                        .color(if ready { p.on_spectre } else { p.tide })
                        .text_style(TextStyle::Button),
                )
                .fill(if ready { p.spectre } else { p.surface_2 })
                .stroke(Stroke::NONE)
                .min_size(egui::vec2(120.0, 34.0));
                if ui.add_enabled(ready, convert).clicked() {
                    self.start(ctx);
                }
                if !ready {
                    let why = if self.inputs.is_empty() {
                        "Add designs to convert."
                    } else {
                        "Choose an output folder."
                    };
                    ui.label(RichText::new(why).small().color(p.tide));
                }
            }
            if let Some(summary) = self.summary() {
                ui.label(RichText::new(summary).color(p.tide));
            }
        });
        if let Some(notice) = &self.notice {
            ui.label(RichText::new(notice).color(p.danger));
        }
    }

    fn summary(&self) -> Option<String> {
        if self.rows.is_empty() {
            return None;
        }
        let count = |status: Status| self.rows.iter().filter(|r| r.status == status).count();
        let (converted, failed) = (count(Status::Converted), count(Status::Failed));
        Some(if self.running() {
            format!("{} of {} done", converted + failed, self.rows.len())
        } else {
            let incomplete = self
                .rows
                .iter()
                .filter(|row| {
                    row.status == Status::Converted && row.report.as_ref().is_some_and(incomplete)
                })
                .count();
            let mut text = if incomplete > 0 {
                format!("{converted} converted ({incomplete} incomplete), {failed} failed")
            } else {
                format!("{converted} converted, {failed} failed")
            };
            let skipped = count(Status::NotStarted);
            if skipped > 0 {
                text.push_str(&format!(", {skipped} not started"));
            }
            text
        })
    }

    fn results(&mut self, ui: &mut egui::Ui) {
        let p = *theme::palette(ui);
        let heading = section_label(ui, &p, "Results");
        let reveal = self.reveal.take();
        if reveal == Some(Reveal::Results) {
            heading.scroll_to_me(Some(Align::Min));
        }
        let first = (reveal == Some(Reveal::FirstIssue)).then(|| first_to_show(&self.rows));
        let mut action = None;
        let running = self.running();
        for (index, row) in self.rows.iter().enumerate() {
            let shown = card(ui, &p, |ui| {
                if let Some(chosen) = row_ui(ui, &p, index, row, self.has_app, running) {
                    action = Some(chosen);
                }
            });
            if first == Some(index) {
                shown.scroll_to_me(Some(Align::Min));
            }
            ui.add_space(theme::SPACE_2);
        }
        let result = match action {
            Some(Action::Open(path)) => platform::open_in_solidcraft(&path),
            Some(Action::Reveal(path)) => platform::reveal(&path),
            Some(Action::Pick(pick)) => {
                worker::pick(pick, self.sender.clone(), ui.ctx().clone());
                Ok(())
            }
            None => Ok(()),
        };
        if let Err(err) = result {
            self.notice = Some(err);
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.ui(ctx);
    }
}

enum Action {
    Open(PathBuf),
    Reveal(PathBuf),
    Pick(Pick),
}

/// One file's result: its status, what was written, and its losses.
fn row_ui(
    ui: &mut egui::Ui,
    p: &Palette,
    index: usize,
    row: &Row,
    has_app: bool,
    running: bool,
) -> Option<Action> {
    let mut action = None;
    let report = row.report.as_ref();
    let (word, colour) = match row.status {
        Status::Waiting => ("Waiting", p.tide),
        Status::Converting => ("Converting", p.tide),
        Status::Converted if report.is_some_and(incomplete) => ("Incomplete", p.lantern_text),
        Status::Converted => ("Converted", p.spectre),
        Status::Failed => ("Failed", p.danger),
        Status::NotStarted => ("Not started", p.tide),
    };
    let output = row
        .output
        .as_ref()
        .filter(|_| row.status == Status::Converted);
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            if row.status == Status::Converting {
                ui.add(egui::Spinner::new().size(14.0).color(p.lantern));
            }
            ui.label(RichText::new(word).heading().color(colour));
            ui.add(egui::Label::new(RichText::new(file_name(&row.input)).heading()).truncate())
                .on_hover_text(display(&row.input));
        },
        |ui| {
            let Some(output) = output else { return };
            if ui.button("Show in folder").clicked() {
                action = Some(Action::Reveal(output.clone()));
            }
            let open = ui.button("Open in SolidCraft");
            let open = if has_app {
                open
            } else {
                open.on_hover_text(format!(
                    "Opens with the system's app for .solidcraft files. Set {} to choose SolidCraft.",
                    platform::APP_ENV
                ))
            };
            if open.clicked() {
                action = Some(Action::Open(output.clone()));
            }
        },
    );
    if let Some(missing) = report.and_then(missing_parts) {
        missing_ui(ui, p, &missing);
        ui.horizontal(|ui| {
            ui.add_enabled_ui(!running && row.job.is_some(), |ui| {
                if ui.button("Search a folder…").clicked() {
                    action = Some(Action::Pick(Pick::SearchFolder(index)));
                }
                if ui.button("Find a file…").clicked() {
                    action = Some(Action::Pick(Pick::SearchFile(index)));
                }
            });
        });
    }
    if let Some(message) = report
        .and_then(|r| r.pointer("/error/message"))
        .and_then(Value::as_str)
    {
        ui.label(RichText::new(message).color(p.danger));
        return action;
    }
    if let (Status::Converted, Some(report)) = (&row.status, report) {
        let output = row.output.as_deref().map(display).unwrap_or_default();
        ui.add(
            egui::Label::new(
                RichText::new(format!("→ {output}"))
                    .monospace()
                    .color(p.tide),
            )
            .truncate(),
        )
        .on_hover_text(output);
        ui.label(RichText::new(outcome(report)).color(p.ink));
        let lines = dutchman::json::completeness_lines(report);
        if let (Some(summary), Some((_, causes))) =
            (dutchman::json::assembly_line(report), lines.split_first())
        {
            let colour = if incomplete(report) { p.danger } else { p.tide };
            ui.label(RichText::new(summary).color(colour));
            for cause in causes {
                ui.label(RichText::new(cause.trim()).small().color(p.ink));
            }
        }
        let losses: Vec<&Value> = report
            .get("losses")
            .and_then(Value::as_array)
            .map(|l| l.iter().collect())
            .unwrap_or_default();
        if !losses.is_empty() {
            let components = dutchman::json::components(report);
            egui::CollapsingHeader::new(RichText::new(loss_counts(&losses)).color(p.tide))
                .id_salt(("losses", index))
                .show(ui, |ui| {
                    // One component: its losses. Several: each in its own section, most that
                    // matters first.
                    if let [only] = components.as_slice() {
                        for loss in &only.losses {
                            loss_line(ui, p, loss);
                        }
                        return;
                    }
                    for (n, component) in components.iter().enumerate() {
                        let name = component.file.as_deref().unwrap_or("The design");
                        let colour = if component.dropped > 0 {
                            p.danger
                        } else if component.degraded > 0 || component.flattened.is_some() {
                            p.lantern_text
                        } else {
                            p.tide
                        };
                        egui::CollapsingHeader::new(
                            RichText::new(format!("{name}: {}", component.summary())).color(colour),
                        )
                        .id_salt(("component", index, n))
                        .show(ui, |ui| {
                            for loss in &component.losses {
                                loss_line(ui, p, loss);
                            }
                        });
                    }
                });
            if ui.button("Save the full log…").clicked() {
                action = Some(Action::Pick(Pick::SaveLog(index)));
            }
        }
    }
    action
}

/// Whether `report` is of an assembly that converted with placements left out.
fn incomplete(report: &Value) -> bool {
    report
        .pointer("/completeness/left_out")
        .and_then(Value::as_array)
        .is_some_and(|causes| !causes.is_empty())
}

/// What marks a part file an assembly places as not found, in its loss.
const NOT_FOUND: &str = ": not found at its stored path";

/// The part files an assembly's conversion could not find, by name, when it missed any; an
/// empty list when it failed for want of all of them, without naming them.
fn missing_parts(report: &Value) -> Option<Vec<String>> {
    let mut names: Vec<String> = report
        .get("losses")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|loss| loss.get("message").and_then(Value::as_str))
        .filter_map(|message| message.find(NOT_FOUND).map(|at| message.get(..at)))
        .flatten()
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    let none_found = report
        .pointer("/error/message")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("parts could be found or read"));
    (!names.is_empty() || none_found).then_some(names)
}

/// The part files not found, the first few by name.
fn missing_ui(ui: &mut egui::Ui, p: &Palette, missing: &[String]) {
    const SHOWN: usize = 5;
    let text = match missing.len() {
        0 => "Its part files were not found.".to_owned(),
        n => {
            let mut text = format!(
                "{n} part file{} not found: {}",
                if n == 1 { " was" } else { "s were" },
                missing
                    .iter()
                    .take(SHOWN)
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if n > SHOWN {
                text.push_str(&format!(" and {} more", n - SHOWN));
            }
            text
        }
    };
    ui.label(RichText::new(text).color(p.danger));
    ui.label(
        RichText::new(
            "Choose the folder that holds them, or one of them, to convert this file again \
             searching there too.",
        )
        .small()
        .color(p.tide),
    );
}

/// What was written, in a sentence.
fn outcome(report: &Value) -> String {
    let field = |pointer: &str| report.pointer(pointer).and_then(Value::as_str);
    let history = |mapped: u64, total: u64| format!("{mapped} of {total} features mapped");
    let counts = (
        report.pointer("/history/mapped").and_then(Value::as_u64),
        report.pointer("/history/total").and_then(Value::as_u64),
    );
    match field("/mode") {
        Some("history") => {
            let check = match (field("/verification/status"), field("/verification/reason")) {
                (Some("verified"), _) => "verified in SolidCraft".to_owned(),
                (_, Some(reason)) => format!("unverified: {reason}"),
                _ => "unverified".to_owned(),
            };
            match counts {
                (Some(m), Some(t)) => format!("Editable history, {}; {check}.", history(m, t)),
                _ => format!("Editable history; {check}."),
            }
        }
        _ => match (field("/verification/status"), field("/verification/reason")) {
            (Some("flattened"), Some(reason)) => {
                format!("Geometry without history (flattened: {reason}).")
            }
            _ => "Geometry without history.".to_owned(),
        },
    }
}

fn loss_counts(losses: &[&Value]) -> String {
    let count = |level: &str| {
        losses
            .iter()
            .filter(|l| l.get("level").and_then(Value::as_str) == Some(level))
            .count()
    };
    let parts: Vec<String> = ["Info", "Degraded", "Dropped"]
        .into_iter()
        .map(|level| (level, count(level)))
        .filter(|(_, n)| *n > 0)
        .map(|(level, n)| format!("{n} {level}"))
        .collect();
    format!("Losses: {}", parts.join(", "))
}

/// One loss: its level word, coloured, then its subject and message.
fn loss_line(ui: &mut egui::Ui, p: &Palette, loss: &Value) {
    let field = |key: &str| loss.get(key).and_then(Value::as_str).unwrap_or("");
    let level = field("level");
    let colour = match level {
        "Degraded" => p.lantern_text,
        "Dropped" => p.danger,
        _ => p.tide,
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_1;
        ui.label(RichText::new(level).monospace().color(colour));
        ui.label(
            RichText::new(format!("{}: {}", field("subject"), field("message")))
                .monospace()
                .color(p.ink),
        );
    });
}

fn section_label(ui: &mut egui::Ui, p: &Palette, text: &str) -> egui::Response {
    ui.label(
        RichText::new(text.to_uppercase())
            .text_style(theme::label())
            .color(p.tide),
    )
}

/// The row to show first when a run ends: the first that failed or converted incomplete,
/// else the first.
fn first_to_show(rows: &[Row]) -> usize {
    rows.iter()
        .position(|row| {
            row.status == Status::Failed
                || (row.status == Status::Converted && row.report.as_ref().is_some_and(incomplete))
        })
        .unwrap_or(0)
}

/// A raised panel on `surface-2` with a hairline border (spectre while files hover over it).
/// It holds ink and tide text only: by day, spectre and danger text need `surface`.
fn panel(ui: &mut egui::Ui, p: &Palette, highlight: bool, body: impl FnOnce(&mut egui::Ui)) {
    framed(ui, p, p.surface_2, highlight, body);
}

/// A result card: `surface` with a hairline border, so status and loss colours read on it.
fn card(ui: &mut egui::Ui, p: &Palette, body: impl FnOnce(&mut egui::Ui)) -> egui::Response {
    framed(ui, p, p.surface, false, body)
}

fn framed(
    ui: &mut egui::Ui,
    p: &Palette,
    fill: egui::Color32,
    highlight: bool,
    body: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let stroke = if highlight {
        Stroke::new(1.5_f32, p.spectre)
    } else {
        Stroke::new(1.0_f32, p.hairline)
    };
    Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(CornerRadius::same(theme::RADIUS_LG))
        .inner_margin(Margin::same(theme::SPACE_4 as i8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        })
        .response
}

/// A file's name, then its folder in `tide`, cut short to fit; the whole path on hover.
fn path_label(ui: &mut egui::Ui, p: &Palette, path: &Path) {
    let folder = path.parent().map(display).unwrap_or_default();
    let mut job = egui::text::LayoutJob::default();
    RichText::new(file_name(path)).append_to(
        &mut job,
        ui.style(),
        egui::FontSelection::Default,
        Align::Center,
    );
    RichText::new(format!("   {folder}"))
        .small()
        .color(p.tide)
        .append_to(
            &mut job,
            ui.style(),
            egui::FontSelection::Default,
            Align::Center,
        );
    ui.add(egui::Label::new(job).truncate())
        .on_hover_text(display(path));
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| display(path))
}

fn display(path: &Path) -> String {
    path.display().to_string()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    fn row(status: Status) -> Row {
        Row {
            input: PathBuf::from("a.f3d"),
            output: None,
            status,
            report: None,
            job: None,
        }
    }

    #[test]
    fn a_finished_run_shows_its_first_failure_first() {
        let rows = [
            row(Status::Converted),
            row(Status::Failed),
            row(Status::Failed),
        ];
        assert_eq!(first_to_show(&rows), 1);
        assert_eq!(first_to_show(&[row(Status::Converted)]), 0);
        let mut incomplete = row(Status::Converted);
        incomplete.report = Some(json!({"completeness": {"placed": 1, "placements": 2,
            "left_out": [{"file": "pin.ipt", "reason": "gone", "stage": "input", "count": 1}]}}));
        assert_eq!(
            first_to_show(&[row(Status::Converted), incomplete, row(Status::Failed)]),
            1
        );
        assert_eq!(first_to_show(&[]), 0);
    }

    #[test]
    fn the_parts_an_assembly_could_not_find_are_named() {
        let report = json!({"ok": true, "losses": [
            {"level": "Dropped", "subject": "occurrence \"Pin:1\"",
             "message": "pin.ipt: not found at its stored path or in the project folder /p"},
            {"level": "Dropped", "subject": "sub.iam: occurrence \"Pin:2\"",
             "message": "pin.ipt: not found at its stored path or in the project folder /p"},
            {"level": "Dropped", "subject": "occurrence \"Base:1\"",
             "message": "base.ipt: not found at its stored path or in the project folder /p"},
            {"level": "Info", "subject": "history", "message": "flattened"},
        ]});
        assert_eq!(missing_parts(&report).unwrap(), ["base.ipt", "pin.ipt"]);
        let failed = json!({"ok": false, "error": {"message": "cannot read this design: none of \
            the assembly's parts could be found or read; if they are elsewhere, give the project \
            folder that holds them"}});
        assert_eq!(missing_parts(&failed).unwrap(), Vec::<String>::new());
        assert_eq!(missing_parts(&json!({"ok": true, "losses": []})), None);
    }

    #[test]
    fn an_assembly_with_placements_left_out_is_incomplete() {
        assert!(incomplete(
            &json!({"completeness": {"placed": 1, "placements": 2,
            "left_out": [{"file": "pin.ipt", "reason": "gone", "stage": "input", "count": 1}]}})
        ));
        assert!(!incomplete(
            &json!({"completeness": {"placed": 2, "placements": 2,
            "left_out": []}})
        ));
        assert!(!incomplete(&json!({"ok": true})));
    }
}
