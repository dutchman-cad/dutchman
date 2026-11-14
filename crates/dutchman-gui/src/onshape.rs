//! Connect Onshape: save, test and forget your API keys, and open a Part Studio from its
//! link. The keys go to the system credential store through the same code as
//! `dutchman onshape login`; the window never shows or logs them.
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui::{self, RichText};

use crate::theme;

/// Where Onshape API keys are created.
const KEYS_URL: &str = "https://dev-portal.onshape.com/keys";

enum Message {
    /// Whether keys are saved, as one line.
    Connection(String),
    Tested(Result<String, String>),
    Saved(Result<String, String>),
    Forgotten(Result<String, String>),
    Fetched(Result<PathBuf, String>),
}

/// The Connect Onshape window.
pub(crate) struct Panel {
    pub(crate) open: bool,
    access: String,
    secret: String,
    link: String,
    /// What is known about the connection, as one line.
    connection: String,
    /// The result of the last button pressed, and whether it went well.
    outcome: Option<(bool, String)>,
    busy: bool,
    sender: Sender<Message>,
    receiver: Receiver<Message>,
}

impl Panel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = channel();
        Self {
            open: false,
            access: String::new(),
            secret: String::new(),
            link: String::new(),
            connection: String::new(),
            outcome: None,
            busy: false,
            sender,
            receiver,
        }
    }

    /// Opens or closes the window; opening asks Onshape who the saved keys belong to.
    pub(crate) fn toggle(&mut self, ctx: &egui::Context) {
        self.open = !self.open;
        if self.open {
            self.refresh(ctx);
        }
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

    fn refresh(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, |sender| {
            let store = dutchman::onshape::Store::system();
            let line = match dutchman::onshape::status(
                &store,
                &mut dutchman::onshape::NetworkClient::default(),
            ) {
                Ok(dutchman::onshape::Connection::Connected { user, source }) => {
                    format!("Connected as {user} (keys from {}).", source.label())
                }
                Ok(dutchman::onshape::Connection::NotConnected) => "Not connected.".to_owned(),
                Ok(dutchman::onshape::Connection::Unchecked { source, error }) => format!(
                    "Keys are saved in {}, but Onshape could not confirm them: {}",
                    source.label(),
                    error.message
                ),
                Err(error) => error.message.to_owned(),
            };
            let _ = sender.send(Message::Connection(line));
        });
    }

    fn keys(&self) -> Result<dutchman::onshape::Credentials, String> {
        dutchman::onshape::Credentials::new(
            self.access.trim().to_owned(),
            self.secret.trim().to_owned(),
        )
        .map_err(|error| error.message.to_owned())
    }

    fn test(&mut self, ctx: &egui::Context) {
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
            .map(|user| format!("The keys work. Connected as {user}."))
            .map_err(|error| error.message.to_owned());
            let _ = sender.send(Message::Tested(result));
        });
    }

    fn save(&mut self, ctx: &egui::Context) {
        let keys = match self.keys() {
            Ok(keys) => keys,
            Err(why) => {
                self.outcome = Some((false, why));
                return;
            }
        };
        self.spawn(ctx, move |sender| {
            let result = dutchman::onshape::Store::system()
                .save(&keys)
                .map(|source| format!("Saved in {}.", source.label()))
                .map_err(|error| error.message.to_owned());
            let _ = sender.send(Message::Saved(result));
        });
    }

    fn disconnect(&mut self, ctx: &egui::Context) {
        self.spawn(ctx, |sender| {
            let result = dutchman::onshape::Store::system()
                .forget()
                .map(|()| "Saved Onshape keys removed.".to_owned())
                .map_err(|error| error.message.to_owned());
            let _ = sender.send(Message::Forgotten(result));
        });
    }

    fn open_from_link(&mut self, ctx: &egui::Context) {
        let link = self.link.trim().to_owned();
        let element = match dutchman::onshape::DocumentUrl::parse(&link, None) {
            Ok(parsed) => parsed.element,
            Err(error) => {
                self.outcome = Some((false, error.message.to_owned()));
                return;
            }
        };
        let folder = std::env::temp_dir().join("dutchman-onshape");
        self.spawn(ctx, move |sender| {
            let result = (|| {
                std::fs::create_dir_all(&folder).map_err(|_| {
                    "Could not make a working folder for the Onshape download.".to_owned()
                })?;
                let out = folder.join(format!("{element}.onshape-bundle"));
                let report = dutchman::onshape::run(&dutchman::onshape::FetchOptions {
                    document_url: link,
                    out: Some(out.clone()),
                    version: None,
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
            })();
            let _ = sender.send(Message::Fetched(result));
        });
    }

    /// Draws the window. Returns the bundle of a Part Studio that has just been fetched, ready
    /// to convert.
    pub(crate) fn window(&mut self, ctx: &egui::Context, top: f32) -> Option<PathBuf> {
        let mut fetched = None;
        while let Ok(message) = self.receiver.try_recv() {
            self.busy = false;
            match message {
                Message::Connection(line) => self.connection = line,
                Message::Tested(result) => self.outcome = Some(split(result)),
                Message::Saved(result) => {
                    if result.is_ok() {
                        self.access.clear();
                        self.secret.clear();
                        self.connection = "Connected.".into();
                    }
                    self.outcome = Some(split(result));
                }
                Message::Forgotten(result) => {
                    if result.is_ok() {
                        self.connection = "Not connected.".into();
                    }
                    self.outcome = Some(split(result));
                }
                Message::Fetched(result) => match result {
                    Ok(path) => {
                        self.outcome = Some((true, "Downloaded. Converting…".into()));
                        fetched = Some(path);
                    }
                    Err(why) => self.outcome = Some((false, why)),
                },
            }
        }
        if !self.open {
            return fetched;
        }
        let mut open = self.open;
        egui::Window::new("Connect Onshape")
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, top + 8.0))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .show(ctx, |ui| self.contents(ui, ctx));
        self.open = open;
        fetched
    }

    fn contents(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let p = *theme::palette(ui);
        if !self.connection.is_empty() {
            ui.label(&self.connection);
        }
        ui.add_space(theme::SPACE_2);
        ui.label(RichText::new("Your Onshape API keys").strong());
        ui.horizontal_wrapped(|ui| {
            ui.label(
                "Create them in Onshape under My account > Developer > API keys, with read access.",
            );
            ui.hyperlink_to("Open the API keys page", KEYS_URL);
        });
        egui::Grid::new("onshape_keys")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label("Access key");
                ui.add(egui::TextEdit::singleline(&mut self.access).desired_width(300.0));
                ui.end_row();
                ui.label("Secret key");
                ui.add(
                    egui::TextEdit::singleline(&mut self.secret)
                        .password(true)
                        .desired_width(300.0),
                );
                ui.end_row();
            });
        ui.label(
            RichText::new("Keys stay on this computer, in its credential store. They are never sent to Dutchman.")
                .small()
                .color(p.tide),
        );
        let ready = !self.busy && !self.access.trim().is_empty() && !self.secret.trim().is_empty();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(ready, egui::Button::new("Test connection"))
                .clicked()
            {
                self.test(ctx);
            }
            if ui.add_enabled(ready, egui::Button::new("Save")).clicked() {
                self.save(ctx);
            }
            if ui
                .add_enabled(!self.busy, egui::Button::new("Disconnect"))
                .clicked()
            {
                self.disconnect(ctx);
            }
        });
        ui.separator();
        ui.label(RichText::new("Open from Onshape").strong());
        ui.label("Paste a link to one of your own Part Studios.");
        ui.add(
            egui::TextEdit::singleline(&mut self.link)
                .hint_text("https://cad.onshape.com/documents/…/w/…/e/…")
                .desired_width(f32::INFINITY),
        );
        let can_open = !self.busy && !self.link.trim().is_empty();
        if ui
            .add_enabled(can_open, egui::Button::new("Open from Onshape"))
            .clicked()
        {
            self.open_from_link(ctx);
        }
        if self.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Working…");
            });
        }
        if let Some((good, text)) = &self.outcome {
            let color = if *good { p.tide } else { p.danger };
            ui.label(RichText::new(text).color(color));
        }
    }
}

fn split(result: Result<String, String>) -> (bool, String) {
    match result {
        Ok(text) => (true, text),
        Err(text) => (false, text),
    }
}
