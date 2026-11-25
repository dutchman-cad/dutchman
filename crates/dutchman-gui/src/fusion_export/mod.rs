//! Export from Fusion: the person picks designs, Dutchman asks the Fusion exporter running
//! inside their Fusion to export them (job protocol 1), follows its progress, and converts
//! the export folder like any other input.
//!
//! One window, one [`Panel`]. While no exporter has announced itself (Stage::NoAgent) it shows
//! the find-and-install part ([`installer`]): what is missing, a one-click install, and the one
//! step the person does in Fusion. Once the exporter announces itself it carries on with the
//! job side: the inventory job and the design picker, the export job and its progress, Stop,
//! cleanup and the plain-words errors. The protocol is in the exporter's `docs/job-protocol.md`.
pub mod agent;
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub mod fake;
pub mod install;
pub mod installer;
pub mod inventory;
pub mod job;
mod view;

/// The one place in Dutchman's public text that names the exporter's project; everywhere else it
/// is "the Fusion exporter".
pub const PROJECT_NAME: &str = "Lifeboat's Fusion exporter";
/// The exporter's project page.
pub const PROJECT_URL: &str = "https://github.com/cad-lifeboat/fusion-bulk-export";

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent::{Agent, Presence};
use installer::Installer;
use inventory::Inventory;
use job::{Kind, Next, Status, Update};

/// Where things live and how long to wait; replaced in tests and screenshots.
#[derive(Clone, Debug)]
pub struct Env {
    /// `~/.cad-export`; `None` when the person's home cannot be found.
    pub cad_dir: Option<PathBuf>,
    /// Dutchman's own folders for inventories and exports.
    pub work_dir: PathBuf,
    /// How often a job's status file is read, and the agent file looked for.
    pub poll: Duration,
    /// Whether a process is running (the heartbeat rule asks when the heartbeat is old).
    pub pid_alive: fn(u32) -> bool,
    /// How long Fusion may be gone while a job waits before Dutchman gives up.
    pub give_up: Duration,
    /// How long a written job may wait for its first status before Dutchman gives up.
    pub no_answer: Duration,
}

impl Env {
    pub fn system() -> Self {
        let home = if cfg!(windows) {
            std::env::var_os("USERPROFILE")
        } else {
            std::env::var_os("HOME")
        }
        .map(PathBuf::from)
        .unwrap_or_default();
        Self {
            cad_dir: agent::cad_export_dir(),
            work_dir: home.join("Dutchman Fusion exports"),
            poll: Duration::from_millis(500),
            pid_alive: agent::pid_alive_system,
            give_up: Duration::from_secs(60),
            no_answer: Duration::from_secs(120),
        }
    }

    /// A test home: quick polling, the real waits.
    #[doc(hidden)]
    pub fn for_test(home: &std::path::Path) -> Self {
        Self {
            cad_dir: Some(home.join(".cad-export")),
            work_dir: home.join("Dutchman Fusion exports"),
            poll: Duration::from_millis(20),
            ..Self::system()
        }
    }
}

/// Fusion only runs on Windows and macOS, so only they are offered the entry.
pub const fn supported_here() -> bool {
    cfg!(any(windows, target_os = "macos"))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    /// No exporter has announced itself.
    NoAgent,
    /// The inventory job is running, or its list is being read.
    Listing,
    /// The list could not be made; the person chooses to try again.
    ListFailed,
    Picking,
    Exporting,
    /// The export ended without a clean `done`, or with failed designs.
    Finished,
}

struct Running {
    id: String,
    kind: Kind,
    stop: Arc<AtomicBool>,
    abandon: Arc<AtomicBool>,
    stopping: bool,
    status: Option<Status>,
    /// When the job was written.
    since: Instant,
    /// Since when the exporter has been gone, while this job waits.
    gone_since: Option<Instant>,
}

/// How the last export ended, for the Finished screen.
struct Ended {
    status: Status,
    folder: PathBuf,
}

