//! The "Export from Fusion" window, first half: is the Fusion exporter running in Fusion, and if
//! it is not there, one button to install it and the steps to switch it on once.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, RichText};

use super::agent::{self, Agent, Presence};
use super::install::{self, Failure, Installed, Options, Step};
use super::{PROJECT_NAME, PROJECT_URL};
use crate::theme;

/// How often the agent folder is read while the window is open.
const POLL: Duration = Duration::from_secs(3);

/// What the install button has done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Install {
    Idle,
    Running(Step),
    Done(String),
    Failed(String),
}

enum Message {
    Poll(Presence, Option<String>),
    Step(Step),
    Finished(Result<Installed, Failure>),
    Latest(String),
}

/// The window's state.
pub struct Panel {
    /// Whether the window is showing.
    pub open: bool,
    offered: bool,
    presence: Presence,
    installed: Option<String>,
    install: Install,
    latest: Option<String>,
    options: Option<Options>,
    frozen: bool,
    sender: Sender<Message>,
    receiver: Receiver<Message>,
    stop: Option<Arc<AtomicBool>>,
}

impl Default for Panel {
    fn default() -> Self {
        Self::new()
    }
}

impl Panel {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self {
            open: false,
            offered: install::supported(),
            presence: Presence::Absent,
            installed: None,
            install: Install::Idle,
            latest: None,
            options: None,
            frozen: false,
            sender,
            receiver,
            stop: None,
        }
    }

    /// Whether the app offers this entry at all: Fusion runs on Windows and macOS only.
    pub fn offered(&self) -> bool {
        self.offered
    }

    /// Shows or hides the window; polling runs only while it is showing.
    pub fn toggle(&mut self, ctx: &egui::Context) {
        self.open = !self.open;
        if self.open {
            self.start(ctx);
        } else {
            self.halt();
        }
    }

    fn halt(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    fn start(&mut self, ctx: &egui::Context) {
        self.halt();
        if self.frozen {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        self.stop = Some(stop.clone());
        let (sender, ctx2) = (self.sender.clone(), ctx.clone());
        let addins = self.options.as_ref().map(|o| o.addins.clone());
        std::thread::spawn(move || {
            while !stop.load(Ordering::Relaxed) {
                let addins = addins.clone().or_else(install::addins_dir);
                let installed = addins.and_then(|dir| install::installed_version(&dir));
                if sender
                    .send(Message::Poll(agent::presence(), installed))
                    .is_err()
                {
                    return;
                }
                ctx2.request_repaint();
                let mut waited = Duration::ZERO;
                while waited < POLL && !stop.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(100));
                    waited += Duration::from_millis(100);
                }
            }
        });
        if crate::update::network_allowed() {
            let (sender, ctx) = (self.sender.clone(), ctx.clone());
            std::thread::spawn(move || {
                if let Ok((version, _)) = install::latest_release() {
                    let _ = sender.send(Message::Latest(version));
                    ctx.request_repaint();
                }
            });
        }
    }

    fn drain(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                Message::Poll(presence, installed) => {
                    self.presence = presence;
                    self.installed = installed;
                }
                Message::Step(step) => self.install = Install::Running(step),
                Message::Finished(Ok(done)) => {
                    self.installed = Some(done.version.clone());
                    self.install = Install::Done(done.version);
                }
                Message::Finished(Err(why)) => self.install = Install::Failed(why.to_string()),
                Message::Latest(version) => self.latest = Some(version),
            }
        }
    }

    fn start_install(&mut self, ctx: &egui::Context) {
        let options = match self.options.clone().map_or_else(Options::pinned, Ok) {
            Ok(options) => options,
            Err(why) => {
                self.install = Install::Failed(why.to_string());
                return;
            }
        };
        self.install = Install::Running(Step::Downloading);
        let (sender, ctx) = (self.sender.clone(), ctx.clone());
        std::thread::spawn(move || {
            // Re-read the agent file each time: it may have started exporting meanwhile.
            let busy = || agent::find().is_some_and(|a| a.busy.is_some() || a.fusion_busy);
            let tell = sender.clone();
            let ctx2 = ctx.clone();
            let result = install::install(&options, &busy, &mut |step| {
                let _ = tell.send(Message::Step(step));
                ctx2.request_repaint();
            });
            let _ = sender.send(Message::Finished(result));
            ctx.request_repaint();
        });
    }

    /// The window, anchored under the header like the other small windows.
    pub fn window(&mut self, ctx: &egui::Context, top: f32) {
        if !self.open {
            return;
        }
        self.drain();
        let mut open = true;
        egui::Window::new("Export from Fusion")
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-24.0, top + 8.0))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(480.0)
            .show(ctx, |ui| self.ui(ui));
        if !open {
            self.open = false;
            self.halt();
        }
    }

    /// The window's content.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let palette = theme::palette(ui);
        let found = match &self.presence {
            Presence::Running(agent) => Some((agent.clone(), false)),
            Presence::Unresponsive(agent) => Some((agent.clone(), true)),
            Presence::Absent | Presence::Gone => None,
        };
        match found {
            Some((agent, false)) => self.running(ui, &agent),
            Some((agent, true)) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Fusion is busy").strong());
                });
                ui.label(format!(
                    "The Fusion exporter {} is in Fusion, which is busy. Fusion can take a few \
                     minutes while it opens a large design. Waiting for it to answer.",
                    agent.version
                ));
            }
            None => self.not_found(ui),
        }
        if let Install::Failed(why) = &self.install {
            if self.presence_none() {
                ui.add_space(theme::SPACE_2);
                ui.label(RichText::new(why).color(palette.danger));
            }
        }
    }

    fn presence_none(&self) -> bool {
        matches!(self.presence, Presence::Absent | Presence::Gone)
    }

    fn running(&mut self, ui: &mut egui::Ui, agent: &Agent) {
        let palette = theme::palette(ui);
        ui.label(
            RichText::new("The Fusion exporter is running")
                .strong()
                .color(palette.spectre),
        );
        let mut line = format!("Version {}", agent.version);
        if let Some(fusion) = &agent.fusion_version {
            line.push_str(&format!(", in Fusion {fusion}"));
        }
        ui.label(line + ".");
        if agent.busy.is_some() {
            ui.label("It is exporting right now.");
        }
        self.newer_note(ui, &agent.version);
    }

    fn newer_note(&self, ui: &mut egui::Ui, have: &str) {
        if let Some(latest) = self
            .latest
            .as_deref()
            .filter(|l| install::is_newer(l, have))
        {
            ui.add_space(theme::SPACE_2);
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("A newer Fusion exporter is available ({latest})."));
                ui.hyperlink_to(
                    format!("{PROJECT_NAME}: releases"),
                    format!("{PROJECT_URL}/releases/latest"),
                );
            });
        }
    }

    fn not_found(&mut self, ui: &mut egui::Ui) {
        let palette = theme::palette(ui);
        match self.install.clone() {
            Install::Running(step) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(match step {
                        Step::Downloading => "Downloading the Fusion exporter…",
                        Step::Checking => "Checking the download…",
                        Step::Installing => "Installing the Fusion exporter…",
                    });
                });
                return;
            }
            Install::Done(version) => {
                ui.label(
                    RichText::new(format!("The Fusion exporter {version} is installed"))
                        .strong()
                        .color(palette.spectre),
                );
                switch_on_steps(ui);
                return;
            }
            Install::Idle | Install::Failed(_) => {}
        }
        if !self.offered {
            ui.label(
                "Export from Fusion needs Fusion, which runs on Windows and macOS. \
                 Use it from one of those computers.",
            );
            return;
        }
        let behind = self
            .installed
            .as_deref()
            .is_some_and(|have| install::is_newer(install::PIN.version, have));
        if let Some(have) = &self.installed {
            ui.label(
                RichText::new("The Fusion exporter is installed, but not switched on").strong(),
            );
            ui.label(format!(
                "Version {have} is in Fusion's Add-Ins folder; Fusion has not started it."
            ));
            switch_on_steps(ui);
            self.newer_note(ui, have);
            if !behind {
                return;
            }
            ui.add_space(theme::SPACE_2);
            ui.label(format!(
                "Version {} is newer, and speaks the protocol Export from Fusion uses.",
                install::PIN.version
            ));
        } else {
            ui.label(RichText::new("The Fusion exporter was not found").strong());
            ui.label(
                "Export from Fusion asks an add-in, the Fusion exporter, to export your designs \
                 from inside your Fusion. It is not running in your Fusion.",
            );
        }
        ui.add_space(theme::SPACE_2);
        if ui.button("Install the Fusion exporter").clicked() {
            self.start_install(ui.ctx());
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "Downloads version {} from {PROJECT_NAME}, checks it, and puts it in Fusion's Add-Ins folder.",
                install::PIN.version
            ));
            ui.hyperlink_to("Project page", PROJECT_URL);
        });
    }

    /// Shows a state without looking at this computer: for tests and screenshots. Polling stops.
    #[doc(hidden)]
    pub fn preview(
        &mut self,
        offered: bool,
        presence: Presence,
        installed: Option<&str>,
        step: Install,
    ) {
        self.halt();
        self.frozen = true;
        self.open = true;
        self.offered = offered;
        self.presence = presence;
        self.installed = installed.map(str::to_owned);
        self.install = step;
    }

    /// Installs into another folder, from another address.
    #[doc(hidden)]
    pub fn set_install_options(&mut self, options: Options) {
        self.options = Some(options);
    }

    /// What the install button has done so far.
    #[doc(hidden)]
    pub fn install_state(&mut self) -> Install {
        self.drain();
        self.install.clone()
    }

    #[doc(hidden)]
    pub fn set_latest(&mut self, version: &str) {
        self.latest = Some(version.into());
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        self.halt();
    }
}

