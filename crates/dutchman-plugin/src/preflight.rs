//! Bounded ZIP metadata only; no native model is decoded here.
use dutchman_core::{
    estimate::{Choices, Sizes},
    Error, Format,
};
use std::io::{Read, Seek};

/// SolidCraft's default "Max plugin fuel" ceiling. A package runs with exactly the fuel its
/// manifest declares and cannot read what the host would allow, so asking for more would make a
/// default install refuse the plugin for every file. Designs whose estimate exceeds this go to
/// the Dutchman app instead.
pub(crate) const PLUGIN_FUEL: u64 = 100_000_000_000;

/// The guest's linear memory: `runtime.limits.memory_mib` in the manifest, which SolidCraft grants
/// when its Max plugin memory preference allows it. The plugin cannot read that preference, so
/// the declared figure is the budget; a test keeps the two equal.
#[cfg(test)]
pub(crate) const GUEST_MEMORY_MIB: u64 = 2048;

/// What the memory estimate may spend of the guest's memory. The rest is the stack, the output
/// design being written, and the allocator's slack.
pub(crate) const PEAK_BYTES: u64 = 1_700_000_000;

/// Memory, then time, then fuel: the existing estimate gates against this budget.
pub(crate) const PLUGIN_BUDGET: Budget = Budget {
    peak_bytes: PEAK_BYTES,
    seconds: 300,
    fuel: PLUGIN_FUEL,
};
#[derive(Clone, Copy)]
pub(crate) struct Budget {
    pub peak_bytes: u64,
    pub seconds: u64,
    pub fuel: u64,
}
pub(crate) const DOWNLOAD: &str = "https://github.com/dutchman-cad/dutchman/releases/latest";

pub(crate) fn check<R: Read + Seek>(
    format: Format,
    size: u64,
    reader: R,
    history: bool,
) -> Result<(), Error> {
    check_with(format, size, reader, history, PLUGIN_BUDGET)
}
fn check_with<R: Read + Seek>(
    format: Format,
    size: u64,
    reader: R,
    history: bool,
    budget: Budget,
) -> Result<(), Error> {
    let (bytes, members, parts) = if matches!(
        format,
        Format::FusionArchive | Format::FusionMultiDocument | Format::FreeCadDocument
    ) {
        let Ok((mut archive, _)) = bounded_archive(reader, 10_000) else {
            return Ok(());
        };
        let mut bytes = 0u64;
        let mut nested = 0u64;
        let mut parts = 0u64;
        // Only small inner ZIP containers may be inflated for their index. This is not CAD decoding.
        let mut remaining = 16u64 << 20;
        let mut designs = Vec::new();
        for i in 0..archive.len() {
            let Ok(member) = archive.by_index_raw(i) else {
                continue;
            };
            bytes = bytes.saturating_add(member.size());
            if member
                .name()
                .is_ok_and(|n| n.to_ascii_lowercase().ends_with(".f3d"))
            {
                parts = parts.saturating_add(1);
                let start = member.data_start().ok_or_else(|| {
                    Error::Decode(
                        "The archive index could not locate a part; select the file again".into(),
                    )
                })?;
                designs.push((
                    i,
                    start,
                    member.size(),
                    member.compression() == zip::CompressionMethod::Stored,
                ));
            }
        }
        let members = archive.len() as u64;
        // A sufficient outer size already proves refusal; do not inflate inner containers.
        check_sizes(
            Sizes {
                format,
                bytes,
                members,
            },
            parts.max(1),
            history,
            budget,
        )?;
        // Read compressed inner containers once, sharing a strict total inflation budget.
        for &(i, _, size, stored) in &designs {
            if stored {
                continue;
            }
            if size > remaining {
                return Err(Error::MemoryBudget(format!(
                    "This design needs a larger archive check than the SolidCraft plugin can run. Open it in the Dutchman app, which converts it to a .sc you can open here. Download the app: {DOWNLOAD}"
                )));
            }
            remaining -= size;
            let mut data = Vec::new();
            let inner = archive
                .by_index(i)
                .ok()
                .and_then(|m| m.take(size.saturating_add(1)).read_to_end(&mut data).ok())
                .filter(|_| data.len() as u64 == size)
                .and_then(|_| bounded_archive(std::io::Cursor::new(data), 10_000).ok())
                .and_then(|(inner, _)| inner.decompressed_size())
                .map(|n| n.min(u64::MAX as u128) as u64);
            if let Some(size) = inner {
                nested = nested.saturating_add(size);
            }
        }
        let mut raw = archive.into_inner();
        for &(_, start, size, stored) in &designs {
            if !stored {
                continue;
            }
            if let Some(size) = bounded_archive(
                Window {
                    reader: &mut raw,
                    start,
                    size,
                    position: 0,
                },
                10_000,
            )
            .ok()
            .and_then(|(inner, _)| inner.decompressed_size())
            {
                nested = nested.saturating_add(size.min(u64::MAX as u128) as u64);
            }
        }
        (bytes.max(nested), members, parts.max(1))
    } else {
        (size, 0, 1)
    };
    check_sizes(
        Sizes {
            format,
            bytes,
            members,
        },
        parts,
        history,
        budget,
    )
}

