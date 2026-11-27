//! Stage and scoped running-step progress. No work is done per geometry entity.

use crate::time::Instant;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(not(target_family = "wasm"))]
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::Stage;

/// A snapshot of one conversion, independent of its final report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProgressEvent {
    pub stage: Stage,
    pub elapsed_ms: u64,
    pub designs_read: u64,
    /// Unknown when the decoder owns the document loop or discovery is still running.
    pub designs_total: Option<u64>,
    pub heartbeat: bool,
    pub running: Option<RunningStep>,
}

impl std::fmt::Display for ProgressEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", super::progress_words::summary(self))
    }
}

/// Default usual-time thresholds for work without a matching estimate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepKind {
    Opening,
    Reading,
    Preparing,
    Checking,
    CheckingOriginal,
    CheckingHistory,
    CheckingBlends,
    CheckingPrefix,
    CheckingGeometry,
    Writing,
    Projecting,
}
impl StepKind {
    pub const ALL: [Self; 11] = [
        Self::Opening,
        Self::Reading,
        Self::Preparing,
        Self::Checking,
        Self::CheckingOriginal,
        Self::CheckingHistory,
        Self::CheckingBlends,
        Self::CheckingPrefix,
        Self::CheckingGeometry,
        Self::Writing,
        Self::Projecting,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Opening => "opening",
            Self::Reading => "reading",
            Self::Preparing => "preparing",
            Self::Checking => "checking",
            Self::CheckingOriginal => "checking_original",
            Self::CheckingHistory => "checking_history",
            Self::CheckingBlends => "checking_blends",
            Self::CheckingPrefix => "checking_prefix",
            Self::CheckingGeometry => "checking_geometry",
            Self::Writing => "writing",
            Self::Projecting => "projecting",
        }
    }
    pub fn usual_ms(self) -> u64 {
        match self {
            Self::Opening | Self::Writing | Self::Projecting => 10_000,
            Self::Reading | Self::Preparing => 30_000,
            Self::Checking
            | Self::CheckingOriginal
            | Self::CheckingHistory
            | Self::CheckingBlends
            | Self::CheckingPrefix
            | Self::CheckingGeometry => 60_000,
        }
    }
    pub(crate) fn is_check(self) -> bool {
        matches!(
            self,
            Self::Checking
                | Self::CheckingOriginal
                | Self::CheckingHistory
                | Self::CheckingBlends
                | Self::CheckingPrefix
                | Self::CheckingGeometry
        )
    }
    fn stage(stage: Stage) -> Self {
        match stage {
            Stage::Input => Self::Opening,
            Stage::Decode => Self::Reading,
            Stage::Project => Self::Projecting,
            Stage::Map => Self::Preparing,
            Stage::Write => Self::Writing,
            Stage::SolidCraft => Self::Checking,
        }
    }
}
/// Names are private local display text. Anonymous sinks must never serialize this record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningStep {
    pub kind: StepKind,
    pub name: Option<String>,
    pub elapsed_ms: u64,
    pub usual_ms: u64,
    pub limit_ms: Option<u64>,
    /// Fixed consequence text, supplied by the owner of the limit.
    pub consequence: Option<String>,
    /// The timeline feature this step's worker is on, when it goes feature by feature.
    pub feature: Option<FeatureStep>,
}
/// One feature of a design's timeline, as history mapping reaches it. The name is private
/// local display text, like [`RunningStep::name`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureStep {
    /// Position in the source timeline, from 1.
    pub index: u64,
    /// Features in the source timeline, suppressed ones included.
    pub total: u64,
    pub name: Option<String>,
    /// Checked by measurement in the target application, which takes longer than mapping.
    pub measured: bool,
}
impl ProgressEvent {
    pub fn running_line(&self) -> String {
        match &self.running {
            Some(step) => super::progress_words::running(step),
            None => super::progress_words::running(&RunningStep {
                kind: StepKind::stage(self.stage),
                name: None,
                elapsed_ms: self.elapsed_ms,
                usual_ms: StepKind::stage(self.stage).usual_ms(),
                limit_ms: None,
                consequence: None,
                feature: None,
            }),
        }
    }

    /// "Feature 29 of 37 · Extrude 1" while a design's history goes feature by feature.
    pub fn feature_line(&self) -> Option<String> {
        self.running
            .as_ref()
            .and_then(|step| step.feature.as_ref())
            .map(super::progress_words::feature)
    }