pub struct Panel {
    pub(crate) open: bool,
    offered: bool,
    env: Env,
    stage: Stage,
    agent: Option<Agent>,
    /// What the agent folder said when it was last looked at.
    presence: Presence,
    installer: Installer,
    /// Why the agent file is not used, when it is not.
    agent_problem: Option<String>,
    last_agent_check: Option<Instant>,
    last_watch: Option<Instant>,
    inventory: Option<Inventory>,
    /// The inventory file is being read off the window's thread.
    reading_list: bool,
    list_error: Option<String>,
    picked: BTreeSet<String>,
    query: String,
    /// The designs the search shows, kept until the text or the list changes.
    shown: Vec<String>,
    shown_query: Option<String>,
    running: Option<Running>,
    /// What an export of the picked designs is called, kept for "Try again".
    export_of: Option<(Vec<String>, PathBuf)>,
    ended: Option<Ended>,
    error: Option<(String, Next)>,
    /// A line for the main window when something ends while this window is closed.
    notice: Option<String>,
    repaint: Option<eframe::egui::Context>,
    sender: Sender<(String, Update)>,
    receiver: Receiver<(String, Update)>,
    list_sender: Sender<Result<Inventory, String>>,
    list_receiver: Receiver<Result<Inventory, String>>,
}

impl Panel {
    pub(crate) fn new() -> Self {
        let (sender, receiver) = channel();
        let (list_sender, list_receiver) = channel();
        Self {
            open: false,
            offered: supported_here(),
            env: Env::system(),
            stage: Stage::NoAgent,
            agent: None,
            presence: Presence::Absent,
            installer: Installer::new(),
            agent_problem: None,
            last_agent_check: None,
            last_watch: None,
            inventory: None,
            reading_list: false,
            list_error: None,
            picked: BTreeSet::new(),
            query: String::new(),
            shown: Vec::new(),
            shown_query: None,
            running: None,
            export_of: None,
            ended: None,
            error: None,
            notice: None,
            repaint: None,
            sender,
            receiver,
            list_sender,
            list_receiver,
        }
    }

    /// Whether the main screen offers "Export from Fusion…".
    pub(crate) fn offered(&self) -> bool {
        self.offered
    }

    /// Points the panel at another home and shows the entry on any system (tests and
    /// screenshots only).
    pub(crate) fn preview(&mut self, env: Env) {
        let mut installer = Installer::new();
        installer.preview(None, installer::Install::Idle);
        *self = Self {
            env,
            offered: true,
            installer,
            ..Self::new()
        };
    }

    /// Shows the find-and-install part in a given state, without looking at this computer
    /// (tests and screenshots only). Nothing is read in the background.
    pub(crate) fn preview_install(
        &mut self,
        offered: bool,
        presence: Presence,
        installed: Option<&str>,
        step: installer::Install,
    ) {
        self.open = true;
        self.offered = offered;
        self.stage = Stage::NoAgent;
        self.presence = presence;
        self.installer.preview(installed, step);
        // Looking at the real agent folder would replace the state shown.
        self.last_agent_check = Some(Instant::now() + Duration::from_secs(3600));
    }

    /// The install button and what it has done (tests only).
    #[doc(hidden)]
    pub fn installer_mut(&mut self) -> &mut Installer {
        &mut self.installer
    }

    /// Shows the entry on any system (tests only).
    pub(crate) fn offer(&mut self) {
        self.offered = true;
    }

    pub(crate) fn stage(&self) -> Stage {
        self.stage
    }

    /// A line for the main window, once, when something ended while this window was closed.
    pub(crate) fn take_notice(&mut self) -> Option<String> {
        self.notice.take()
    }

    pub(crate) fn toggle(&mut self) {
        self.open = !self.open;
        self.last_agent_check = None;
        if !self.open {
            self.installer.halt();
        }
    }

    /// What the agent folder says now, with the heartbeat rule applied.
    fn look(&mut self) -> Presence {
        let presence = match self.env.cad_dir.as_deref() {
            Some(cad) => agent::presence_in(cad, agent::now_secs(), &self.env.pid_alive),
            None => Presence::Absent,
        };
        self.agent_problem = match &presence {
            Presence::Invalid(why) => Some(why.clone()),
            _ => None,
        };
        self.presence = presence.clone();
        presence
    }

