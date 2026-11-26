//! Middle-out scheduling for a batch: the largest files start first, and a file starts only
//! when its predicted peak memory fits what is free.
//!
//! Big files started early keep every core busy to the end, and small files fill the gaps
//! beside them. Only the order files start in changes; reports, the journal and printed
//! output stay in input order.

use std::path::Path;
use std::time::{Duration, Instant};

use dutchman_core::Format;

/// How a file is predicted to behave: `work` orders files (largest first), `peak` is the
/// most memory its conversion is expected to hold at once, in bytes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cost {
    /// Predicted conversion effort, comparable between files.
    pub work: u64,
    /// Predicted peak memory in bytes.
    pub peak: u64,
    /// Assemblies and multi-document archives start before every other file.
    pub first: bool,
}

/// What one format costs per byte of input, and what it holds at least and at most.
struct Factors {
    /// Effort per input byte, in thousandths.
    work_milli: u64,
    /// Peak memory per input byte, in thousandths.
    peak_milli: u64,
    /// Peak memory of the smallest file of the format.
    floor: u64,
    /// The most a file of the format has been measured to hold.
    cap: u64,
}

const MIB: u64 = 1 << 20;
const GIB: u64 = 1 << 30;

/// The fitted table: predicted peak = `floor + size x peak_milli / 1000`, at most `cap`.
///
/// Source: per-file peak memory (cgroup `memory.peak` of one `dutchman convert-all` run per
/// file, release build of main c73884ca, SolidCraft 0.48.0, aurora, 2026-11-24) over the
/// public 50 set and three public RailCore II .f3z files. Measured points (input size, peak):
/// .f3d 0.07 MB 70 MB; 0.65 MB 683 MB; 2.09 MB 593 MB; 2.87 MB 1.37 GB; 5.84 MB 850 MB;
/// 20.7 MB 3.2 GB (p90 of the 45 .f3d files about 0.5 GB). .f3z 5.95 MB 1.8 GB; 13.2 MB 4.0 GB;
/// 13.3 MB 4.2 GB; 13.8 MB 5.9 GB; and about 10 GB for the 65 MB Voron master model.
/// The lines run through those points (not above them): a line above them kept three
/// RailCore archives, 12 GB together, from running side by side in a 20 GB scope. The quarter
/// of free memory kept back absorbs the difference. A prediction never passes `cap`, the most
/// a file of the format has been seen to hold. Formats not measured yet use the .f3d line,
/// the one Dutchman converts most; refit when they are.
fn factors(format: Option<Format>) -> Factors {
    match format {
        Some(Format::FusionMultiDocument) => Factors {
            work_milli: 1_000,
            peak_milli: 300_000,
            floor: 256 * MIB,
            cap: 10 * GIB,
        },
        _ => Factors {
            work_milli: 1_000,
            peak_milli: 150_000,
            floor: 128 * MIB,
            cap: 4 * GIB,
        },
    }
}

/// Whether the format starts first: it holds its geometry in many documents or parts.
fn starts_first(format: Option<Format>) -> bool {
    format.is_some_and(|f| f.is_assembly() || f == Format::FusionMultiDocument)
}

/// The bytes a job's input holds: a file's size, a folder's files (one level down).
pub(crate) fn input_bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    std::fs::read_dir(path).map_or(0, |entries| {
        entries
            .flatten()
            .filter_map(|entry| entry.metadata().ok())
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len())
            .sum()
    })
}

/// The predicted cost of converting `input`.
pub(crate) fn predict(input: &Path) -> Cost {
    cost_of(Format::from_path(input), input_bytes(input))
}

/// The predicted cost of a file of `format` holding `bytes`.
pub(crate) fn cost_of(format: Option<Format>, bytes: u64) -> Cost {
    let f = factors(format);
    Cost {
        work: bytes.saturating_mul(f.work_milli) / 1000,
        peak: (f.floor + bytes.saturating_mul(f.peak_milli) / 1000).min(f.cap),
        first: starts_first(format),
    }
}

/// The order files start in: first-class files, then by predicted work, largest first.
/// Equal costs keep input order.
pub(crate) fn order(costs: &[Cost]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..costs.len()).collect();
    order.sort_by(|&a, &b| {
        let (a_cost, b_cost) = (
            costs.get(a).copied().unwrap_or_default(),
            costs.get(b).copied().unwrap_or_default(),
        );
        b_cost
            .first
            .cmp(&a_cost.first)
            .then(b_cost.work.cmp(&a_cost.work))
            .then(a.cmp(&b))
    });
    order
}

/// The least free memory kept back from files started alongside others. This is the
/// margin `dutchman_core::parallel::workers` keeps for part workers: a quarter of free
/// memory, at least 1 GiB.
fn margin(free: u64) -> u64 {
    (free / 4).max(1 << 30)
}

fn peak_of(costs: &[Cost], index: usize) -> u64 {
    costs.get(index).map_or(0, |cost| cost.peak)
}

/// What the scheduler asks of a worker.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Take {
    /// Convert the job at this index now.
    Job(usize),
    /// Nothing fits yet: ask again when a file finishes or memory changes.
    Wait,
    /// Every file has started.
    Done,
}

