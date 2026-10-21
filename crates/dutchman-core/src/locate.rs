//! Finding the files an Inventor assembly places, as Inventor does: at the path the assembly
//! stores (relative to it, else absolute with the saving machine's drive or root moved onto
//! this one), else by name in the project's folders: in the most of the stored folders, then
//! nearest the assembly.

use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The most files and folders one project search looks through.
pub const MAX_SEARCHED: usize = 200_000;
/// How deep below the project folder a search goes.
const MAX_SEARCH_DEPTH: usize = 32;
/// How many folders above an assembly the search reaches when no project folder is given
/// and none holds an Inventor project file (`.ipj`).
const DEFAULT_LEVELS: usize = 2;
/// The most folders a stored path may name before its file.
const MAX_COMPONENTS: usize = 64;

/// Where to look for the files an assembly places, beyond the paths it stores.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartSearch {
    /// The project folder, searched with its subfolders. Without one, it is the nearest
    /// folder above the assembly that holds an Inventor project file (`.ipj`), else the
    /// folder two above the assembly's.
    pub project: Option<PathBuf>,
    /// More folders, searched with their subfolders, in order, for files the project folder
    /// does not hold.
    pub also: Vec<PathBuf>,
}

impl PartSearch {
    /// A search of `project` alone.
    pub fn project(project: impl Into<PathBuf>) -> PartSearch {
        PartSearch {
            project: Some(project.into()),
            also: Vec::new(),
        }
    }
}

/// Where an assembly's files are looked for: the project folder, given or found from the
/// first assembly read, then the other folders given, each with the Inventor files under it
/// by name, listed once when first needed.
#[derive(Default)]
pub(crate) struct Finder {
    given: Option<PathBuf>,
    root: OnceCell<PathBuf>,
    index: OnceCell<Index>,
    also: Vec<(PathBuf, OnceCell<Index>)>,
}

/// The Inventor files under the project folder, by lower-case file name.
#[derive(Default)]
struct Index {
    by_name: BTreeMap<String, Vec<PathBuf>>,
    /// Whether the search stopped at [`MAX_SEARCHED`] before it had seen every folder.
    truncated: bool,
}

/// A file found for a stored path.
#[derive(Debug, PartialEq)]
pub(crate) struct Found {
    pub path: PathBuf,
    /// When it was found by searching the project and other files have its name too: how
    /// many, and where the one used is, relative to the project folder.
    pub ambiguous: Option<(usize, String)>,
}

/// Why no file was found for a stored path.
#[derive(Debug, PartialEq)]
pub(crate) struct NotFound {
    /// The project folder searched.
    pub root: PathBuf,
    /// How many other folders were searched.
    pub also: usize,
    /// Whether the search stopped at [`MAX_SEARCHED`] before it had seen every folder.
    pub truncated: bool,
}

impl std::fmt::Display for NotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "not found at its stored path or in the project folder {}",
            self.root.display()
        )?;
        match self.also {
            0 => {}
            1 => write!(f, " or the other folder given")?,
            n => write!(f, " or the {n} other folders given")?,
        }
        if self.truncated {
            write!(f, " (the search stopped after {MAX_SEARCHED} files)")?;
        }
        Ok(())
    }
}

impl Finder {
    /// A finder searching as `search` says; without a project folder, under the nearest
    /// folder above the first assembly that holds an `.ipj`, else [`DEFAULT_LEVELS`] folders
    /// above it.
    pub(crate) fn new(search: &PartSearch) -> Finder {
        Finder {
            given: search.project.clone(),
            also: search
                .also
                .iter()
                .map(|dir| (absolute(dir), OnceCell::new()))
                .collect(),
            ..Finder::default()
        }
    }

