//! Bounded ZIP metadata only; no native model is decoded here.
use dutchman_core::{
    estimate::{Choices, Sizes},
    Error, Format,
};
use std::io::{Read, Seek};

/// The plugin's fuel, not its memory, is what a large design runs out of. Measured in
/// SolidCraft 0.45 with the manifest's fuel: a design estimated at 464 MB peak opens, and
/// one estimated at 600 MB stops for fuel. The estimate stands in for the work involved.
pub(crate) const PLUGIN_BUDGET: Budget = Budget {
    peak_bytes: 520_000_000,
    seconds: 300,
};
#[derive(Clone, Copy)]
pub(crate) struct Budget {
    pub peak_bytes: u64,
    pub seconds: u64,
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
    if let Some(estimate) = sizes.calculate(&choices) {
        if estimate.peak_bytes > budget.peak_bytes || estimate.seconds > budget.seconds {
            return Err(Error::MemoryBudget(format!(
                "This design is too large to open inside SolidCraft (about {parts} {}). Open it in the Dutchman app, which converts it to a .sc you can open here. Download the app: {DOWNLOAD}", if parts == 1 { "part" } else { "parts" })));
        }
    }
    Ok(())
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
