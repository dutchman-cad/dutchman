//! What a conversion keeps, as a preset and per-choice switches, and what keeping it is
//! expected to cost before the design is read.
//!
//! Presets: `Fast` keeps the shape only; `Standard` (the default) keeps feature history,
//! sketches and joints, but leaves feature history out when the estimate says
//! it would not fit in the memory this machine has free, and says so; `Full` keeps everything.
//! Per-choice switches (`--no-history`, `--no-sketches`, `--no-joints`) narrow any preset.

use dutchman_core::Format;
use std::path::Path;

/// A named set of choices.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Preset {
    /// The shape only: one import, no feature history, sketches or joints. Least memory.
    Fast,
    /// Feature history, sketches and joints, with history left out (and the
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
    /// Standard only: leave history out when it would not fit in free memory.
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
    /// Expected time, rounded up to whole seconds for compatibility.
    pub seconds: u64,
    /// Expected elapsed time before rounding; time bands apply to this value.
    pub seconds_exact: f64,
    /// Legacy shared band. Use the separate bands for calibrated coverage; when time is
    /// uncalibrated this retains the previous heuristic for existing consumers.
    pub band: f64,
    /// Relative peak-memory band (0.3 is ±30 %); unmeasured formats retain a heuristic.
    pub memory_band: f64,
    /// Relative time band, or None when measured timings do not support the current model.
    pub time_band: Option<f64>,
}

/// A human-readable estimate shared by the CLI and desktop app.
pub fn estimate_text(estimate: Estimate) -> String {
    format!(
        "{}, {}",
        memory_text(estimate, false),
        time_text(estimate, false)
    )
}

/// The same estimate with its measured error bands, as the command line prints it.
pub fn estimate_text_with_bands(estimate: Estimate) -> String {
    format!(
        "{}, {}",
        memory_text(estimate, true),
        time_text(estimate, true)
    )
}

fn memory_text(estimate: Estimate, band: bool) -> String {
    if estimate.peak_bytes < 100_000_000 {
        "under 0.1 GB".to_owned()
    } else if band {
        format!(
            "about {:.1} GB (±{:.0} %)",
            gb(estimate.peak_bytes),
            estimate.memory_band * 100.0
        )
    } else {
        format!("about {:.1} GB", gb(estimate.peak_bytes))
    }
}

/// A time without a measured band for its format reads as rough.
fn time_text(estimate: Estimate, band: bool) -> String {
    if estimate.seconds < 60 {
        return "under a minute".to_owned();
    }
    let minutes = estimate.seconds.div_ceil(60);
    match estimate.time_band {
        Some(time_band) if band => format!("about {minutes} min (±{:.0} %)", time_band * 100.0),
        Some(_) => format!("about {minutes} min"),
        None => format!("roughly {minutes} min"),
    }
}

/// The result of checking a conversion against currently available memory.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MemoryCheck {
    Ok(Option<Estimate>),
    TooBig {
        estimate: Estimate,
        available: u64,
        lighter: Option<Preset>,
    },
}

impl MemoryCheck {
    /// The same refusal text for every front end.
    pub fn refusal(self) -> Option<String> {
        match self {
            Self::Ok(_) => None,
            Self::TooBig { estimate, available, lighter } => Some(format!(
                "this conversion needs about {:.1} GB and {:.1} GB are free; try --preset {} (or --no-memory-check to run anyway)",
                gb(estimate.peak_bytes), gb(available), lighter.map_or("fast", Preset::name)
            )),
        }
    }
}

/// Bytes as decimal gigabytes, as displayed by every front end.
pub fn gb(bytes: u64) -> f64 {
    bytes as f64 / 1_000_000_000.0
}

/// Checks without decoding; unknown estimates or available memory never block a run.
pub fn memory_check(input: &Path, options: &crate::ConvertOptions) -> MemoryCheck {
    let available = if options.no_memory_check || options.preset == Preset::Fast {
        None
    } else {
        available_memory()
    };
    memory_check_with(input, options, available)
}

pub(crate) fn memory_check_with(
    input: &Path,
    options: &crate::ConvertOptions,
    available: Option<u64>,
) -> MemoryCheck {
    let choices = options.choices();
    let estimate = estimate_with_options(input, &choices, options);
    if options.no_memory_check || options.preset == Preset::Fast {
        return MemoryCheck::Ok(estimate);
    }
    let (Some(full), Some(available)) = (estimate, available) else {
        return MemoryCheck::Ok(estimate);
    };
    if full.peak_bytes <= available {
        return MemoryCheck::Ok(estimate);
    }
    if choices.history && choices.history_within_budget {
        let reduced = estimate_without_history(input, choices, options);
        if reduced.is_some_and(|e| e.peak_bytes <= available) {
            return MemoryCheck::Ok(reduced);
        }
    }
    MemoryCheck::TooBig {
        estimate: full,
        available,
        lighter: options.preset.lighter(),
    }
}