    /// The file `stored` names, for an assembly in `folder`.
    pub(crate) fn locate(&self, stored: &str, folder: &Path) -> Result<Found, NotFound> {
        let folder = absolute(folder);
        let root = self.root.get_or_init(|| self.project_root(&folder));
        let found = |path| Found {
            path,
            ambiguous: None,
        };
        let (absolute_path, components) = split(stored);
        let Some((file, _)) = components.split_last() else {
            return Err(self.not_found(root));
        };
        if !absolute_path {
            if let Some(path) = resolve(&folder, &components) {
                return Ok(found(path));
            }
        } else {
            if let Some(path) = as_stored(stored) {
                return Ok(found(path));
            }
            // The saving machine's folders, moved onto this one: the longest tail of the path
            // that is under a folder above the assembly, nearest first.
            let above = ancestors(&folder, root);
            for start in 0..components.len().saturating_sub(1) {
                let tail = components.get(start..).unwrap_or_default();
                for ancestor in &above {
                    if let Some(path) = resolve(ancestor, tail) {
                        return Ok(found(path));
                    }
                }
            }
        }
        if let Some(found) = self.search(&components, &folder, root) {
            return Ok(found);
        }
        // Beside an assembly outside the project folder.
        resolve(&folder, &[*file])
            .map(found)
            .ok_or_else(|| self.not_found(root))
    }

    fn not_found(&self, root: &Path) -> NotFound {
        NotFound {
            root: root.to_path_buf(),
            also: self.also.len(),
            truncated: std::iter::once(&self.index)
                .chain(self.also.iter().map(|(_, index)| index))
                .any(|index| index.get().is_some_and(|index| index.truncated)),
        }
    }

    /// The project folder for an assembly in `folder`.
    fn project_root(&self, folder: &Path) -> PathBuf {
        if let Some(given) = &self.given {
            return absolute(given);
        }
        if let Some(found) = folder
            .ancestors()
            .take(MAX_COMPONENTS)
            .find(|dir| holds_ipj(dir))
        {
            return found.to_path_buf();
        }
        // No project file: a few folders up, but never the home folder or a whole drive.
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| absolute(Path::new(&home)));
        let mut root = folder;
        for _ in 0..DEFAULT_LEVELS {
            match root.parent() {
                Some(parent) if parent.parent().is_some() && Some(parent) != home.as_deref() => {
                    root = parent;
                }
                _ => break,
            }
        }
        root.to_path_buf()
    }

    /// The file the stored path's `components` end in, under the project folder, else under
    /// the other folders given, in order.
    fn search(&self, components: &[&str], folder: &Path, root: &Path) -> Option<Found> {
        let index = self.index.get_or_init(|| Index::of(root));
        pick(index, components, folder, root).or_else(|| {
            self.also.iter().find_map(|(dir, index)| {
                pick(
                    index.get_or_init(|| Index::of(dir)),
                    components,
                    folder,
                    dir,
                )
            })
        })
    }
}

/// The file in `index` (of the files under `base`) that the stored path's `components` end
/// in. Of several with its name: the one in the most of the stored path's folders
/// (`Parts/Brackets/` before `Brackets/`), then the nearest `folder` (in the folder above it
/// that holds it), then the one fewest folders down, then by path.
fn pick(index: &Index, components: &[&str], folder: &Path, base: &Path) -> Option<Found> {
    let file = components.last()?;
    let candidates = index.by_name.get(&file.to_lowercase())?;
    let matching = |path: &Path| {
        path.components()
            .rev()
            .skip(1)
            .zip(components.iter().rev().skip(1))
            .take_while(|(have, want)| {
                have.as_os_str().to_string_lossy().to_lowercase() == want.to_lowercase()
            })
            .count()
    };
    let shared = |path: &Path| {
        path.ancestors()
            .find(|dir| folder.starts_with(dir))
            .map_or(0, |dir| dir.components().count())
    };
    let path = candidates.iter().min_by(|a, b| {
        matching(b)
            .cmp(&matching(a))
            .then_with(|| shared(b).cmp(&shared(a)))
            .then_with(|| a.components().count().cmp(&b.components().count()))
            .then_with(|| a.cmp(b))
    })?;
    let ambiguous = (candidates.len() > 1).then(|| {
        let relative = path.strip_prefix(base).unwrap_or(path);
        (candidates.len(), relative.display().to_string())
    });
    Some(Found {
        path: path.clone(),
        ambiguous,
    })
}