    /// The agent as it is now, with the heartbeat rule applied: `Err` says in plain words why
    /// no job can go to it. Fusion busy (a stale heartbeat, a live process) still takes a job.
    fn fresh_agent(&mut self) -> Result<Agent, String> {
        match self.look() {
            Presence::Running(agent) | Presence::Unresponsive(agent) => Ok(agent),
            Presence::Invalid(why) => Err(why),
            Presence::Absent | Presence::Gone => Err("The Fusion exporter is not running. Open Fusion with the exporter switched on, then try again.".to_owned()),
        }
    }

    /// Looks for the exporter while none is there. Its first appearance, with no list yet,
    /// asks for the list; nothing else ever does that by itself. While Fusion is busy (an old
    /// heartbeat, the process still there) the window waits for it to answer.
    fn find_agent(&mut self) {
        match self.look() {
            Presence::Running(agent) => {
                self.agent = Some(agent);
                if self.stage == Stage::NoAgent && self.inventory.is_none() {
                    self.list_designs();
                }
            }
            _ => self.agent = None,
        }
    }

    /// A list that could not be made: the reason, and the person decides to try again.
    fn list_failed(&mut self, why: String) {
        self.running = None;
        self.reading_list = false;
        self.list_error = Some(why);
        self.stage = Stage::ListFailed;
    }

    /// Starts the inventory job: the list of the person's hubs, projects and designs.
    pub(crate) fn list_designs(&mut self) {
        self.error = None;
        self.list_error = None;
        let agent = match self.fresh_agent() {
            Ok(agent) => agent,
            Err(why) => {
                self.agent = None;
                if self.agent_problem.is_some() {
                    self.list_failed(why);
                } else {
                    self.stage = if self.inventory.is_some() {
                        self.error = Some((why, Next::TryAgain));
                        Stage::Picking
                    } else {
                        Stage::NoAgent
                    };
                }
                return;
            }
        };
        self.agent = Some(agent.clone());
        self.stage = Stage::Listing;
        let folder = self.inventory_folder();
        let started = std::fs::create_dir_all(&folder)
            .map_err(|why| format!("Cannot use Dutchman's folder for Fusion exports: {why}"))
            .and_then(|()| {
                let _ = std::fs::remove_file(folder.join("inventory.json"));
                let id = job::new_id();
                let request = job::build_job(&id, Kind::Inventory, &folder, &[]);
                job::submit(&agent, Kind::Inventory, &request)
            });
        match started {
            Ok(id) => self.follow(agent, id, Kind::Inventory),
            Err(why) => self.list_failed(why),
        }
    }

    fn inventory_folder(&self) -> PathBuf {
        self.env.work_dir.join("inventory")
    }

    /// Exports the picked designs, with everything they insert.
    pub(crate) fn export_picked(&mut self) {
        let designs = inventory::sorted(&self.picked);
        if designs.is_empty() {
            return;
        }
        let folder = job::export_folder(&self.env.work_dir.join("exports"), &designs);
        self.export_of = Some((designs, folder));
        self.start_export();
    }

    /// Runs the same export again; the exporter carries on where it stopped. Fusion may
    /// have closed since the list was made, so the exporter is looked for first.
    pub(crate) fn start_export(&mut self) {
        self.error = None;
        let Some((designs, folder)) = self.export_of.clone() else {
            return;
        };
        let agent = match self.fresh_agent() {
            Ok(agent) => agent,
            Err(why) => {
                self.agent = None;
                self.error = Some((why, Next::TryAgain));
                return;
            }
        };
        self.agent = Some(agent.clone());
        self.ended = None;
        let started = std::fs::create_dir_all(&folder)
            .map_err(|why| format!("Cannot use Dutchman's folder for Fusion exports: {why}"))
            .and_then(|()| {
                let id = job::new_id();
                let request = job::build_job(&id, Kind::Export, &folder, &designs);
                job::submit(&agent, Kind::Export, &request)
            });
        match started {
            Ok(id) => {
                self.stage = Stage::Exporting;
                self.follow(agent, id, Kind::Export);
            }
            Err(why) => {
                self.error = Some((why, Next::TryAgain));
                self.stage = if self.ended.is_none() && self.inventory.is_some() {
                    Stage::Picking
                } else {
                    self.stage
                };
            }
        }
    }