/// Which files wait, which run, and what the running ones are still expected to take.
pub(crate) struct Queue {
    order: Vec<usize>,
    started: Vec<bool>,
    running: Vec<(usize, Instant)>,
    /// Whether to hold files back for memory; off for a single worker.
    admit: bool,
    /// How long a started file is still counted as growing toward its predicted peak, for
    /// each GiB it is predicted to reach: a big file keeps growing longer than a small one.
    ramp: Duration,
}

impl Queue {
    /// Largest first, admitted by memory.
    pub(crate) fn middle_out(costs: &[Cost], ramp: Duration) -> Self {
        Self::with(order(costs), costs.len(), true, ramp)
    }

    /// Input order, no holding back: what one worker has always done.
    pub(crate) fn in_order(len: usize) -> Self {
        Self::with((0..len).collect(), len, false, Duration::ZERO)
    }

    fn with(order: Vec<usize>, len: usize, admit: bool, ramp: Duration) -> Self {
        Self {
            order,
            started: vec![false; len],
            running: Vec::new(),
            admit,
            ramp,
        }
    }

    /// The next file to start, given a fresh reading of free memory (`None`: unknown, so
    /// only the worker count limits). The first file that fits starts, so a small file
    /// fills the gap a big one does not fit in. Nothing running always admits the first
    /// file left, however large: a file bigger than all memory runs alone.
    pub(crate) fn take(&mut self, costs: &[Cost], free: Option<u64>, now: Instant) -> Take {
        let started = &self.started;
        let mut waiting = self
            .order
            .iter()
            .copied()
            .filter(|&i| !started.get(i).copied().unwrap_or(true));
        let Some(head) = waiting.next() else {
            return Take::Done;
        };
        let pick = if !self.admit || self.running.is_empty() {
            Some(head)
        } else if let Some(free) = free {
            // Files just started have not yet reached their peak: keep that room for them.
            let reserved: u64 = self
                .running
                .iter()
                .map(|(i, since)| (peak_of(costs, *i), *since))
                .filter(|(peak, since)| {
                    let gib = u32::try_from((peak / GIB).max(1)).unwrap_or(u32::MAX);
                    now.saturating_duration_since(*since) < self.ramp.saturating_mul(gib)
                })
                .map(|(peak, _)| peak)
                .sum();
            let budget = free.saturating_sub(margin(free)).saturating_sub(reserved);
            std::iter::once(head)
                .chain(waiting)
                .find(|&i| peak_of(costs, i) <= budget)
        } else {
            Some(head)
        };
        match pick {
            Some(index) => {
                if let Some(slot) = self.started.get_mut(index) {
                    *slot = true;
                }
                self.running.push((index, now));
                Take::Job(index)
            }
            None => Take::Wait,
        }
    }

