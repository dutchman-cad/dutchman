//! Finding the files an Inventor assembly places, as Inventor does: at the path the assembly
//! stores (relative to it, else absolute with the saving machine's drive or root moved onto
//! this one), else by name in the project's folders: in the most of the stored folders, then
//! nearest the assembly. The project's folders are the project folder, then the workspace,
//! workgroup, library and imported-components folders its Inventor project file (`.ipj`)
//! names.

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
/// The largest Inventor project file read.
const MAX_IPJ_BYTES: u64 = 4 << 20;
/// The most Inventor project files read for one search, through the projects they include.
const MAX_IPJ_FILES: usize = 8;
/// The most folders the project files may add to a search.
const MAX_NAMED: usize = 64;

/// Where to look for the files an assembly places, beyond the paths it stores.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartSearch {
    /// The project folder, searched with its subfolders. Without one, it is the nearest
    /// folder above the assembly that holds an Inventor project file (`.ipj`), else the
    /// folder two above the assembly's. Either way, the folders named by the nearest project
    /// file at or above it (else above the assembly) are searched next.
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
/// first assembly read, then the folders its Inventor project file names, then the other
/// folders given, each with the Inventor files under it by name, listed once when first
/// needed.
#[derive(Default)]
pub(crate) struct Finder {
    given: Option<PathBuf>,
    project: OnceCell<Project>,
    index: OnceCell<Index>,
    also: Vec<(PathBuf, OnceCell<Index>)>,
}

/// The project an assembly's files are searched in.
struct Project {
    /// The project folder.
    root: PathBuf,
    /// The Inventor project files read, the governing one first.
    files: Vec<PathBuf>,
    /// The folders they name, each with its index when it is searched apart from the
    /// project folder and the folders before it (`None` when one of those holds it).
    named: Vec<(NamedFolder, Option<OnceCell<Index>>)>,
}