    /// The note shown while the current feature is checked by measurement.
    pub fn measured_note(&self) -> Option<&'static str> {
        self.running
            .as_ref()
            .and_then(|step| step.feature.as_ref())
            .filter(|feature| feature.measured)
            .map(|_| super::progress_words::measured())
    }
}

/// How often feature changes alone may emit an event; a heartbeat flushes the latest one.
const FEATURE_INTERVAL: Duration = Duration::from_millis(250);

#[cfg(not(target_family = "wasm"))]
type Worker = std::thread::ThreadId;
#[cfg(target_family = "wasm")]
type Worker = ();
fn current_worker() -> Worker {
    #[cfg(not(target_family = "wasm"))]
    {
        std::thread::current().id()
    }
}
fn step_worker(step: &ActiveStep) -> Worker {
    #[cfg(not(target_family = "wasm"))]
    {
        step.thread
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = step;
    }
}
struct ActiveFeature {
    id: u64,
    worker: Worker,
    feature: FeatureStep,
}
fn display_name(name: &str) -> String {
    name.chars()
        .take(240)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// A design's timeline on one worker; ends with the guard, also when work unwinds.
pub struct FeatureGuard<'p, 'a> {
    progress: &'p Progress<'a>,
    id: u64,
}
impl FeatureGuard<'_, '_> {
    /// The worker reached feature `index` (from 1). Cheap: events for feature changes alone
    /// are limited to four a second, and the heartbeat shows the latest one.
    pub fn at(&self, index: u64, name: Option<&str>) {
        if let Ok(mut features) = self.progress.features.lock() {
            if let Some(active) = features.iter_mut().find(|f| f.id == self.id) {
                active.feature.index = index;
                active.feature.name = name.map(display_name);
                active.feature.measured = false;
            }
        }
        self.progress.pending.store(true, Ordering::Relaxed);
        self.progress.flush_feature();
    }
}
impl Drop for FeatureGuard<'_, '_> {
    fn drop(&mut self) {
        if let Ok(mut features) = self.progress.features.lock() {
            features.retain(|f| f.id != self.id);
        }
        self.progress.pending.store(false, Ordering::Relaxed);
        self.progress.emit(None, false, false);
    }
}

/// Marks this worker's current feature as checked by measurement until dropped.
pub struct MeasureGuard<'p, 'a> {
    progress: &'p Progress<'a>,
    id: Option<u64>,
}
impl Drop for MeasureGuard<'_, '_> {
    fn drop(&mut self) {
        if let Some(id) = self.id {
            self.progress.set_measured(id, false);
        }
    }
}
struct ActiveStep {
    id: u64,
    #[cfg(not(target_family = "wasm"))]
    thread: std::thread::ThreadId,
    started: Instant,
    step: RunningStep,
}
fn same_worker(a: &ActiveStep, b: &ActiveStep) -> bool {
    #[cfg(not(target_family = "wasm"))]
    {
        a.thread == b.thread
    }
    #[cfg(target_family = "wasm")]
    {
        let _ = (a, b);
        true
    }
}
/// Ends a step on every return path, including unwinding.
pub struct StepGuard<'p, 'a> {
    progress: &'p Progress<'a>,
    id: u64,
}
impl Drop for StepGuard<'_, '_> {
    fn drop(&mut self) {
        let passed_limit = self.progress.steps.lock().is_ok_and(|steps| {
            steps.iter().any(|s| {
                s.id == self.id
                    && s.step
                        .limit_ms
                        .is_some_and(|limit| s.started.elapsed().as_millis() >= u128::from(limit))
            })
        });
        // Preserve a timeout consequence even when the operation ends between heartbeats.
        if passed_limit {
            self.progress.emit(None, false, false);
        }
        if let Ok(mut steps) = self.progress.steps.lock() {
            steps.retain(|s| s.id != self.id);
        }
        self.progress.emit(None, false, false);
    }
}

/// `seconds` as one duration: "45 s", "2 min 5 s" or "2 h 1 min".
/// Below an hour, retain seconds; from an hour on, retain minutes.
pub fn elapsed(seconds: u64) -> String {
    let (hours, minutes, rest) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if seconds < 60 {
        format!("{seconds} s")
    } else if hours == 0 {
        if rest == 0 {
            format!("{minutes} min")
        } else {
            format!("{minutes} min {rest} s")
        }
    } else if minutes == 0 {
        format!("{hours} h")
    } else {
        format!("{hours} h {minutes} min")
    }
}

/// Decoder member activity, including repeated reads of the same member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberEvent {
    Start,
    Finish,
    Fail,
}