/// The one thing the person has to do themselves, in plain words.
fn switch_on_steps(ui: &mut egui::Ui) {
    ui.add_space(theme::SPACE_2);
    ui.label(RichText::new("Switch it on once in Fusion").strong());
    for step in [
        "Open Fusion and press Shift+S (Utilities, Add-Ins, Scripts and Add-Ins).",
        "Open the Add-Ins tab and find FusionBulkExport.",
        "Tick Run on Startup.",
        "Switch Run on.",
    ]
    .iter()
    .enumerate()
    {
        ui.label(format!("{}. {}", step.0 + 1, step.1));
    }
    ui.add_space(theme::SPACE_2);
    ui.label("Dutchman cannot do this for you. This window keeps waiting and updates when the exporter starts.");
    ui.horizontal(|ui| {
        ui.spinner();
        ui.label("Waiting for the Fusion exporter…");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    fn harness(panel: Panel) -> Harness<'static, Panel> {
        Harness::builder()
            .with_size(egui::vec2(600.0, 500.0))
            .build_state(|ctx, panel: &mut Panel| panel.window(ctx, 0.0), panel)
    }

    fn found() -> Agent {
        Agent {
            jobs_dir: "/jobs".into(),
            features: vec![],
            version: "0.4.0".into(),
            busy: None,
            job: None,
            fusion_version: Some("2.0.21508".into()),
            pid: 1,
            fusion_busy: false,
        }
    }

    #[test]
    fn not_found_offers_one_install_button_where_fusion_runs() {
        let mut panel = Panel::new();
        panel.preview(true, Presence::Absent, None, Install::Idle);
        let mut h = harness(panel);
        h.run();
        h.get_by_label("Install the Fusion exporter");
        h.get_by_label("The Fusion exporter was not found");
        let mut panel = Panel::new();
        panel.preview(false, Presence::Absent, None, Install::Idle);
        let mut h = harness(panel);
        h.run();
        assert!(h.query_by_label("Install the Fusion exporter").is_none());
    }

    #[test]
    fn a_found_exporter_offers_no_install() {
        for presence in [
            Presence::Running(found()),
            Presence::Unresponsive(Agent {
                fusion_busy: true,
                ..found()
            }),
        ] {
            let busy = matches!(presence, Presence::Unresponsive(_));
            let mut panel = Panel::new();
            panel.preview(true, presence, None, Install::Idle);
            let mut h = harness(panel);
            h.run_steps(4);
            assert!(h.query_by_label("Install the Fusion exporter").is_none());
            if busy {
                h.get_by_label("Fusion is busy");
            } else {
                h.get_by_label("The Fusion exporter is running");
            }
        }
    }

    #[test]
    fn a_newer_release_is_only_mentioned() {
        let mut panel = Panel::new();
        panel.preview(true, Presence::Running(found()), None, Install::Idle);
        panel.set_latest("0.4.1");
        let mut h = harness(panel);
        h.run();
        h.get_by_label_contains("A newer Fusion exporter is available (0.4.1)");
        assert!(h.query_by_label("Install the Fusion exporter").is_none());
    }

    #[test]
    fn the_install_button_installs_and_shows_the_switch_on_steps() {
        let zip = {
            let mut out = std::io::Cursor::new(Vec::new());
            let mut z = zip::ZipWriter::new(&mut out);
            let o = zip::write::SimpleFileOptions::default();
            for name in ["install-stamp.json", "FusionBulkExport.manifest"] {
                z.start_file(format!("FusionBulkExport-0.4.0/FusionBulkExport/{name}"), o)
                    .unwrap();
                std::io::Write::write_all(&mut z, br#"{"version":"0.4.0"}"#).unwrap();
            }
            z.finish().unwrap();
            out.into_inner()
        };
        let sum = {
            use sha2::{Digest, Sha256};
            Sha256::digest(&zip)
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 2048];
                let _ = std::io::Read::read(&mut stream, &mut buf);
                let _ = std::io::Write::write_all(
                    &mut stream,
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        zip.len()
                    )
                    .as_bytes(),
                );
                let _ = std::io::Write::write_all(&mut stream, &zip);
            }
        });
        let addins = tempfile::tempdir().unwrap();
        let mut panel = Panel::new();
        panel.preview(true, Presence::Absent, None, Install::Idle);
        panel.set_install_options(Options {
            version: "0.4.0".into(),
            zip_url: format!("http://127.0.0.1:{port}/z.zip"),
            sha256: sum,
            addins: addins.path().to_path_buf(),
            connect_timeout: Duration::from_secs(2),
            stall_timeout: Duration::from_secs(2),
            deadline: Duration::from_secs(20),
            allow_http: true,
        });
        let mut h = harness(panel);
        h.run();
        h.get_by_label("Install the Fusion exporter").click();
        let started = std::time::Instant::now();
        while !matches!(h.state_mut().install_state(), Install::Done(_)) {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "install did not finish"
            );
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        h.run_steps(4);
        h.get_by_label("Switch it on once in Fusion");
        h.get_by_label_contains("Shift+S");
        assert!(addins
            .path()
            .join("FusionBulkExport/FusionBulkExport.manifest")
            .is_file());
    }
}
