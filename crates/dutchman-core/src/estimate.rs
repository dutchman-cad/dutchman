//! Measured conversion costs shared by desktop planners and bounded hosts.
use crate::Format;

/// What converting a file with some choices is expected to cost.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate {
    /// Expected peak memory, in bytes.
    pub peak_bytes: u64,
    /// Expected time, rounded up to whole seconds for compatibility.
    pub seconds: u64,
    /// Expected elapsed time before rounding; time bands apply to this value.
    pub seconds_exact: f64,
    /// Shared compatibility band. Phase models use their total-memory envelope;
    /// older combined models retain their heuristic. Prefer the separate bands.
    pub band: f64,
    /// Relative peak-memory band (0.3 is ±30 %); unmeasured formats retain a heuristic.
    pub memory_band: f64,
    /// Relative time band, or None when measured timings do not support the current model.
    pub time_band: Option<f64>,
}

impl Estimate {
    /// Independent history fits cannot make retaining history cheaper than omitting it.
    fn at_least(self, without_history: Self) -> Self {
        Self {
            peak_bytes: self.peak_bytes.max(without_history.peak_bytes),
            seconds: self.seconds.max(without_history.seconds),
            seconds_exact: self.seconds_exact.max(without_history.seconds_exact),
            ..self
        }
    }
}

#[derive(Clone, Copy)]
pub struct Choices {
    pub history: bool,
}
struct Coefficients {
    format: Format,
    model: CostModel,
    band: f64,
    memory_band: f64,
    time_band: Option<f64>,
}

impl Coefficients {
    fn template(&self) -> Estimate {
        Estimate {
            peak_bytes: 0,
            seconds: 0,
            seconds_exact: 0.0,
            band: self.band,
            memory_band: self.memory_band,
            time_band: self.time_band,
        }
    }
}

enum CostModel {
    Stages(Stages),
    Legacy(LegacyCost),
}

struct Stages {
    read: StageCost,
    convert: StageCost,
    history: StageCost,
    check: StageCost,
    geometry_check: StageCost,
}

impl Stages {
    fn split(&self, sizes: Sizes, choices: &Choices, template: Estimate) -> ReadConvertSplit {
        let geometry = self.convert.estimate(sizes, template);
        let convert = if choices.history {
            self.history.estimate(sizes, template).at_least(geometry)
        } else {
            geometry
        };
        ReadConvertSplit {
            read: self.read.estimate(sizes, template),
            convert,
        }
    }
}

// Existing combined models for formats without phase measurements.
struct LegacyCost {
    base_bytes: u64,
    bytes_per_byte: f64,
    base_seconds: f64,
    seconds_per_mib: f64,
    history_factor: f64,
    history_seconds_factor: f64,
    startup: Option<StartupTime>,
    check_share: f64,
}

/// Linear costs from archive metadata. Memory after decode and checking is additional
/// to the preceding peak, so adding the three stages gives one consistent total.
struct StageCost {
    base_bytes: u64,
    // Some checking fits plateau at the observed assembly scale; this is an
    // empirical estimate, not a ceiling on what an unseen design can allocate.
    work_bytes_limit: Option<u64>,
    bytes_per_byte: f64,
    bytes_per_member: f64,
    base_seconds: f64,
    seconds_per_mib: f64,
    seconds_per_member: f64,
}

impl StageCost {
    fn estimate(&self, sizes: Sizes, template: Estimate) -> Estimate {
        let seconds_exact = self.base_seconds
            + self.seconds_per_mib * sizes.bytes as f64 / (1 << 20) as f64
            + self.seconds_per_member * sizes.members as f64;
        let work = (self.bytes_per_byte * sizes.bytes as f64
            + self.bytes_per_member * sizes.members as f64)
            .ceil() as u64;
        Estimate {
            peak_bytes: self
                .base_bytes
                .saturating_add(work.min(self.work_bytes_limit.unwrap_or(u64::MAX))),
            seconds: seconds_exact.ceil() as u64,
            seconds_exact,
            ..template
        }
    }
}

