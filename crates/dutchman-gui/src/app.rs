//! The window: designs in, an output folder, Convert, and what became of each file.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use dutchman::batch::{Event, Job};
use dutchman::present::FileView;
use dutchman::{json, present, ConvertOptions};
use eframe::egui::{
    self, Align, CornerRadius, Frame, Layout, Margin, RichText, ScrollArea, Stroke, TextStyle,
    ThemePreference,
};
use serde_json::Value;

use crate::platform;
use crate::theme::{self, Palette};
use crate::worker::{self, Message, Pick, Run};

pub use dutchman::present::Progress as Status;

/// One file of a run.
pub struct Row {
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub status: Status,
    pub progress: Option<dutchman_core::ProgressEvent>,
    /// Its `dutchman.convert/1` report, once it has one.
    pub report: Option<Value>,
    /// The job that converts it; none for an input that could not be planned.
    pub job: Option<Job>,
}

impl Row {
    fn view(&self) -> FileView {
        let mut view = FileView::new(
            &self.input,
            self.output.as_deref(),
            self.status,
            self.report.as_ref(),
        );
        if view.capability.is_none() {
            view.capability = dutchman_core::Format::from_path(&self.input)
                .map(|format| format.status_for(&self.input));
        }
        view
    }
}

/// Dutchman Convert's window.
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
    checker: Result<PathBuf, dutchman_solidcraft::NoCli>,
    detected_version: dutchman_solidcraft::SolidCraftVersion,
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
        let detected_version = dutchman_solidcraft::SolidCraftVersion::detected(
            &dutchman_solidcraft::Verification::Auto,
        );
        App {
            inputs: Vec::new(),
            out_dir: None,
            options: ConvertOptions {
                solidcraft_version: Some(detected_version),
                ..ConvertOptions::default()
            },
            rows: Vec::new(),
            offset: 0,
            run: None,
            messages,
            sender,
            notice: None,
            has_app: platform::has_solidcraft_app(),
            checker: dutchman_solidcraft::usable_cli(&dutchman_solidcraft::Verification::Auto),
            detected_version,
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
        self.refresh_blocked_checker();
        self.notice = None;
        self.run = Some(Run::retry(
            job,
            job_index,
            &self.options,
            self.sender.clone(),
            ctx.clone(),
        ));
    }

    /// Opens the GitHub issue form about the file of the row at `index`, filled in from its
    /// report. When its anonymous report is too long for the link, it is copied for the
    /// reporter to paste, and the notice says so.
    pub fn report_on_github(&mut self, index: usize, ctx: &egui::Context) -> Result<(), String> {
        let Some(report) = self.rows.get(index).and_then(|row| row.report.as_ref()) else {
            return Ok(());
        };
        let out_dir = self.out_dir.clone().unwrap_or_default();
        let run = dutchman::anon::Run {
            verify: !self.options.no_verify,
            import_only: self.options.import_only,
            jobs: 1,
            ..dutchman::anon::Run::default()
        };
        let link = present::ReportLink::new(dutchman::issue::issue(report, &out_dir, &run)?);
        let issue = link.issue;
        if let Some(notice) = link.copy_notice {
            ctx.copy_text(issue.anonymous_report.clone());
            self.notice = Some(notice.to_owned());
        }
        ctx.open_url(egui::OpenUrl::new_tab(issue.url));
        Ok(())
    }

    /// Whether a run is in progress.
    pub fn running(&self) -> bool {
        self.run.is_some()
    }

    fn refresh_blocked_checker(&mut self) {
        if matches!(self.checker, Err(dutchman_solidcraft::NoCli::Blocked(_))) {
            self.checker =
                dutchman_solidcraft::usable_cli(&dutchman_solidcraft::Verification::Auto);
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        self.refresh_blocked_checker();
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
                            progress: None,
                            job: None,
                        })
                        .chain(jobs.into_iter().map(|job| Row {
                            input: job.input.clone(),
                            output: Some(job.output.clone()),
                            status: Status::Waiting,
                            progress: None,
                            report: None,
                            job: Some(job),
                        }))
                        .collect();
                }
                Message::Progress(Event::Started(index)) => {
                    if let Some(row) = self.rows.get_mut(self.offset + index) {
                        row.status = Status::Converting;
                        row.progress = None;
                    }
                }
                Message::Progress(Event::Progress(index, event)) => {
                    if let Some(row) = self.rows.get_mut(self.offset + index) {
                        row.progress = Some(event);
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
        if ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::WindowFocused(true)))
        }) {
            self.refresh_blocked_checker();
        }
        self.receive(ctx);
        self.take_dropped_files(ctx);
        let view = self.view();

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
                        self.output_panel(ui, ctx, view.summary.as_deref());
                        if !self.rows.is_empty() {
                            ui.add_space(theme::SPACE_8);
                            self.results(ui, &view);
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
                ui.label(RichText::new(crate::APP_NAME).text_style(theme::title()));
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
        ui.hyperlink_to("Report a problem", crate::REPORT_PROBLEM_URL);
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
                        RichText::new("Fusion, Inventor, SolidWorks, FreeCAD, CATIA, Creo, NX, Creo View, Rhino, ACIS, STEP and IGES")
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
                        |ui| {
                            path_label(ui, &p, input);
                            if let Some(format) = dutchman_core::Format::from_path(input) {
                                let level = format
                                    .headline_level()
                                    .map(|level| format!("  {level}"))
                                    .unwrap_or_default();
                                ui.label(format!("{}{level}", format.status_for(input)));
                            }
                        },
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

    fn output_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, summary: Option<&str>) {
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
                // On the line of the geometry-only choice, so the window keeps its height and
                // Convert stays in view.
                ui.horizontal_wrapped(|ui| {
                    ui.checkbox(
                        &mut self.options.import_only,
                        "Write geometry only, without feature history",
                    );
                    ui.add_space(theme::SPACE_4);
                    egui::ComboBox::from_label("Output format")
                        .selected_text(match self.options.to {
                            dutchman::OutputFormat::Solidcraft => "SolidCraft",
                            dutchman::OutputFormat::Freecad => "FreeCAD macro (experimental)",
                            dutchman::OutputFormat::Fcstd => "FreeCAD document (experimental)",
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.options.to,
                                dutchman::OutputFormat::Solidcraft,
                                "SolidCraft",
                            );
                            ui.selectable_value(
                                &mut self.options.to,
                                dutchman::OutputFormat::Freecad,
                                "FreeCAD macro (experimental)",
                            );
                            ui.selectable_value(
                                &mut self.options.to,
                                dutchman::OutputFormat::Fcstd,
                                "FreeCAD document (experimental)",
                            );
                        });
                });
                let mut replace = !self.options.no_clobber;
                if ui
                    .checkbox(&mut replace, "Replace designs already in the output folder")
                    .changed()
                {
                    self.options.no_clobber = !replace;
                }
                egui::ComboBox::from_id_salt("solidcraft_version")
                    .selected_text(format!(
                        "Target SolidCraft {}",
                        self.options
                            .solidcraft_version
                            .unwrap_or(self.detected_version)
                    ))
                    .show_ui(ui, |ui| {
                        for (version, label) in [
                            (
                                self.detected_version,
                                format!("Found version ({})", self.detected_version),
                            ),
                            (
                                dutchman_solidcraft::SolidCraftVersion::default(),
                                "0.18.2 and newer".to_owned(),
                            ),
                            (
                                dutchman_solidcraft::SolidCraftVersion::V0_25_1,
                                "0.25.1 and newer".to_owned(),
                            ),
                        ] {
                            ui.selectable_value(
                                &mut self.options.solidcraft_version,
                                Some(version),
                                label,
                            );
                        }
                    });
                let mut verify = !self.options.no_verify;
                if ui
                    .checkbox(&mut verify, "Check feature history in SolidCraft")
                    .changed()
                {
                    self.options.no_verify = !verify;
                }
                let checker = match &self.checker {
                    Ok(cli) => format!("Checks with {}.", display(cli)),
                    Err(dutchman_solidcraft::NoCli::Blocked(why)) => why.clone(),
                    Err(dutchman_solidcraft::NoCli::TooOld(why)) => {
                        format!("History is written unchecked: {why}.")
                    }
                    Err(dutchman_solidcraft::NoCli::NotFound(_)) => format!(
                        "No SolidCraft found, so history is written unchecked: install SolidCraft, \
                         or set {} to its solidcraft-cli.",
                        dutchman_solidcraft::CLI_ENV
                    ),
                };
                let label = egui::Label::new(RichText::new(&checker).small().color(p.tide));
                if matches!(self.checker, Err(dutchman_solidcraft::NoCli::Blocked(_))) {
                    ui.add(label.wrap());
                } else {
                    // Keep paths on one line; the whole path is on hover.
                    ui.add(label.truncate()).on_hover_text(checker);
                }
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
            if let Some(summary) = summary.filter(|_| !self.rows.is_empty()) {
                ui.label(RichText::new(summary).color(p.tide));
            }
            ui.menu_button("Formats", |ui| {
                for format in dutchman_core::Format::ALL {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format.name());
                        ui.label(format.words());
                        if let Some(level) = format.headline_level() {
                            ui.label(level);
                        }
                    });
                }
            });
        });
        if let Some(notice) = &self.notice {
            ui.label(RichText::new(notice).color(p.danger));
        }
    }

    fn view(&self) -> present::RunView {
        present::RunView::from_files(
            self.rows.iter().map(Row::view).collect(),
            self.rows.iter().filter_map(|row| row.report.as_ref()),
            self.running(),
        )
    }

    fn results(&mut self, ui: &mut egui::Ui, view: &present::RunView) {
        let p = *theme::palette(ui);
        let heading = section_label(ui, &p, "Results");
        let reveal = self.reveal.take();
        if reveal == Some(Reveal::Results) {
            heading.scroll_to_me(Some(Align::Min));
        }
        let first = (reveal == Some(Reveal::FirstIssue)).then_some(view.first_to_show);
        let mut action = None;
        let running = self.running();
        let mut files: Vec<_> = view.files.iter().collect();
        files.sort_by_key(|(index, _)| *index);
        for (index, file) in files {
            let index = *index;
            let Some(row) = self.rows.get(index) else {
                continue;
            };
            let shown = card(ui, &p, |ui| {
                if let Some(chosen) = row_ui(ui, &p, index, row, file, self.has_app, running) {
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
            Some(Action::Report(index)) => self.report_on_github(index, ui.ctx()),
            None => Ok(()),
        };
        if let Err(err) = result {
            self.notice = Some(err);
        }
        if !running {
            if let Some(pitch) = &view.invitation {
                ui.horizontal_wrapped(|ui| {
                    for (index, part) in pitch.split(json::FIXTURES_URL).enumerate() {
                        if index > 0 {
                            ui.hyperlink_to(json::FIXTURES_URL, json::FIXTURES_URL);
                        }
                        for (index, text) in part.split(json::DISCORD_URL).enumerate() {
                            if index > 0 {
                                ui.hyperlink_to(json::DISCORD_URL, json::DISCORD_URL);
                            }
                            if !text.is_empty() {
                                ui.label(text);
                            }
                        }
                    }
                });
            }
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
    /// Open the GitHub issue form about the row's file, filled in.
    Report(usize),
}

/// One file's result: its status, what was written, and its losses.
fn row_ui(
    ui: &mut egui::Ui,
    p: &Palette,
    index: usize,
    row: &Row,
    view: &FileView,
    has_app: bool,
    running: bool,
) -> Option<Action> {
    let mut action = None;
    let colour = match view.status {
        present::Status::Incomplete => p.lantern_text,
        present::Status::Converted
        | present::Status::ConvertedWithLosses
        | present::Status::Flattened => p.spectre,
        present::Status::Failed => p.danger,
        _ => p.tide,
    };
    let word = view.status_word;
    let output = row
        .output
        .as_ref()
        .filter(|_| row.status == Status::Converted);
    egui::Sides::new().shrink_left().truncate().show(
        ui,
        |ui| {
            ui.label(RichText::new(word).heading().color(colour));
            ui.add(egui::Label::new(RichText::new(&view.name).heading()).truncate())
                .on_hover_text(display(&row.input));
        },
        |ui| {
            let Some(output) = output else { return };
            if ui.button("Show in folder").clicked() {
                action = Some(Action::Reveal(output.clone()));
            }
            if output
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("FCMacro"))
            {
                ui.label("Run macro in FreeCAD")
                    .on_hover_text("In FreeCAD, choose Macro > Macros… > Execute.");
                return;
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
    if row.status == Status::Converting {
        if let Some(event) = row.progress {
            ui.label(event.to_string());
            if let Some(total) = event.designs_total.filter(|total| *total > 0) {
                ui.add(
                    egui::ProgressBar::new(
                        (event.designs_read as f32 / total as f32).clamp(0.0, 1.0),
                    )
                    .desired_width(200.0),
                );
            } else {
                ui.add(egui::Spinner::new().size(14.0).color(p.lantern));
            }
        } else {
            ui.add(egui::Spinner::new().size(14.0).color(p.lantern));
        }
    }
    if let Some(status) = &view.capability {
        ui.label(status);
    }
    if let Some(text) = &view.missing_summary {
        missing_ui(ui, p, text);
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
    if let Some(message) = &view.error {
        ui.label(RichText::new(message).color(p.danger));
        if view.can_report {
            ui.label(RichText::new(present::REPORT_HELP).small().color(p.tide));
            if ui.button("Report this on GitHub").clicked() {
                action = Some(Action::Report(index));
            }
        }
        return action;
    }
    if row.status == Status::Converted && row.report.is_some() {
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
        ui.label(RichText::new(&view.outcome).color(p.ink));
        if let Some(summary) = &view.assembly {
            let colour = if view.incomplete { p.danger } else { p.tide };
            ui.label(RichText::new(summary).color(colour));
            for cause in &view.assembly_causes {
                ui.label(RichText::new(cause).small().color(p.ink));
            }
        }
        if !view.components.is_empty() {
            let components = &view.components;
            egui::CollapsingHeader::new(RichText::new(&view.loss_summary).color(p.tide))
                .id_salt(("losses", index))
                .show(ui, |ui| {
                    // One component: its losses. Several: each in its own section, most that
                    // matters first.
                    if let [only] = components.as_slice() {
                        for loss in &only.lines {
                            loss_line(ui, p, loss);
                        }
                        return;
                    }
                    for (n, component) in components.iter().enumerate() {
                        let colour = match component.severity_rank {
                            0 => p.danger,
                            1 => p.lantern_text,
                            _ => p.tide,
                        };
                        egui::CollapsingHeader::new(
                            RichText::new(&component.heading).color(colour),
                        )
                        .id_salt(("component", index, n))
                        .show(ui, |ui| {
                            for loss in &component.lines {
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

/// The missing-part summary and recovery instructions.
fn missing_ui(ui: &mut egui::Ui, p: &Palette, text: &str) {
    ui.label(RichText::new(text).color(p.danger));
    ui.label(RichText::new(present::MISSING_HELP).small().color(p.tide));
}

/// One loss: its level word, coloured, then its subject and message.
fn loss_line(ui: &mut egui::Ui, p: &Palette, loss: &present::LossLine) {
    let level = loss.level.as_str();
    let colour = match level {
        "Degraded" => p.lantern_text,
        "Dropped" => p.danger,
        _ => p.tide,
    };
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = theme::SPACE_1;
        ui.label(RichText::new(level).monospace().color(colour));
        ui.label(RichText::new(&loss.text).monospace().color(p.ink));
    });
}

fn section_label(ui: &mut egui::Ui, p: &Palette, text: &str) -> egui::Response {
    ui.label(
        RichText::new(text.to_uppercase())
            .text_style(theme::label())
            .color(p.tide),
    )
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

    #[test]
    fn result_list_uses_shared_strings() {
        use egui_kittest::kittest::Queryable;
        let reports = [
            json!({"ok": true, "mode": "history", "history": {"mapped": 3, "total": 3},
                "verification": {"status": "verified"}, "losses": []}),
            json!({"ok": true, "mode": "import", "verification": {"status": "flattened", "reason": "fillet is not mapped yet"},
                "losses": [{"level": "Degraded", "subject": "history", "message": "flattened: fillet is not mapped yet"}]}),
            json!({"ok": true, "completeness": {"placed": 1, "placements": 2, "left_out": [{"file": "pin.ipt", "count": 1, "reason": "not found", "stage": "input"}]},
                "losses": [{"level": "Dropped", "subject": "occurrence", "message": "pin.ipt: not found at its stored path"}]}),
            json!({"ok": false, "error": {"message": "cannot read this design: no geometry could be read", "stage": "decode", "upstream": "cadmpeg"}}),
        ];
        let rows: Vec<Row> = reports
            .into_iter()
            .enumerate()
            .map(|(n, report)| Row {
                input: PathBuf::from(format!("design-{n}.f3d")),
                output: Some(PathBuf::from(format!("converted/design-{n}.solidcraft"))),
                status: if report.get("ok") == Some(&Value::Bool(true)) {
                    Status::Converted
                } else {
                    Status::Failed
                },
                report: Some(report),
                progress: None,
                job: None,
            })
            .collect();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1000.0, 1200.0))
            .build_ui(|ui| {
                theme::install(ui.ctx());
                let p = *theme::palette(ui);
                for (index, row) in rows.iter().enumerate() {
                    card(ui, &p, |ui| {
                        row_ui(ui, &p, index, row, &row.view(), true, false);
                    });
                    ui.add_space(theme::SPACE_2);
                }
            });
        harness.run();
        for label in [
            "Editable history, 3 of 3 features mapped; verified in SolidCraft.",
            "Geometry without history (flattened: fillet is not mapped yet).",
            "Incomplete",
            "1 part file was not found: pin.ipt",
            "Losses: 1 Degraded",
            "Losses: 1 Dropped",
            "cannot read this design: no geometry could be read",
            "Report this on GitHub",
        ] {
            assert!(harness.query_by_label(label).is_some(), "missing {label}");
        }
        use egui_kittest::kittest::NodeT;
        let labels = harness
            .query_all_by(|node| node.label().is_some())
            .filter_map(|node| node.accesskit_node().label())
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(dir) = std::env::var_os("DUTCHMAN_PRESENT_SCREENSHOTS") {
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(PathBuf::from(&dir).join("synthetic-results.txt"), &labels).unwrap();

            harness
                .render()
                .unwrap()
                .save(PathBuf::from(&dir).join("synthetic-results.png"))
                .unwrap();
            if let Some(before) = std::env::var_os("DUTCHMAN_PRESENT_COMPARE") {
                assert_eq!(
                    std::fs::read_to_string(PathBuf::from(&before).join("synthetic-results.txt"))
                        .unwrap(),
                    labels
                );
                let before = image::open(PathBuf::from(before).join("synthetic-results.png"))
                    .unwrap()
                    .to_rgba8();
                let after = image::open(PathBuf::from(dir).join("synthetic-results.png"))
                    .unwrap()
                    .to_rgba8();
                assert_eq!(before.dimensions(), after.dimensions());
                let changed = before
                    .pixels()
                    .zip(after.pixels())
                    .filter(|(a, b)| a != b)
                    .count();
                assert_eq!(changed, 0, "result list pixels changed");
            }
        }
    }

    #[test]
    fn converting_cards_show_stage_counts_and_elapsed_time() {
        use egui_kittest::kittest::Queryable;
        for total in [Some(4), None] {
            let event = dutchman_core::ProgressEvent {
                stage: dutchman_core::Stage::Decode,
                elapsed_ms: 15_000,
                designs_read: 2,
                designs_total: total,
                heartbeat: true,
            };
            let row = Row {
                input: PathBuf::from("assembly.f3z"),
                output: None,
                status: Status::Converting,
                progress: Some(event),
                report: None,
                job: None,
            };
            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(1200.0, 400.0))
                .build_ui(|ui| {
                    theme::install(ui.ctx());
                    let p = *theme::palette(ui);
                    row_ui(ui, &p, 0, &row, &row.view(), true, false);
                });
            harness.run_steps(2);
            assert!(harness.query_by_label(&event.to_string()).is_some());
        }
    }

    #[test]
    fn a_blocked_checker_shows_the_recovery_message() {
        use egui_kittest::kittest::Queryable;
        let message =
            "macOS has not yet allowed SolidCraft to run: open SolidCraft once from Applications.";
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(760.0, 680.0))
            .build_eframe(move |cc| {
                let mut app = App::new(cc);
                app.checker = Err(dutchman_solidcraft::NoCli::Blocked(message.to_owned()));
                app
            });
        harness.run();
        assert!(harness.query_by_label(message).is_some());
    }
}