/// Optional diagnostics sink. Paths are private decoder identities; the sink must hash
/// them before publishing. Codes are fixed categories, never decoder messages.
pub trait MemberObserver: Send + Sync {
    /// Private, in-memory snapshot for local reproduction. Never publish the model.
    fn model(&self, _model: &crate::Model) {}

    /// Fixed phase categories only. Bytes are the source container length, not inflated size.
    fn phase_event(&self, _phase: &'static str, _event: MemberEvent, _ms: u64, _bytes: u64) {}

    /// Preparation timings in milliseconds. Paths remain private until hashed by the sink.
    fn member_classified(
        &self,
        _path: &str,
        _ms: u64,
        _nested_scan_ms: u64,
        _layers_ms: u64,
        _metadata_ms: u64,
    ) {
    }

    fn member_event(&self, path: &str, event: MemberEvent, ms: u64, code: Option<&'static str>);
}

/// A thread-safe reporter, shared by all document readers of one conversion.
/// Callbacks run in order, must return promptly, and must not re-enter this reporter.
pub struct Progress<'a> {
    pub(crate) members: Option<Arc<dyn MemberObserver>>,
    started: Instant,
    stage: Mutex<Stage>,
    stage_started: Mutex<Instant>,
    steps: Mutex<Vec<ActiveStep>>,
    next_step: AtomicU64,
    stage_usual: Vec<(StepKind, u64)>,
    read: AtomicU64,
    total: AtomicU64,
    features: Mutex<Vec<ActiveFeature>>,
    feature_emitted: Mutex<Option<Instant>>,
    #[cfg(test)]
    feature_now: Mutex<Option<Instant>>,
    /// A feature change not yet emitted, flushed by the heartbeat thread.
    pending: AtomicBool,
    callback: &'a (dyn Fn(ProgressEvent) + Send + Sync),
}

impl<'a> Progress<'a> {
    pub fn new(callback: &'a (dyn Fn(ProgressEvent) + Send + Sync)) -> Self {
        Self {
            members: None,
            started: Instant::now(),
            stage: Mutex::new(Stage::Input),
            stage_started: Mutex::new(Instant::now()),
            steps: Mutex::new(Vec::new()),
            next_step: AtomicU64::new(0),
            stage_usual: Vec::new(),
            read: AtomicU64::new(0),
            total: AtomicU64::new(u64::MAX),
            features: Mutex::new(Vec::new()),
            feature_emitted: Mutex::new(None),
            #[cfg(test)]
            feature_now: Mutex::new(None),
            pending: AtomicBool::new(false),
            callback,
        }
    }

    /// Whole-stage estimates apply to whole-design work, never individual assembly parts.
    pub fn with_stage_estimates(mut self, estimates: &[(StepKind, u64)]) -> Self {
        self.stage_usual = estimates.to_vec();
        self
    }

    pub fn with_members(mut self, members: Option<Arc<dyn MemberObserver>>) -> Self {
        self.members = members;
        self
    }

    pub fn step(
        &self,
        kind: StepKind,
        name: Option<&str>,
        limit: Option<Duration>,
        consequence: Option<&str>,
    ) -> StepGuard<'_, 'a> {
        self.start_step(kind, name, limit, consequence, kind.usual_ms())
    }

    /// Whole-design work may carry its name while using the existing stage estimate.
    /// Separate assembly members retain their own per-design threshold.
    pub fn stage_step(&self, kind: StepKind, name: Option<&str>) -> StepGuard<'_, 'a> {
        let usual_ms = if kind == StepKind::Reading && self.read.load(Ordering::Relaxed) > 0 {
            kind.usual_ms()
        } else {
            self.stage_usual(kind)
        };
        self.start_step(kind, name, None, None, usual_ms)
    }

    fn stage_usual(&self, kind: StepKind) -> u64 {
        self.stage_usual
            .iter()
            .find(|(candidate, _)| *candidate == kind)
            .map_or_else(|| kind.usual_ms(), |(_, ms)| *ms)
    }