// Fixed elapsed startup is independent of content/history scaling. Windows history
// includes the memory guard and CLI discovery. A possible CLI-probe timeout is not
// normal startup work; only its ordinary startup cost belongs in the estimate.
struct StartupTime {
    windows_shape: f64,
    windows_history: f64,
    mac_shape: f64,
    mac_history: f64,
    windows_verify_shape: f64,
    windows_verify_history: f64,
    // FreeCAD recipe data support a much smaller content term than the old guess.
    content_rates: Option<(f64, f64)>,
}

// Fresh Linux measurements use the shared-index main build, four requested workers,
// six public Fusion assemblies and five smaller public/synthetic Fusion, Inventor and
// SolidWorks parts. Read, conversion (history on/off), and checking (history on/off)
// have independent memory/time fits. Small foreign parts calibrate floors only;
// their previous content slopes remain heuristics for larger files. Formats without
// phase measurements keep their existing combined model. See docs/presets.md for
// the coefficients' scope and measured misses. All format-dependent numbers live here.
const COEFFICIENTS: &[Coefficients] = &[
    Coefficients {
        format: Format::FusionArchive,
        model: CostModel::Stages(Stages {
            read: StageCost {
                base_bytes: 15_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 5.7,
                bytes_per_member: 1_700_000.0,
                base_seconds: 0.0,
                seconds_per_mib: 0.1,
                seconds_per_member: 0.061,
            },
            convert: StageCost {
                base_bytes: 12_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 15.0,
                bytes_per_member: 0.0,
                base_seconds: 0.0,
                seconds_per_mib: 0.11,
                seconds_per_member: 0.062,
            },
            history: StageCost {
                base_bytes: 9_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 26.0,
                bytes_per_member: 0.0,
                base_seconds: 0.0,
                seconds_per_mib: 0.35,
                seconds_per_member: 0.0,
            },
            check: StageCost {
                base_bytes: 23_000_000,
                work_bytes_limit: Some(1_191_000_000),
                bytes_per_byte: 260.0,
                bytes_per_member: 0.0,
                base_seconds: 0.032,
                seconds_per_mib: 0.38,
                seconds_per_member: 0.0,
            },
            geometry_check: StageCost {
                base_bytes: 25_000_000,
                work_bytes_limit: Some(740_000_000),
                bytes_per_byte: 230.0,
                bytes_per_member: 0.0,
                base_seconds: 0.03,
                seconds_per_mib: 0.46,
                seconds_per_member: 0.0,
            },
        }),
        band: 0.45,
        memory_band: 0.45,
        time_band: None,
    },
    Coefficients {
        format: Format::FusionMultiDocument,
        model: CostModel::Stages(Stages {
            read: StageCost {
                base_bytes: 29_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 20.0,
                bytes_per_member: 0.0,
                base_seconds: 0.011,
                seconds_per_mib: 1.2,
                seconds_per_member: 0.0,
            },
            convert: StageCost {
                base_bytes: 24_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 16.0,
                bytes_per_member: 0.0,
                base_seconds: 0.02,
                seconds_per_mib: 0.32,
                seconds_per_member: 0.0,
            },
            history: StageCost {
                base_bytes: 12_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 19.0,
                bytes_per_member: 0.0,
                base_seconds: 0.019,
                seconds_per_mib: 0.38,
                seconds_per_member: 0.0,
            },
            check: StageCost {
                base_bytes: 23_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 25.0,
                bytes_per_member: 0.0,
                base_seconds: 0.032,
                seconds_per_mib: 1.8,
                seconds_per_member: 0.0,
            },
            geometry_check: StageCost {
                base_bytes: 25_000_000,
                work_bytes_limit: Some(2_055_000_000),
                bytes_per_byte: 230.0,
                bytes_per_member: 0.0,
                base_seconds: 0.03,
                seconds_per_mib: 0.69,
                seconds_per_member: 0.0,
            },
        }),
        band: 0.30,
        memory_band: 0.30,
        time_band: None,
    },
    Coefficients {
        format: Format::InventorPart,
        model: CostModel::Stages(Stages {
            read: StageCost {
                base_bytes: 15_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 20.0,
                bytes_per_member: 0.0,
                base_seconds: 0.003,
                seconds_per_mib: 1.0,
                seconds_per_member: 0.0,
            },
            convert: StageCost {
                base_bytes: 18_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 5.0,
                bytes_per_member: 0.0,
                base_seconds: 0.014,
                seconds_per_mib: 1.0,
                seconds_per_member: 0.0,
            },
            history: StageCost {
                base_bytes: 17_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 30.0,
                bytes_per_member: 0.0,
                base_seconds: 0.042,
                seconds_per_mib: 3.0,
                seconds_per_member: 0.0,
            },
            check: StageCost {
                base_bytes: 20_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 55.0,
                bytes_per_member: 0.0,
                base_seconds: 0.002,
                seconds_per_mib: 0.0,
                seconds_per_member: 0.0,
            },
            geometry_check: StageCost {
                base_bytes: 30_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 27.5,
                bytes_per_member: 0.0,
                base_seconds: 0.046,
                seconds_per_mib: 0.0,
                seconds_per_member: 0.0,
            },
        }),
        band: 1.00,
        memory_band: 1.00,
        time_band: None,
    },
    Coefficients {
        format: Format::InventorAssembly,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 30.0,
            base_seconds: 1.0,
            seconds_per_mib: 2.0,
            history_factor: 3.0,
            history_seconds_factor: 3.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::SolidWorksPart,
        model: CostModel::Stages(Stages {
            read: StageCost {
                base_bytes: 22_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 20.0,
                bytes_per_member: 0.0,
                base_seconds: 0.006,
                seconds_per_mib: 2.5,
                seconds_per_member: 0.0,
            },
            convert: StageCost {
                base_bytes: 11_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 5.0,
                bytes_per_member: 0.0,
                base_seconds: 0.016,
                seconds_per_mib: 2.5,
                seconds_per_member: 0.0,
            },
            history: StageCost {
                base_bytes: 20_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 22.5,
                bytes_per_member: 0.0,
                base_seconds: 0.017,
                seconds_per_mib: 6.0,
                seconds_per_member: 0.0,
            },
            check: StageCost {
                base_bytes: 26_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 46.75,
                bytes_per_member: 0.0,
                base_seconds: 0.023,
                seconds_per_mib: 0.0,
                seconds_per_member: 0.0,
            },
            geometry_check: StageCost {
                base_bytes: 20_000_000,
                work_bytes_limit: None,
                bytes_per_byte: 27.5,
                bytes_per_member: 0.0,
                base_seconds: 0.025,
                seconds_per_mib: 0.0,
                seconds_per_member: 0.0,
            },
        }),
        band: 1.00,
        memory_band: 1.00,
        time_band: None,
    },
    Coefficients {
        format: Format::SolidWorksAssembly,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 30.0,
            base_seconds: 1.0,
            seconds_per_mib: 5.0,
            history_factor: 3.0,
            history_seconds_factor: 3.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::FreeCadDocument,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 29_000_000,
            bytes_per_byte: 150.0,
            base_seconds: 1.0,
            seconds_per_mib: 2.0,
            history_factor: 1.04,
            history_seconds_factor: 3.0,
            startup: Some(StartupTime {
                windows_shape: 0.183,
                windows_history: 1.600,
                mac_shape: 0.025,
                mac_history: 0.050,
                windows_verify_shape: 0.350,
                windows_verify_history: 1.900,
                content_rates: Some((0.2, 0.01)),
            }),
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 0.5,
        time_band: None,
    },
    Coefficients {
        format: Format::CatiaPart,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 30.0,
            base_seconds: 1.0,
            seconds_per_mib: 2.0,
            history_factor: 3.0,
            history_seconds_factor: 3.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::CreoPart,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 30.0,
            base_seconds: 1.0,
            seconds_per_mib: 2.0,
            history_factor: 3.0,
            history_seconds_factor: 3.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::NxPart,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 30.0,
            base_seconds: 1.0,
            seconds_per_mib: 2.0,
            history_factor: 3.0,
            history_seconds_factor: 3.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::CreoView,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 30.0,
            base_seconds: 1.0,
            seconds_per_mib: 2.0,
            history_factor: 3.0,
            history_seconds_factor: 3.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::Rhino,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 30.0,
            base_seconds: 1.0,
            seconds_per_mib: 2.0,
            history_factor: 3.0,
            history_seconds_factor: 3.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::Acis,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 30.0,
            base_seconds: 1.0,
            seconds_per_mib: 2.0,
            history_factor: 3.0,
            history_seconds_factor: 3.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::Step,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 20.0,
            base_seconds: 1.0,
            seconds_per_mib: 1.0,
            history_factor: 2.0,
            history_seconds_factor: 2.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    // Onshape bundles: provisional, from STEP's row (a bundle's heavy part is its STEP export; the feature JSON is
    // small). Replace with measured values once real Onshape documents are converted.
    Coefficients {
        format: Format::OnshapeBundle,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 20.0,
            base_seconds: 1.0,
            seconds_per_mib: 1.0,
            history_factor: 2.0,
            history_seconds_factor: 2.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::Iges,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 20.0,
            base_seconds: 1.0,
            seconds_per_mib: 1.0,
            history_factor: 2.0,
            history_seconds_factor: 2.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::ModuleBoard,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 20.0,
            base_seconds: 1.0,
            seconds_per_mib: 1.0,
            history_factor: 2.0,
            history_seconds_factor: 2.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
    Coefficients {
        format: Format::UnclassifiedPart,
        model: CostModel::Legacy(LegacyCost {
            base_bytes: 100_000_000,
            bytes_per_byte: 30.0,
            base_seconds: 1.0,
            seconds_per_mib: 2.0,
            history_factor: 3.0,
            history_seconds_factor: 3.0,
            startup: None,
            check_share: 1.1,
        }),
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
    },
];

pub fn combine_held_stages(conversion: Estimate, check: Estimate) -> Estimate {
    let seconds_exact = conversion.seconds_exact + check.seconds_exact;
    Estimate {
        peak_bytes: conversion.peak_bytes.saturating_add(check.peak_bytes),
        seconds: seconds_exact.ceil() as u64,
        seconds_exact,
        time_band: None,
        ..conversion
    }
}

pub fn check_cost(sizes: Sizes, choices: &Choices, conversion: Estimate) -> Estimate {
    let Some(coefficients) = COEFFICIENTS.iter().find(|c| c.format == sizes.format) else {
        return Estimate {
            peak_bytes: 0,
            seconds: 0,
            seconds_exact: 0.0,
            ..conversion
        };
    };
    match &coefficients.model {
        CostModel::Stages(stages) => {
            let geometry = stages
                .geometry_check
                .estimate(sizes, coefficients.template());
            if choices.history {
                stages
                    .check
                    .estimate(sizes, coefficients.template())
                    .at_least(geometry)
            } else {
                geometry
            }
        }
        CostModel::Legacy(legacy) => {
            let seconds_exact = if cfg!(target_os = "windows") {
                legacy.startup.as_ref().map_or(0.0, |startup| {
                    let shape = startup.windows_verify_shape - startup.windows_shape;
                    if choices.history {
                        (startup.windows_verify_history - startup.windows_history).max(shape)
                    } else {
                        shape
                    }
                })
            } else {
                0.0
            };
            Estimate {
                peak_bytes: (conversion.peak_bytes as f64 * legacy.check_share).ceil() as u64,
                seconds: seconds_exact.ceil() as u64,
                seconds_exact,
                time_band: None,
                ..conversion
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Sizes {
    pub format: Format,
    pub bytes: u64,
    pub members: u64,
}
impl Sizes {
    pub fn calculate(self, choices: &Choices) -> Option<Estimate> {
        let coefficients = COEFFICIENTS.iter().find(|c| c.format == self.format)?;
        Some(match &coefficients.model {
            CostModel::Stages(stages) => {
                let split = stages.split(self, choices, coefficients.template());
                combine_held_stages(split.read, split.convert)
            }
            CostModel::Legacy(legacy) => legacy.estimate(self, choices, coefficients.template()),
        })
    }
}

impl LegacyCost {
    fn estimate(&self, sizes: Sizes, choices: &Choices, template: Estimate) -> Estimate {
        let factor = if choices.history {
            self.history_factor
        } else {
            1.0
        };
        let seconds_factor = if choices.history {
            self.history_seconds_factor
        } else {
            1.0
        };
        let time_rate = self
            .startup
            .as_ref()
            .filter(|_| cfg!(target_os = "windows"))
            .and_then(|s| s.content_rates)
            .map_or(self.seconds_per_mib * seconds_factor, |(shape, history)| {
                if choices.history {
                    history
                } else {
                    shape
                }
            });
        let fixed = self
            .startup
            .as_ref()
            .map_or(self.base_seconds * seconds_factor, |startup| {
                if cfg!(target_os = "windows") {
                    if choices.history {
                        startup.windows_history
                    } else {
                        startup.windows_shape
                    }
                } else if cfg!(target_os = "macos") {
                    if choices.history {
                        startup.mac_history
                    } else {
                        startup.mac_shape
                    }
                } else {
                    self.base_seconds * seconds_factor
                }
            });
        let seconds_exact = fixed + time_rate * sizes.bytes as f64 / (1 << 20) as f64;
        let estimate = Estimate {
            peak_bytes: (self.base_bytes as f64 + self.bytes_per_byte * sizes.bytes as f64 * factor)
                .ceil() as u64,
            seconds: seconds_exact.ceil() as u64,
            seconds_exact,
            ..template
        };
        if choices.history {
            // Also cover combined models, including platform-specific startup/content
            // rates whose history and geometry fits may cross at larger sizes.
            estimate.at_least(self.estimate(sizes, &Choices { history: false }, template))
        } else {
            estimate
        }
    }
}

/// Read peak and additional conversion cost beyond that peak.
#[derive(Clone, Copy, Debug)]
pub struct ReadConvertSplit {
    pub read: Estimate,
    pub convert: Estimate,
}

pub fn read_convert_split(sizes: Sizes, choices: &Choices) -> Option<ReadConvertSplit> {
    let coefficients = COEFFICIENTS.iter().find(|c| c.format == sizes.format)?;
    match &coefficients.model {
        CostModel::Stages(stages) => Some(stages.split(sizes, choices, coefficients.template())),
        CostModel::Legacy(_) => None,
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;
    #[test]
    fn coefficient_table_covers_each_format_once() {
        assert_eq!(COEFFICIENTS.len(), Format::ALL.len());
        for format in Format::ALL {
            let entries: Vec<_> = COEFFICIENTS.iter().filter(|c| c.format == format).collect();
            assert_eq!(entries.len(), 1, "{format:?}");
            let c = entries[0];
            match &c.model {
                CostModel::Stages(stages) => {
                    for cost in [
                        &stages.read,
                        &stages.convert,
                        &stages.history,
                        &stages.check,
                        &stages.geometry_check,
                    ] {
                        assert!(cost.bytes_per_byte.is_finite() && cost.bytes_per_byte >= 0.0);
                        assert!(cost.bytes_per_member.is_finite() && cost.bytes_per_member >= 0.0);
                        assert!(cost.base_seconds.is_finite() && cost.base_seconds >= 0.0);
                        assert!(cost.seconds_per_mib.is_finite() && cost.seconds_per_mib >= 0.0);
                        assert!(
                            cost.seconds_per_member.is_finite() && cost.seconds_per_member >= 0.0
                        );
                    }
                }
                CostModel::Legacy(legacy) => {
                    assert!(legacy.base_bytes > 0);
                    assert!(legacy.bytes_per_byte.is_finite() && legacy.bytes_per_byte > 0.0);
                    assert!(legacy.base_seconds.is_finite() && legacy.base_seconds > 0.0);
                    assert!(legacy.seconds_per_mib.is_finite() && legacy.seconds_per_mib > 0.0);
                    assert!(legacy.check_share.is_finite() && legacy.check_share >= 0.0);
                    assert!(legacy.history_factor.is_finite() && legacy.history_factor > 0.0);
                    assert!(
                        legacy.history_seconds_factor.is_finite()
                            && legacy.history_seconds_factor > 0.0
                    );
                }
            }
            assert!(c.band.is_finite() && c.band > 0.0 && c.band <= 1.0);
            assert!(c.memory_band.is_finite() && c.memory_band > 0.0 && c.memory_band <= c.band);
            if let Some(time_band) = c.time_band {
                assert!(time_band.is_finite() && time_band > 0.0 && time_band <= c.band);
            }
        }
    }
}
