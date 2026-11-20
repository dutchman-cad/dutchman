//! Onshape connection setup. Saved keys are never retained by the form.
use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui::{self, RichText};

use crate::theme;

const KEYS_URL: &str = "https://dev-portal.onshape.com/keys";

enum Message {
    Stored(Result<(bool, bool), String>),
    Profile(Result<String, String>),
    Tested(Result<String, String>),
    Saved(Result<String, String>),
    Forgotten(Result<(), String>),
}

pub(crate) struct Panel {
    pub(crate) open: bool,
    connected: bool,
    bearer: bool,
    pub(crate) bearer_token_file: Option<std::path::PathBuf>,
    user: Option<String>,
    changing_keys: bool,
    access: String,
    secret: String,
    pub(crate) link: String,
    continue_link: bool,
    focus_link: bool,
    pub(crate) saved: bool,
    outcome: Option<(bool, String)>,
    busy: bool,
    sender: Sender<Message>,
    receiver: Receiver<Message>,
}

impl Panel {
    pub(crate) fn new(ctx: &egui::Context) -> Self {
        let (sender, receiver) = channel();
        let mut panel = Self {
            open: false,
            connected: false,
            bearer: false,
            bearer_token_file: None,
            user: None,
            changing_keys: false,
            access: String::new(),
            secret: String::new(),
            link: String::new(),
            continue_link: false,
            focus_link: false,
            saved: false,
            outcome: None,
            busy: false,
            sender,
            receiver,
        };
        // Local lookup only: launching the app never contacts Onshape.
        panel.refresh_auth(ctx);
        panel
    }

    pub(crate) fn connected(&self) -> bool {
        self.connected
    }

    pub(crate) fn toggle(&mut self, ctx: &egui::Context, clipboard: &str) {
        self.open = !self.open;
        self.focus_link = self.open && self.connected;
        if self.focus_link {
            crate::paste::prefill_onshape(&mut self.link, clipboard);
        }
        if self.open && self.connected && !self.bearer && self.user.is_none() && !self.busy {
            self.spawn(ctx, |sender| {
                let result = dutchman::onshape::Store::system()
                    .load()
                    .and_then(|keys| {
                        dutchman::onshape::session_user(
                            &mut dutchman::onshape::NetworkClient::default(),
                            &keys,
                        )
                    })
                    .map_err(|why| why.message.to_owned());
                let _ = sender.send(Message::Profile(result));
            });
        }
    }

    pub(crate) fn offer_link(&mut self, clipboard: &str) {
        self.open = true;
        self.focus_link = true;
        crate::paste::prefill_onshape(&mut self.link, clipboard);
    }

    pub(crate) fn keep_link(&mut self, link: String) {
        self.link = link;
        self.continue_link = true;
        self.open = true;
    }