fn estimate_without_history(
    input: &Path,
    mut choices: Choices,
    options: &crate::ConvertOptions,
) -> Option<Estimate> {
    choices.history = false;
    estimate_with_options(input, &choices, options)
}

// Independent embedded designs expose parallel work before native decoding. The serial
// fraction covers decode, assembly coordination and container writing.
struct MultipartTime {
    seconds_per_mib: f64,
    serial_fraction: f64,
}

struct Coefficients {
    format: Format,
    // Fixed process overhead; history scales the content term, not this floor.
    base_bytes: u64,
    bytes_per_byte: f64,
    base_seconds: f64,
    seconds_per_mib: f64,
    multipart_time: Option<MultipartTime>,
    history_factor: f64,
    history_seconds_factor: f64,
    band: f64,
    memory_band: f64,
    time_band: Option<f64>,
    startup: Option<StartupTime>,
}

// Fixed elapsed startup is independent of content/history scaling. Windows history
// includes the memory guard and CLI discovery. The older Mac binary exhibits both
// immediate and ten-second CLI probes. Allow the ten-second version-probe timeout
// until an explicit target version bypasses discovery; this remains a rough estimate.
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

// Calibrated on: release builds converting a 350-occurrence Fusion printer frame (74 MB of
// nested archive content), a 33-placement Fusion assembly (259 MB), a 60 MB single Fusion design
// of 202 MB content with 1,500 parts, a 168 MB Fusion archive (859 MB, over 24 GB with history),
// Fusion and Inventor recipe parts and SolidWorks parts of 0.3 to 6.4 MB, each with history off
// and on, on Linux and Windows, with the default parallel part workers. Peak memory is about 52
// bytes per byte of Fusion design content (57 with history) and 140 per byte of SolidWorks
// file. Formats without measurements keep a conservative guess and a band of 100 %.
// Small-file floors were refit on the synthetic Inventor and SolidWorks cubes and
// FreeCAD recipe/fixture documents: exact-bundle Windows conversions and a pre-preset macOS
// nightly (see guide.md). Fixed overhead is separate from content-dependent history memory.
// Memory and time have separate history factors so a RAM fit does not change time estimates.
// Band refit: all 5,516 successful CSV rows from Windows and both Macs, evaluated
// against the current model. Add one percentage point of slack, then round up to 5%.
// Four measured formats have timing misses beyond their previous bands (subsecond
// rounding and host-specific CLI-probe startup), so their time bands are uncalibrated.
// Keep their legacy shared heuristics for old consumers; never widen to hide those misses.
// All format-dependent coefficients, including history factors and bands, live in this table.
const COEFFICIENTS: &[Coefficients] = &[
    Coefficients {
        format: Format::FusionArchive,
        base_bytes: 33_000_000,
        bytes_per_byte: 52.0,
        base_seconds: 0.2,
        seconds_per_mib: 1.4,
        multipart_time: Some(MultipartTime {
            seconds_per_mib: 0.75,
            serial_fraction: 0.60,
        }),
        history_factor: 1.1,
        history_seconds_factor: 1.1,
        band: 0.6,
        memory_band: 0.55,
        time_band: None,
        startup: Some(StartupTime {
            windows_shape: 0.160,
            windows_history: 1.535,
            mac_shape: 0.020,
            mac_history: 10.040,
            windows_verify_shape: 0.330,
            windows_verify_history: 1.690,
            content_rates: None,
        }),
    },
    Coefficients {
        format: Format::FusionMultiDocument,
        base_bytes: 33_000_000,
        bytes_per_byte: 52.0,
        base_seconds: 0.2,
        seconds_per_mib: 1.4,
        multipart_time: Some(MultipartTime {
            seconds_per_mib: 0.75,
            serial_fraction: 0.60,
        }),
        history_factor: 1.1,
        history_seconds_factor: 1.1,
        band: 0.25,
        memory_band: 0.25,
        time_band: Some(0.2),
        startup: None,
    },
    Coefficients {
        format: Format::InventorPart,
        base_bytes: 18_000_000,
        bytes_per_byte: 25.0,
        base_seconds: 0.1,
        seconds_per_mib: 2.0,
        multipart_time: None,
        history_factor: 2.0,
        history_seconds_factor: 2.0,
        band: 0.5,
        memory_band: 0.3,
        time_band: None,
        startup: Some(StartupTime {
            windows_shape: 0.162,
            windows_history: 1.550,
            mac_shape: 0.010,
            mac_history: 10.040,
            windows_verify_shape: 0.335,
            windows_verify_history: 1.760,
            content_rates: None,
        }),
    },
    Coefficients {
        format: Format::InventorAssembly,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        multipart_time: None,
        history_factor: 3.0,
        history_seconds_factor: 3.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
    Coefficients {
        format: Format::SolidWorksPart,
        base_bytes: 18_000_000,
        bytes_per_byte: 25.0,
        base_seconds: 1.0,
        seconds_per_mib: 5.0,
        multipart_time: None,
        history_factor: 1.7,
        history_seconds_factor: 1.7,
        band: 0.4,
        memory_band: 0.35,
        time_band: None,
        startup: Some(StartupTime {
            windows_shape: 0.170,
            windows_history: 1.668,
            mac_shape: 0.010,
            mac_history: 10.040,
            windows_verify_shape: 0.340,
            windows_verify_history: 1.660,
            content_rates: None,
        }),
    },
    Coefficients {
        format: Format::SolidWorksAssembly,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 5.0,
        multipart_time: None,
        history_factor: 3.0,
        history_seconds_factor: 3.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
    Coefficients {
        format: Format::FreeCadDocument,
        base_bytes: 29_000_000,
        bytes_per_byte: 150.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        multipart_time: None,
        history_factor: 1.04,
        history_seconds_factor: 3.0,
        band: 1.0,
        memory_band: 0.5,
        time_band: None,
        startup: Some(StartupTime {
            windows_shape: 0.183,
            windows_history: 1.600,
            mac_shape: 0.025,
            mac_history: 10.050,
            windows_verify_shape: 0.350,
            windows_verify_history: 1.900,
            content_rates: Some((0.2, 0.01)),
        }),
    },
    Coefficients {
        format: Format::CatiaPart,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        multipart_time: None,
        history_factor: 3.0,
        history_seconds_factor: 3.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
    Coefficients {
        format: Format::CreoPart,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        multipart_time: None,
        history_factor: 3.0,
        history_seconds_factor: 3.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
    Coefficients {
        format: Format::NxPart,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        multipart_time: None,
        history_factor: 3.0,
        history_seconds_factor: 3.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
    Coefficients {
        format: Format::CreoView,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        multipart_time: None,
        history_factor: 3.0,
        history_seconds_factor: 3.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
    Coefficients {
        format: Format::Rhino,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        multipart_time: None,
        history_factor: 3.0,
        history_seconds_factor: 3.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
    Coefficients {
        format: Format::Acis,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        multipart_time: None,
        history_factor: 3.0,
        history_seconds_factor: 3.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
    Coefficients {
        format: Format::Step,
        base_bytes: 100_000_000,
        bytes_per_byte: 20.0,
        base_seconds: 1.0,
        seconds_per_mib: 1.0,
        multipart_time: None,
        history_factor: 2.0,
        history_seconds_factor: 2.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
    Coefficients {
        format: Format::Iges,
        base_bytes: 100_000_000,
        bytes_per_byte: 20.0,
        base_seconds: 1.0,
        seconds_per_mib: 1.0,
        multipart_time: None,
        history_factor: 2.0,
        history_seconds_factor: 2.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
    Coefficients {
        format: Format::UnclassifiedPart,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        multipart_time: None,
        history_factor: 3.0,
        history_seconds_factor: 3.0,
        band: 1.0,
        memory_band: 1.0,
        time_band: None,
        startup: None,
    },
];

/// Estimate without native decoding, using automatic part workers. Nested Fusion ZIP
/// containers are inflated only to read their bounded directories.
pub fn estimate(input: &Path, choices: &Choices) -> Option<Estimate> {
    estimate_with_threads(input, choices, 0)
}

/// Estimate using the requested part workers (0 selects the machine's automatic limit).
pub fn estimate_with_threads(input: &Path, choices: &Choices, threads: usize) -> Option<Estimate> {
    estimate_using_workers(input, choices, threads, dutchman_core::parallel::workers)
}

/// Estimate with verification and preflight context, without running a CLI probe.
/// Verification replay has not been measured, so it never inherits a conversion-only band.
pub fn estimate_with_options(
    input: &Path,
    choices: &Choices,
    options: &crate::ConvertOptions,
) -> Option<Estimate> {
    let mut result = estimate_with_threads(input, choices, options.threads)?;
    let startup = Format::from_path(input)
        .and_then(|format| COEFFICIENTS.iter().find(|c| c.format == format))
        .and_then(|c| c.startup.as_ref());
    if cfg!(target_os = "macos")
        && startup.is_some()
        && options.solidcraft_version.is_some()
        && choices.history
    {
        result.seconds_exact = (result.seconds_exact - 10.0).max(0.001);
        result.seconds = result.seconds_exact.ceil() as u64;
    }
    if !options.no_verify {
        if cfg!(target_os = "macos")
            && startup.is_some()
            && !choices.history
            && options.solidcraft_version.is_none()
        {
            // Verification can discover a version even when history is not requested.
            result.seconds_exact += 10.0;
            result.seconds = result.seconds_exact.ceil() as u64;
        }
        if cfg!(target_os = "windows") {
            if let Some(startup) = startup {
                let (without, with) = if choices.history {
                    (startup.windows_history, startup.windows_verify_history)
                } else {
                    (startup.windows_shape, startup.windows_verify_shape)
                };
                result.seconds_exact += with - without;
                result.seconds = result.seconds_exact.ceil() as u64;
            }
        }
        result.time_band = None;
    }
    // Explicit target versions bypass history's version discovery. We cannot subtract
    // its cost from a combined startup measurement, so retain a rough estimate.
    if options.solidcraft_version.is_some() || options.no_memory_check {
        result.time_band = None;
    }
    Some(result)
}

fn estimate_using_workers(
    input: &Path,
    choices: &Choices,
    threads: usize,
    workers: impl FnOnce(usize, usize, u64) -> usize,
) -> Option<Estimate> {
    let format = Format::from_path(input)?;
    if !format.can_convert() {
        return None;
    }
    let metadata = std::fs::metadata(input).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let coefficients = COEFFICIENTS.iter().find(|c| c.format == format)?;
    let (bytes, _members, designs) = if matches!(
        format,
        Format::FusionArchive | Format::FusionMultiDocument | Format::FreeCadDocument
    ) {
        archive_size(
            input,
            matches!(format, Format::FusionArchive | Format::FusionMultiDocument),
        )?
    } else {
        (metadata.len(), 0, 0)
    };
    let factor = if choices.history {
        coefficients.history_factor
    } else {
        1.0
    };
    let seconds_factor = if choices.history {
        coefficients.history_seconds_factor
    } else {
        1.0
    };
    let time_rate = if let Some(time) = coefficients.multipart_time.as_ref().filter(|_| designs > 1)
    {
        // The native body counts are not available yet. Use the converter's 256 MiB
        // minimum per-worker budget and embedded design count as pre-decode proxies.
        let count = if threads == 1 {
            1
        } else {
            workers(threads, designs, 256 << 20)
        };
        time.seconds_per_mib
            * (time.serial_fraction + (1.0 - time.serial_fraction) / count.max(1) as f64)
    } else {
        coefficients.seconds_per_mib
    };
    let time_rate = coefficients
        .startup
        .as_ref()
        .filter(|_| cfg!(target_os = "windows"))
        .and_then(|s| s.content_rates)
        .map_or(time_rate * seconds_factor, |(shape, history)| {
            if choices.history {
                history
            } else {
                shape
            }
        });
    let content_seconds = time_rate * bytes as f64 / (1 << 20) as f64;
    let seconds_exact = if let Some(startup) = &coefficients.startup {
        let fixed = if cfg!(target_os = "windows") {
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
            // No Linux timing measurements: retain the previous startup guess.
            coefficients.base_seconds * seconds_factor
        };
        fixed + content_seconds
    } else {
        // Preserve the already-calibrated multipart Fusion centre and worker model.
        (coefficients.base_seconds * seconds_factor + content_seconds).ceil()
    };
    Some(Estimate {
        peak_bytes: (coefficients.base_bytes as f64
            + coefficients.bytes_per_byte * bytes as f64 * factor)
            .ceil() as u64,
        seconds: seconds_exact.ceil() as u64,
        seconds_exact,
        band: coefficients.band,
        memory_band: coefficients.memory_band,
        time_band: coefficients.time_band,
    })
}

/// Open a zip only after bounding its directory and entry count.
pub(crate) fn bounded_archive<R: std::io::Read + std::io::Seek>(
    mut file: R,
) -> Option<zip::ZipArchive<R>> {
    use std::io::SeekFrom;
    let length = file.seek(SeekFrom::End(0)).ok()?;
    // Bound directory allocation, including ZIP64's extensible record, before zip reads it.
    let tail_len = length.min(65_633);
    file.seek(SeekFrom::End(-i64::try_from(tail_len).ok()?))
        .ok()?;
    let mut tail = vec![0; usize::try_from(tail_len).ok()?];
    file.read_exact(&mut tail).ok()?;
    let start = tail.windows(4).rposition(|w| w == b"PK\x05\x06")?;
    let end = tail.get(start..)?;
    let word = |offset: usize| -> Option<u16> {
        Some(u16::from_le_bytes(
            end.get(offset..offset + 2)?.try_into().ok()?,
        ))
    };
    let dword = |offset: usize| -> Option<u32> {
        Some(u32::from_le_bytes(
            end.get(offset..offset + 4)?.try_into().ok()?,
        ))
    };
    if end.len() != 22 + usize::from(word(20)?) || word(4)? != 0 || word(6)? != 0 {
        return None;
    }
    let locator = start
        .checked_sub(20)
        .and_then(|offset| tail.get(offset..start));
    let (members, directory_bytes, directory_offset) =
        if let Some(locator) = locator.filter(|l| l.starts_with(b"PK\x06\x07")) {
            let disk = u32::from_le_bytes(locator.get(4..8)?.try_into().ok()?);
            let offset = u64::from_le_bytes(locator.get(8..16)?.try_into().ok()?);
            let disks = u32::from_le_bytes(locator.get(16..20)?.try_into().ok()?);
            if disk != 0 || disks != 1 || offset.checked_add(56)? > length {
                return None;
            }
            file.seek(SeekFrom::Start(offset)).ok()?;
            let mut record = [0; 56];
            file.read_exact(&mut record).ok()?;
            if !record.starts_with(b"PK\x06\x06") {
                return None;
            }
            let value = |at: usize| -> Option<u64> {
                Some(u64::from_le_bytes(record.get(at..at + 8)?.try_into().ok()?))
            };
            let size = value(4)?;
            if !(44..=16 << 20).contains(&size)
                || offset.checked_add(12)?.checked_add(size)? > length
                || record.get(16..24)? != [0; 8]
                || value(24)? != value(32)?
            {
                return None;
            }
            (value(32)?, value(40)?, value(48)?)
        } else {
            if word(8)? != word(10)? {
                return None;
            }
            (
                u64::from(word(10)?),
                u64::from(dword(12)?),
                u64::from(dword(16)?),
            )
        };
    if members > 10_000
        || directory_bytes > 16 << 20
        || directory_offset.checked_add(directory_bytes)? > length
    {
        return None;
    }
    let archive = zip::ZipArchive::new(file).ok()?;
    if archive.len() > 10_000 {
        return None;
    }
    Some(archive)
}

fn archive_size(input: &Path, fusion: bool) -> Option<(u64, u64, usize)> {
    let mut archive = bounded_archive(std::fs::File::open(input).ok()?)?;
    let bytes = match archive.decompressed_size() {
        Some(bytes) => u64::try_from(bytes).unwrap_or(u64::MAX),
        None => {
            let mut total = 0_u64;
            for index in 0..archive.len() {
                total = total.saturating_add(archive.by_index_raw(index).ok()?.size());
            }
            total
        }
    };
    let members = archive.len() as u64;
    let (nested_bytes, designs) = if fusion {
        nested_size(&mut archive)
    } else {
        (0, 0)
    };
    Some((bytes.max(nested_bytes), members, designs))
}

/// The uncompressed size of the designs a Fusion archive holds as zips of their own (an `.f3z`
/// holds each design as an `.f3d`), read from each inner directory: one member at a time is
/// read, never the designs inside it; 0 when none can be read.
fn nested_size<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> (u64, usize) {
    /// The most bytes of inner archives read in all.
    const LIMIT: u64 = 2 << 30;
    let mut total = 0_u64;
    let mut read = 0_u64;
    let mut designs = 0_usize;
    for index in 0..archive.len() {
        let Ok(mut member) = archive.by_index(index) else {
            continue;
        };
        if !member
            .name()
            .is_ok_and(|name| name.to_ascii_lowercase().ends_with(".f3d"))
        {
            continue;
        }
        read = read.saturating_add(member.size());
        if read > LIMIT {
            break;
        }
        if let Some(size) = nested_content_size(&mut member, LIMIT) {
            total = total.saturating_add(size);
            designs += 1;
        }
    }
    (total, designs)
}

/// Read only a bounded nested archive's directory, never its design contents.
pub(crate) fn nested_content_size(reader: &mut impl std::io::Read, limit: u64) -> Option<u64> {
    use std::io::Read;
    let mut bytes = Vec::new();
    reader
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    let archive = bounded_archive(std::io::Cursor::new(bytes))?;
    archive
        .decompressed_size()
        .and_then(|n| u64::try_from(n).ok())
}

/// Available memory, including process limits where available.
pub use dutchman_core::available_memory;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ConvertOptions;
    use std::io::Write;

    #[test]
    fn presets_only_narrow_and_have_a_lighter_choice() {
        for preset in [Preset::Fast, Preset::Standard, Preset::Full] {
            let options = ConvertOptions {
                preset,
                import_only: true,
                no_sketches: true,
                no_joints: true,
                ..Default::default()
            };
            let narrowed = options.choices();
            assert!(!narrowed.history && !narrowed.sketches && !narrowed.joints);
            assert_eq!(narrowed.history_within_budget, preset == Preset::Standard);
        }
        assert_eq!(Preset::Full.lighter(), Some(Preset::Standard));
        assert_eq!(Preset::Standard.lighter(), Some(Preset::Fast));
        assert_eq!(Preset::Fast.lighter(), None);
        let fast = Choices::of(Preset::Fast);
        assert!(!fast.history && !fast.sketches && !fast.joints);
        assert!(Choices::of(Preset::Full).history);
    }

    #[test]
    fn coefficient_table_covers_each_format_once() {
        assert_eq!(COEFFICIENTS.len(), Format::ALL.len());
        for format in Format::ALL {
            let entries: Vec<_> = COEFFICIENTS.iter().filter(|c| c.format == format).collect();
            assert_eq!(entries.len(), 1, "{format:?}");
            let c = entries[0];
            assert!(
                c.base_bytes > 0
                    && c.bytes_per_byte.is_finite()
                    && c.bytes_per_byte > 0.0
                    && c.base_seconds.is_finite()
                    && c.base_seconds > 0.0
                    && c.seconds_per_mib.is_finite()
                    && c.seconds_per_mib > 0.0
            );
            if let Some(time) = &c.multipart_time {
                assert!(time.seconds_per_mib.is_finite() && time.seconds_per_mib > 0.0);
                assert!(
                    time.serial_fraction.is_finite() && (0.0..=1.0).contains(&time.serial_fraction)
                );
            }
            assert!(c.history_factor.is_finite() && c.history_factor > 1.0);
            assert!(c.history_seconds_factor.is_finite() && c.history_seconds_factor > 1.0);
            assert!(c.band.is_finite() && c.band > 0.0 && c.band <= 1.0);
            assert!(c.memory_band.is_finite() && c.memory_band > 0.0 && c.memory_band <= c.band);
            if let Some(time_band) = c.time_band {
                assert!(time_band.is_finite() && time_band > 0.0 && time_band <= c.band);
            }
        }
    }

    #[test]
    fn fusion_archives_count_the_designs_they_store_as_zips() {
        // An .f3z stores each design as an .f3d zip of its own: what counts is the inner
        // content, read from the inner directory, not the stored bytes.
        let mut inner = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut inner));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("Design1/BulkStream.dat", options).unwrap();
            zip.write_all(&vec![0; 4 << 20]).unwrap();
            zip.finish().unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.f3z");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("Part.f3d", stored).unwrap();
        zip.write_all(&inner).unwrap();
        zip.finish().unwrap();
        assert_eq!(archive_size(&path, true), Some((4 << 20, 1, 1)));
        assert_eq!(archive_size(&path, false), Some((inner.len() as u64, 1, 0)));
    }

    #[test]
    fn multipart_time_uses_requested_workers_without_changing_memory() {
        let mut inner = Vec::new();
        {
            let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut inner));
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("Design/BulkStream.dat", options).unwrap();
            zip.write_all(&vec![0; 4 << 20]).unwrap();
            zip.finish().unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.f3z");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for name in ["a.f3d", "b.f3d", "c.f3d", "d.f3d"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&inner).unwrap();
        }
        zip.finish().unwrap();
        let choices = Choices::of(Preset::Fast);
        let serial = estimate_using_workers(&path, &choices, 1, |_, _, _| {
            panic!("serial needs no probe")
        })
        .unwrap();
        let auto = estimate_using_workers(&path, &choices, 0, |requested, parts, budget| {
            assert_eq!((requested, parts, budget), (0, 4, 256 << 20));
            4
        })
        .unwrap();
        let limited = estimate_using_workers(&path, &choices, 2, |requested, _, _| {
            assert_eq!(requested, 2);
            2
        })
        .unwrap();
        assert_eq!(serial.seconds, 13);
        assert_eq!(auto.seconds, 9);
        assert_eq!(limited.seconds, 10);
        assert_eq!(auto.peak_bytes, serial.peak_bytes);
        assert_eq!(auto.band, serial.band);
        assert_eq!(auto.memory_band, serial.memory_band);
        assert_eq!(auto.time_band, serial.time_band);
    }

    #[test]
    fn zip_members_alone_do_not_imply_parallel_designs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.f3d");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for name in ["Design/a", "Design/b"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(&vec![0; 1 << 20]).unwrap();
        }
        zip.finish().unwrap();
        let estimate = estimate_using_workers(&path, &Choices::of(Preset::Fast), 0, |_, _, _| {
            panic!("one design needs no parallel estimate")
        })
        .unwrap();
        assert_eq!(estimate.seconds, 3);
    }

    #[test]
    fn fractional_time_and_unmeasured_verification_do_not_claim_coverage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tiny.ipt");
        std::fs::write(&path, [0; 16]).unwrap();
        let choices = Choices::of(Preset::Fast);
        let options = crate::ConvertOptions {
            preset: Preset::Fast,
            no_verify: true,
            ..crate::ConvertOptions::default()
        };
        let fast = estimate_with_options(&path, &choices, &options).unwrap();
        assert!(fast.seconds_exact > 0.0 && fast.seconds_exact < 1.0);
        assert_eq!(fast.seconds, 1);
        assert!(fast.time_band.is_none());
        let verified = estimate_with_options(
            &path,
            &choices,
            &crate::ConvertOptions {
                no_verify: false,
                ..options
            },
        )
        .unwrap();
        assert_eq!(verified.peak_bytes, fast.peak_bytes);
        assert!(verified.seconds_exact >= fast.seconds_exact);
        assert!(verified.time_band.is_none());
    }

    #[test]
    fn history_does_not_multiply_the_fixed_process_floor() {
        let dir = tempfile::tempdir().unwrap();
        for extension in ["ipt", "sldprt"] {
            let path = dir.path().join(format!("synthetic.{extension}"));
            std::fs::write(&path, []).unwrap();
            let fast = estimate(&path, &Choices::of(Preset::Fast)).unwrap();
            let full = estimate(&path, &Choices::of(Preset::Full)).unwrap();
            assert_eq!(fast.peak_bytes, full.peak_bytes);
            // Content still costs more when history is kept.
            std::fs::write(&path, [0; 5120]).unwrap();
            let fast_content = estimate(&path, &Choices::of(Preset::Fast)).unwrap();
            let full_content = estimate(&path, &Choices::of(Preset::Full)).unwrap();
            assert!(fast_content.peak_bytes > fast.peak_bytes);
            assert!(full_content.peak_bytes > fast_content.peak_bytes);
        }
    }

    #[cfg(feature = "freecad")]
    #[test]
    fn freecad_time_separates_startup_from_the_recipe_content_term() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.FCStd");
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        zip.start_file("synthetic", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&vec![0; 1 << 20]).unwrap();
        zip.finish().unwrap();
        let fast = estimate(&path, &Choices::of(Preset::Fast)).unwrap();
        let full = estimate(&path, &Choices::of(Preset::Full)).unwrap();
        // RAM's 1.04 multiplier must not replace the existing 3x time multiplier.
        assert!(fast.seconds_exact > 0.2);
        assert!(full.seconds_exact > fast.seconds_exact);
        assert!(full.time_band.is_none());
    }

    #[test]
    fn archives_use_uncompressed_sizes_without_decoding_members() {
        let dir = tempfile::tempdir().unwrap();
        for extension in ["f3d", "f3z", "FCStd"] {
            let path = dir.path().join(format!("synthetic.{extension}"));
            let mut zip = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            zip.start_file("not-a-design", options).unwrap();
            zip.write_all(&vec![0; 1 << 20]).unwrap();
            zip.start_file("second-member", options).unwrap();
            zip.write_all(b"synthetic").unwrap();
            zip.finish().unwrap();
            assert_eq!(archive_size(&path, false), Some(((1 << 20) + 9, 2, 0)));
            assert!(std::fs::metadata(&path).unwrap().len() < 1 << 20);
            if !Format::from_path(&path).unwrap().can_convert() {
                assert!(estimate(&path, &Choices::of(Preset::Fast)).is_none());
                continue;
            }
            let light = estimate(&path, &Choices::of(Preset::Fast)).unwrap();
            let full = estimate(&path, &Choices::of(Preset::Full)).unwrap();
            let c = COEFFICIENTS
                .iter()
                .find(|c| c.format == Format::from_path(&path).unwrap())
                .unwrap();
            assert_eq!(
                light.peak_bytes,
                (c.base_bytes as f64 + c.bytes_per_byte * ((1 << 20) + 9) as f64).ceil() as u64
            );
            assert!(full.peak_bytes > light.peak_bytes);
            assert!(full.seconds >= light.seconds);
        }
    }

    #[test]
    fn zip64_and_descriptor_archives_are_estimated_without_inflation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.f3z");
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("synthetic", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"synthetic").unwrap();
        let mut bytes = zip.finish().unwrap().into_inner();
        let mut end = bytes.split_off(bytes.len() - 22);
        let members = u64::from(u16::from_le_bytes(end[10..12].try_into().unwrap()));
        let directory_bytes = u64::from(u32::from_le_bytes(end[12..16].try_into().unwrap()));
        let directory_offset = u64::from(u32::from_le_bytes(end[16..20].try_into().unwrap()));
        let zip64_offset = bytes.len() as u64;
        bytes.extend_from_slice(b"PK\x06\x06");
        bytes.extend_from_slice(&44_u64.to_le_bytes());
        bytes.extend_from_slice(&[45, 0, 45, 0]);
        bytes.extend_from_slice(&[0; 8]);
        for value in [members, members, directory_bytes, directory_offset] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(b"PK\x06\x07");
        bytes.extend_from_slice(&[0; 4]);
        bytes.extend_from_slice(&zip64_offset.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        end[8..12].fill(255);
        end[12..20].fill(255);
        bytes.extend_from_slice(&end);
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(archive_size(&path, false), Some((9, 1, 0)));
        // A hostile ZIP64 count must be refused before zip can allocate its directory.
        bytes[zip64_offset as usize + 24..zip64_offset as usize + 32]
            .copy_from_slice(&u64::MAX.to_le_bytes());
        bytes[zip64_offset as usize + 32..zip64_offset as usize + 40]
            .copy_from_slice(&u64::MAX.to_le_bytes());
        std::fs::write(&path, &bytes).unwrap();
        assert!(archive_size(&path, false).is_none());

        let mut zip = zip::ZipWriter::new_stream(std::fs::File::create(&path).unwrap());
        zip.start_file("synthetic", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"synthetic").unwrap();
        zip.finish().unwrap();
        assert_eq!(archive_size(&path, false), Some((9, 1, 0)));
    }

    #[test]
    fn unreadable_and_overlarge_directories_have_unknown_estimates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.f3d");
        assert!(estimate(&path, &Choices::of(Preset::Full)).is_none());
        std::fs::write(&path, b"not a zip").unwrap();
        assert!(estimate(&path, &Choices::of(Preset::Full)).is_none());
        let mut end = vec![0; 22];
        end[..4].copy_from_slice(b"PK\x05\x06");
        end[8..10].copy_from_slice(&10_001_u16.to_le_bytes());
        end[10..12].copy_from_slice(&10_001_u16.to_le_bytes());
        std::fs::write(&path, end).unwrap();
        assert!(archive_size(&path, false).is_none());
    }

    #[test]
    fn memory_policy_keeps_full_budgets_standard_and_skips_fast() {
        let dir = tempfile::tempdir().unwrap();
        let assembly = dir.path().join("synthetic.iam");
        let part = dir.path().join("synthetic.ipt");
        std::fs::write(&assembly, b"synthetic").unwrap();
        std::fs::write(&part, b"synthetic").unwrap();
        let mut options = ConvertOptions::default();
        let full = estimate(&assembly, &options.choices()).unwrap();
        let reduced = estimate_without_history(&assembly, options.choices(), &options).unwrap();
        assert_eq!(
            memory_check_with(&assembly, &options, Some(full.peak_bytes)),
            MemoryCheck::Ok(Some(full))
        );
        assert_eq!(
            memory_check_with(&assembly, &options, Some(reduced.peak_bytes)),
            MemoryCheck::Ok(Some(reduced))
        );
        assert!(matches!(
            memory_check_with(&assembly, &options, Some(reduced.peak_bytes - 1)),
            MemoryCheck::TooBig {
                lighter: Some(Preset::Fast),
                ..
            }
        ));
        assert!(matches!(
            memory_check_with(
                &part,
                &options,
                Some(
                    estimate_without_history(&part, options.choices(), &options)
                        .unwrap()
                        .peak_bytes
                )
            ),
            MemoryCheck::Ok(_)
        ));
        options.preset = Preset::Full;
        assert!(matches!(
            memory_check_with(&assembly, &options, Some(reduced.peak_bytes)),
            MemoryCheck::TooBig {
                lighter: Some(Preset::Standard),
                ..
            }
        ));
        options.no_memory_check = true;
        assert_eq!(
            memory_check_with(&assembly, &options, Some(0)),
            MemoryCheck::Ok(Some(full))
        );
        options.no_memory_check = false;
        options.preset = Preset::Fast;
        assert_eq!(
            memory_check_with(&assembly, &options, Some(0)),
            MemoryCheck::Ok(Some(reduced))
        );
        options.preset = Preset::Standard;
        options.import_only = true;
        assert!(matches!(
            memory_check_with(&assembly, &options, Some(0)),
            MemoryCheck::TooBig { .. }
        ));
        assert_eq!(
            memory_check_with(&assembly, &options, None),
            MemoryCheck::Ok(Some(reduced))
        );
        assert_eq!(
            memory_check_with(&dir.path().join("missing.ipt"), &options, Some(0)),
            MemoryCheck::Ok(None)
        );
    }

    #[test]
    fn refusal_is_shared_and_uses_decimal_gigabytes() {
        let check = MemoryCheck::TooBig {
            estimate: Estimate {
                peak_bytes: 2_500_000_000,
                seconds: 60,
                seconds_exact: 60.0,
                band: 0.5,
                memory_band: 0.5,
                time_band: Some(0.5),
            },
            available: 1_000_000_000,
            lighter: Some(Preset::Fast),
        };
        assert_eq!(check.refusal().unwrap(), "this conversion needs about 2.5 GB and 1.0 GB are free; try --preset fast (or --no-memory-check to run anyway)");
    }

    #[test]
    fn estimates_show_lower_bounds_and_round_minutes_up() {
        for (peak_bytes, seconds, expected) in [
            (0, 0, "under 0.1 GB, under a minute"),
            (99_999_999, 59, "under 0.1 GB, under a minute"),
            (100_000_000, 60, "about 0.1 GB, about 1 min"),
            (2_500_000_000, 121, "about 2.5 GB, about 3 min"),
            (100_000_000, 59, "about 0.1 GB, under a minute"),
            (99_999_999, 60, "under 0.1 GB, about 1 min"),
        ] {
            assert_eq!(
                estimate_text(Estimate {
                    peak_bytes,
                    seconds,
                    band: 0.3,
                    memory_band: 0.3,
                    time_band: Some(0.3),
                }),
                expected
            );
        }
        let measured = Estimate {
            peak_bytes: 2_500_000_000,
            seconds: 121,
            band: 0.25,
            memory_band: 0.25,
            time_band: Some(0.2),
        };
        assert_eq!(
            estimate_text_with_bands(measured),
            "about 2.5 GB (±25 %), about 3 min (±20 %)"
        );
        let rough = Estimate {
            time_band: None,
            ..measured
        };
        assert_eq!(estimate_text(rough), "about 2.5 GB, roughly 3 min");
        assert_eq!(
            estimate_text_with_bands(rough),
            "about 2.5 GB (±25 %), roughly 3 min"
        );
        let small = Estimate {
            peak_bytes: 1,
            seconds: 1,
            ..rough
        };
        assert_eq!(
            estimate_text_with_bands(small),
            "under 0.1 GB, under a minute"
        );
    }
}
