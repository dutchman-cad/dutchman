//! Coarse conversion progress. No work is done per geometry entity.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::Stage;

/// A snapshot of one conversion, independent of its final report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgressEvent {
    pub stage: Stage,
    pub elapsed_ms: u64,
    pub designs_read: u64,
    /// Unknown when the decoder owns the document loop or discovery is still running.
    pub designs_total: Option<u64>,
    pub heartbeat: bool,
}

impl std::fmt::Display for ProgressEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stage = match self.stage {
            Stage::Input => "Opening input",
            Stage::Decode => "Reading geometry and history",
            Stage::Project => "Checking the decoded model",
            Stage::Map => "Building and checking the output design",
            Stage::Write => "Saving the output",
            Stage::SolidCraft => "Checking in SolidCraft",
        };
        write!(f, "{stage}")?;
        if let Some(total) = self.designs_total {
            write!(f, "; {} of {total} designs read", self.designs_read)?;
        } else if self.designs_read > 0 {
            write!(f, "; {} designs read", self.designs_read)?;
        }
        write!(f, "; {} elapsed", elapsed(self.elapsed_ms / 1000))?;
        if self.heartbeat {
            write!(f, "; still working")?;
        }
        Ok(())
    }
}

/// `seconds` as people read it: the seconds, and past a minute also hours and minutes, as
/// "45 s", "125 s (2 min 5 s)" or "7261 s (2 h 1 min)".
pub fn elapsed(seconds: u64) -> String {
    let (hours, minutes, rest) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    if seconds < 60 {
        format!("{seconds} s")
    } else if hours == 0 {
        format!("{seconds} s ({minutes} min {rest} s)")
    } else {
        format!("{seconds} s ({hours} h {minutes} min)")
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
    /// Fixed phase categories only. Bytes are the source container length, not inflated size.
    fn phase_event(&self, _phase: &'static str, _event: MemberEvent, _ms: u64, _bytes: u64) {}

    fn member_event(&self, path: &str, event: MemberEvent, ms: u64, code: Option<&'static str>);
}

/// A thread-safe reporter, shared by all document readers of one conversion.
/// Callbacks run in order, must return promptly, and must not re-enter this reporter.
pub struct Progress<'a> {
    pub(crate) members: Option<Arc<dyn MemberObserver>>,
    started: Instant,
    stage: Mutex<Stage>,
    read: AtomicU64,
    total: AtomicU64,
    callback: &'a (dyn Fn(ProgressEvent) + Send + Sync),
}

impl<'a> Progress<'a> {
    pub fn new(callback: &'a (dyn Fn(ProgressEvent) + Send + Sync)) -> Self {
        Self {
            members: None,
            started: Instant::now(),
            stage: Mutex::new(Stage::Input),
            read: AtomicU64::new(0),
            total: AtomicU64::new(u64::MAX),
            callback,
        }
    }

    pub fn with_members(mut self, members: Option<Arc<dyn MemberObserver>>) -> Self {
        self.members = members;
        self
    }

    pub fn stage(&self, stage: Stage) {
        self.emit(Some(stage), false, false);
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
            });
        }
    }

    /// Run work with a five-second heartbeat. Stopping wakes the timer immediately;
    /// its thread is joined before returning, also when work unwinds.
    pub fn run<T>(&self, work: impl FnOnce() -> T) -> T {
        self.run_every(Duration::from_secs(5), work)
    }

    fn run_every<T>(&self, interval: Duration, work: impl FnOnce() -> T) -> T {
        std::thread::scope(|scope| {
            let (stop, receive) = mpsc::channel::<()>();
            scope.spawn(move || {
                while matches!(
                    receive.recv_timeout(interval),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    self.heartbeat();
                }
            });
            let _stop = stop;
            work()
        })
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
            let event = receive.recv_timeout(Duration::from_secs(2)).unwrap();
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
    fn long_runs_also_read_in_hours_and_minutes() {
        assert_eq!(elapsed(45), "45 s");
        assert_eq!(elapsed(125), "125 s (2 min 5 s)");
        assert_eq!(elapsed(7261), "7261 s (2 h 1 min)");
        assert_eq!(elapsed(3600), "3600 s (1 h 0 min)");
    }
}