/// A folder an Inventor project file names for its files.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NamedFolder {
    /// What it is: `workspace`, `workgroup`, `library` or `imported components`, with its
    /// name in the project file when it has one.
    pub(crate) kind: String,
    /// The path as the project file stores it.
    pub(crate) stored: String,
    /// Where it is on this machine, when it is here.
    pub(crate) path: Option<PathBuf>,
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
    /// How many folders the project's Inventor project file named were searched too.
    pub named: usize,
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
        match self.named {
            0 => {}
            1 => write!(f, ", the folder its Inventor project file names")?,
            n => write!(f, ", the {n} folders its Inventor project file names")?,
        }
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
        let project = self.project.get_or_init(|| self.project(&folder));
        let root = &project.root;
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
        if let Some(found) = self.search(&components, &folder, project) {
            return Ok(found);
        }
        // Beside an assembly outside the project folder.
        resolve(&folder, &[*file])
            .map(found)
            .ok_or_else(|| self.not_found(root))
    }

    fn not_found(&self, root: &Path) -> NotFound {
        let named = self.project.get().map_or(&[][..], |p| p.named.as_slice());
        NotFound {
            root: root.to_path_buf(),
            named: named
                .iter()
                .filter(|(folder, _)| folder.path.is_some())
                .count(),
            also: self.also.len(),
            truncated: std::iter::once(&self.index)
                .chain(named.iter().filter_map(|(_, index)| index.as_ref()))
                .chain(self.also.iter().map(|(_, index)| index))
                .any(|index| index.get().is_some_and(|index| index.truncated)),
        }
    }

    /// Every path the search was given or found: the project folder, its Inventor project
    /// files, the folders they name (as stored and as found) and the other folders searched.
    pub(crate) fn paths(&self) -> Vec<String> {
        let mut out: Vec<String> = self.given.iter().map(|p| p.display().to_string()).collect();
        out.extend(self.also.iter().map(|(p, _)| p.display().to_string()));
        if let Some(project) = self.project.get() {
            out.push(project.root.display().to_string());
            out.extend(project.files.iter().map(|p| p.display().to_string()));
            for (folder, _) in &project.named {
                out.push(folder.stored.clone());
                out.extend(folder.path.iter().map(|p| p.display().to_string()));
            }
        }
        out
    }

    /// When a part was searched for: the folders searched, in order, for the report.
    pub(crate) fn searched_note(&self) -> Option<String> {
        self.index.get()?;
        let project = self.project.get()?;
        let mut note = format!(
            "parts not at their stored paths were searched for in the project folder {}",
            project.root.display()
        );
        if let Some(governing) = project.files.first() {
            let name = governing.file_name().unwrap_or_default().to_string_lossy();
            note.push_str(&format!(
                ", then the folders the Inventor project file {name} names"
            ));
            if project.named.is_empty() {
                note.push_str(" (none)");
            }
            let folders: Vec<String> = project
                .named
                .iter()
                .map(|(folder, _)| match &folder.path {
                    Some(path) => format!("{} {}", folder.kind, path.display()),
                    None => format!("{} {} (not on this machine)", folder.kind, folder.stored),
                })
                .collect();
            if !folders.is_empty() {
                note.push_str(&format!(": {}", folders.join("; ")));
            }
        }
        if !self.also.is_empty() {
            let also: Vec<String> = self
                .also
                .iter()
                .map(|(dir, _)| dir.display().to_string())
                .collect();
            note.push_str(&format!(", then the folders given: {}", also.join("; ")));
        }
        Some(note)
    }

    /// The project for an assembly in `folder`: its folder, and the folders named by the
    /// nearest Inventor project file at or above it, else above the assembly.
    fn project(&self, folder: &Path) -> Project {
        let root = self.project_root(folder);
        let governing = root
            .ancestors()
            .chain(folder.ancestors())
            .take(2 * MAX_COMPONENTS)
            .find_map(|dir| {
                let files = ipj_files(dir);
                (!files.is_empty()).then_some(files)
            })
            .unwrap_or_default();
        let mut files = Vec::new();
        let mut named = Vec::new();
        let mut queue = governing;
        while let Some(file) = (!queue.is_empty()).then(|| queue.remove(0)) {
            if files.len() >= MAX_IPJ_FILES || files.contains(&file) {
                continue;
            }
            if let Ok(read) = read_ipj(&file) {
                named.extend(read.folders);
                queue.extend(read.included);
            }
            files.push(file);
        }
        named.truncate(MAX_NAMED);
        // A folder inside the project folder, or one named before it, is searched with it.
        let mut searched = vec![root.clone()];
        let named = named
            .into_iter()
            .map(|folder| {
                let index = match &folder.path {
                    Some(path) if !searched.iter().any(|done| path.starts_with(done)) => {
                        searched.push(path.clone());
                        Some(OnceCell::new())
                    }
                    _ => None,
                };
                (folder, index)
            })
            .collect();
        Project { root, files, named }
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
    /// the folders its project file names, else under the other folders given, in order.
    fn search(&self, components: &[&str], folder: &Path, project: &Project) -> Option<Found> {
        let root = &project.root;
        let index = self.index.get_or_init(|| Index::of(root));
        pick(index, components, folder, root)
            .or_else(|| {
                project.named.iter().find_map(|(named, index)| {
                    let (Some(dir), Some(index)) = (&named.path, index) else {
                        return None;
                    };
                    pick(
                        index.get_or_init(|| Index::of(dir)),
                        components,
                        folder,
                        dir,
                    )
                })
            })
            .or_else(|| {
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
    /// folders holding another project file, hidden folders and symbolic links, so a link
    /// cannot make the search loop.
    fn of(root: &Path) -> Index {
        let mut index = Index::default();
        let mut seen = 0usize;
        let mut folders = vec![(root.to_path_buf(), 0usize)];
        while let Some((folder, depth)) = folders.pop() {
            let Ok(entries) = std::fs::read_dir(&folder) else {
                continue;
            };
            let mut entries: Vec<_> = entries.filter_map(Result::ok).collect();
            // A folder below with its own project file is another project (a Pack and Go
            // copy, say), whose copies of the files are not this project's.
            if depth > 0
                && entries.iter().any(|entry| {
                    entry
                        .file_name()
                        .to_string_lossy()
                        .to_lowercase()
                        .ends_with(".ipj")
                        && entry.file_type().is_ok_and(|kind| kind.is_file())
                })
            {
                continue;
            }
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
    walk(base, components).filter(|path| path.is_file())
}

/// The folder `components` name below `base`, matched as [`resolve`] matches a file's.
fn resolve_folder(base: &Path, components: &[&str]) -> Option<PathBuf> {
    walk(base, components).filter(|path| path.is_dir())
}

/// `components` below `base`, each named as its folder lists it, when each is there.
fn walk(base: &Path, components: &[&str]) -> Option<PathBuf> {
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
    Some(path)
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
    !ipj_files(dir).is_empty()
}

/// The Inventor project files in `dir` (not below it), by name.
fn ipj_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            Path::new(&entry.file_name())
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("ipj"))
                && entry.file_type().is_ok_and(|kind| kind.is_file())
        })
        .map(|entry| entry.path())
        .take(MAX_IPJ_FILES)
        .collect();
    files.sort();
    files
}

/// What an Inventor project file says about where its files are.
#[derive(Debug, Default, PartialEq)]
struct Ipj {
    /// Its workspace, workgroup and library folders, then its imported-components folders
    /// in each workspace.
    folders: Vec<NamedFolder>,
    /// The project files it includes.
    included: Vec<PathBuf>,
}

/// Reads the Inventor project file at `path`: XML, in UTF-16 as Inventor writes it, or UTF-8.
/// Its paths are taken from its own folder.
fn read_ipj(path: &Path) -> Result<Ipj, String> {
    let size = std::fs::metadata(path)
        .map_err(|err| err.to_string())?
        .len();
    if size > MAX_IPJ_BYTES {
        return Err("the project file is too large".to_owned());
    }
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let base = absolute(path.parent().unwrap_or(Path::new(".")));
    parse_ipj(&text(&bytes)?, &base)
}

/// `bytes` as text, by their byte-order mark: UTF-16 either way, else UTF-8.
fn text(bytes: &[u8]) -> Result<String, String> {
    let utf16 = |rest: &[u8], unit: fn([u8; 2]) -> u16| {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| unit(*pair))
            .collect();
        String::from_utf16(&units).map_err(|err| err.to_string())
    };
    match bytes {
        [0xFF, 0xFE, rest @ ..] => utf16(rest, u16::from_le_bytes),
        [0xFE, 0xFF, rest @ ..] => utf16(rest, u16::from_be_bytes),
        [0xEF, 0xBB, 0xBF, rest @ ..] => {
            String::from_utf8(rest.to_vec()).map_err(|err| err.to_string())
        }
        _ => String::from_utf8(bytes.to_vec()).map_err(|err| err.to_string()),
    }
}