    fn start_step(
        &self,
        kind: StepKind,
        name: Option<&str>,
        limit: Option<Duration>,
        consequence: Option<&str>,
        usual_ms: u64,
    ) -> StepGuard<'_, 'a> {
        let id = self.next_step.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut steps) = self.steps.lock() {
            steps.push(ActiveStep {
                id,
                #[cfg(not(target_family = "wasm"))]
                thread: std::thread::current().id(),
                started: Instant::now(),
                step: RunningStep {
                    kind,
                    name: name.map(display_name),
                    elapsed_ms: 0,
                    usual_ms,
                    limit_ms: limit.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                    consequence: consequence.map(str::to_owned),
                    feature: None,
                },
            });
        }
        self.emit(None, false, false);
        StepGuard { progress: self, id }
    }

    pub fn stage(&self, stage: Stage) {
        self.emit(Some(stage), false, false);
    }

    /// Starts going through a timeline of `total` features on this worker; see
    /// [`FeatureGuard::at`]. Shown with the worker's running step until the guard ends.
    pub fn features(&self, total: u64) -> FeatureGuard<'_, 'a> {
        let id = self.next_step.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut features) = self.features.lock() {
            features.push(ActiveFeature {
                id,
                worker: current_worker(),
                feature: FeatureStep {
                    index: 0,
                    total,
                    name: None,
                    measured: false,
                },
            });
        }
        FeatureGuard { progress: self, id }
    }

    /// Marks the feature this worker is on as checked by measurement, at once, until the
    /// guard ends. Does nothing outside a timeline.
    pub fn measuring(&self) -> MeasureGuard<'_, 'a> {
        let worker = current_worker();
        let id = self.features.lock().ok().and_then(|features| {
            features
                .iter()
                .filter(|f| f.worker == worker)
                .map(|f| f.id)
                .max()
        });
        if let Some(id) = id {
            self.set_measured(id, true);
        }
        MeasureGuard { progress: self, id }
    }

    /// Tests may freeze the scheduling clock without changing elapsed-time reporting.
    fn feature_time(&self) -> Instant {
        #[cfg(test)]
        if let Ok(now) = self.feature_now.lock() {
            if let Some(now) = *now {
                return now;
            }
        }
        Instant::now()
    }

    /// Use one clock for immediate updates and timer flushes, so a flushed update cannot
    /// be followed immediately by another feature-only event.
    fn flush_feature(&self) {
        let due = self
            .feature_emitted
            .lock()
            .map(|mut last| {
                let now = self.feature_time();
                if last.is_none_or(|at| now.duration_since(at) >= FEATURE_INTERVAL)
                    && self.pending.swap(false, Ordering::Relaxed)
                {
                    *last = Some(now);
                    true
                } else {
                    false
                }
            })
            .unwrap_or_else(|_| self.pending.swap(false, Ordering::Relaxed));
        if due {
            self.emit(None, false, false);
        }
    }

    fn set_measured(&self, id: u64, measured: bool) {
        if let Ok(mut features) = self.features.lock() {
            if let Some(active) = features.iter_mut().find(|f| f.id == id) {
                active.feature.measured = measured;
            }
        }
        self.pending.store(false, Ordering::Relaxed);
        self.emit(None, false, false);
    }

    /// The innermost timeline on `worker`, or on any worker for a whole-stage snapshot.
    fn feature_on(&self, worker: Option<Worker>) -> Option<FeatureStep> {
        let features = self.features.lock().ok()?;
        features
            .iter()
            .filter(|f| worker.is_none_or(|worker| f.worker == worker) && f.feature.index > 0)
            .max_by_key(|f| f.id)
            .map(|f| f.feature.clone())
    }

    /// Set the number of unique designs once discovery is complete. Do not count placements.
    pub fn designs_total(&self, total: u64) {
        self.total.store(total, Ordering::Relaxed);
        self.emit(None, false, false);
    }

    /// Call once after each unique design has been read successfully, on any reader thread.
    pub fn design_read(&self) {
        self.emit(None, true, false);
    }

    pub fn heartbeat(&self) {
        self.emit(None, false, true);
    }

    fn emit(&self, next: Option<Stage>, completed: bool, heartbeat: bool) {
        // Serialize snapshots and delivery, including concurrent document completions.
        if let Ok(mut stage) = self.stage.lock() {
            if let Some(next) = next {
                *stage = next;
                if let Ok(mut started) = self.stage_started.lock() {
                    *started = Instant::now();
                }
            }
            // Counted under the lock, so a load and a store cannot lose a completion.
            if completed {
                let read = self.read.load(Ordering::Relaxed);
                self.read.store(read.saturating_add(1), Ordering::Relaxed);
            }
            let total = self.total.load(Ordering::Relaxed);
            (self.callback)(ProgressEvent {
                stage: *stage,
                elapsed_ms: u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
                designs_read: self.read.load(Ordering::Relaxed),
                designs_total: (total != u64::MAX).then_some(total),
                heartbeat,
                running: self
                    .steps
                    .lock()
                    .ok()
                    .and_then(|steps| {
                        // Only the innermost step on each worker is currently running.
                        steps
                            .iter()
                            .filter(|s| {
                                !steps
                                    .iter()
                                    .any(|other| same_worker(other, s) && other.id > s.id)
                            })
                            .min_by_key(|s| s.started)
                            .map(|s| {
                                let mut step = s.step.clone();
                                step.elapsed_ms = u64::try_from(s.started.elapsed().as_millis())
                                    .unwrap_or(u64::MAX);
                                step.feature = self.feature_on(Some(step_worker(s)));
                                step
                            })
                    })
                    .or_else(|| {
                        Some(RunningStep {
                            kind: StepKind::stage(*stage),
                            name: None,
                            elapsed_ms: self
                                .stage_started
                                .lock()
                                .map(|s| u64::try_from(s.elapsed().as_millis()).unwrap_or(u64::MAX))
                                .unwrap_or(0),
                            usual_ms: self.stage_usual(StepKind::stage(*stage)),
                            limit_ms: None,
                            consequence: None,
                            feature: self.feature_on(None),
                        })
                    }),
            });
        }
    }

    /// Run work with a five-second heartbeat. Stopping wakes the timer immediately;
    /// its thread is joined before returning, also when work unwinds.
    pub fn run<T>(&self, work: impl FnOnce() -> T) -> T {
        self.run_every(Duration::from_secs(5), work)
    }

    #[cfg(not(target_family = "wasm"))]
    fn timer_tick(&self, beat: &mut Instant, interval: Duration) {
        let now = self.feature_time();
        if now.duration_since(*beat) >= interval {
            *beat = now;
            self.pending.store(false, Ordering::Relaxed);
            self.heartbeat();
        } else {
            self.flush_feature();
        }
    }

    /// Periodically flushes feature changes that were not emitted at once.
    #[cfg(not(target_family = "wasm"))]
    fn run_every<T>(&self, interval: Duration, work: impl FnOnce() -> T) -> T {
        std::thread::scope(|scope| {
            let (stop, receive) = mpsc::channel::<()>();
            scope.spawn(move || {
                let mut beat = Instant::now();
                while matches!(
                    receive.recv_timeout(interval.min(FEATURE_INTERVAL)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    self.timer_tick(&mut beat, interval);
                }
            });
            let _stop = stop;
            work()
        })
    }
}

