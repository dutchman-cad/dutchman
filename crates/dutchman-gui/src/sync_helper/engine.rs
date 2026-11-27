//! Runs the sync engine for every folder the shared state asks for, and turns what it reports
//! into notifications and "last checked" times.
//!
//! The engine itself is `dutchman::sync::watch`; [`Engine`] is the seam the tests replace.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

use dutchman::sync::{Observed, Observer, Status, WatchSettings};
use dutchman::sync_state::{self, Folder, State};

/// What an engine tells the supervisor about a folder.
pub enum Event {
    /// A design's outcome.
    Design(Observed),
    /// The folder was looked at.
    Scanned,
    /// The engine could not run for the folder (it is not a folder, the conversion program is
    /// missing); the folder is not watched until it is turned off and on again.
    Broken(String),
}

/// Watches one folder of converted designs.
pub trait Engine: Send + Sync + 'static {
    /// Converts the folder's source folder into it where it has no design yet, so a folder
    /// chosen for the first time has link records to watch. Stops early when `stop` is set.
    fn prepare(&self, folder: &Folder, stop: &AtomicBool, events: &mut dyn FnMut(Event));
    /// Watches `folder` until `stop` is set. While `auto_update` is off a changed source is
    /// reported and left alone.
    fn watch(
        &self,
        folder: &Path,
        auto_update: &AtomicBool,
        stop: &AtomicBool,
        events: &mut dyn FnMut(Event),
    );
}

/// Shows the user a message outside any window.
pub trait Notifier: Send + Sync + 'static {
    fn notify(&self, title: &str, body: &str);
}

/// The engine as `dutchman sync --watch` runs it.
pub struct RealEngine {
    pub settings: WatchSettings,
}

struct Forward<'a> {
    auto_update: &'a AtomicBool,
    events: &'a mut dyn FnMut(Event),
}

impl Observer for Forward<'_> {
    fn auto_update(&self) -> bool {
        self.auto_update.load(Ordering::Relaxed)
    }
    fn event(&mut self, observed: Observed) {
        (self.events)(Event::Design(observed));
    }
    fn scanned(&mut self) {
        (self.events)(Event::Scanned);
    }
}

impl Engine for RealEngine {
    fn prepare(&self, folder: &Folder, stop: &AtomicBool, events: &mut dyn FnMut(Event)) {
        let Some(source) = folder.source.as_ref().filter(|source| source.is_dir()) else {
            return;
        };
        // Existing designs are kept (--no-clobber): only sources with no design are converted.
        let mut child = match std::process::Command::new(&self.settings.program)
            .args(["convert-all", "--no-clobber", "--out-dir"])
            .arg(&folder.output)
            .arg(source)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                events(Event::Broken(format!("cannot start the converter: {e}")));
                return;
            }
        };
        loop {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) if stop.load(Ordering::Relaxed) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            }
        }
    }

    fn watch(
        &self,
        folder: &Path,
        auto_update: &AtomicBool,
        stop: &AtomicBool,
        events: &mut dyn FnMut(Event),
    ) {
        let mut observer = Forward {
            auto_update,
            events,
        };
        if let Err(why) = dutchman::sync::watch(folder, &self.settings, stop, &mut observer) {
            (observer.events)(Event::Broken(why));
        }
    }
}

/// The notification for a design's outcome, if it deserves one: its title and body.
pub fn describe(observed: &Observed) -> Option<(String, String)> {
    let name = observed
        .design
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    match observed.status {
        Status::Updated => Some((
            "Design updated".to_owned(),
            format!("{name} was converted again."),
        )),
        Status::Failed => Some((
            "Update failed".to_owned(),
            format!(
                "{name}: {}. The previous design is kept.",
                first_line(&observed.message)
            ),
        )),
        Status::Changed => Some((
            "Source changed".to_owned(),
            format!("The source of {name} changed. Auto-update is off, so it was not converted."),
        )),
        Status::Unchanged | Status::SourceMissing => None,
    }
}