impl Index {
    /// The Inventor files under `root`, leaving out Inventor's backup folders (`OldVersions`),
    /// hidden folders and symbolic links, so a link cannot make the search loop.
    fn of(root: &Path) -> Index {
        let mut index = Index::default();
        let mut seen = 0usize;
        let mut folders = vec![(root.to_path_buf(), 0usize)];
        while let Some((folder, depth)) = folders.pop() {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
            entries.sort_by_key(std::fs::DirEntry::file_name);
            for entry in entries.iter().rev() {
                seen += 1;
                if seen > MAX_SEARCHED {
                    index.truncated = true;
                    return index;
                }
                let Ok(kind) = entry.file_type() else {
                    continue;
                };
                let name = entry.file_name().to_string_lossy().to_lowercase();
                if kind.is_dir() {
                    if depth < MAX_SEARCH_DEPTH && !name.starts_with('.') && name != "oldversions" {
                        folders.push((entry.path(), depth + 1));
                    }
                } else if kind.is_file() && (name.ends_with(".ipt") || name.ends_with(".iam")) {
                    index.by_name.entry(name).or_default().push(entry.path());
                }
            }
        }
        index
    }
}

/// Whether `stored` is absolute on the machine that saved it (a drive, a share, or a leading
/// separator), and its folders and file, split at either separator.
fn split(stored: &str) -> (bool, Vec<&str>) {
    let mut parts: Vec<&str> = stored.split(['/', '\\']).collect();
    let drive = parts.first().is_some_and(|first| {
        let bytes = first.as_bytes();
        bytes.len() == 2
            && bytes.first().is_some_and(u8::is_ascii_alphabetic)
            && bytes.get(1) == Some(&b':')
    });
    let absolute = drive || stored.starts_with(['/', '\\']);
    if drive {
        parts.remove(0);
    }
    parts.retain(|part| !part.is_empty() && *part != ".");
    if parts.len() > MAX_COMPONENTS + 1 {
        parts.drain(..parts.len() - MAX_COMPONENTS - 1);
    }
    (absolute, parts)
}

/// The file at `stored` when it is an absolute path on this machine, with each name as its
/// folder lists it.
fn as_stored(stored: &str) -> Option<PathBuf> {
    let path = Path::new(stored);
    if !path.is_absolute() || !path.is_file() {
        return None;
    }
    let mut base = PathBuf::new();
    let mut names = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::Normal(name) => names.push(name.to_str()?),
            std::path::Component::ParentDir => names.push(".."),
            std::path::Component::CurDir => {}
            other => base.push(other),
        }
    }
    resolve(&base, &names)
}

/// `components` below `base`, each matched ignoring case as the saving system may have, and
/// named as the folder lists it, whatever the file system's case rules, so reports show the
/// real names; `Some` only when that is a file.
fn resolve(base: &Path, components: &[&str]) -> Option<PathBuf> {
    let mut path = base.to_path_buf();
    for component in components {
        if *component == ".." {
            path.pop();
            continue;
        }
        let names: Vec<std::ffi::OsString> = std::fs::read_dir(&path)
            .ok()?
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .collect();
        let wanted = component.to_lowercase();
        let name = names
            .iter()
            .find(|name| name.as_os_str() == std::ffi::OsStr::new(component))
            .or_else(|| {
                names
                    .iter()
                    .find(|name| name.to_string_lossy().to_lowercase() == wanted)
            })?;
        path.push(name);
    }
    path.is_file().then_some(path)
}

/// The folders from `folder` up to `root`, nearest first; only `folder` when `root` is not
/// above it.
fn ancestors(folder: &Path, root: &Path) -> Vec<PathBuf> {
    if !folder.starts_with(root) {
        return vec![folder.to_path_buf()];
    }
    folder
        .ancestors()
        .take_while(|dir| dir.starts_with(root))
        .map(Path::to_path_buf)
        .collect()
}

/// Whether `dir` holds an Inventor project file.
fn holds_ipj(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|entries| {
        entries.filter_map(Result::ok).any(|entry| {
            Path::new(&entry.file_name())
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ipj"))
                && entry.file_type().is_ok_and(|kind| kind.is_file())
        })
    })
}

/// `path` made absolute, through the links in it when it exists, as people write it: without
/// the `\\?\` prefix Windows gives a resolved path.
fn absolute(path: &Path) -> PathBuf {
    let resolved = path
        .canonicalize()
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf());
    plain(resolved)
}

