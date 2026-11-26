//! The one way Dutchman writes a file the user keeps: a design, a report, a log.
//!
//! Every write is atomic (a temporary file in the same folder, written and synced, then moved
//! into place), so a failure or a stop part-way leaves the user's files as they were and no
//! stray temporary file. A file is never truncated in place, and nothing is replaced unless the
//! caller passes `replace`, which only the user's explicit opt-in sets.
//!
//! Output slots: the design file a conversion writes and the side files kept beside it
//! (`.report.json`, `.failed.json`, `.replay.json`). A conversion never replaces a file
//! Dutchman did not write in that slot: when the name is taken it uses the next free one,
//! `name (2).sc`, `name (3).sc`, and so on, with its side files carrying the same number.

use std::path::{Path, PathBuf};

/// The side files kept beside a design, by the suffix that replaces its extension.
pub const SIDE_SUFFIXES: [&str; 3] = ["report.json", "failed.json", "replay.json"];

/// Whether something is already at `path`. A path that cannot be checked counts as taken.
fn present(path: &Path) -> bool {
    path.try_exists().unwrap_or(true)
}

/// Whether the design at `path`, or any side file beside it, already exists.
pub fn slot_taken(path: &Path) -> bool {
    present(path)
        || SIDE_SUFFIXES
            .iter()
            .any(|suffix| present(&path.with_extension(suffix)))
}

/// `path` with ` (n)` added to its name before the extension: `part.sc` is `part (2).sc`.
pub fn numbered(path: &Path, n: u32) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut name = std::ffi::OsString::from(format!("{stem} ({n})"));
    if let Some(extension) = path.extension() {
        name.push(".");
        name.push(extension);
    }
    path.with_file_name(name)
}

/// `path` when `taken` does not hold it, else the first of `path (2)`, `path (3)`, ... that
/// `taken` does not hold.
pub fn next_free(path: &Path, taken: &dyn Fn(&Path) -> bool) -> PathBuf {
    if !taken(path) {
        return path.to_path_buf();
    }
    let mut n = 2_u32;
    loop {
        let candidate = numbered(path, n);
        if !taken(&candidate) || n == u32::MAX {
            return candidate;
        }
        n += 1;
    }
}

/// The design path to write: `path` when its slot is free, else the next free numbered one.
pub fn free_slot(path: &Path) -> PathBuf {
    next_free(path, &slot_taken)
}

/// Whether `candidate` is `base`, or `base` numbered, in the same folder: the places one
/// input's output can have landed in an earlier run.
pub fn is_variant_of(candidate: &Path, base: &Path) -> bool {
    if candidate.parent() != base.parent() || candidate.extension() != base.extension() {
        return false;
    }
    let (Some(name), Some(stem)) = (candidate.file_stem(), base.file_stem()) else {
        return false;
    };
    let (name, stem) = (name.to_string_lossy(), stem.to_string_lossy());
    name == stem
        || name
            .strip_prefix(&*stem)
            .and_then(|rest| rest.strip_prefix(" ("))
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether `a` and `b` are the same file: the same path once symlinks are followed, or two
/// names of one file (a hard link). A `b` that does not exist yet is the same as `a` only when
/// its folder and name are.
pub fn same_file(a: &Path, b: &Path) -> bool {
    let (Ok(first), Some(second)) = (
        std::fs::canonicalize(a),
        std::fs::canonicalize(b).ok().or_else(|| {
            let parent = b.parent().filter(|p| !p.as_os_str().is_empty());
            let folder = std::fs::canonicalize(parent.unwrap_or(Path::new("."))).ok()?;
            Some(folder.join(b.file_name()?))
        }),
    ) else {
        return false;
    };
    if first == second {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        if let (Ok(x), Ok(y)) = (std::fs::metadata(&first), std::fs::metadata(&second)) {
            return x.dev() == y.dev() && x.ino() == y.ino();
        }
    }
    false
}

/// Fills a new temporary file beside `path` with `fill`, syncs it, and moves it to `path`:
/// over what is there when `replace` is set (atomically), else only if nothing is there
/// ([`std::io::ErrorKind::AlreadyExists`] otherwise). The temporary file is removed on any
/// failure, and `path` is then as it was.
pub fn write_with(
    path: &Path,
    replace: bool,
    fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    write_with_mode(path, replace, None, fill)
}

fn write_with_mode(
    path: &Path,
    replace: bool,
    private: Option<u32>,
    fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".dutchman-")
        .suffix(".tmp")
        .tempfile_in(parent)?;
    fill(temporary.as_file_mut())?;
    // A temporary file is private to its owner; the design is not. A replaced file keeps its mode.
    #[cfg(not(unix))]
    let _ = private;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = if let Some(private) = private {
            private
        } else if replace {
            std::fs::metadata(path).map_or(0o644, |m| m.permissions().mode() & 0o7777)
        } else {
            0o644
        };
        temporary
            .as_file()
            .set_permissions(std::fs::Permissions::from_mode(mode))?;
    }
    temporary.as_file().sync_all()?;
    if replace {
        temporary.persist(path).map(drop).map_err(|e| e.error)
    } else {
        temporary
            .persist_noclobber(path)
            .map(drop)
            .map_err(|e| e.error)
    }
}