    fn follow(&mut self, agent: Agent, id: String, kind: Kind) {
        let stop = Arc::new(AtomicBool::new(false));
        let abandon = Arc::new(AtomicBool::new(false));
        self.running = Some(Running {
            id: id.clone(),
            kind,
            stop: stop.clone(),
            abandon: abandon.clone(),
            stopping: false,
            status: None,
            since: Instant::now(),
            gone_since: None,
        });
        let ctx = self.repaint.clone();
        job::follow(
            agent.jobs_dir,
            id,
            stop,
            abandon,
            self.env.poll,
            self.sender.clone(),
            move || {
                if let Some(ctx) = &ctx {
                    ctx.request_repaint();
                }
            },
        );
    }

    /// Asks the exporter to stop: the designs finished so far are kept.
    pub(crate) fn stop(&mut self) {
        if let Some(running) = &mut self.running {
            running.stop.store(true, Ordering::SeqCst);
            running.stopping = true;
        }
    }

    /// Stops following the job. An unclaimed job is withdrawn so it cannot start later; one
    /// the exporter has taken is asked to cancel.
    fn abandon_job(&mut self) -> Option<Running> {
        let running = self.running.take()?;
        running.abandon.store(true, Ordering::SeqCst);
        if let Some(agent) = &self.agent {
            if running.status.is_none() && job::read_status(&agent.jobs_dir, &running.id).is_none()
            {
                job::cleanup(&agent.jobs_dir, &running.id);
            } else {
                let _ = job::cancel(&agent.jobs_dir, &running.id);
            }
        }
        Some(running)
    }

    /// "Stop waiting": leave this stage without an answer from the exporter.
    pub(crate) fn stop_waiting(&mut self) {
        let Some(running) = self.abandon_job() else {
            return;
        };
        match running.kind {
            Kind::Inventory => {
                self.list_failed("Stopped waiting for the list of your designs.".into())
            }
            Kind::Export => {
                self.stage = if self.inventory.is_some() {
                    Stage::Picking
                } else {
                    Stage::ListFailed
                };
                self.error = Some((
                    "Stopped waiting. If Fusion is still exporting, it carries on; choose the same designs again to follow on where it stops.".into(),
                    Next::Nothing,
                ));
            }
        }
    }

    /// Gives up on a job: the exporter is gone, or never took it. `reason` is one of the
    /// codes `job::reason_info` words.
    fn give_up(&mut self, reason: &str) {
        let Some(running) = self.abandon_job() else {
            return;
        };
        let (text, _) = job::reason_info(reason, "");
        match running.kind {
            Kind::Inventory => self.list_failed(text),
            Kind::Export => {
                let folder = self
                    .export_of
                    .as_ref()
                    .map(|(_, folder)| folder.clone())
                    .unwrap_or_default();
                let status = Status {
                    state: "failed".into(),
                    reason: Some(reason.into()),
                    ..Status::default()
                };
                self.stage = Stage::Finished;
                self.ended = Some(Ended { status, folder });
                self.tell_if_closed("The export from Fusion stopped: Fusion went away. Open Export from Fusion to carry on.");
            }
        }
    }

    fn tell_if_closed(&mut self, text: &str) {
        if !self.open {
            self.notice = Some(text.to_owned());
        }
    }

    /// While a job waits: the heartbeat and process rule for the exporter, and the time the
    /// first status has taken.
    fn watch(&mut self) {
        if self.running.is_none() {
            return;
        }
        let alive = self.look().agent().is_some();
        let Some(running) = &self.running else {
            return;
        };
        let (gone, waited) = (running.gone_since, running.since.elapsed());
        let first_status_missing = running.status.is_none();
        if let Some(running) = &mut self.running {
            running.gone_since = if alive {
                None
            } else {
                gone.or(Some(Instant::now()))
            };
        }
        if gone.is_some_and(|since| !alive && since.elapsed() >= self.env.give_up) {
            self.give_up("agent_gone");
        } else if first_status_missing && waited >= self.env.no_answer {
            self.give_up("no_answer");
        }
    }