/// A Windows path without its `\\?\` prefix (`\\?\UNC\host\share` as `\\host\share`), so
/// it reads, joins and compares as written; any other path as it is.
fn plain(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    if let Some(share) = text.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{share}"))
    } else if let Some(rest) = text
        .strip_prefix(r"\\?\")
        .filter(|rest| rest.as_bytes().get(1) == Some(&b':'))
    {
        PathBuf::from(rest)
    } else {
        path
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
pub(crate) mod tests {
    use super::*;

    /// `path` made absolute as the finder makes it.
    pub(crate) fn absolute_for_tests(path: &Path) -> PathBuf {
        absolute(path)
    }

    /// An Inventor project as it is usually laid out, with a project file at its top.
    fn project(ipj: bool) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Robot");
        for folder in [
            "Assemblies/Arm",
            "Parts/Brackets",
            "SubAssembly",
            "OldVersions",
        ] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        for file in [
            "Assemblies/Arm/top.iam",
            "Parts/Brackets/Bracket.ipt",
            "Parts/Plate.ipt",
            "SubAssembly/wrist.iam",
            "OldVersions/Lost.ipt",
            "Assemblies/Arm/Pin.ipt",
            "Parts/Pin.ipt",
            "Parts/Brackets/Pin.ipt",
        ] {
            std::fs::write(root.join(file), b"").unwrap();
        }
        if ipj {
            std::fs::write(root.join("Robot.ipj"), b"").unwrap();
        }
        dir
    }

    fn arm(dir: &tempfile::TempDir) -> PathBuf {
        absolute(&dir.path().join("Robot/Assemblies/Arm"))
    }

    #[test]
    fn a_path_from_another_machine_is_moved_onto_the_project() {
        let dir = project(true);
        let finder = Finder::new(&PartSearch::default());
        let root = absolute(&dir.path().join("Robot"));
        for stored in [
            r"C:\Users\someone\Projects\Robot\Parts\Plate.ipt",
            r"\\server\cad\Robot\Parts\Plate.ipt",
            "/Users/someone/Robot/Parts/Plate.ipt",
            r"D:\Robot\PARTS\plate.IPT",
        ] {
            assert_eq!(
                finder.locate(stored, &arm(&dir)).unwrap().path,
                root.join("Parts/Plate.ipt"),
                "{stored}"
            );
        }
    }

    #[test]
    fn a_relative_path_is_taken_from_the_assembly() {
        let dir = project(false);
        let finder = Finder::new(&PartSearch::default());
        let found = finder
            .locate(r"..\..\SubAssembly\wrist.iam", &arm(&dir))
            .unwrap();
        assert!(found.path.ends_with("SubAssembly/wrist.iam"), "{found:?}");
    }

    #[test]
    fn a_file_moved_since_is_found_by_searching_the_project() {
        let dir = project(true);
        let finder = Finder::new(&PartSearch::default());
        let found = finder
            .locate(r"C:\Old\Layout\Bracket.ipt", &arm(&dir))
            .unwrap();
        assert!(
            found.path.ends_with("Parts/Brackets/Bracket.ipt"),
            "{found:?}"
        );
        assert_eq!(found.ambiguous, None);
    }

    #[test]
    fn the_copy_nearest_the_assembly_is_used_and_the_others_said() {
        let dir = project(true);
        let finder = Finder::new(&PartSearch::default());
        // Beside the assembly is nearest.
        let beside = finder.locate(r"C:\Old\Pin.ipt", &arm(&dir)).unwrap();
        assert!(beside.path.ends_with("Assemblies/Arm/Pin.ipt"));
        // From the sub-assembly's folder, every copy is as near; the one in Parts is fewest
        // folders down.
        let sub = absolute(&dir.path().join("Robot/SubAssembly"));
        let searched = finder.locate(r"C:\Old\Pin.ipt", &sub).unwrap();
        assert!(searched.path.ends_with("Parts/Pin.ipt"), "{searched:?}");
        assert_eq!(
            searched.ambiguous,
            Some((3, Path::new("Parts").join("Pin.ipt").display().to_string()))
        );
    }

    #[test]
    fn the_stored_folders_choose_between_files_of_one_name() {
        let dir = project(true);
        let finder = Finder::new(&PartSearch::default());
        // Nearest is the copy beside the assembly, but the stored path says Brackets.
        let found = finder
            .locate(r"C:\Elsewhere\Brackets\Pin.ipt", &arm(&dir))
            .unwrap();
        assert!(found.path.ends_with("Parts/Brackets/Pin.ipt"), "{found:?}");
        assert_eq!(found.ambiguous.map(|(count, _)| count), Some(3));
    }

    #[test]
    fn backups_are_not_searched_and_a_miss_names_the_project_folder() {
        let dir = project(true);
        let finder = Finder::new(&PartSearch::default());
        let missing = finder.locate(r"C:\Old\Lost.ipt", &arm(&dir)).unwrap_err();
        assert_eq!(missing.root, absolute(&dir.path().join("Robot")));
        assert!(!missing.truncated);
    }

    #[test]
    fn without_a_project_file_the_search_reaches_two_folders_up() {
        let dir = project(false);
        let finder = Finder::new(&PartSearch::default());
        assert!(finder.locate("Bracket.ipt", &arm(&dir)).is_ok());
        // A given project folder is searched instead.
        let given = Finder::new(&PartSearch::project(dir.path().join("Robot/SubAssembly")));
        let missing = given.locate("Bracket.ipt", &arm(&dir)).unwrap_err();
        assert_eq!(
            missing.root,
            absolute(&dir.path().join("Robot/SubAssembly"))
        );
    }

    #[test]
    fn other_folders_are_searched_after_the_project() {
        let dir = project(true);
        let elsewhere = dir.path().join("Library/Fasteners");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("Screw.ipt"), b"").unwrap();
        std::fs::write(elsewhere.join("Plate.ipt"), b"").unwrap();
        let finder = Finder::new(&PartSearch {
            project: None,
            also: vec![dir.path().join("Library")],
        });
        let screw = finder.locate(r"C:\Lib\Screw.ipt", &arm(&dir)).unwrap();
        assert!(
            screw.path.ends_with("Library/Fasteners/Screw.ipt"),
            "{screw:?}"
        );
        // The project's own copy comes first.
        let plate = finder.locate(r"C:\Old\Plate.ipt", &arm(&dir)).unwrap();
        assert!(plate.path.ends_with("Robot/Parts/Plate.ipt"), "{plate:?}");
        let missing = finder.locate("Gone.ipt", &arm(&dir)).unwrap_err();
        assert_eq!(missing.also, 1);
        assert!(
            missing.to_string().ends_with("or the other folder given"),
            "{missing}"
        );
    }

    #[test]
    fn found_paths_have_the_names_the_folders_list() {
        let dir = project(true);
        let found = Finder::new(&PartSearch::default())
            .locate(r"C:\X\ROBOT\parts\PLATE.IPT", &arm(&dir))
            .unwrap();
        let names: Vec<String> = found
            .path
            .components()
            .rev()
            .take(3)
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Plate.ipt", "Parts", "Robot"]);
    }

    #[test]
    fn windows_paths_lose_their_verbatim_prefix() {
        assert_eq!(
            plain(PathBuf::from(r"\\?\C:\a\b")),
            PathBuf::from(r"C:\a\b")
        );
        assert_eq!(
            plain(PathBuf::from(r"\\?\UNC\host\share\b")),
            PathBuf::from(r"\\host\share\b")
        );
        assert_eq!(plain(PathBuf::from("/a/b")), PathBuf::from("/a/b"));
    }

    #[test]
    fn stored_paths_split_at_either_separator() {
        assert_eq!(split(r"C:\a\b.ipt"), (true, vec!["a", "b.ipt"]));
        assert_eq!(
            split(r"\\host\share\b.ipt"),
            (true, vec!["host", "share", "b.ipt"])
        );
        assert_eq!(split("../x/./b.ipt"), (false, vec!["..", "x", "b.ipt"]));
        assert_eq!(split("b.ipt"), (false, vec!["b.ipt"]));
        assert_eq!(split(""), (false, vec![]));
    }
}