#[cfg(target_family = "wasm")]
impl Progress<'_> {
    fn run_every<T>(&self, _interval: Duration, work: impl FnOnce() -> T) -> T {
        work()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concurrent_completions_are_ordered_and_totals_are_explicit() {
        let events = Mutex::new(Vec::new());
        let callback = |event| events.lock().unwrap().push(event);
        let progress = Progress::new(&callback);
        progress.stage(Stage::Decode);
        progress.designs_total(8);
        std::thread::scope(|scope| {
            for _ in 0..8 {
                scope.spawn(|| progress.design_read());
            }
        });
        progress.stage(Stage::Map);
        let events = events.into_inner().unwrap();
        assert_eq!(events[0].designs_total, None);
        for (index, event) in events[2..10].iter().enumerate() {
            assert_eq!(event.designs_read, index as u64 + 1);
            assert_eq!(event.designs_total, Some(8));
            assert_eq!(event.stage, Stage::Decode);
        }
        assert_eq!(events.last().unwrap().stage, Stage::Map);
        assert!(events
            .windows(2)
            .all(|pair| pair[0].elapsed_ms <= pair[1].elapsed_ms));
    }

    #[test]
    fn timer_emits_while_work_is_blocked_and_stops_before_return() {
        let (send, receive) = mpsc::channel();
        let callback = |event| {
            send.send(event).unwrap();
        };
        let progress = Progress::new(&callback);
        progress.stage(Stage::Decode);
        receive.recv().unwrap();
        progress.run_every(Duration::from_millis(10), || {
            let event = receive.recv_timeout(Duration::from_secs(30)).unwrap();
            assert!(event.heartbeat);
            assert_eq!(event.stage, Stage::Decode);
            assert!(event.elapsed_ms >= 10);
            assert_eq!(event.designs_total, None);
        });
        while receive.try_recv().is_ok() {}
        assert!(receive.recv_timeout(Duration::from_millis(30)).is_err());
    }
}