    fn spawn(&mut self, ctx: &egui::Context, work: impl FnOnce(&Sender<Message>) + Send + 'static) {
        self.busy = true;
        self.outcome = None;
        let sender = self.sender.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            work(&sender);
            ctx.request_repaint();
        });
    }

    fn refresh_auth(&mut self, ctx: &egui::Context) {
        let token_file = self.bearer_token_file.clone();
        self.connected = false;
        self.bearer = false;
        self.user = None;
        self.spawn(ctx, move |sender| {
            let result = dutchman::onshape::FetchAuth::find(
                &dutchman::onshape::Store::system(),
                token_file.as_deref(),
            )
            .map(|found| (found.is_some(), found.is_some_and(|auth| auth.is_bearer())))
            .map_err(|why| why.message.to_owned());
            let _ = sender.send(Message::Stored(result));
        });
    }

    fn keys(&self) -> Result<dutchman::onshape::Credentials, String> {
        dutchman::onshape::Credentials::new(
            self.access.trim().to_owned(),
            self.secret.trim().to_owned(),
        )
        .map_err(|why| why.message.to_owned())
    }

    fn test_or_save(&mut self, ctx: &egui::Context, save: bool) {
        let keys = match self.keys() {
            Ok(keys) => keys,
            Err(why) => {
                self.outcome = Some((false, why));
                return;
            }
        };
        self.spawn(ctx, move |sender| {
            let result = dutchman::onshape::session_user(
                &mut dutchman::onshape::NetworkClient::default(),
                &keys,
            )
            .and_then(|user| {
                if save {
                    dutchman::onshape::Store::system().save(&keys)?;
                }
                Ok(user)
            })
            .map_err(|why| why.message.to_owned());
            let _ = sender.send(if save {
                Message::Saved(result)
            } else {
                Message::Tested(result)
            });
        });
    }

    pub(crate) fn poll(&mut self) -> Option<String> {
        let mut link = None;
        while let Ok(message) = self.receiver.try_recv() {
            self.busy = false;
            match message {
                Message::Stored(Ok((connected, bearer))) => {
                    self.connected = connected;
                    self.bearer = bearer;
                    self.user = None;
                    if bearer {
                        self.changing_keys = false;
                        self.access.clear();
                        self.secret.clear();
                    }
                    if connected && self.continue_link {
                        link = Some(std::mem::take(&mut self.link));
                        self.continue_link = false;
                        self.open = false;
                    }
                }
                Message::Profile(Ok(user)) => self.user = Some(user),
                Message::Tested(Ok(user)) => {
                    self.outcome = Some((true, format!("The keys work. Connected as {user}.")));
                }
                Message::Saved(Ok(user)) => {
                    self.connected = true;
                    self.user = Some(user);
                    self.access.clear();
                    self.secret.clear();
                    self.changing_keys = false;
                    self.outcome = None;
                    self.open = false;
                    self.saved = true;
                    if self.continue_link {
                        link = Some(std::mem::take(&mut self.link));
                        self.continue_link = false;
                    }
                }
                Message::Forgotten(Ok(())) => {
                    self.connected = false;
                    self.changing_keys = false;
                    self.user = None;
                    self.access.clear();
                    self.secret.clear();
                    self.outcome = None;
                }
                Message::Stored(Err(why))
                | Message::Profile(Err(why))
                | Message::Tested(Err(why))
                | Message::Saved(Err(why))
                | Message::Forgotten(Err(why)) => self.outcome = Some((false, why)),
            }
        }
        link
    }

    pub(crate) fn window(
        &mut self,
        ctx: &egui::Context,
        top: f32,
        can_open: bool,
    ) -> Option<String> {
        if !self.open {
            return None;
        }
        let mut open = true;
        let mut link = None;
        egui::Window::new("Connect Onshape")
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, top + 8.0))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .show(ctx, |ui| {
                if ui
                    .add_enabled(!self.busy, egui::Button::new("Use bearer token file"))
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Choose private Onshape bearer token file")
                        .pick_file()
                    {
                        self.bearer_token_file = Some(path);
                        self.refresh_auth(ctx);
                    }
                }
                if self.bearer_token_file.is_some()
                    && ui
                        .add_enabled(!self.busy, egui::Link::new("Clear token file"))
                        .clicked()
                {
                    self.bearer_token_file = None;
                    self.refresh_auth(ctx);
                }
                if self.bearer {
                    ui.label("Using an OAuth bearer token for fetching.");
                }
                if self.connected && !self.bearer {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(self.user.as_ref().map_or_else(
                            || "Connected to Onshape".to_owned(),
                            |name| format!("Connected to Onshape as {name}"),
                        ));
                        ui.label("·");
                        if ui.link("Change keys").clicked() {
                            self.changing_keys = true;
                        }
                        ui.label("·");
                        if ui
                            .add_enabled(!self.busy, egui::Button::new("Disconnect"))
                            .clicked()
                        {
                            self.spawn(ctx, |sender| {
                                let result = dutchman::onshape::Store::system()
                                    .forget()
                                    .map_err(|why| why.message.to_owned());
                                let _ = sender.send(Message::Forgotten(result));
                            });
                        }
                    });
                }
                if (!self.connected || self.changing_keys) && !self.bearer {
                    self.keys_ui(ui, ctx);
                }
                if self.connected && !self.changing_keys {
                    ui.separator();
                    let focus = std::mem::take(&mut self.focus_link);
                    if link_ui(
                        ui,
                        &mut self.link,
                        "Paste your Onshape Part Studio link",
                        focus,
                        can_open && !self.busy,
                    ) {
                        link = Some(std::mem::take(&mut self.link));
                    }
                }
                if self.busy {
                    ui.spinner();
                }
                if let Some((good, text)) = &self.outcome {
                    let p = theme::palette(ui);
                    ui.label(RichText::new(text).color(if *good { p.tide } else { p.danger }));
                }
            });
        self.open = open && link.is_none() && self.open;
        link
    }

    fn keys_ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.label(RichText::new("Your Onshape API keys").strong());
        ui.horizontal_wrapped(|ui| {
            ui.label("Create them in Onshape under My account > Developer > API keys, with two permissions: read your profile information, and read your documents.");
            ui.hyperlink_to("Open the API keys page", KEYS_URL);
        });
        egui::Grid::new("onshape_keys")
            .num_columns(2)
            .show(ui, |ui| {
                let access = ui.label("Access key");
                ui.add(
                    egui::TextEdit::singleline(&mut self.access)
                        .password(true)
                        .desired_width(300.0),
                )
                .labelled_by(access.id);
                ui.end_row();
                let secret = ui.label("Secret key");
                ui.add(
                    egui::TextEdit::singleline(&mut self.secret)
                        .password(true)
                        .desired_width(300.0),
                )
                .labelled_by(secret.id);
                ui.end_row();
            });
        ui.small(
            "Keys stay on this computer, in its credential store. They are never sent to Dutchman.",
        );
        let ready = !self.busy && !self.access.trim().is_empty() && !self.secret.trim().is_empty();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ready, egui::Button::new("Test connection"))
                .clicked()
            {
                self.test_or_save(ctx, false);
            }
            if ui.add_enabled(ready, egui::Button::new("Save")).clicked() {
                self.test_or_save(ctx, true);
            }
        });
    }

    pub(crate) fn preview_saved(&self, user: &str) {
        let _ = self.sender.send(Message::Saved(Ok(user.to_owned())));
    }

    pub(crate) fn preview(&mut self, user: Option<&str>) {
        let (sender, receiver) = channel();
        self.sender = sender;
        self.receiver = receiver;
        self.busy = false;
        self.connected = user.is_some();
        self.user = user.map(str::to_owned);
    }
}

