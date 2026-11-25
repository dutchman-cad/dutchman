//! The Fusion exporter's find-and-install part of the "Export from Fusion" window: shown while no
//! exporter has announced itself. It says what is missing, installs the exporter with one
//! click, and tells the person the one step they do themselves in Fusion. The window's
//! [`Panel`](super::Panel) looks for the agent file; once the exporter announces itself it
//! carries on with the list of designs.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{self, RichText};

use super::agent::{self, Agent, Presence};
use super::install::{self, Failure, Installed, Options, Step};
use super::{PROJECT_NAME, PROJECT_URL};
use crate::theme;

/// How often the add-ins folder is read while the window is open.
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
    Installed(Option<String>),
    Step(Step),
    Finished(Result<Installed, Failure>),
    Latest(String),
}

/// The install state: what is in Fusion's Add-Ins folder, what the install button is doing,
/// and the newest release the exporter's project announces.
pub struct Installer {
    installed: Option<String>,
    install: Install,
    latest: Option<String>,
    options: Option<Options>,
    /// No background reads (tests and screenshots).
    frozen: bool,
    latest_asked: bool,
    sender: Sender<Message>,
    receiver: Receiver<Message>,
    stop: Option<Arc<AtomicBool>>,
}

impl Default for Installer {
    fn default() -> Self {
        Self::new()
    }
}

impl Installer {
    pub fn new() -> Self {
        let (sender, receiver) = channel();
        Self {
            installed: None,
            install: Install::Idle,
            latest: None,
            options: None,
            frozen: false,
            latest_asked: false,
            sender,
            receiver,
            stop: None,
        }
    }