fn first_line(message: &str) -> &str {
    let line = message.lines().next().unwrap_or_default().trim();
    line.strip_suffix('.').unwrap_or(if line.is_empty() {
        "the conversion failed"
    } else {
        line
    })
}

/// `time` as RFC 3339 in UTC, to the second: `2026-11-22T09:30:00Z`.
pub fn rfc3339(time: SystemTime) -> String {
    let secs = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX / 2));
    let (days, rest) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

struct Running {
    stop: Arc<AtomicBool>,
    auto_update: Arc<AtomicBool>,
    join: JoinHandle<()>,
}

/// Keeps one engine thread per watched folder, as the state asks.
pub struct Supervisor {
    engine: Arc<dyn Engine>,
    notifier: Arc<dyn Notifier>,
    state_path: Option<PathBuf>,
    running: BTreeMap<PathBuf, Running>,
}

impl Supervisor {
    /// `state_path` is where "last checked" is written; `None` writes nothing.
    pub fn new(
        engine: Arc<dyn Engine>,
        notifier: Arc<dyn Notifier>,
        state_path: Option<PathBuf>,
    ) -> Self {
        Self {
            engine,
            notifier,
            state_path,
            running: BTreeMap::new(),
        }
    }

    /// The folders being watched.
    pub fn watching(&self) -> Vec<&Path> {
        self.running.keys().map(PathBuf::as_path).collect()
    }

    /// Starts the folders `state` turns on, stops the ones it turns off (all, when paused) and
    /// hands each running folder its current auto-update setting.
    pub fn reconcile(&mut self, state: &State) {
        let wanted: BTreeMap<&Path, &Folder> = if state.paused {
            BTreeMap::new()
        } else {
            state.enabled().map(|f| (f.output.as_path(), f)).collect()
        };
        let gone: Vec<PathBuf> = self
            .running
            .keys()
            .filter(|path| !wanted.contains_key(path.as_path()))
            .cloned()
            .collect();
        for path in gone {
            if let Some(running) = self.running.remove(&path) {
                running.stop.store(true, Ordering::Relaxed);
                // The thread ends within a scan; it is not waited for here.
                drop(running.join);
            }
        }
        for (path, folder) in wanted {
            if let Some(running) = self.running.get(path) {
                running
                    .auto_update
                    .store(folder.auto_update, Ordering::Relaxed);
                continue;
            }
            let running = self.start(folder);
            self.running.insert(path.to_path_buf(), running);
        }
    }

    fn start(&self, folder: &Folder) -> Running {
        let stop = Arc::new(AtomicBool::new(false));
        let auto_update = Arc::new(AtomicBool::new(folder.auto_update));
        let (engine, notifier) = (Arc::clone(&self.engine), Arc::clone(&self.notifier));
        let (stop_flag, auto_flag) = (Arc::clone(&stop), Arc::clone(&auto_update));
        let (folder, state_path) = (folder.clone(), self.state_path.clone());
        let join = std::thread::spawn(move || {
            let checked = Mutex::new(None::<Instant>);
            let mut events = |event: Event| match event {
                Event::Design(observed) => {
                    if let Some((title, body)) = describe(&observed) {
                        notifier.notify(&title, &body);
                    }
                    record_checked(&state_path, &folder.output, &checked, true);
                }
                Event::Scanned => record_checked(&state_path, &folder.output, &checked, false),
                Event::Broken(why) => notifier.notify(
                    "Dutchman Sync cannot watch a folder",
                    &format!("{}: {why}", folder.output.display()),
                ),
            };
            engine.prepare(&folder, &stop_flag, &mut events);
            if !stop_flag.load(Ordering::Relaxed) {
                engine.watch(&folder.output, &auto_flag, &stop_flag, &mut events);
            }
        });
        Running {
            stop,
            auto_update,
            join,
        }
    }

    /// Stops every folder and waits for the threads to end.
    pub fn stop_all(&mut self) {
        for running in self.running.values() {
            running.stop.store(true, Ordering::Relaxed);
        }
        for (_, running) in std::mem::take(&mut self.running) {
            let _ = running.join.join();
        }
    }
}

