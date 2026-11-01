//! Local diagnosis wizard. Every operation starts with a click; network submission is absent.
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

use dutchman::diagnose::{narrow, Settings};
use eframe::egui;
use serde_json::Value;

enum Message {
    Progress(String),
    Finished(Result<Value, String>),
    Checked(PathBuf, Result<Value, String>),
    Cancelled,
}

/// State retained while the wizard works off the UI thread.
pub(crate) struct Wizard {
    input: PathBuf,
    out: PathBuf,
    settings: narrow::NarrowSettings,
    receiver: Option<Receiver<Message>>,
    progress: String,
    recipe: String,
    summary: String,
    small: Option<PathBuf>,
    checked: bool,
    report_ready: bool,
}

impl Wizard {
    pub(crate) fn new(input: PathBuf, options: dutchman::ConvertOptions) -> Self {
        let name = input.file_name().unwrap_or_default().to_string_lossy();
        let out = input.with_file_name(format!("{name}.diagnosis"));
        Self {
            input,
            out,
            settings: narrow::NarrowSettings::new(
                Settings {
                    options,
                    ..Default::default()
                },
                None,
            ),
            receiver: None,
            progress: String::new(),
            recipe: String::new(),
            summary: String::new(),
            small: None,
            checked: false,
            report_ready: false,
        }
    }

    pub(crate) fn start(&mut self, ctx: &egui::Context) {
        let (send, receive) = channel();
        self.receiver = Some(receive);
        self.progress = "Starting or resuming the local diagnosis…".into();
        self.checked = false;
        self.report_ready = false;
        let (input, out, settings, ctx) = (
            self.input.clone(),
            self.out.clone(),
            self.settings.clone(),
            ctx.clone(),
        );
        std::thread::spawn(move || {
            let result = narrow::run(&input, &out, &settings, &|progress| {
                let _ = send.send(Message::Progress(progress.to_string()));
                ctx.request_repaint();
            });
            let _ = send.send(Message::Finished(result));
            ctx.request_repaint();
        });
    }

    fn check(&mut self, ctx: &egui::Context) {
        let (send, receive) = channel();
        self.receiver = Some(receive);
        self.checked = false;
        self.report_ready = false;
        let (out, settings, ctx) = (self.out.clone(), self.settings.clone(), ctx.clone());
        std::thread::spawn(move || {
            let picked = pollster::block_on(
                rfd::AsyncFileDialog::new()
                    .set_title("Choose your small rebuilt file")
                    .pick_file(),
            );
            if let Some(file) = picked {
                let small = file.path().to_path_buf();
                let result = narrow::same_failure(&small, &out, &settings, &|progress| {
                    let _ = send.send(Message::Progress(progress.to_string()));
                    ctx.request_repaint();
                });
                let _ = send.send(Message::Checked(small, result));
            } else {
                let _ = send.send(Message::Cancelled);
            }
            ctx.request_repaint();
        });
    }

    fn report(&mut self, ctx: &egui::Context) {
        let Some(small) = self.small.clone() else {
            return;
        };
        let (send, receive) = channel();
        self.receiver = Some(receive);
        let (out, ctx) = (self.out.clone(), ctx.clone());
        std::thread::spawn(move || {
            let result = narrow::make_report(&small, &out);
            let _ = send.send(Message::Finished(result));
            ctx.request_repaint();
        });
    }

    /// Returns true when the user closes an idle wizard.
    pub(crate) fn ui(&mut self, ctx: &egui::Context) -> bool {
        let messages: Vec<_> = self
            .receiver
            .as_ref()
            .map(|r| r.try_iter().collect())
            .unwrap_or_default();
        for message in messages {
            match message {
                Message::Progress(text) => self.progress = text,
                Message::Cancelled => self.receiver = None,
                Message::Finished(result) => {
                    self.receiver = None;
                    match result {
                        Ok(value) => {
                            self.summary = value
                                .get("summary")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            if let Some(recipe) =
                                value.pointer("/result/recipe").and_then(Value::as_str)
                            {
                                self.recipe = recipe.to_owned();
                            }
                            self.report_ready = value.get("schema").and_then(Value::as_str)
                                == Some(dutchman::json::MADE_REPORT);
                        }
                        Err(message) => self.summary = message,
                    }
                }
                Message::Checked(small, result) => {
                    self.receiver = None;
                    self.small = Some(small);
                    match result {
                        Ok(value) => {
                            self.checked = matches!(
                                value.get("verdict").and_then(Value::as_str),
                                Some("same" | "too_broad")
                            );
                            self.summary = value
                                .get("summary")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_owned();
                        }
                        Err(message) => self.summary = message,
                    }
                }
            }
        }
        let busy = self.receiver.is_some();
        let mut close = false;
        egui::Window::new("Diagnose this file").default_width(660.0).show(ctx, |ui| {
            ui.label("Works locally. Keep the original and local-key.json private.");
            ui.label("A stopped run resumes from its checkpoint when you choose Diagnose this file again.");
            if busy { ui.spinner(); ui.label(&self.progress); }
            ui.label(&self.summary);
            egui::ScrollArea::vertical().max_height(380.0).show(ui, |ui| {
                for line in self.recipe.lines() { ui.label(line); }
            });
            ui.add_enabled_ui(!busy, |ui| {
                if ui.button("Resume diagnosis").clicked() { self.start(ctx); }
                if ui.add_enabled(!self.recipe.is_empty(), egui::Button::new("Check my small file")).clicked() { self.check(ctx); }
                if ui.add_enabled(self.checked, egui::Button::new("Make the report")).clicked() { self.report(ctx); }
                if self.report_ready {
                    ui.label("The ZIP contains the small file's geometry. Review it before choosing where to send it. Nothing was sent.");
                }
                if ui.button("Open diagnosis folder").clicked() {
                    if let Err(message) = crate::platform::reveal(&self.out) { self.summary = message; }
                }
                if ui.button("Close").clicked() { close = true; }
            });
        });
        close
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn progress_recipe_and_report_controls_follow_the_worker_results() {
        let mut wizard = Wizard::new(PathBuf::from("synthetic.f3d"), Default::default());
        let (send, receiver) = channel();
        wizard.receiver = Some(receiver);
        send.send(Message::Progress(
            "step 3 of 5: narrowing part 17 (feature 41 of 80)".into(),
        ))
        .unwrap();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(900.0, 800.0))
            .build_state(
                |ctx, wizard: &mut Wizard| {
                    wizard.ui(ctx);
                },
                wizard,
            );
        harness.run_steps(2);
        harness.get_by_label("step 3 of 5: narrowing part 17 (feature 41 of 80)");
        send.send(Message::Finished(Ok(serde_json::json!({"summary": "Failure narrowed.", "result": {"recipe": "1. Make a new sketch.\n2. Use a 20 mm rectangle."}})))).unwrap();
        harness.run_steps(2);
        harness.get_by_label("Failure narrowed.");
        harness.get_by_label("1. Make a new sketch.");
        harness.get_by_label("Check my small file");
        assert!(!harness.state().checked);
        assert!(!harness.state().report_ready);
        // Closing an idle wizard is a click; it does not start any background work.
        harness.get_by_label("Close").click();
        harness.run_steps(2);
        assert!(harness.state().receiver.is_none());
    }
}