fn check_sizes(sizes: Sizes, parts: u64, history: bool, budget: Budget) -> Result<(), Error> {
    let choices = Choices { history };
    // An assembly's own file is a list of parts, not the design: its memory is judged part by
    // part as they are read (`PartsLoad`), so only time and fuel are judged here.
    let assembly = matches!(
        sizes.format,
        Format::InventorAssembly | Format::SolidWorksAssembly
    );
    if let Some(fuel) = estimated_fuel(sizes, parts).filter(|fuel| *fuel > budget.fuel) {
        // `parts` counts the archive's design files (documents), not its components: a
        // 50-component design can sit in a handful of documents, so say what is counted.
        return Err(too_large(format!(
            "about {} of work for the {} design file{} in the archive, and the plugin's limit is {}",
            billions(fuel),
            parts,
            if parts == 1 { "" } else { "s" },
            billions(budget.fuel)
        )));
    }
    if let Some(estimate) = sizes.calculate(&choices) {
        if !assembly && estimate.peak_bytes > budget.peak_bytes {
            return Err(too_large(format!(
                "about {} of memory and the plugin has {}",
                megabytes(estimate.peak_bytes),
                megabytes(budget.peak_bytes)
            )));
        }
        if estimate.seconds > budget.seconds {
            return Err(too_large(format!(
                "about {} and the plugin stops a conversion after {}",
                minutes(estimate.seconds),
                minutes(budget.seconds)
            )));
        }
    }
    Ok(())
}

/// One plain line: which limit, what the design needs, what the plugin has, and where to go.
fn too_large(reason: String) -> Error {
    Error::MemoryBudget(format!(
        "This design is too large to open inside SolidCraft: it needs {reason}. Open it in the Dutchman app, which converts it to a .sc you can open here. Download the app: {DOWNLOAD}"
    ))
}
fn megabytes(bytes: u64) -> String {
    format!("{} MB", bytes.div_ceil(1_000_000))
}
fn minutes(seconds: u64) -> String {
    if seconds < 120 {
        format!("{seconds} seconds")
    } else {
        format!("{} minutes", seconds.div_ceil(60))
    }
}
fn billions(fuel: u64) -> String {
    format!("{} billion steps", fuel.div_ceil(1_000_000_000))
}

/// Memory for the files an assembly reads, judged before each one is read. The core reads one
/// part at a time and frees its bytes once it is decoded; what stays is the decoded design,
/// which grows with the sizes of the files read so far. A part is refused before its bytes are
/// requested, when the total so far plus this part would not fit.
#[cfg(any(test, target_arch = "wasm32"))]
pub(crate) struct PartsLoad {
    read: std::cell::Cell<u64>,
    peak_bytes: u64,
}
/// Memory held before any file is read: the module's own data, the stack and the writer.
#[cfg(any(test, target_arch = "wasm32"))]
const BASE_BYTES: u64 = 100_000_000;
/// Peak guest bytes per byte of CAD file read, measured on public Inventor assemblies
/// (docs/plugin.md, Measurements).
#[cfg(any(test, target_arch = "wasm32"))]
const BYTES_PER_FILE_BYTE: u64 = 60;
#[cfg(any(test, target_arch = "wasm32"))]
impl PartsLoad {
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn new(main_bytes: u64) -> Self {
        Self::with_peak(main_bytes, PEAK_BYTES)
    }
    pub(crate) fn with_peak(main_bytes: u64, peak_bytes: u64) -> Self {
        Self {
            read: std::cell::Cell::new(main_bytes),
            peak_bytes,
        }
    }
    /// Count a part of `bytes` that is about to be read.
    pub(crate) fn add(&self, bytes: u64) -> Result<(), Error> {
        let total = self.read.get().saturating_add(bytes);
        let needed = BASE_BYTES.saturating_add(total.saturating_mul(BYTES_PER_FILE_BYTE));
        if needed > self.peak_bytes {
            return Err(too_large(format!(
                "about {} of memory for the {} of files it reads, and the plugin has {}",
                megabytes(needed),
                megabytes(total),
                megabytes(self.peak_bytes)
            )));
        }
        self.read.set(total);
        Ok(())
    }
}

