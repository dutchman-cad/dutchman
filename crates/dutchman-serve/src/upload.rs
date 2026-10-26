//! Uploaded files: safe names on disk, and project zips unpacked within limits. Every name and
//! size here comes from the browser, so all of it is treated as hostile.

use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

/// The most characters kept of an uploaded file's name.
const MAX_NAME: usize = 200;

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
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|_| format!("{label} could not be opened as a zip file"))?;
    if zip.len() > limits.entries {
        return Err(format!(
            "{label} holds more than {} entries; zip a smaller folder",
            limits.entries
        ));
    }

    limits.entries -= zip.len();
    let mut names = Vec::with_capacity(zip.len());
    for index in 0..zip.len() {
        let entry = zip
            .by_index(index)
            .map_err(|_| format!("{label} could not be read"))?;
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
