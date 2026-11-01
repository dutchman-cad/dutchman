//! Click-driven report review. Network work is always off the UI thread.
use dutchman::report_send::{self, Prepared};
use eframe::egui;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

#[derive(Default)]
pub struct Consent {
    pub ids: Vec<String>,
    pub card: Option<Card>,
    #[cfg(feature = "test-support")]
    pub test_transport: Option<(String, Option<String>)>,
}

pub enum Action {
    Private(usize, Option<PathBuf>),
    Github(usize),
}

pub struct Card {
    pub row: usize,
    pub prepared: Prepared,
    pub folder: PathBuf,
    summary: bool,
    public_test: bool,
    status: String,
    failed: bool,
    sent: bool,
    rx: Option<Receiver<Result<Value, String>>>,
}

impl Card {
    pub fn new(row: usize, prepared: Prepared, folder: PathBuf) -> Self {
        Self {
            row,
            folder,
            summary: prepared.bundle_sha256.is_none(),
            prepared,
            public_test: false,
            status: String::new(),
            failed: false,
            sent: false,
            rx: None,
        }
    }
}

impl Consent {
    /// Returns the row whose existing private-send card should be used.
    pub fn window(&mut self, ctx: &egui::Context) -> Option<Action> {
        let card = self.card.as_mut()?;
        if let Some(rx) = &card.rx {
            match rx.try_recv() {
                Ok(result) => {
                    card.sent = result.as_ref().is_ok_and(|value| {
                        value.get("sent").and_then(Value::as_bool) == Some(true)
                    });
                    card.failed = !card.sent;
                    card.status = match result {
                        Ok(value) if value.get("sent").and_then(Value::as_bool) == Some(true) => {
                            "Sent. The report was received.".into()
                        }
                        Ok(value) => value
                            .get("summary")
                            .and_then(Value::as_str)
                            .unwrap_or("It did not go through. Your report is kept locally.")
                            .into(),
                        Err(error) => error,
                    };
                    card.rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    card.failed = true;
                    card.status = "It did not go through. Your report is kept locally.".into();
                    card.rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut open = true;
        let mut private = None;
        egui::Window::new("Send a problem report").open(&mut open).show(ctx, |ui| {
            ui.label(format!("Report {}", card.prepared.report_id));
            if card.prepared.bundle_sha256.is_some() {
                ui.label("Bundles contain geometry and are used privately to reproduce the problem.");
                ui.radio_value(&mut card.summary, false, "Send the bundle");
            }
            ui.radio_value(&mut card.summary, true, "Anonymous summary only");
            if card.prepared.bundle_sha256.is_none() {
                ui.label("No bundle is available yet. The summary contains no design geometry.");
            } else {
                ui.label(format!("Bundle: {} bytes (limit 25 MiB)", card.prepared.bundle_size));
                for part in &card.prepared.parts { ui.label(format!("{}: {} bytes", part.role, part.bytes)); }
            }
            ui.checkbox(&mut card.public_test, "You may use this report in a public test case");
            if ui.button("Send the whole file privately").clicked() { private = Some(Action::Private(card.row, None)); }
            ui.label(format!("Anonymous summary: {} bytes — app version, operating system, file format, source app, failure details, your sharing choices and a code for updates", card.prepared.metadata("summary", card.public_test).to_string().len()));
            ui.label(format!("Kept locally: {}", card.folder.display()));
            let can_send = card.rx.is_none() && !card.sent && (card.summary || card.prepared.bundle_sha256.is_some());
            if ui.add_enabled(can_send, egui::Button::new(if card.failed { "Retry" } else { "Send" })).clicked() {
                let prepared = card.prepared.clone();
                let folder = card.folder.clone();
                let choice = if card.summary { "summary" } else { "bundle" };
                let public_test = card.public_test;
                let (tx, rx) = channel();
                card.rx = Some(rx);
                card.failed = false;
                card.status = "Sending…".into();
                if !self.ids.contains(&prepared.report_id) { self.ids.push(prepared.report_id.clone()); }
                let endpoint = report_send::endpoint();
                #[cfg(feature = "test-support")]
                let endpoint = self.test_transport.as_ref().map(|(url, _)| url.clone()).or(endpoint);
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    let result = report_send::submit(&prepared, &folder, choice, public_test, true, endpoint.as_deref());
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
            }
            ui.label(&card.status);
            if card.prepared.bundle_sha256.is_some() && ui.button("Send the saved bundle privately").clicked() {
                private = Some(Action::Private(card.row, Some(card.folder.join("bundle.json.gz"))));
            }
            if ui.button("Get updates on Discord").clicked() {
                ctx.copy_text(card.prepared.claim());
                ctx.open_url(egui::OpenUrl::new_tab(dutchman::report_policy::ReportPolicy::bundled().dm_url()));
                card.status = "The message for updates is copied. Paste it in Dutchy's DM to get updates.".into();
            }
            if ui.button("Open GitHub with anonymous summary only").clicked() { private = Some(Action::Github(card.row)); }
            ui.label("GitHub is public. Share only the anonymous summary there, never the bundle.");
        });
        if private.is_some() {
            open = false;
        }
        if !open {
            self.card = None;
        }
        private
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::{kittest::Queryable, Harness};

    #[test]
    fn consent_with_bundle_selects_it_without_public_permission() {
        let dir = tempfile::tempdir().unwrap();
        let mut prepared =
            Prepared::new("0123456789abcdef", "f3d", "decode", None, dir.path()).unwrap();
        prepared.bundle_sha256 = Some("synthetic-bundle".into());
        let card = Card::new(0, prepared, dir.path().to_path_buf());
        assert!(!card.summary);
        assert!(!card.public_test);
    }

    #[test]
    fn consent_without_bundle_selects_summary_and_an_unticked_permission() {
        let dir = tempfile::tempdir().unwrap();
        let prepared =
            Prepared::new("0123456789abcdef", "f3d", "decode", None, dir.path()).unwrap();
        let state = Consent {
            ids: Vec::new(),
            card: Some(Card::new(0, prepared, dir.path().to_path_buf())),
            #[cfg(feature = "test-support")]
            test_transport: None,
        };
        let mut harness = Harness::new_state(
            |ctx, state: &mut Consent| {
                state.window(ctx);
            },
            state,
        );
        harness.run_steps(3);
        assert!(harness.state().card.as_ref().unwrap().summary);
        assert!(harness.query_by_label("Send the bundle").is_none());
        harness.get_by_label("Send");
        assert!(harness.query_by_label("Retry").is_none());
        assert!(!harness.state().card.as_ref().unwrap().public_test);
        harness
            .get_by_label("Anonymous summary only")
            .click_accesskit();
        harness.step();
        assert!(harness.state().card.as_ref().unwrap().summary);
        harness
            .get_by_label("You may use this report in a public test case")
            .click_accesskit();
        harness.step();
        assert!(harness.state().card.as_ref().unwrap().public_test);
        assert!(harness.state().ids.is_empty());
        assert!(!dir.path().join("submission.json").exists());
    }
}
