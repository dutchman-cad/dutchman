//! What a conversion keeps, as a preset and per-choice switches, and what keeping it is
//! expected to cost before the design is read.
//!
//! Presets: `Fast` keeps the shape only; `Standard` (the default) keeps feature history,
//! sketches and joints, but leaves an assembly's feature history out when the estimate says
//! it would not fit in the memory this machine has free, and says so; `Full` keeps everything.
//! Per-choice switches (`--no-history`, `--no-sketches`, `--no-joints`) narrow any preset.

use std::path::Path;

/// A named set of choices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Preset {
    /// The shape only: one import, no feature history, sketches or joints. Least memory.
    Fast,
    /// Feature history, sketches and joints, with an assembly's history left out (and the
    /// report saying so) when it would not fit in free memory.
    #[default]
    Standard,
    /// Everything, whatever it costs.
    Full,
}

impl Preset {
    /// The next lighter preset, when there is one.
    pub fn lighter(self) -> Option<Preset> {
        match self {
            Preset::Full => Some(Preset::Standard),
            Preset::Standard => Some(Preset::Fast),
            Preset::Fast => None,
        }
    }

    /// The name the CLI, MCP and the app use for it.
    pub fn name(self) -> &'static str {
        match self {
            Preset::Fast => "fast",
            Preset::Standard => "standard",
            Preset::Full => "full",
        }
    }
}

/// What a conversion keeps, after its preset and switches are applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choices {
    /// Feature history, replayed and checked in SolidCraft when it maps.
    pub history: bool,
    /// Sketches, also when history is left out (kept for reference).
    pub sketches: bool,
    /// Assembly joints.
    pub joints: bool,
    /// Standard only: leave an assembly's history out when it would not fit in free memory.
    pub history_within_budget: bool,
}

impl Choices {
    /// The choices `preset` makes, before any switch narrows them.
    pub fn of(preset: Preset) -> Choices {
        match preset {
            Preset::Fast => Choices {
                history: false,
                sketches: false,
                joints: false,
                history_within_budget: false,
            },
            Preset::Standard => Choices {
                history: true,
                sketches: true,
                joints: true,
                history_within_budget: true,
            },
            Preset::Full => Choices {
                history: true,
                sketches: true,
                joints: true,
                history_within_budget: false,
            },
        }
    }
}

/// What converting a file with some choices is expected to cost.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate {
    /// Expected peak memory, in bytes.
    pub peak_bytes: u64,
    /// Expected time, in seconds.
    pub seconds: u64,
    /// The relative error band of both numbers (0.3 is ±30 %), from the calibration set.
    pub band: f64,
}

/// The cost of converting `input` with `choices`, from a quick read of the file (its size,
/// format and, for archives, member count) without decoding it; `None` when the file cannot
/// be pre-read. Implemented by the estimator (lane A); calibrated on measured conversions.
pub fn estimate(input: &Path, choices: &Choices) -> Option<Estimate> {
    let _ = (input, choices);
    None
}

/// The memory this machine has free for a conversion now, in bytes; `None` where it cannot
/// be read. Implemented by lane A.
pub fn available_memory() -> Option<u64> {
    None
}
