//! What the importer tells SolidCraft's operations panel while it converts. The bar only moves
//! forward, each report is a small step (a job has a budget of host calls), and the numbers are
//! the same on every SolidCraft: `control.progress` exists in `craft:plugin@0.1.0` and 0.2.0, and a host
//! that does not draw it ignores it.
use std::sync::atomic::{AtomicU32, Ordering};

/// Where the work starts and ends: reading the first file, then the parts, then the check of
/// the rebuilt history in SolidCraft. The steps in between (mapping) report nothing of their own.
const PARTS_FROM: f32 = 0.05;
const PARTS_TO: f32 = 0.35;
const CHECKS_FROM: f32 = 0.40;
const CHECKS_TO: f32 = 0.90;
/// After this many parts (or checks) the bar is halfway through its range.
const PARTS_HALF: f32 = 8.0;
const CHECKS_HALF: f32 = 4.0;

#[derive(Debug, Default)]
pub(crate) struct Progress {
    parts: AtomicU32,
    checks: AtomicU32,
    /// The last fraction sent, as bits; the bar never goes back.
    last: AtomicU32,
}

fn along(from: f32, to: f32, count: u32, half: f32) -> f32 {
    from + (to - from) * (1.0 - 1.0 / (1.0 + count as f32 / half))
}

impl Progress {
    /// A report to send, or none when it would not move the bar by a visible step (1%).
    fn step(&self, fraction: f32, message: String) -> Option<(f32, String)> {
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        // One atomic step, so two callers can never report out of order. Non-negative floats
        // order the same as their bit patterns.
        let mut seen = self.last.load(Ordering::Relaxed);
        loop {
            if fraction < f32::from_bits(seen) + 0.01 {
                return None;
            }
            match self.last.compare_exchange_weak(
                seen,
                fraction.to_bits(),
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Some((fraction, message)),
                Err(now) => seen = now,
            }
        }
    }

    /// The main file is read; the parts of an assembly come next.
    pub(crate) fn file_read(&self) -> Option<(f32, String)> {
        self.step(PARTS_FROM, "Reading the design".into())
    }

    /// One more part of an assembly was read.
    pub(crate) fn part_read(&self) -> Option<(f32, String)> {
        let n = self.parts.fetch_add(1, Ordering::Relaxed) + 1;
        self.step(
            along(PARTS_FROM, PARTS_TO, n, PARTS_HALF),
            format!("Reading part {}", n + 1),
        )
    }

    /// The history is about to be checked in SolidCraft once more.
    pub(crate) fn check_started(&self) -> Option<(f32, String)> {
        let n = self.checks.fetch_add(1, Ordering::Relaxed) + 1;
        self.step(
            along(CHECKS_FROM, CHECKS_TO, n, CHECKS_HALF),
            format!("Checking the history in SolidCraft ({n})"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(
        progress: &Progress,
        calls: usize,
        f: fn(&Progress) -> Option<(f32, String)>,
    ) -> Vec<f32> {
        (0..calls)
            .filter_map(|_| f(progress).map(|(x, _)| x))
            .collect()
    }

    #[test]
    fn parts_move_the_bar_forward_inside_their_range() {
        let progress = Progress::default();
        assert_eq!(progress.file_read().map(|r| r.0), Some(PARTS_FROM));
        let seen = all(&progress, 5000, Progress::part_read);
        assert!(seen.windows(2).all(|w| w[1] >= w[0] + 0.01));
        assert!(seen.iter().all(|f| (PARTS_FROM..=PARTS_TO).contains(f)));
        assert!(seen.len() > 5, "the bar must move per part, not stay put");
        assert!(
            seen.len() < 40,
            "a 5000-part assembly stays a few dozen host calls"
        );
    }

    #[test]
    fn checks_never_send_a_fraction_the_host_rejects_or_go_back() {
        let progress = Progress::default();
        progress.part_read();
        let seen = all(&progress, 100_000, Progress::check_started);
        assert!(seen
            .iter()
            .all(|f| f.is_finite() && (0.0..=1.0).contains(f)));
        assert!(seen.windows(2).all(|w| w[1] > w[0]));
        assert!(seen.iter().all(|f| (CHECKS_FROM..=CHECKS_TO).contains(f)));
        // Later phases never pull an earlier one back.
        assert!(progress.part_read().is_none());
        assert!(progress.file_read().is_none());
    }

    #[test]
    fn bad_fractions_are_clamped() {
        let progress = Progress::default();
        assert_eq!(progress.step(f32::NAN, String::new()), None);
        assert_eq!(progress.step(7.0, "x".into()).map(|r| r.0), Some(1.0));
    }
}