    /// The file at `index` is no longer running.
    pub(crate) fn finish(&mut self, index: usize) {
        self.running.retain(|(i, _)| *i != index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cost(work: u64, peak: u64, first: bool) -> Cost {
        Cost { work, peak, first }
    }

    #[test]
    fn largest_first_with_assemblies_and_archives_ahead_and_ties_in_input_order() {
        let costs = [
            cost(5, 0, false),
            cost(9, 0, false),
            cost(1, 0, true),
            cost(5, 0, false),
            cost(3, 0, true),
        ];
        assert_eq!(order(&costs), [4, 2, 1, 0, 3]);
    }

    #[test]
    fn archives_and_assemblies_are_first_class_and_bigger_files_cost_more() {
        assert!(cost_of(Some(Format::FusionMultiDocument), 1).first);
        assert!(cost_of(Some(Format::InventorAssembly), 1).first);
        assert!(!cost_of(Some(Format::FusionArchive), 1).first);
        let small = cost_of(Some(Format::FusionArchive), 1 << 20);
        let big = cost_of(Some(Format::FusionArchive), 50 << 20);
        assert!(big.work > small.work && big.peak >= small.peak);
    }

    #[test]
    fn predicted_memory_follows_the_measured_points_and_stops_at_the_cap() {
        let f3d = |bytes: u64| cost_of(Some(Format::FusionArchive), bytes).peak;
        let f3z = |bytes: u64| cost_of(Some(Format::FusionMultiDocument), bytes).peak;
        let mb = |n: u64| n * 1_000_000;
        // The 20.7 MB file held 3.2 GB; a small file is near the floor.
        assert!(f3d(20_700_000) > 5 * GIB / 2 && f3d(20_700_000) <= 4 * GIB);
        assert!(f3d(70_000) < GIB / 4);
        // The 13.8 MB archives held 4 to 5.9 GB; the 65 MB one about 10 GB, the cap.
        assert!(f3z(13_800_000) > 4 * GIB && f3z(13_800_000) < 6 * GIB);
        assert_eq!(f3z(mb(65)), 10 * GIB);
        // Three RailCore archives fit a 20 GB scope's budget together, as they ran.
        let budget = 20 * GIB - margin(20 * GIB);
        assert!(3 * f3z(13_800_000) <= budget);
    }

    /// Starts files as a worker would, one reading of free memory each time.
    fn drain(costs: &[Cost], free: u64, ramp: Duration) -> Vec<usize> {
        let mut queue = Queue::middle_out(costs, ramp);
        let now = Instant::now();
        let mut started = Vec::new();
        while let Take::Job(i) = queue.take(costs, Some(free), now) {
            started.push(i);
        }
        started
    }

    #[test]
    fn admission_never_starts_more_than_fits_free_memory_less_the_margin() {
        // 16 GiB free: a quarter is kept back, 12 GiB can start. Each file predicts 5 GiB.
        let costs = [cost(1, 5 * GIB, false); 6];
        let started = drain(&costs, 16 * GIB, Duration::from_secs(60));
        // The first runs alone-admitted; the second fits 12 - 5 = 7; the third 12 - 10 = 2: no.
        assert_eq!(started.len(), 2);
        let total: u64 = started.iter().map(|&i| costs[i].peak).sum();
        assert!(total <= 16 * GIB - margin(16 * GIB));
    }

    #[test]
    fn small_files_fill_the_gap_a_big_file_does_not_fit_in() {
        // Big (6 GiB) first, then a second big one that no longer fits, then small ones.
        let costs = [
            cost(9, 6 * GIB, false),
            cost(8, 6 * GIB, false),
            cost(1, GIB / 4, false),
            cost(1, GIB / 4, false),
        ];
        let started = drain(&costs, 12 * GIB, Duration::from_secs(60));
        assert_eq!(started, [0, 2, 3]);
    }

    #[test]
    fn a_file_larger_than_all_memory_runs_alone() {
        let costs = [cost(9, 100 * GIB, false), cost(1, GIB / 4, false)];
        let mut queue = Queue::middle_out(&costs, Duration::from_secs(60));
        let now = Instant::now();
        assert_eq!(queue.take(&costs, Some(8 * GIB), now), Take::Job(0));
        // Nothing else joins it, and the one that waits starts only once it is done.
        assert_eq!(queue.take(&costs, Some(8 * GIB), now), Take::Wait);
        queue.finish(0);
        assert_eq!(queue.take(&costs, Some(8 * GIB), now), Take::Job(1));
        assert_eq!(queue.take(&costs, Some(8 * GIB), now), Take::Done);
    }

    #[test]
    fn free_memory_is_read_again_for_each_start() {
        let costs = [cost(2, 3 * GIB, false), cost(1, 3 * GIB, false)];
        let mut queue = Queue::middle_out(&costs, Duration::ZERO);
        let now = Instant::now();
        assert_eq!(queue.take(&costs, Some(32 * GIB), now), Take::Job(0));
        // Memory fell: 4 GiB free keeps 1 GiB back and leaves 3 GiB, which just fits.
        assert_eq!(queue.take(&costs, Some(3 * GIB), now), Take::Wait);
        assert_eq!(queue.take(&costs, Some(4 * GIB), now), Take::Job(1));
    }

    #[test]
    fn a_file_that_has_ramped_up_is_already_counted_in_free_memory() {
        let costs = [cost(2, 5 * GIB, false), cost(1, 5 * GIB, false)];
        let mut queue = Queue::middle_out(&costs, Duration::from_secs(10));
        let now = Instant::now();
        assert_eq!(queue.take(&costs, Some(16 * GIB), now), Take::Job(0));
        // Just started: its 5 GiB is still reserved, so 16 GiB free leaves 12 - 5 = 7 and fits.
        assert_eq!(queue.take(&costs, Some(16 * GIB), now), Take::Job(1));
        let costs = [cost(2, 8 * GIB, false), cost(1, 8 * GIB, false)];
        let mut queue = Queue::middle_out(&costs, Duration::from_secs(10));
        assert_eq!(queue.take(&costs, Some(16 * GIB), now), Take::Job(0));
        assert_eq!(queue.take(&costs, Some(16 * GIB), now), Take::Wait);
        // By now its memory is already out of what is free.
        // It reached 8 GiB at ten seconds a GiB, so past 80 s it is.
        let later = now + Duration::from_secs(81);
        assert_eq!(queue.take(&costs, Some(16 * GIB), later), Take::Job(1));
    }

    #[test]
    fn unknown_free_memory_limits_only_by_workers() {
        let costs = [cost(2, 100 * GIB, false), cost(1, 100 * GIB, false)];
        let mut queue = Queue::middle_out(&costs, Duration::from_secs(60));
        let now = Instant::now();
        assert_eq!(queue.take(&costs, None, now), Take::Job(0));
        assert_eq!(queue.take(&costs, None, now), Take::Job(1));
    }

    #[test]
    fn one_worker_keeps_input_order() {
        let costs = [cost(1, 0, false), cost(9, 0, true), cost(5, 0, false)];
        let mut queue = Queue::in_order(costs.len());
        let now = Instant::now();
        let mut seen = Vec::new();
        while let Take::Job(i) = queue.take(&costs, Some(0), now) {
            seen.push(i);
            queue.finish(i);
        }
        assert_eq!(seen, [0, 1, 2]);
    }
}