/// Fusion work envelope: 3 * (500M + 16K * expanded bytes + 100M * outer
/// entries + 500M * indexed documents). Bytes are max(outer expanded bytes,
/// summed inner expanded bytes); documents are not placed occurrences. No feature
/// or B-rep payload is decoded. Saturation refuses counts whose cost overflows.
///
/// Calibrated at f174dac3 before this update in Wasmtime 50.0.2, history enabled and real CLI
/// evaluation: arm (11,718,465 bytes / 4 entries / 2 documents),
/// 167,740,144,095 fuel; MIC6 bed (261,515 / 17 / 1), 4,710,741,081;
/// Mini inlet plate (260,519 / 17 / 1), 3,350,956,153; switch plate
/// (198,568 / 17 / 1), 2,412,586,136. The unmultiplied model bounds all four;
/// 3x reserves room for runtime differences and unseen feature complexity. The
/// manifest requests SolidCraft's default cap. RailCore Mini (110,045,607 / 45 / 43)
/// and 300ZL (74,341,274 / 71 / 69) exceed this budget as well as the old gate.
/// Other formats retain their existing memory/time checks until measured.
fn estimated_fuel(sizes: Sizes, parts: u64) -> Option<u64> {
    if !matches!(
        sizes.format,
        Format::FusionArchive | Format::FusionMultiDocument
    ) {
        return None;
    }
    Some(
        500_000_000u64
            .saturating_add(sizes.bytes.saturating_mul(16_000))
            .saturating_add(sizes.members.saturating_mul(100_000_000))
            .saturating_add(parts.saturating_mul(500_000_000))
            .saturating_mul(3),
    )
}

/// A seekable view of a stored ZIP member, restricted to that member's bytes.
struct Window<R> {
    reader: R,
    start: u64,
    size: u64,
    position: u64,
}
impl<R: Read + Seek> Read for Window<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.reader.seek(std::io::SeekFrom::Start(
            self.start.saturating_add(self.position),
        ))?;
        let length = buf
            .len()
            .min(usize::try_from(self.size.saturating_sub(self.position)).unwrap_or(usize::MAX));
        let count = self
            .reader
            .read(buf.get_mut(..length).unwrap_or_default())?;
        self.position = self.position.saturating_add(count as u64);
        Ok(count)
    }
}
impl<R> Seek for Window<R> {
    fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
        let next = match from {
            std::io::SeekFrom::Start(n) => i128::from(n),
            std::io::SeekFrom::End(n) => i128::from(self.size) + i128::from(n),
            std::io::SeekFrom::Current(n) => i128::from(self.position) + i128::from(n),
        };
        if next < 0 || next > i128::from(self.size) {
            return Err(std::io::Error::other("Archive index exceeds its file"));
        }
        self.position = next as u64;
        Ok(self.position)
    }
}

// Same directory bounds as the CLI's upload/index guard, including ZIP64.
/// Why a ZIP directory cannot be opened within its limits.
#[derive(Debug)]
pub enum ArchiveError {
    /// The directory cannot be read within the metadata bounds.
    Directory,
    /// The declared entry count exceeds the remaining project budget.
    Entries,
}

/// Opens a ZIP after bounding directory allocation, returning its raw entry count.
/// Directory data and ZIP64 extensible records are each limited to 16 MiB.
pub fn bounded_archive<R: Read + std::io::Seek>(
    mut file: R,
    max_entries: usize,
) -> Result<(zip::ZipArchive<R>, usize), ArchiveError> {
    let (members, directory_bytes, directory_offset, length) =
        archive_directory(&mut file).ok_or(ArchiveError::Directory)?;
    let members = usize::try_from(members).map_err(|_| ArchiveError::Entries)?;
    if members > max_entries {
        return Err(ArchiveError::Entries);
    }
    if directory_bytes > 16 << 20
        || directory_offset
            .checked_add(directory_bytes)
            .is_none_or(|end| end > length)
    {
        return Err(ArchiveError::Directory);
    }
    let archive = zip::ZipArchive::new(file).map_err(|_| ArchiveError::Directory)?;
    if archive.len() > max_entries {
        return Err(ArchiveError::Entries);
    }
    Ok((archive, members))
}

