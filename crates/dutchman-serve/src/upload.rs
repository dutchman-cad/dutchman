//! Uploaded files: safe names on disk, and project zips unpacked within limits. Every name and
//! size here comes from the browser, so all of it is treated as hostile.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

/// The most characters kept of an uploaded file's name.
const MAX_NAME: usize = 200;

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

/// What all zips of one upload may unpack to.
#[derive(Clone, Copy, Debug)]
pub struct ZipLimits {
    /// The most entries it may hold.
    pub entries: usize,
    /// The most bytes it may unpack to, all entries together.
    pub bytes: u64,
}

/// A file name for an upload: its last path component, with characters that are unsafe in a
/// file name on any system replaced. Never empty, never `.` or `..`.
pub fn safe_name(name: &str) -> String {
    let last = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = last
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(MAX_NAME)
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == '.' || c.is_whitespace());
    if trimmed.is_empty() {
        "upload".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// `path`, or the first of `name (1).ext`, `name (2).ext`, … beside it that does not exist.
pub fn unique(path: &Path) -> io::Result<PathBuf> {
    if !path.exists() {
        return Ok(path.to_path_buf());
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    for n in 1..10_000 {
        let candidate = path.with_file_name(format!("{stem} ({n}){extension}"));
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(io::Error::other("too many files with the same name"))
}

/// Unpacks the zip at `archive` into `dest` and removes it. A zip of one folder unpacks as that
/// folder; one with several entries at its top unpacks into a folder named after the zip.
/// Refuses entries that would land outside that folder, skips symbolic links, and stops when
/// the remaining job budget is exhausted, whatever the headers claim. Subtracts the entries
/// and actual unpacked bytes from `limits` for the next zip in the upload.
pub fn unzip(archive: &Path, dest: &Path, limits: &mut ZipLimits) -> Result<(), String> {
    let label = archive
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let file = File::open(archive).map_err(|err| format!("{label}: {err}"))?;
    let (mut zip, entries) = bounded_archive(file, limits.entries).map_err(|error| match error {
        ArchiveError::Entries => format!("{label} holds more than {} entries; zip a smaller folder", limits.entries),
        ArchiveError::Directory => format!("{label} could not be opened as a zip file within the directory limit of 16 MB; zip the project folder itself"),
    })?;
    limits.entries -= entries;
    let mut names = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|_| format!("{label} could not be read"))?;
        if entry
            .name()
            .is_ok_and(|name| name.contains('\\') || name.contains(':'))
        {
            return Err(format!(
                "{label} holds a path that is not portable; zip the project folder itself"
            ));
        }
        let Some(name) = entry.enclosed_name() else {
            return Err(format!(
                "{label} holds a path outside its own folder ({}); zip the project folder itself",
                entry.name().unwrap_or_default()
            ));
        };
        names.push((index, name, entry.is_dir(), entry.is_symlink()));
    }

    let tops: BTreeSet<_> = names
        .iter()
        .filter_map(|(_, name, _, _)| name.components().next())
        .collect();
    let nested = tops.len() == 1
        && names
            .iter()
            .all(|(_, name, dir, _)| *dir || name.components().count() > 1);
    let root = match tops.iter().next() {
        Some(Component::Normal(top)) if nested && !dest.join(top).exists() => dest.to_path_buf(),
        _ => {
            let stem = archive
                .file_stem()
                .map(|s| safe_name(&s.to_string_lossy()))
                .unwrap_or_else(|| "project".to_owned());
            unique(&dest.join(stem)).map_err(|err| format!("{label}: {err}"))?
        }
    };

    let mut left = limits.bytes;
    for (index, name, dir, link) in names {
        if link {
            continue;
        }
        let target = root.join(&name);
        if dir {
            fs::create_dir_all(&target).map_err(|err| format!("{label}: {err}"))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|err| format!("{label}: {err}"))?;
        }
        let entry = zip
            .by_index(index)
            .map_err(|_| format!("{label} could not be read"))?;
        let mut out = File::create(&target).map_err(|err| format!("{label}: {err}"))?;
        let copied = io::copy(&mut entry.take(left.saturating_add(1)), &mut out)
            .map_err(|_| format!("{label} could not be unpacked"))?;
        left = left.checked_sub(copied).ok_or_else(|| {
            format!(
                "{label} unpacks to more than {} MB; zip a smaller folder",
                limits.bytes >> 20
            )
        })?;
    }
    limits.bytes = left;
    drop(zip);
    fs::remove_file(archive).map_err(|err| format!("{label}: {err}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(clippy::unwrap_used)]
    fn empty_zip() -> Vec<u8> {
        let zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn entry_count_is_refused_before_opening_the_directory() {
        let mut bytes = empty_zip();
        bytes[8..10].copy_from_slice(&60_000_u16.to_le_bytes());
        bytes[10..12].copy_from_slice(&60_000_u16.to_le_bytes());
        assert!(matches!(
            bounded_archive(std::io::Cursor::new(bytes), 100),
            Err(ArchiveError::Entries)
        ));
    }

    #[test]
    fn directory_metadata_is_bounded_before_opening() {
        let mut bytes = empty_zip();
        bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(matches!(
            bounded_archive(std::io::Cursor::new(bytes), 100),
            Err(ArchiveError::Directory)
        ));
    }
}