#[cfg(test)]
mod elapsed_tests {
    use super::elapsed;

    #[test]
    fn durations_have_one_format_and_omit_zero_units() {
        for (seconds, text) in [
            (0, "0 s"),
            (45, "45 s"),
            (59, "59 s"),
            (60, "1 min"),
            (61, "1 min 1 s"),
            (120, "2 min"),
            (125, "2 min 5 s"),
            (151, "2 min 31 s"),
            (3599, "59 min 59 s"),
            (3600, "1 h"),
            (3601, "1 h"),
            (3900, "1 h 5 min"),
            (7261, "2 h 1 min"),
        ] {
            assert_eq!(elapsed(seconds), text);
        }
    }
}

#[cfg(test)]
mod running_tests {
    use super::*;

    #[test]
    fn stage_estimates_do_not_become_each_parts_usual_duration() {
        let saved = Mutex::new(None);
        let callback = |event| *saved.lock().unwrap() = Some(event);
        let progress = Progress::new(&callback)
            .with_stage_estimates(&[(StepKind::Preparing, 240_000), (StepKind::Reading, 90_000)]);
        let usual = || {
            saved
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .running
                .as_ref()
                .unwrap()
                .usual_ms
        };
        progress.stage(Stage::Map);
        assert_eq!(usual(), 240_000);
        let whole = progress.stage_step(StepKind::Preparing, Some("Assembly"));
        assert_eq!(usual(), 240_000);
        let part = progress.step(StepKind::Preparing, Some("Bracket"), None, None);
        assert_eq!(usual(), 30_000);
        drop(part);
        drop(whole);
        let root = progress.stage_step(StepKind::Reading, Some("Assembly.iam"));
        assert_eq!(usual(), 90_000);
        drop(root);
        progress.design_read();
        let _member = progress.stage_step(StepKind::Reading, Some("Bracket.ipt"));
        assert_eq!(usual(), 30_000);
    }

    #[test]
    fn heartbeat_identifies_a_slow_part_and_guard_finishes_it() {
        let (send, receive) = mpsc::channel();
        let callback = |event| {
            send.send(event).unwrap();
        };
        let progress = Progress::new(&callback);
        let step = progress.step(StepKind::Preparing, Some("Bracket"), None, None);
        receive.recv().unwrap();
        progress.run_every(Duration::from_millis(15), || {
            let event = receive.recv_timeout(Duration::from_secs(30)).unwrap();
            assert!(event.heartbeat);
            let running = event.running.as_ref().unwrap();
            assert_eq!(running.name.as_deref(), Some("Bracket"));
            assert_eq!(running.kind, StepKind::Preparing);
            assert!(running.elapsed_ms >= 15);
            assert!(event.to_string().contains("Building history for Bracket"));
        });
        drop(step);
        assert!(receive
            .try_iter()
            .last()
            .unwrap()
            .running
            .unwrap()
            .name
            .is_none());
    }