    /// Reads what the exporter wrote since the last frame. Returns the export folder when
    /// it is ready to convert.
    pub(crate) fn poll(&mut self, ctx: &eframe::egui::Context) -> Option<PathBuf> {
        self.repaint = Some(ctx.clone());
        if self.open {
            self.installer.start(ctx);
        }
        if self.open
            && self.stage == Stage::NoAgent
            && self
                .last_agent_check
                .is_none_or(|at| at.elapsed() >= self.env.poll)
        {
            self.last_agent_check = Some(Instant::now());
            self.find_agent();
            ctx.request_repaint_after(self.env.poll);
        }
        if self.running.is_some()
            && self
                .last_watch
                .is_none_or(|at| at.elapsed() >= self.env.poll)
        {
            self.last_watch = Some(Instant::now());
            self.watch();
            ctx.request_repaint_after(self.env.poll);
        }
        while let Ok(list) = self.list_receiver.try_recv() {
            if self.stage != Stage::Listing || !self.reading_list {
                continue;
            }
            self.reading_list = false;
            match list {
                Ok(inventory) => {
                    let known: BTreeSet<String> = inventory.matching_ids("").into_iter().collect();
                    self.picked.retain(|id| known.contains(id));
                    self.inventory = Some(inventory);
                    self.shown_query = None;
                    self.stage = Stage::Picking;
                }
                Err(why) => self.list_failed(why),
            }
        }
        let mut convert = None;
        while let Ok((id, update)) = self.receiver.try_recv() {
            let Some(running) = self.running.as_mut().filter(|r| r.id == id) else {
                continue;
            };
            match update {
                Update::Status(status) => running.status = Some(status),
                Update::Lost(why) => {
                    let kind = running.kind;
                    self.running = None;
                    match kind {
                        Kind::Inventory => self.list_failed(why),
                        Kind::Export => {
                            self.error = Some((why, Next::TryAgain));
                            self.stage = Stage::Picking;
                        }
                    }
                }
                Update::Final(status) => {
                    let kind = running.kind;
                    self.running = None;
                    convert = convert.or(self.finished(kind, status));
                }
            }
        }
        convert
    }

    fn finished(&mut self, kind: Kind, status: Status) -> Option<PathBuf> {
        match kind {
            Kind::Inventory => {
                if status.state == "done" {
                    // The list is always the file in Dutchman's own folder, never a path
                    // the status names; it is read off the window's thread.
                    let path = self.inventory_folder().join("inventory.json");
                    let sender = self.list_sender.clone();
                    let ctx = self.repaint.clone();
                    self.reading_list = true;
                    self.stage = Stage::Listing;
                    std::thread::spawn(move || {
                        let _ = sender.send(Inventory::read(&path));
                        if let Some(ctx) = ctx {
                            ctx.request_repaint();
                        }
                    });
                } else if status.state == "cancelled" {
                    self.list_failed("Listing your designs was stopped.".into());
                } else {
                    let (text, _) = job::reason_info(
                        status.reason.as_deref().unwrap_or("error"),
                        &status.message,
                    );
                    self.list_failed(text);
                }
                None
            }
            Kind::Export => {
                let folder = self
                    .export_of
                    .as_ref()
                    .map(|(_, folder)| folder.clone())
                    .unwrap_or_default();
                if status.state == "done" && status.failed == 0 {
                    self.stage = Stage::Picking;
                    self.open = false;
                    return Some(folder);
                }
                self.stage = Stage::Finished;
                self.ended = Some(Ended { status, folder });
                self.tell_if_closed(
                    "The export from Fusion ended with problems. Open Export from Fusion to see them.",
                );
                None
            }
        }
    }

    /// Takes what exported so far to the converter.
    pub(crate) fn convert_what_exported(&mut self) -> Option<PathBuf> {
        let ended = self.ended.take()?;
        self.stage = Stage::Picking;
        self.open = false;
        Some(ended.folder)
    }

    /// Back to the list of designs.
    pub(crate) fn back_to_picking(&mut self) {
        self.ended = None;
        self.error = None;
        self.stage = if self.inventory.is_some() {
            Stage::Picking
        } else {
            Stage::ListFailed
        };
        if self.inventory.is_none() {
            self.list_error
                .get_or_insert_with(|| "There is no list of your designs yet.".into());
        }
    }
}

#[cfg(test)]
mod tests;