    /// Reads the Add-Ins folder now and then while the window is open, and asks once for the
    /// newest release. Does nothing when already running.
    pub fn start(&mut self, ctx: &egui::Context) {
        if self.frozen || self.stop.is_some() {
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
                if sender.send(Message::Installed(installed)).is_err() {
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
        if !self.latest_asked && crate::update::network_allowed() {
            self.latest_asked = true;
            let (sender, ctx) = (self.sender.clone(), ctx.clone());
            std::thread::spawn(move || {
                if let Ok((version, _)) = install::latest_release() {
                    let _ = sender.send(Message::Latest(version));
                    ctx.request_repaint();
                }
            });
        }
    }

    /// Stops reading the Add-Ins folder (the window closed).
    pub fn halt(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
    }

    pub fn drain(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                Message::Installed(installed) => self.installed = installed,
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

    /// Downloads and installs the pinned release. `cad_dir` and `pid_alive` are how the agent
    /// file is read again just before replacing anything.
    fn start_install(
        &mut self,
        ctx: &egui::Context,
        cad_dir: Option<PathBuf>,
        pid_alive: fn(u32) -> bool,
    ) {
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
            let busy = || {
                cad_dir.as_deref().is_some_and(|cad| {
                    match agent::presence_in(cad, agent::now_secs(), &pid_alive) {
                        Presence::Running(a) => a.busy.is_some(),
                        Presence::Unresponsive(_) => true,
                        _ => false,
                    }
                })
            };
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

    /// The content while no exporter is there to take a job. `offered` is whether this
    /// computer can run Fusion at all; `presence` is what the agent folder says.
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        offered: bool,
        presence: &Presence,
        cad_dir: Option<PathBuf>,
        pid_alive: fn(u32) -> bool,
    ) {
        self.drain();
        let palette = theme::palette(ui);
        match presence {
            Presence::Running(agent) => {
                ui.label(
                    RichText::new("The Fusion exporter is running")
                        .strong()
                        .color(palette.spectre),
                );
                ui.label(running_line(agent));
            }
            Presence::Unresponsive(agent) => {
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
            Presence::Invalid(why) => {
                ui.label(RichText::new(why).color(palette.danger));
                self.not_found(ui, offered, cad_dir, pid_alive);
            }
            Presence::Absent | Presence::Gone => self.not_found(ui, offered, cad_dir, pid_alive),
        }
        if let Install::Failed(why) = &self.install {
            if presence.agent().is_none() {
                ui.add_space(theme::SPACE_2);
                ui.label(RichText::new(why).color(palette.danger));
            }
        }
    }

    /// A line under the design picker when the exporter project announces a newer release.
    pub fn newer_note(&self, ui: &mut egui::Ui, have: &str) {
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

    fn not_found(
        &mut self,
        ui: &mut egui::Ui,
        offered: bool,
        cad_dir: Option<PathBuf>,
        pid_alive: fn(u32) -> bool,
    ) {
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
        if !offered {
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
        if let Some(have) = self.installed.clone() {
            ui.label(
                RichText::new("The Fusion exporter is installed, but not switched on").strong(),
            );
            ui.label(format!(
                "Version {have} is in Fusion's Add-Ins folder; Fusion has not started it."
            ));
            switch_on_steps(ui);
            self.newer_note(ui, &have);
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
            self.start_install(ui.ctx(), cad_dir, pid_alive);
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "Downloads version {} from {PROJECT_NAME}, checks it, and puts it in Fusion's Add-Ins folder.",
                install::PIN.version
            ));
            ui.hyperlink_to("Project page", PROJECT_URL);
        });
        if self.installed.is_none() {
            ui.add_space(theme::SPACE_2);
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Waiting for the Fusion exporter. This window carries on by itself when it starts.");
            });
        }
    }

    /// Shows a state without looking at this computer: for tests and screenshots. Background
    /// reads stop.
    #[doc(hidden)]
    pub fn preview(&mut self, installed: Option<&str>, step: Install) {
        self.halt();
        self.frozen = true;
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

    /// What the install button has done, as of the last frame (tests only).
    #[doc(hidden)]
    pub fn state(&self) -> &Install {
        &self.install
    }

    #[doc(hidden)]
    pub fn set_latest(&mut self, version: &str) {
        self.latest = Some(version.into());
    }
}

impl Drop for Installer {
    fn drop(&mut self) {
        self.halt();
    }
}

/// "Version 0.4.0, in Fusion 2.0.21508."
fn running_line(agent: &Agent) -> String {
    let mut line = format!("Version {}", agent.version);
    if let Some(fusion) = &agent.fusion_version {
        line.push_str(&format!(", in Fusion {fusion}"));
    }
    line + "."
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

    /// The installer drawn as the window's content, with the state the panel would hand it.
    struct State {
        installer: Installer,
        offered: bool,
        presence: Presence,
    }

    fn state(offered: bool, presence: Presence, installed: Option<&str>, step: Install) -> State {
        let mut installer = Installer::new();
        installer.preview(installed, step);
        State {
            installer,
            offered,
            presence,
        }
    }

    fn harness(state: State) -> Harness<'static, State> {
        Harness::builder()
            .with_size(egui::vec2(600.0, 500.0))
            .build_state(
                |ctx, state: &mut State| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        state
                            .installer
                            .ui(ui, state.offered, &state.presence, None, |_| false)
                    });
                },
                state,
            )
    }

    fn found() -> Agent {
        Agent {
            jobs_dir: "/jobs".into(),
            features: vec![],
            version: "0.4.0".into(),
            pid: Some(1),
            updated: Some(0),
            busy: None,
            job: None,
            fusion_version: Some("2.0.21508".into()),
        }
    }

    #[test]
    fn not_found_offers_one_install_button_where_fusion_runs() {
        let mut h = harness(state(true, Presence::Absent, None, Install::Idle));
        h.run_steps(4);
        h.get_by_label("Install the Fusion exporter");
        h.get_by_label("The Fusion exporter was not found");
        h.get_by_label_contains("carries on by itself");
        let mut h = harness(state(false, Presence::Gone, None, Install::Idle));
        h.run_steps(4);
        assert!(h.query_by_label("Install the Fusion exporter").is_none());
        h.get_by_label_contains("Windows and macOS");
    }

    #[test]
    fn an_agent_file_that_cannot_be_used_is_said_and_the_install_still_offered() {
        let why = "The Fusion exporter's agent file names a jobs folder outside your .cad-export folder, so Dutchman will not use it.";
        let mut h = harness(state(
            true,
            Presence::Invalid(why.into()),
            None,
            Install::Idle,
        ));
        h.run_steps(4);
        h.get_by_label(why);
        h.get_by_label("Install the Fusion exporter");
    }

    #[test]
    fn fusion_busy_waits_and_offers_no_install() {
        let mut h = harness(state(
            true,
            Presence::Unresponsive(found()),
            None,
            Install::Idle,
        ));
        h.run_steps(4);
        assert!(h.query_by_label("Install the Fusion exporter").is_none());
        h.get_by_label("Fusion is busy");
    }

    #[test]
    fn a_found_exporter_offers_no_install() {
        let mut h = harness(state(true, Presence::Running(found()), None, Install::Idle));
        h.run_steps(4);
        assert!(h.query_by_label("Install the Fusion exporter").is_none());
        h.get_by_label("The Fusion exporter is running");
    }

    #[test]
    fn an_installed_but_not_started_exporter_gets_the_switch_on_steps() {
        let mut h = harness(state(true, Presence::Absent, Some("0.4.0"), Install::Idle));
        h.run_steps(4);
        h.get_by_label("The Fusion exporter is installed, but not switched on");
        h.get_by_label_contains("Shift+S");
        assert!(h.query_by_label("Install the Fusion exporter").is_none());
    }

    #[test]
    fn a_newer_release_is_only_mentioned() {
        let mut s = state(true, Presence::Absent, Some("0.4.0"), Install::Idle);
        s.installer.set_latest("0.4.1");
        let mut h = harness(s);
        h.run_steps(4);
        h.get_by_label_contains("A newer Fusion exporter is available (0.4.1)");
        assert!(h.query_by_label("Install the Fusion exporter").is_none());
    }

    /// A one-answer-per-connection server answering every request with `reply`.
    fn serve(reply: Vec<u8>) -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut stream = stream;
                let mut buf = [0u8; 2048];
                let _ = std::io::Read::read(&mut stream, &mut buf);
                let _ = std::io::Write::write_all(&mut stream, &reply);
            }
        });
        port
    }

    fn click_install(h: &mut Harness<'static, State>) {
        h.run_steps(4);
        h.get_by_label("Install the Fusion exporter").click();
        let started = std::time::Instant::now();
        while matches!(
            h.state_mut().installer.install_state(),
            Install::Idle | Install::Running(_)
        ) {
            assert!(
                started.elapsed() < Duration::from_secs(10),
                "install did not finish"
            );
            std::thread::sleep(Duration::from_millis(20));
            h.step();
        }
        h.run_steps(4);
    }

    fn options_at(port: u16, addins: &std::path::Path, sha256: String) -> Options {
        Options {
            version: "0.4.0".into(),
            zip_url: format!("http://127.0.0.1:{port}/z.zip"),
            sha256,
            addins: addins.to_path_buf(),
            connect_timeout: Duration::from_secs(2),
            stall_timeout: Duration::from_secs(2),
            deadline: Duration::from_secs(20),
            allow_http: true,
        }
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
        let mut reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            zip.len()
        )
        .into_bytes();
        reply.extend_from_slice(&zip);
        let port = serve(reply);
        let addins = tempfile::tempdir().unwrap();
        let mut s = state(true, Presence::Absent, None, Install::Idle);
        s.installer
            .set_install_options(options_at(port, addins.path(), sum));
        let mut h = harness(s);
        click_install(&mut h);
        assert!(matches!(
            h.state_mut().installer.install_state(),
            Install::Done(_)
        ));
        h.get_by_label("Switch it on once in Fusion");
        h.get_by_label_contains("Shift+S");
        assert!(addins
            .path()
            .join("FusionBulkExport/FusionBulkExport.manifest")
            .is_file());
    }

    #[test]
    fn a_download_that_is_not_published_says_so_plainly_and_can_be_tried_again() {
        let port = serve(
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 9\r\nConnection: close\r\n\r\nnot found"
                .to_vec(),
        );
        let addins = tempfile::tempdir().unwrap();
        let mut s = state(true, Presence::Absent, None, Install::Idle);
        s.installer
            .set_install_options(options_at(port, addins.path(), "00".repeat(32)));
        let mut h = harness(s);
        click_install(&mut h);
        let Install::Failed(why) = h.state_mut().installer.install_state() else {
            panic!("a 404 is a failed install, not a hang");
        };
        assert!(why.contains("not published yet"), "{why}");
        assert!(why.contains("project page"), "{why}");
        assert!(!why.contains("404") && !why.contains("http"), "{why}");
        // The message is on screen, and the button and the project page are still there.
        h.get_by_label(&why);
        h.get_by_label("Install the Fusion exporter");
        h.get_by_label("Project page");
        assert_eq!(
            std::fs::read_dir(addins.path()).unwrap().count(),
            0,
            "nothing was installed"
        );
    }
}