    #[test]
    fn longest_current_worker_wins_and_nested_check_replaces_preparation() {
        let events = Mutex::new(Vec::new());
        let callback = |event| events.lock().unwrap().push(event);
        let progress = Progress::new(&callback);
        let _outer = progress.step(StepKind::Preparing, Some("Bracket"), None, None);
        let check = progress.step(StepKind::Checking, Some("Bracket"), None, None);
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _newer = progress.step(StepKind::Preparing, Some("Pin"), None, None);
                    progress.heartbeat();
                    let events = events.lock().unwrap();
                    let current = events.last().unwrap().running.as_ref().unwrap();
                    assert_eq!(current.kind, StepKind::Checking);
                    assert_eq!(current.name.as_deref(), Some("Bracket"));
                })
                .join()
                .unwrap();
        });
        drop(check);
        assert_eq!(
            events
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .running
                .as_ref()
                .unwrap()
                .kind,
            StepKind::Preparing
        );
    }

    #[test]
    fn timer_flush_and_immediate_features_share_the_rate_limit() {
        let events = Mutex::new(Vec::new());
        let callback = |event: ProgressEvent| events.lock().unwrap().push(event);
        let progress = Progress::new(&callback);
        let now = Instant::now();
        *progress.feature_now.lock().unwrap() = Some(now);
        let _step = progress.step(StepKind::Preparing, None, None, None);
        let timeline = progress.features(3);
        timeline.at(1, Some("Sketch"));
        timeline.at(2, Some("Extrude"));
        let before = events.lock().unwrap().len();
        // Make the timer flush due without waiting for wall-clock time.
        *progress.feature_now.lock().unwrap() = Some(now + FEATURE_INTERVAL);
        let mut beat = now;
        progress.timer_tick(&mut beat, Duration::from_secs(5));
        assert_eq!(events.lock().unwrap().len(), before + 1);
        timeline.at(3, Some("Fillet"));
        assert_eq!(events.lock().unwrap().len(), before + 1);
        assert!(progress.pending.load(Ordering::Relaxed));
    }

    #[test]
    fn features_follow_their_worker_are_limited_and_flushed_by_the_timer() {
        let events = Mutex::new(Vec::new());
        let callback = |event: ProgressEvent| events.lock().unwrap().push(event);
        let progress = Progress::new(&callback);
        let now = Instant::now();
        *progress.feature_now.lock().unwrap() = Some(now);
        let feature = |events: &Mutex<Vec<ProgressEvent>>| {
            events
                .lock()
                .unwrap()
                .last()
                .unwrap()
                .running
                .as_ref()
                .unwrap()
                .feature
                .clone()
        };
        let _part = progress.step(StepKind::Preparing, Some("Bracket"), None, None);
        let timeline = progress.features(37);
        assert_eq!(feature(&events), None);
        timeline.at(1, Some("Sketch\n1"));
        let first = feature(&events).unwrap();
        assert_eq!((first.index, first.total), (1, 37));
        assert_eq!(first.name.as_deref(), Some("Sketch 1"));
        // A thousand fast features do not each print a line.
        let before = events.lock().unwrap().len();
        for index in 2..=1000 {
            timeline.at(index, Some("Extrude 1"));
        }
        assert_eq!(events.lock().unwrap().len(), before);
        timeline.at(29, Some("Extrude 1"));
        let mut beat = now;
        let interval = Duration::from_millis(400);
        // A tick just before the feature interval must leave the update pending.
        *progress.feature_now.lock().unwrap() =
            Some(now + FEATURE_INTERVAL - Duration::from_nanos(1));
        progress.timer_tick(&mut beat, interval);
        assert_eq!(events.lock().unwrap().len(), before);
        assert!(progress.pending.load(Ordering::Relaxed));
        // The first due timer tick flushes the latest feature before a heartbeat is due.
        *progress.feature_now.lock().unwrap() = Some(now + FEATURE_INTERVAL);
        progress.timer_tick(&mut beat, interval);
        assert_eq!(feature(&events).unwrap().index, 29);
        assert_eq!(events.lock().unwrap().len(), before + 1);
        assert!(!events.lock().unwrap().last().unwrap().heartbeat);
        // The next tick is a heartbeat and retains the worker's current feature.
        *progress.feature_now.lock().unwrap() = Some(now + interval);
        progress.timer_tick(&mut beat, interval);
        assert!(events.lock().unwrap().last().unwrap().heartbeat);
        assert_eq!(feature(&events).unwrap().index, 29);
        {
            let _measuring = progress.measuring();
            assert!(feature(&events).unwrap().measured);
            // Another worker's step never shows this worker's feature.
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        let _other = progress.step(StepKind::Preparing, Some("Pin"), None, None);
                        assert!(progress.feature_on(Some(current_worker())).is_none());
                    })
                    .join()
                    .unwrap();
            });
        }
        assert!(!feature(&events).unwrap().measured);
        drop(timeline);
        assert_eq!(feature(&events), None);
        // Outside a timeline, measuring changes nothing.
        let _measuring = progress.measuring();
        assert_eq!(feature(&events), None);
    }
}
