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

/// A name folded the way the strictest filesystem folds it: Unicode-normalised (the accented
/// `e` as one character or as `e` plus a combining accent is one name on macOS) and
/// case-folded (macOS and Windows keep one file for `Part.sc` and `part.sc`). Two names with
/// the same key must never be two output files, so the output is the same on every system.
pub fn portable_key(name: &str) -> String {
    let nfd = icu_normalizer::DecomposingNormalizer::new_nfd();
    nfd.normalize(&nfd.normalize(name).to_lowercase())
        .into_owned()
}

/// [`portable_key`] of a whole path, folder names included. The key is made from the path's
/// components, so `out/part.sc` and `out\part.sc` (what Windows makes of it) are one key.
pub fn portable_path_key(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .map(|part| match part {
            std::path::Component::RootDir => String::new(),
            other => other.as_os_str().to_string_lossy().into_owned(),
        })
        .collect();
    portable_key(&parts.join("/"))
}

/// Whether something is already at `path`, by the filesystem's own answer, or a file in that
/// folder has a name that is `path`'s apart from normalisation and case (the same file on
/// macOS and Windows). A path that cannot be checked counts as taken.
pub fn present(path: &Path) -> bool {
    match path.try_exists() {
        Ok(true) | Err(_) => return true,
        Ok(false) => {}
    }
    let Some(name) = path.file_name() else {
        return false;
    };
    let wanted = portable_key(&name.to_string_lossy());
    let folder = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    match std::fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .any(|entry| portable_key(&entry.file_name().to_string_lossy()) == wanted),
        // A folder that is not there holds nothing; one that cannot be listed might.
        Err(err) => err.kind() != std::io::ErrorKind::NotFound,
    }
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
    let key = |p: Option<&Path>| p.map(portable_path_key);
    let ext = |p: &Path| p.extension().map(|e| portable_key(&e.to_string_lossy()));
    if key(candidate.parent()) != key(base.parent()) || ext(candidate) != ext(base) {
        return false;
    }
    let (Some(name), Some(stem)) = (candidate.file_stem(), base.file_stem()) else {
        return false;
    };
    let (name, stem) = (
        portable_key(&name.to_string_lossy()),
        portable_key(&stem.to_string_lossy()),
    );
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
    // Names that differ only in normalisation or case in one folder are one file on macOS and
    // Windows, so they count as one here too (the safe side for "never write over the input").
    if first.parent() == second.parent()
        && first
            .file_name()
            .zip(second.file_name())
            .is_some_and(|(x, y)| {
                portable_key(&x.to_string_lossy()) == portable_key(&y.to_string_lossy())
            })
    {
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

/// What happens to a file a write replaces: it is moved out of the way, never destroyed.
pub type Retire = fn(&Path) -> std::io::Result<()>;

static RETIRE: std::sync::OnceLock<Retire> = std::sync::OnceLock::new();

/// Sets what becomes of a replaced file (the program sends it to the trash); once only.
pub fn set_retire(retire: Retire) {
    let _ = RETIRE.set(retire);
}

/// Keeps `path` beside itself as `<name>.replaced-<time>.<extension>`. The default, and what
/// the trash falls back to.
pub fn keep_as_replaced(path: &Path) -> std::io::Result<()> {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut n = 0_u32;
    loop {
        let mut name = std::ffi::OsString::from(format!("{stem}.replaced-{seconds}"));
        if n > 0 {
            name.push(format!("-{n}"));
        }
        if let Some(extension) = path.extension() {
            name.push(".");
            name.push(extension);
        }
        let target = path.with_file_name(name);
        if !present(&target) {
            return std::fs::rename(path, target);
        }
        n += 1;
    }
}

fn retire(path: &Path) -> std::io::Result<()> {
    RETIRE.get().copied().unwrap_or(keep_as_replaced)(path)
}

/// Fills a new temporary file beside `path` with `fill`, syncs it, and moves it to `path`:
/// over what is there when `replace` is set (atomically), else only if nothing is there
/// ([`std::io::ErrorKind::AlreadyExists`] otherwise). The temporary file is removed on any
/// failure, and `path` is then as it was. A file replaced is retired first (see [`set_retire`]),
/// never overwritten in place.
pub fn write_with(
    path: &Path,
    replace: bool,
    fill: impl FnOnce(&mut std::fs::File) -> std::io::Result<()>,
) -> std::io::Result<()> {
    write_with_mode(path, replace, true, None, fill)
}

fn write_with_mode(
    path: &Path,
    replace: bool,
    retire_old: bool,
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
    if replace && !retire_old {
        return temporary.persist(path).map(drop).map_err(|e| e.error);
    }
    // A name that only matches an existing file apart from case is that file on macOS and
    // Windows, where it exists; on a case-sensitive filesystem there is nothing to move.
    if replace && path.try_exists().unwrap_or(true) {
        retire(path)?;
    }
    temporary
        .persist_noclobber(path)
        .map(drop)
        .map_err(|e| e.error)
}

/// For a file Dutchman owns outright (a report it regenerates): replaced in one atomic step, with
/// no retired copy.
pub fn write_own(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    write_with_mode(path, true, false, None, |file| file.write_all(bytes))
}

/// [`write_own`], readable by its owner only (a private report or key file).
pub fn write_own_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    write_with_mode(path, true, false, Some(0o600), |file| file.write_all(bytes))
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
        if n < u32::MAX && present(&candidate) {
            candidate = numbered(path, n);
            n += 1;
            continue;
        }
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
    fn a_replaced_file_is_kept_beside_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.sc");
        write(&path, b"old", false).unwrap();
        write(&path, b"new", true).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let kept: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .filter(|name| name.starts_with("a.replaced-") && name.ends_with(".sc"))
            .collect();
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert_eq!(std::fs::read(dir.path().join(&kept[0])).unwrap(), b"old");
        // A file Dutchman owns is replaced in one step, with no copy.
        write_own(&path, b"newer").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"newer");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
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
    }

    const NFC: &str = "caf\u{e9}";
    const NFD: &str = "cafe\u{301}";

    #[test]
    fn a_path_key_ignores_separators() {
        assert_eq!(
            portable_path_key(Path::new("out/part.sc")),
            portable_path_key(&Path::new("out").join("part.sc"))
        );
        assert_ne!(
            portable_path_key(Path::new("out/part.sc")),
            portable_path_key(Path::new("out/other.sc"))
        );
    }

    #[test]
    fn portable_keys_fold_normalisation_and_case() {
        assert_eq!(portable_key(NFC), portable_key(NFD));
        assert_eq!(portable_key("Part.SC"), portable_key("part.sc"));
        assert_eq!(portable_key("CAF\u{c9}"), portable_key(NFD));
        assert_ne!(portable_key("part.sc"), portable_key("part2.sc"));
    }

    #[test]
    fn a_name_that_differs_by_normalisation_or_case_takes_the_slot() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(format!("{NFC}.sc")), b"mine").unwrap();
        std::fs::write(dir.path().join("Part.sc"), b"mine").unwrap();
        // NFD asks for the NFC file's slot; a lower-case name asks for the capitalised one's.
        assert_eq!(
            free_slot(&dir.path().join(format!("{NFD}.sc"))),
            dir.path().join(format!("{NFD} (2).sc"))
        );
        assert_eq!(
            free_slot(&dir.path().join("part.sc")),
            dir.path().join("part (2).sc")
        );
        // A side file with a variant name takes the slot too.
        std::fs::write(dir.path().join("Other.REPORT.json"), b"mine").unwrap();
        assert!(slot_taken(&dir.path().join("other.sc")));
        assert!(!slot_taken(&dir.path().join("another.sc")));
    }

    #[test]
    fn write_new_never_lands_on_a_variant_name() {
        let dir = tempfile::tempdir().unwrap();
        let first = write_new(&dir.path().join(format!("{NFC}.txt")), b"one", false).unwrap();
        let second = write_new(&dir.path().join(format!("{NFD}.txt")), b"two", false).unwrap();
        let third = write_new(
            &dir.path().join(format!("{}.TXT", NFC.to_uppercase())),
            b"x",
            false,
        )
        .unwrap();
        assert_eq!(first, dir.path().join(format!("{NFC}.txt")));
        assert_eq!(second, dir.path().join(format!("{NFD} (2).txt")));
        assert_ne!(third, first);
        assert_ne!(third, second);
        assert_eq!(std::fs::read(&first).unwrap(), b"one");
        assert_eq!(std::fs::read(&second).unwrap(), b"two");
    }

    #[test]
    fn variants_and_same_file_ignore_normalisation_and_case() {
        let base = PathBuf::from(format!("o/{NFC}.sc"));
        assert!(is_variant_of(Path::new(&format!("o/{NFD}.sc")), &base));
        assert!(is_variant_of(Path::new(&format!("o/{NFD} (3).SC")), &base));
        assert!(!is_variant_of(Path::new(&format!("o/{NFD}2.sc")), &base));
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join(format!("{NFC}.f3d"));
        std::fs::write(&a, b"x").unwrap();
        assert!(same_file(&a, &dir.path().join(format!("{NFD}.f3d"))));
        assert!(same_file(
            &a,
            &dir.path().join(format!("{}.F3D", NFC.to_uppercase()))
        ));
        assert!(!same_file(&a, &dir.path().join(format!("{NFD}x.f3d"))));
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