pub(crate) fn looks_like_link(text: &str) -> bool {
    dutchman::onshape::DocumentUrl::parse(text.trim(), None).is_ok()
}

pub(crate) fn link_ui(
    ui: &mut egui::Ui,
    link: &mut String,
    label: &str,
    focus: bool,
    enabled: bool,
) -> bool {
    let label = ui.label(label);
    let field = ui
        .add(
            egui::TextEdit::singleline(link)
                .hint_text("cad.onshape.com/documents/…")
                .desired_width(f32::INFINITY),
        )
        .labelled_by(label.id);
    if focus {
        field.request_focus();
    }
    ui.add_enabled(enabled && looks_like_link(link), egui::Button::new("Open"))
        .clicked()
        || (enabled
            && looks_like_link(link)
            && field.lost_focus()
            && ui.input(|i| i.key_pressed(egui::Key::Enter)))
}

pub(crate) fn fetch(link: String, folder: &std::path::Path) -> Result<std::path::PathBuf, String> {
    fetch_with_token_file(link, folder, None)
}

pub(crate) fn fetch_with_token_file(
    link: String,
    folder: &std::path::Path,
    bearer_token_file: Option<std::path::PathBuf>,
) -> Result<std::path::PathBuf, String> {
    let element = dutchman::onshape::DocumentUrl::parse(&link, None)
        .map_err(|why| why.message.to_owned())?
        .element;
    let out = folder.join(format!("{element}.onshape-bundle"));
    let report = dutchman::onshape::run(&dutchman::onshape::FetchOptions {
        document_url: link,
        out: Some(out.clone()),
        version: None,
        bearer_token_file,
        max_calls: 50,
        json: false,
    });
    if report.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
        Ok(out)
    } else {
        Err(report
            .pointer("/error/message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Onshape could not be fetched; check the link and retry.")
            .to_owned())
    }
}

/// The source archive stays intact; report transport wraps it in its existing gzip envelope.
pub(crate) struct ReportBundle(pub(crate) Vec<u8>);

pub(crate) fn is_bundle(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("onshape-bundle"))
}

impl ReportBundle {
    pub(crate) fn read(path: &std::path::Path) -> Result<Option<Self>, String> {
        use std::io::Read;
        // Match the report transport's maximum uncompressed attachment size.
        const CAP: u64 = 128 * 1024 * 1024;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(CAP + 1).read_to_end(&mut bytes))
            .map_err(|_| "Could not read the Onshape bundle; check that it is still in the output folder and retry.".to_owned())?;
        // The review still offers the original file through the private-send card.
        Ok((bytes.len() as u64 <= CAP).then_some(Self(bytes)))
    }
}