/// The folders and included projects of a project file's `xml`, its paths taken from `base`.
fn parse_ipj(xml: &str, base: &Path) -> Result<Ipj, String> {
    let document = roxmltree::Document::parse(xml).map_err(|err| err.to_string())?;
    let root = document.root_element();
    let child = |node: roxmltree::Node, name: &str| {
        node.children()
            .find(|child| child.has_tag_name(name))
            .and_then(|child| child.text())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let mut ipj = Ipj::default();
    let mut workspaces = Vec::new();
    for entry in root
        .descendants()
        .filter(|node| node.has_tag_name("ProjectPath"))
    {
        let kind = match entry.attribute("pathtype") {
            Some(kind) if kind.eq_ignore_ascii_case("workspace") => "workspace",
            Some(kind) if kind.eq_ignore_ascii_case("workgroup") => "workgroup",
            Some(kind) if kind.eq_ignore_ascii_case("library") => "library",
            _ => continue,
        };
        let Some(stored) = child(entry, "Path") else {
            continue;
        };
        let kind = match child(entry, "PathName") {
            Some(name) if kind != "workspace" => format!("{kind} \"{name}\""),
            _ => kind.to_owned(),
        };
        let path = named_folder(base, &stored);
        if kind == "workspace" {
            workspaces.push(path.clone());
        }
        ipj.folders.push(NamedFolder { kind, stored, path });
    }
    // Without a workspace, the project file's folder is its workspace.
    if workspaces.is_empty() {
        workspaces.push(Some(base.to_path_buf()));
        ipj.folders.insert(
            0,
            NamedFolder {
                kind: "workspace".to_owned(),
                stored: ".".to_owned(),
                path: Some(base.to_path_buf()),
            },
        );
    }
    // Imported components go in a folder of each workspace.
    let options = root
        .children()
        .find(|node| node.has_tag_name("ProjectOptions"));
    let mut imported: Vec<String> = Vec::new();
    for tag in ["ImportedCompFolder", "ImportedAsmFolder"] {
        if let Some(name) = options.and_then(|options| child(options, tag)) {
            if !imported.contains(&name) {
                imported.push(name);
            }
        }
    }
    for name in imported {
        for workspace in workspaces.iter().flatten() {
            let path = named_folder(workspace, &name);
            ipj.folders.push(NamedFolder {
                kind: "imported components".to_owned(),
                stored: name.clone(),
                path,
            });
        }
    }
    if let Some(included) = options.and_then(|options| child(options, "IncludedProject")) {
        let (absolute_path, components) = split(&included);
        let found = if absolute_path {
            as_stored(&included)
        } else {
            resolve(base, &components)
        };
        ipj.included.extend(found);
    }
    Ok(ipj)
}

/// The folder a project file stores as `stored`, from its folder `base`: relative to it, else
/// absolute on this machine, else the saving machine's path with its longest tail that is a
/// folder below `base`.
fn named_folder(base: &Path, stored: &str) -> Option<PathBuf> {
    let (absolute_path, components) = split(stored);
    if !absolute_path {
        return if components.is_empty() {
            Some(base.to_path_buf())
        } else {
            resolve_folder(base, &components)
        };
    }
    let here = Path::new(stored);
    if here.is_absolute() && here.is_dir() {
        return Some(absolute(here));
    }
    (0..components.len()).find_map(|start| {
        let tail = components.get(start..).filter(|tail| !tail.is_empty())?;
        resolve_folder(base, tail)
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

    /// `xml` as Inventor writes a project file: UTF-16, little-endian, with its mark.
    fn write_ipj(path: &Path, xml: &str) {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
        std::fs::write(path, bytes).unwrap();
    }

    /// A project file with these `<ProjectPath>` entries and imported-components folder.
    fn ipj_xml(paths: &[(&str, &str, &str)], imported: &str) -> String {
        let paths: String = paths
            .iter()
            .map(|(kind, name, path)| {
                format!(
                    "<ProjectPath pathtype=\"{kind}\"><PathName>{name}</PathName>\
                     <Path>{path}</Path></ProjectPath>"
                )
            })
            .collect();
        format!(
            "<?xml version=\"1.0\" encoding=\"utf-16\" standalone=\"no\" ?>\n\
             <InventorProject schemarevid=\"14\"><ProjectOptions>\
             <ProjectType>None</ProjectType><IncludedProject></IncludedProject>\
             <ImportedCompFolder>{imported}</ImportedCompFolder>\
             <ImportedAsmFolder>{imported}</ImportedAsmFolder></ProjectOptions>\
             <ProjectPaths>{paths}</ProjectPaths></InventorProject>"
        )
    }

    /// A shared workspace, as Inventor lays one out: its project file at the top, with the
    /// workspace `.`, a library beside it and one on a machine that is not this one; a
    /// product in versions, the one converted holding a Pack and Go copy with its own
    /// project file; and a folder of imported components.
    fn workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Shop");
        for folder in [
            "Imported Components",
            "Printer/Version 2",
            "Printer/Version 3/Assembly",
            "Printer/Version 3/PackNGo/PackNGo1",
        ] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        std::fs::create_dir_all(dir.path().join("Vendor Parts")).unwrap();
        for file in [
            "Shop/Imported Components/Clip.ipt",
            "Shop/Printer/Version 2/Extruder.iam",
            "Shop/Printer/Version 3/Assembly/top.iam",
            "Shop/Printer/Version 3/PackNGo/PackNGo1/Extruder.iam",
            "Vendor Parts/Screw.ipt",
        ] {
            std::fs::write(dir.path().join(file), b"").unwrap();
        }
        write_ipj(
            &root.join("Shop.ipj"),
            &ipj_xml(
                &[
                    ("Workspace", "Workspace", "."),
                    ("Library", "Vendor", r"..\Vendor Parts"),
                    ("Library", "Presets", r"C:\Users\someone\AppData\Nowhere"),
                ],
                "Imported Components",
            ),
        );
        write_ipj(
            &root.join("Printer/Version 3/PackNGo/PackNGo1/Copy.ipj"),
            &ipj_xml(&[("Workspace", "Workspace", r".\Workspaces\Workspace")], ""),
        );
        dir
    }

    #[test]
    fn a_project_file_reads_its_folders() {
        let dir = workspace();
        let ipj = read_ipj(&dir.path().join("Shop/Shop.ipj")).unwrap();
        let root = absolute(&dir.path().join("Shop"));
        let kinds: Vec<(&str, Option<PathBuf>)> = ipj
            .folders
            .iter()
            .map(|folder| (folder.kind.as_str(), folder.path.clone()))
            .collect();
        assert_eq!(
            kinds,
            [
                ("workspace", Some(root.clone())),
                (
                    "library \"Vendor\"",
                    Some(absolute(&dir.path().join("Vendor Parts")))
                ),
                ("library \"Presets\"", None),
                (
                    "imported components",
                    Some(root.join("Imported Components"))
                ),
            ]
        );
        assert!(ipj.included.is_empty());
        // A path from the saving machine is found by its tail below the project file.
        let moved = parse_ipj(
            &ipj_xml(
                &[("Library", "Clips", r"D:\Work\Shop\Imported Components")],
                "",
            ),
            &root,
        )
        .unwrap();
        assert_eq!(
            moved.folders.get(1).and_then(|folder| folder.path.clone()),
            Some(root.join("Imported Components"))
        );
        assert!(parse_ipj("not xml", &root).is_err());
    }

    #[test]
    fn a_project_folder_given_inside_a_project_also_searches_the_project() {
        let dir = workspace();
        let assembly = absolute(&dir.path().join("Shop/Printer/Version 3/Assembly"));
        let finder = Finder::new(&PartSearch::project(
            dir.path().join("Shop/Printer/Version 3"),
        ));
        // In another version of the product, through the workspace: not the Pack and Go
        // copy, though it is nearer.
        let extruder = finder.locate(r"C:\Old\Extruder.iam", &assembly).unwrap();
        assert!(
            extruder.path.ends_with("Printer/Version 2/Extruder.iam"),
            "{extruder:?}"
        );
        assert_eq!(extruder.ambiguous, None);
        let clip = finder.locate(r"C:\Old\Clip.ipt", &assembly).unwrap();
        assert!(
            clip.path.ends_with("Imported Components/Clip.ipt"),
            "{clip:?}"
        );
        let screw = finder.locate(r"C:\Old\Screw.ipt", &assembly).unwrap();
        assert!(screw.path.ends_with("Vendor Parts/Screw.ipt"), "{screw:?}");
        let missing = finder.locate("Gone.ipt", &assembly).unwrap_err();
        assert_eq!(missing.named, 3);
        assert!(
            missing
                .to_string()
                .ends_with(", the 3 folders its Inventor project file names"),
            "{missing}"
        );
        let note = finder.searched_note().unwrap();
        assert!(
            note.contains("then the folders the Inventor project file Shop.ipj names: workspace"),
            "{note}"
        );
        assert!(
            note.contains(
                r#"library "Presets" C:\Users\someone\AppData\Nowhere (not on this machine)"#
            ),
            "{note}"
        );
    }

    #[test]
    fn without_a_project_folder_the_project_file_above_governs() {
        let dir = workspace();
        let assembly = absolute(&dir.path().join("Shop/Printer/Version 3/Assembly"));
        let finder = Finder::new(&PartSearch::default());
        assert!(finder.searched_note().is_none(), "nothing searched yet");
        let extruder = finder.locate(r"C:\Old\Extruder.iam", &assembly).unwrap();
        assert!(extruder.path.ends_with("Printer/Version 2/Extruder.iam"));
        let missing = finder.locate("Gone.ipt", &assembly).unwrap_err();
        assert_eq!(missing.root, absolute(&dir.path().join("Shop")));
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
