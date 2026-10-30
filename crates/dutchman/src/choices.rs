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
    /// Expected time, in seconds.
    pub seconds: u64,
    /// The relative error band of both numbers (0.3 is ±30 %), from the calibration set.
    pub band: f64,
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
    let estimate = estimate(input, &choices);
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
        let reduced = estimate_without_history(input, choices);
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

fn estimate_without_history(input: &Path, mut choices: Choices) -> Option<Estimate> {
    choices.history = false;
    estimate(input, &choices)
}

struct Coefficients {
    format: Format,
    base_bytes: u64,
    bytes_per_byte: f64,
    base_seconds: f64,
    seconds_per_mib: f64,
    history_factor: f64,
    band: f64,
}

// Calibrated on: release builds converting a 350-occurrence Fusion printer frame (74 MB of
// nested archive content), a 33-placement Fusion assembly (259 MB), a 168 MB Fusion archive
// (859 MB, over 24 GB with history), Fusion and Inventor recipe parts and SolidWorks parts of
// 0.3 to 6.4 MB, each with history off and on. Peak memory with history is about 40 bytes per
// byte of nested Fusion content and 140 bytes per byte of SolidWorks file. Formats without
// measurements keep a conservative guess and a band of 100 %.
// All format-dependent coefficients, including history factors and bands, live in this table.
const COEFFICIENTS: &[Coefficients] = &[
    Coefficients {
        format: Format::FusionArchive,
        base_bytes: 33_000_000,
        bytes_per_byte: 27.0,
        base_seconds: 0.2,
        seconds_per_mib: 1.0,
        history_factor: 1.5,
        band: 0.6,
    },
    Coefficients {
        format: Format::FusionMultiDocument,
        base_bytes: 33_000_000,
        bytes_per_byte: 27.0,
        base_seconds: 0.2,
        seconds_per_mib: 1.0,
        history_factor: 1.5,
        band: 0.6,
    },
    Coefficients {
        format: Format::InventorPart,
        base_bytes: 32_000_000,
        bytes_per_byte: 25.0,
        base_seconds: 0.1,
        seconds_per_mib: 2.0,
        history_factor: 2.0,
        band: 0.5,
    },
    Coefficients {
        format: Format::InventorAssembly,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        history_factor: 3.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::SolidWorksPart,
        base_bytes: 40_000_000,
        bytes_per_byte: 25.0,
        base_seconds: 1.0,
        seconds_per_mib: 5.0,
        history_factor: 1.7,
        band: 0.4,
    },
    Coefficients {
        format: Format::SolidWorksAssembly,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 5.0,
        history_factor: 3.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::FreeCadDocument,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        history_factor: 3.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::CatiaPart,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        history_factor: 3.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::CreoPart,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        history_factor: 3.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::NxPart,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        history_factor: 3.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::CreoView,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        history_factor: 3.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::Rhino,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        history_factor: 3.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::Acis,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        history_factor: 3.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::Step,
        base_bytes: 100_000_000,
        bytes_per_byte: 20.0,
        base_seconds: 1.0,
        seconds_per_mib: 1.0,
        history_factor: 2.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::Iges,
        base_bytes: 100_000_000,
        bytes_per_byte: 20.0,
        base_seconds: 1.0,
        seconds_per_mib: 1.0,
        history_factor: 2.0,
        band: 1.0,
    },
    Coefficients {
        format: Format::UnclassifiedPart,
        base_bytes: 100_000_000,
        bytes_per_byte: 30.0,
        base_seconds: 1.0,
        seconds_per_mib: 2.0,
        history_factor: 3.0,
        band: 1.0,
    },
];

/// Estimate from file metadata and ZIP directory entries only; never decode or inflate a member.
pub fn estimate(input: &Path, choices: &Choices) -> Option<Estimate> {
    let format = Format::from_path(input)?;
    if !format.can_convert() {
        return None;
    }
    let metadata = std::fs::metadata(input).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let coefficients = COEFFICIENTS.iter().find(|c| c.format == format)?;
    let (bytes, _members) = if matches!(
        format,
        Format::FusionArchive | Format::FusionMultiDocument | Format::FreeCadDocument
    ) {
        archive_size(
            input,
            matches!(format, Format::FusionArchive | Format::FusionMultiDocument),
        )?
    } else {
        (metadata.len(), 0)
    };
    let factor = if choices.history {
        coefficients.history_factor
    } else {
        1.0
    };
    Some(Estimate {
        peak_bytes: ((coefficients.base_bytes as f64 + coefficients.bytes_per_byte * bytes as f64)
            * factor)
            .ceil() as u64,
        seconds: ((coefficients.base_seconds
            + coefficients.seconds_per_mib * bytes as f64 / (1 << 20) as f64)
            * factor)
            .ceil() as u64,
        band: coefficients.band,
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

fn archive_size(input: &Path, fusion: bool) -> Option<(u64, u64)> {
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
    let bytes = if fusion {
        bytes.max(nested_size(&mut archive))
    } else {
        bytes
    };
    Some((bytes, members))
}

/// The uncompressed size of the designs a Fusion archive holds as zips of their own (an `.f3z`
/// holds each design as an `.f3d`), read from each inner directory: one member at a time is
/// read, never the designs inside it; 0 when none can be read.
fn nested_size<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> u64 {
    /// The most bytes of inner archives read in all.
    const LIMIT: u64 = 2 << 30;
    let mut total = 0_u64;
    let mut read = 0_u64;
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
        }
    }
    total
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
            assert!(c.history_factor.is_finite() && c.history_factor > 1.0);
            assert!(c.band.is_finite() && c.band > 0.0 && c.band <= 1.0);
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
        assert_eq!(archive_size(&path, true), Some((4 << 20, 1)));
        assert_eq!(archive_size(&path, false), Some((inner.len() as u64, 1)));
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
            assert_eq!(archive_size(&path, false), Some(((1 << 20) + 9, 2)));
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
        assert_eq!(archive_size(&path, false), Some((9, 1)));
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
        assert_eq!(archive_size(&path, false), Some((9, 1)));
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
        let reduced = estimate_without_history(&assembly, options.choices()).unwrap();
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
                    estimate_without_history(&part, options.choices())
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
                band: 0.5,
            },
            available: 1_000_000_000,
            lighter: Some(Preset::Fast),
        };
        assert_eq!(check.refusal().unwrap(), "this conversion needs about 2.5 GB and 1.0 GB are free; try --preset fast (or --no-memory-check to run anyway)");
    }
}