impl dutchman::report_send::BundleSource for ReportBundle {
    fn parts(&self) -> Result<Vec<dutchman::report_send::BundlePart>, String> {
        Ok(vec![dutchman::report_send::BundlePart {
            role: "Onshape bundle".into(),
            bytes: self.0.clone(),
        }])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_setup_continues_the_link_and_keeps_only_the_file_selection() {
        let mut panel = Panel::new(&egui::Context::default());
        panel.preview(None);
        panel.bearer_token_file = Some(std::path::PathBuf::from("private-token-file"));
        panel.keep_link("kept-link".into());
        panel
            .sender
            .send(Message::Stored(Ok((true, true))))
            .unwrap();
        assert_eq!(panel.poll().as_deref(), Some("kept-link"));
        assert!(panel.connected());
        assert!(panel.bearer);
        assert!(panel.user.is_none());
        assert!(panel.access.is_empty());
        assert!(panel.secret.is_empty());
        assert!(!panel.open);
        assert_eq!(
            panel.bearer_token_file.as_deref(),
            Some(std::path::Path::new("private-token-file"))
        );
    }

    #[test]
    fn bearer_lookup_restores_link_controls_after_abandoned_key_editing() {
        let mut panel = Panel::new(&egui::Context::default());
        panel.preview(Some("Synthetic designer"));
        panel.open = true;
        panel.changing_keys = true;
        panel.access = "synthetic-abandoned-access".into();
        panel.secret = "synthetic-abandoned-secret".into();
        panel
            .sender
            .send(Message::Stored(Ok((true, true))))
            .unwrap();
        assert!(panel.poll().is_none());
        assert!(panel.open);
        assert!(panel.connected());
        assert!(panel.bearer);
        assert!(!panel.changing_keys);
        assert!(panel.access.is_empty());
        assert!(panel.secret.is_empty());
        assert!(panel.user.is_none());
    }

    #[test]
    fn disconnect_abandons_key_editing_before_selecting_a_bearer_file() {
        let mut panel = Panel::new(&egui::Context::default());
        panel.preview(Some("Synthetic designer"));
        panel.changing_keys = true;
        panel.access = "synthetic-abandoned-access".into();
        panel.secret = "synthetic-abandoned-secret".into();
        panel.sender.send(Message::Forgotten(Ok(()))).unwrap();
        panel.poll();
        assert!(!panel.changing_keys);
        assert!(panel.access.is_empty());
        assert!(panel.secret.is_empty());
        panel.bearer_token_file = Some(std::path::PathBuf::from("synthetic-token-file"));
        panel
            .sender
            .send(Message::Stored(Ok((true, true))))
            .unwrap();
        panel.poll();
        assert!(panel.connected());
        assert!(panel.bearer);
        assert!(!panel.changing_keys);
    }

    #[test]
    fn desktop_fetch_uses_the_selected_token_file() {
        let folder = tempfile::tempdir().unwrap();
        let missing = folder.path().join("missing-token");
        let link = "https://cad.onshape.com/documents/111111111111111111111111/v/222222222222222222222222/e/333333333333333333333333";
        let error = fetch_with_token_file(link.into(), folder.path(), Some(missing)).unwrap_err();
        assert!(error.contains("bearer token file"));
        assert!(!error.contains("missing-token"));
    }

    #[test]
    fn saving_uses_the_profile_name_erases_keys_closes_setup_and_continues_the_link() {
        let mut panel = Panel::new(&egui::Context::default());
        panel.preview(None);
        panel.access = "synthetic-access".into();
        panel.secret = "synthetic-secret".into();
        panel.keep_link("kept-link".into());
        panel.preview_saved("Test designer");
        assert_eq!(panel.poll().as_deref(), Some("kept-link"));
        assert!(panel.connected());
        assert_eq!(panel.user.as_deref(), Some("Test designer"));
        assert!(panel.access.is_empty());
        assert!(panel.secret.is_empty());
        assert!(!panel.open);
        assert!(panel.saved);
        panel.sender.send(Message::Forgotten(Ok(()))).unwrap();
        panel.poll();
        assert!(!panel.connected());
        assert!(panel.user.is_none());
    }
}