fn archive_directory<R: Read + std::io::Seek>(file: &mut R) -> Option<(u64, u64, u64, u64)> {
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
    Some((members, directory_bytes, directory_offset, length))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn manifest_fuel_matches_estimate_budget() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../plugin.json")).unwrap();
        assert_eq!(
            manifest["runtime"]["limits"]["fuel"].as_u64(),
            Some(PLUGIN_BUDGET.fuel)
        );
        assert_eq!(PLUGIN_FUEL, 100_000_000_000);
    }

    #[test]
    fn manifest_fuel_request_never_exceeds_solidcraft_default_cap() {
        const SOLIDCRAFT_DEFAULT_MAX_PLUGIN_FUEL: u64 = 100_000_000_000;
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../plugin.json")).unwrap();
        let requested = manifest["runtime"]["limits"]["fuel"].as_u64().unwrap();
        assert!(requested > 0 && requested <= SOLIDCRAFT_DEFAULT_MAX_PLUGIN_FUEL);
        const { assert!(PLUGIN_BUDGET.fuel <= SOLIDCRAFT_DEFAULT_MAX_PLUGIN_FUEL) };
    }

    #[test]
    fn design_above_fuel_budget_is_routed_to_the_app() {
        // Synthetic metadata: 40 documents is above 1e11 fuel, one is below it.
        let sizes = Sizes {
            format: Format::FusionArchive,
            bytes: 1_000_000,
            members: 4,
        };
        let budget = Budget {
            peak_bytes: u64::MAX,
            seconds: u64::MAX,
            ..PLUGIN_BUDGET
        };
        let parts = 40;
        assert!(estimated_fuel(sizes, parts).unwrap() > budget.fuel);
        let error = check_sizes(sizes, parts, true, budget).unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains("too large to open inside SolidCraft"),
            "{message}"
        );
        assert!(message.contains("Open it in the Dutchman app"), "{message}");
        assert!(!message.contains("Max plugin fuel"), "{message}");
        assert!(check_sizes(sizes, 1, true, budget).is_ok());
    }

    #[test]
    fn fuel_envelope_accepts_calibration_metadata() {
        // Numeric metadata only: no third-party design is a test fixture.
        for (format, bytes, members, parts, measured) in [
            (
                Format::FusionMultiDocument,
                11_718_465,
                4,
                2,
                167_740_144_095,
            ),
            (Format::FusionArchive, 261_515, 17, 1, 4_710_741_081),
            (Format::FusionArchive, 260_519, 17, 1, 3_350_956_153),
            (Format::FusionArchive, 198_568, 17, 1, 2_412_586_136),
        ] {
            let sizes = Sizes {
                format,
                bytes,
                members,
            };
            assert!(estimated_fuel(sizes, parts).unwrap() >= 3 * measured);
            for history in [false, true] {
                // The arm's estimate is above the 1e11 budget, so it is routed to the app.
                let fits = estimated_fuel(sizes, parts).unwrap() <= PLUGIN_BUDGET.fuel;
                assert_eq!(fits, measured < 100_000_000_000);
                let verdict = check_sizes(sizes, parts, history, PLUGIN_BUDGET);
                if fits {
                    assert!(verdict.is_ok());
                } else {
                    assert!(verdict.is_err());
                }
            }
        }
    }

    #[test]
    fn fuel_budget_boundary_is_inclusive_independently_of_memory_and_time() {
        let sizes = Sizes {
            format: Format::FusionMultiDocument,
            bytes: 11_718_465,
            members: 4,
        };
        let budget = Budget {
            peak_bytes: u64::MAX,
            seconds: u64::MAX,
            fuel: estimated_fuel(sizes, 2).unwrap(),
        };
        assert!(check_sizes(sizes, 2, true, budget).is_ok());
        let error = check_sizes(
            sizes,
            2,
            true,
            Budget {
                fuel: budget.fuel - 1,
                ..budget
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("Open it in the Dutchman app"));
    }

    #[test]
    fn fuel_gate_bounds_bytes_entries_documents_and_overflow() {
        let budget = Budget {
            peak_bytes: u64::MAX,
            seconds: u64::MAX,
            ..PLUGIN_BUDGET
        };
        for (bytes, members, parts) in [
            (110_045_607, 45, 43), // RailCore Mini metadata.
            (74_341_274, 71, 69),  // RailCore 300ZL metadata.
            (24_000_000, 4, 2),    // Same document count as the arm, more work.
            (1, 10_000, 1),
            (1, 1, 1_000),
            (u64::MAX, u64::MAX, u64::MAX),
        ] {
            let sizes = Sizes {
                format: Format::FusionMultiDocument,
                bytes,
                members,
            };
            assert!(check_sizes(sizes, parts, true, budget).is_err());
        }
        assert_eq!(
            estimated_fuel(
                Sizes {
                    format: Format::FusionArchive,
                    bytes: u64::MAX,
                    members: u64::MAX,
                },
                u64::MAX
            ),
            Some(u64::MAX)
        );
    }

    #[test]
    fn memory_and_time_budget_boundaries_are_inclusive() {
        let format = Format::InventorPart;
        let size = 100;
        let estimate = Sizes {
            format,
            bytes: size,
            members: 0,
        }
        .calculate(&Choices { history: true })
        .unwrap();
        let budget = Budget {
            peak_bytes: estimate.peak_bytes,
            seconds: estimate.seconds,
            fuel: u64::MAX,
        };
        assert!(check_with(format, size, std::io::Cursor::new([]), true, budget).is_ok());
        assert!(check_with(
            format,
            size,
            std::io::Cursor::new([]),
            true,
            Budget {
                peak_bytes: budget.peak_bytes - 1,
                ..budget
            }
        )
        .is_err());
        assert!(check_with(
            format,
            size,
            std::io::Cursor::new([]),
            true,
            Budget {
                seconds: budget.seconds - 1,
                ..budget
            }
        )
        .is_err());
    }
    #[test]
    fn memory_budget_is_the_manifest_memory_less_headroom() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../plugin.json")).unwrap();
        let declared = manifest["runtime"]["limits"]["memory_mib"]
            .as_u64()
            .unwrap();
        assert_eq!(declared, GUEST_MEMORY_MIB);
        let memory = GUEST_MEMORY_MIB << 20;
        assert!(PEAK_BYTES < memory && PEAK_BYTES * 100 >= memory * 75);
    }

    #[test]
    fn refusals_say_which_limit_with_needed_and_available_in_one_line() {
        let sizes = |format, bytes| Sizes {
            format,
            bytes,
            members: 0,
        };
        // Memory: a part far above what the guest can hold.
        let message = check_sizes(
            sizes(Format::InventorPart, 50_000_000),
            1,
            true,
            PLUGIN_BUDGET,
        )
        .unwrap_err()
        .to_string();
        assert!(
            message.contains("it needs about ")
                && message.contains(" MB of memory and the plugin has 1700 MB"),
            "{message}"
        );
        // Time: the host stops a conversion at the manifest's deadline.
        let budget = Budget {
            peak_bytes: u64::MAX,
            seconds: 300,
            ..PLUGIN_BUDGET
        };
        let message = check_sizes(sizes(Format::InventorPart, 150_000_000), 1, true, budget)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("minutes and the plugin stops a conversion after 5 minutes"),
            "{message}"
        );
        // Fuel.
        let message = check_sizes(
            sizes(Format::FusionArchive, 1_000_000),
            40,
            true,
            PLUGIN_BUDGET,
        )
        .unwrap_err()
        .to_string();
        assert!(
            message.contains("billion steps of work for the 40 design files in the archive, and the plugin's limit is 100 billion steps"),
            "{message}"
        );
        assert_eq!(message.lines().count(), 1, "one line: {message}");
        assert!(
            message.starts_with("This design is too large to open inside SolidCraft: it needs ")
        );
        assert!(message.contains("Open it in the Dutchman app") && message.contains(DOWNLOAD));
    }

    #[test]
    fn a_many_component_f3z_with_few_documents_is_refused_by_work_and_says_so() {
        // 4 indexed design files holding many components: the count in the message is the
        // design files the estimate uses, labelled as such, never "parts".
        let sizes = Sizes {
            format: Format::FusionMultiDocument,
            bytes: 400_000_000,
            members: 120,
        };
        let message = check_sizes(sizes, 4, true, PLUGIN_BUDGET)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("billion steps of work for the 4 design files in the archive, and the plugin's limit is 100 billion steps"),
            "{message}"
        );
        assert!(!message.contains("parts"), "{message}");
        assert!(!message.contains("about 4"), "{message}");
        assert_eq!(message.lines().count(), 1, "{message}");
        // One design file is singular.
        let message = check_sizes(sizes, 1, true, PLUGIN_BUDGET)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("for the 1 design file in the archive"),
            "{message}"
        );
    }

    #[test]
    fn an_assembly_file_is_not_judged_by_its_own_size() {
        // The 5.3 MB top-level Inventor assembly of a public 67-part design: its own bytes said
        // 577 MB of memory under the part-sized model, over the old 520 MB gate.
        let sizes = Sizes {
            format: Format::InventorAssembly,
            bytes: 5_342_208,
            members: 0,
        };
        assert!(check_sizes(sizes, 1, true, PLUGIN_BUDGET).is_ok());
        // A part of the same size is still judged by its own memory.
        let part = Sizes {
            format: Format::InventorPart,
            bytes: 50_000_000,
            members: 0,
        };
        assert!(check_sizes(part, 1, true, PLUGIN_BUDGET).is_err());
    }

    #[test]
    fn parts_are_counted_before_they_are_read_and_a_refusal_names_both_amounts() {
        let load = PartsLoad::with_peak(5_000_000, 1_000_000_000);
        // 5 MB assembly + 4 MB parts: 100 MB + 60 * 9 MB = 640 MB, within 1 GB.
        assert!(load.add(2_000_000).is_ok());
        assert!(load.add(2_000_000).is_ok());
        let message = load.add(8_000_000).unwrap_err().to_string();
        assert!(
            message.contains("needs about 1000 MB of memory for the 17 MB of files it reads, and the plugin has 1000 MB")
                || message.contains("MB of memory for the 17 MB of files it reads, and the plugin has 1000 MB"),
            "{message}"
        );
        assert_eq!(message.lines().count(), 1, "{message}");
        assert!(message.contains("Open it in the Dutchman app"), "{message}");
        // The refused part was not counted: a smaller one still fits.
        assert!(load.add(1_000_000).is_ok());
    }

    #[test]
    fn compressed_inner_archive_work_is_bounded_before_inflation() {
        use std::io::{Cursor, Write};
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(
            "synthetic.f3d",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
        zip.write_all(b"not CAD").unwrap();
        let mut data = zip.finish().unwrap().into_inner();
        let central = data.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        data[central + 24..central + 28].copy_from_slice(&(17u32 << 20).to_le_bytes());
        // An unlimited estimate budget isolates the inner inflation bound.
        let error = check_with(
            Format::FusionMultiDocument,
            data.len() as u64,
            Cursor::new(&data),
            true,
            Budget {
                peak_bytes: u64::MAX,
                seconds: u64::MAX,
                fuel: u64::MAX,
            },
        )
        .unwrap_err();
        assert!(matches!(error, Error::MemoryBudget(_)));
        assert!(error.to_string().contains("a larger archive check"));
    }

    #[test]
    fn stored_nested_archive_is_indexed_without_reading_payload() {
        use std::{
            cell::Cell,
            io::{Cursor, Write},
        };
        let mut inner = zip::ZipWriter::new(Cursor::new(Vec::new()));
        inner
            .start_file("synthetic", zip::write::SimpleFileOptions::default())
            .unwrap();
        inner.write_all(b"not CAD").unwrap();
        let mut data = inner.finish().unwrap().into_inner();
        let central = data.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        data[central + 24..central + 28].copy_from_slice(&400_000_000u32.to_le_bytes());
        let mut outer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        outer
            .start_file(
                "part.f3d",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        outer.write_all(&data).unwrap();
        let data = outer.finish().unwrap().into_inner();
        struct Counted<'a> {
            data: Cursor<&'a [u8]>,
            read: &'a Cell<usize>,
        }
        impl Read for Counted<'_> {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
                let n = self.data.read(buf)?;
                self.read.set(self.read.get() + n);
                Ok(n)
            }
        }
        impl Seek for Counted<'_> {
            fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
                self.data.seek(from)
            }
        }
        let read = Cell::new(0);
        let error = check(
            Format::FusionMultiDocument,
            data.len() as u64,
            Counted {
                data: Cursor::new(&data),
                read: &read,
            },
            true,
        )
        .unwrap_err();
        assert!(matches!(error, Error::MemoryBudget(_)));
        assert!(read.get() < 4096, "Only bounded ZIP metadata is needed");
    }
}