/// How often "last checked" is written when nothing else happened: the state file is shared,
/// so it is not rewritten at every scan.
const CHECKED_EVERY: Duration = Duration::from_secs(30);

fn record_checked(
    state_path: &Option<PathBuf>,
    output: &Path,
    last: &Mutex<Option<Instant>>,
    force: bool,
) {
    let Some(path) = state_path else { return };
    let Ok(mut last) = last.lock() else { return };
    if !force && last.is_some_and(|at| at.elapsed() < CHECKED_EVERY) {
        return;
    }
    *last = Some(Instant::now());
    let _ = sync_state::update_at(path, |state| {
        if let Some(folder) = state.folder_mut(output) {
            folder.last_checked = Some(rfc3339(SystemTime::now()));
        }
    });
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::mpsc::{channel, Sender};

    /// What a fake engine was asked.
    #[derive(Debug, PartialEq, Eq)]
    pub enum Call {
        Prepare(PathBuf),
        Watch(PathBuf),
    }

    /// An engine that says what it was asked and replays canned events.
    pub struct Fake {
        pub calls: Sender<Call>,
        pub script: Vec<Observed>,
        pub auto_seen: Sender<(PathBuf, bool)>,
    }

    impl Engine for Fake {
        fn prepare(&self, folder: &Folder, _stop: &AtomicBool, _events: &mut dyn FnMut(Event)) {
            let _ = self.calls.send(Call::Prepare(folder.output.clone()));
        }
        fn watch(
            &self,
            folder: &Path,
            auto_update: &AtomicBool,
            stop: &AtomicBool,
            events: &mut dyn FnMut(Event),
        ) {
            let _ = self.calls.send(Call::Watch(folder.to_path_buf()));
            for observed in &self.script {
                events(Event::Design(observed.clone()));
            }
            events(Event::Scanned);
            let _ = self
                .auto_seen
                .send((folder.to_path_buf(), auto_update.load(Ordering::Relaxed)));
            while !stop.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
    }

    pub struct Recorder(pub Mutex<Sender<(String, String)>>);

    impl Notifier for Recorder {
        fn notify(&self, title: &str, body: &str) {
            if let Ok(sender) = self.0.lock() {
                let _ = sender.send((title.to_owned(), body.to_owned()));
            }
        }
    }

    fn observed(name: &str, status: Status, message: &str) -> Observed {
        Observed {
            design: PathBuf::from("/out").join(name),
            status,
            message: message.to_owned(),
            report: None,
        }
    }

    fn state(folders: &[(&str, bool, bool)], paused: bool) -> State {
        let mut state = State {
            paused,
            ..State::default()
        };
        for (output, auto_update, enabled) in folders {
            state.folders.push(Folder {
                output: PathBuf::from(output),
                source: None,
                auto_update: *auto_update,
                enabled: *enabled,
                last_checked: None,
            });
        }
        state
    }

    struct Rig {
        supervisor: Supervisor,
        calls: std::sync::mpsc::Receiver<Call>,
        auto: std::sync::mpsc::Receiver<(PathBuf, bool)>,
        notes: std::sync::mpsc::Receiver<(String, String)>,
    }

    fn rig(script: Vec<Observed>, state_path: Option<PathBuf>) -> Rig {
        let (calls_tx, calls) = channel();
        let (auto_tx, auto) = channel();
        let (notes_tx, notes) = channel();
        let engine = Arc::new(Fake {
            calls: calls_tx,
            script,
            auto_seen: auto_tx,
        });
        let notifier = Arc::new(Recorder(Mutex::new(notes_tx)));
        Rig {
            supervisor: Supervisor::new(engine, notifier, state_path),
            calls,
            auto,
            notes,
        }
    }

    const WAIT: Duration = Duration::from_secs(5);

    #[test]
    fn enabled_folders_are_prepared_then_watched_and_others_are_not() {
        let mut rig = rig(Vec::new(), None);
        rig.supervisor
            .reconcile(&state(&[("/a", true, true), ("/b", true, false)], false));
        assert_eq!(
            rig.calls.recv_timeout(WAIT).unwrap(),
            Call::Prepare("/a".into())
        );
        assert_eq!(
            rig.calls.recv_timeout(WAIT).unwrap(),
            Call::Watch("/a".into())
        );
        assert_eq!(rig.supervisor.watching(), [Path::new("/a")]);
        rig.supervisor.stop_all();
        assert!(rig.calls.try_recv().is_err());
    }

    #[test]
    fn auto_update_off_is_passed_to_the_engine_and_changes_apply_while_running() {
        let mut rig = rig(Vec::new(), None);
        rig.supervisor
            .reconcile(&state(&[("/a", false, true)], false));
        assert_eq!(rig.auto.recv_timeout(WAIT).unwrap(), ("/a".into(), false));
        rig.supervisor
            .reconcile(&state(&[("/a", true, true)], false));
        let flag = rig.supervisor.running.get(Path::new("/a")).unwrap();
        assert!(flag.auto_update.load(Ordering::Relaxed));
        rig.supervisor.stop_all();
    }

    #[test]
    fn pausing_stops_every_folder_and_resuming_starts_them_again() {
        let mut rig = rig(Vec::new(), None);
        let folders = [("/a", true, true), ("/b", true, true)];
        rig.supervisor.reconcile(&state(&folders, false));
        assert_eq!(rig.supervisor.watching().len(), 2);
        rig.supervisor.reconcile(&state(&folders, true));
        assert!(rig.supervisor.watching().is_empty());
        rig.supervisor.reconcile(&state(&folders, false));
        assert_eq!(rig.supervisor.watching().len(), 2);
        rig.supervisor.stop_all();
        assert!(rig.supervisor.watching().is_empty());
    }

    #[test]
    fn updates_and_failures_notify_and_the_rest_stay_quiet() {
        let script = vec![
            observed("a.sc", Status::Unchanged, ""),
            observed("b.sc", Status::Updated, ""),
            observed(
                "c.sc",
                Status::Failed,
                "cannot read the source.\nsecond line",
            ),
            observed("d.sc", Status::Changed, ""),
            observed("e.sc", Status::SourceMissing, "x not found"),
        ];
        let mut rig = rig(script, None);
        rig.supervisor
            .reconcile(&state(&[("/a", true, true)], false));
        let updated = rig.notes.recv_timeout(WAIT).unwrap();
        assert_eq!(updated.0, "Design updated");
        assert!(updated.1.contains("b.sc"));
        let failed = rig.notes.recv_timeout(WAIT).unwrap();
        assert_eq!(failed.0, "Update failed");
        assert_eq!(
            failed.1,
            "c.sc: cannot read the source. The previous design is kept."
        );
        let changed = rig.notes.recv_timeout(WAIT).unwrap();
        assert_eq!(changed.0, "Source changed");
        assert!(changed.1.contains("Auto-update is off"));
        rig.supervisor.stop_all();
        assert!(rig.notes.try_recv().is_err());
    }

    #[test]
    fn a_scan_records_last_checked_in_the_shared_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(sync_state::FILE_NAME);
        let on = state(&[("/a", true, true)], false);
        sync_state::write_to(&path, &on).unwrap();
        let mut rig = rig(Vec::new(), Some(path.clone()));
        rig.supervisor.reconcile(&on);
        rig.auto.recv_timeout(WAIT).unwrap();
        rig.supervisor.stop_all();
        let after = sync_state::read_from(&path);
        let checked = after.folders.first().unwrap().last_checked.clone().unwrap();
        assert!(checked.ends_with('Z') && checked.len() == 20, "{checked}");
        assert!(after.folders.first().unwrap().enabled);
    }

    #[test]
    fn times_are_rfc3339_utc() {
        let at = |secs| SystemTime::UNIX_EPOCH + Duration::from_secs(secs);
        assert_eq!(rfc3339(at(0)), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(at(951_782_400)), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(at(1_795_339_800)), "2026-11-22T09:30:00Z");
    }
}