/// [`write`] with replacing, for a file Dutchman owns outright (a report it regenerates).
pub fn write_own(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    write(path, bytes, true)
}

/// [`write_own`], readable by its owner only (a private report or key file).
pub fn write_own_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    write_with_mode(path, true, Some(0o600), |file| file.write_all(bytes))
}

/// Writes `bytes` to exactly `path`, atomically (see [`write_with`]).
pub fn write(path: &Path, bytes: &[u8], replace: bool) -> std::io::Result<()> {
    use std::io::Write as _;
    write_with(path, replace, |file| file.write_all(bytes))
}

/// Writes `bytes` to `path`, which is replaced only when `replace` is set; otherwise to the
/// first free numbered name. Returns where it was written.
pub fn write_new(path: &Path, bytes: &[u8], replace: bool) -> std::io::Result<PathBuf> {
    if replace {
        write(path, bytes, true)?;
        return Ok(path.to_path_buf());
    }
    let mut candidate = path.to_path_buf();
    let mut n = 2_u32;
    loop {
        match write(&candidate, bytes, false) {
            Ok(()) => return Ok(candidate),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists && n < u32::MAX => {
                candidate = numbered(path, n);
                n += 1;
            }
            Err(err) => return Err(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_before_the_extension() {
        assert_eq!(
            numbered(Path::new("a/Toob v2.sc"), 2),
            Path::new("a/Toob v2 (2).sc")
        );
    }

    #[test]
    fn side_files_take_a_slot() {
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("part.sc");
        assert_eq!(free_slot(&design), design);
        std::fs::write(dir.path().join("part.report.json"), b"mine").unwrap();
        assert_eq!(free_slot(&design), dir.path().join("part (2).sc"));
        std::fs::write(&design, b"mine").unwrap();
        std::fs::write(dir.path().join("part (2).failed.json"), b"mine").unwrap();
        assert_eq!(free_slot(&design), dir.path().join("part (3).sc"));
    }

    #[test]
    fn variants_are_the_base_or_numbered() {
        let base = Path::new("o/part.sc");
        assert!(is_variant_of(Path::new("o/part.sc"), base));
        assert!(is_variant_of(Path::new("o/part (12).sc"), base));
        assert!(!is_variant_of(Path::new("o/part (x).sc"), base));
        assert!(!is_variant_of(Path::new("o/part2.sc"), base));
        assert!(!is_variant_of(Path::new("p/part.sc"), base));
    }

    #[test]
    fn a_write_that_fails_part_way_leaves_the_file_and_no_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.sc");
        std::fs::write(&path, b"mine").unwrap();
        for replace in [false, true] {
            let err = write_with(&path, replace, |file| {
                use std::io::Write as _;
                file.write_all(b"half of a new design")?;
                Err(std::io::Error::other(
                    "stopped after the temporary file was written",
                ))
            })
            .unwrap_err();
            assert_eq!(
                err.to_string(),
                "stopped after the temporary file was written"
            );
            assert_eq!(std::fs::read(&path).unwrap(), b"mine");
            assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        }
    }

    #[cfg(unix)]
    #[test]
    fn replacing_never_writes_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.sc");
        std::fs::write(&real, b"the user's file").unwrap();
        // A hard link keeps the old bytes: the new file is a new file.
        let linked = dir.path().join("linked.sc");
        std::fs::hard_link(&real, &linked).unwrap();
        write(&real, b"new", true).unwrap();
        assert_eq!(std::fs::read(&real).unwrap(), b"new");
        assert_eq!(std::fs::read(&linked).unwrap(), b"the user's file");
        // A symlink is replaced, and what it pointed to is unchanged.
        let target = dir.path().join("target.sc");
        std::fs::write(&target, b"elsewhere").unwrap();
        let link = dir.path().join("link.sc");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        write(&link, b"new", true).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"elsewhere");
        assert_eq!(std::fs::read(&link).unwrap(), b"new");
        assert!(!std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn the_same_file_is_found_through_links_and_new_names() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.f3d");
        std::fs::write(&a, b"x").unwrap();
        let hard = dir.path().join("hard.f3d");
        std::fs::hard_link(&a, &hard).unwrap();
        let soft = dir.path().join("soft.f3d");
        std::os::unix::fs::symlink(&a, &soft).unwrap();
        assert!(same_file(&a, &a));
        assert!(same_file(&a, &hard));
        assert!(same_file(&a, &soft));
        assert!(same_file(&a, &dir.path().join("./a.f3d")));
        assert!(!same_file(&a, &dir.path().join("b.sc")));
        assert!(!same_file(&a, &dir.path().join("missing").join("a.f3d")));
    }

    #[test]
    fn write_never_replaces_unless_asked_and_replaces_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.sc");
        write(&path, b"first", false).unwrap();
        let err = write(&path, b"second", false).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert_eq!(std::fs::read(&path).unwrap(), b"first");
        write(&path, b"second", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"second");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn write_new_keeps_what_is_there() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, b"old").unwrap();
        let second = write_new(&path, b"new", false).unwrap();
        assert_eq!(second, dir.path().join("a (2).txt"));
        assert_eq!(std::fs::read(&path).unwrap(), b"old");
        assert_eq!(write_new(&path, b"new", true).unwrap(), path);
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
    }
}
