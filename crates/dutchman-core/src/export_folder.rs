//! A bulk export's `manifest.json`: which designs an exported Fusion assembly inserts, and
//! where the export wrote each one.
//!
//! A Fusion assembly saved as one `.f3d` holds only its own parts: the designs it inserts live in
//! other documents. A bulk export writes each of those as a `.f3d` of its own beside the
//! assembly and lists them in the manifest, per assembly, as `references` (the design's `id` and,
//! in newer manifests, its `version` and the path of the design's newest exported file). This
//! module reads that JSON listing and nothing else: it never opens a design file. Every path it
//! returns lies inside the export folder.
//!
//! The manifest is parsed into typed rows holding only the fields used (unknown fields are
//! ignored, a field of an unexpected type reads as absent) and indexed once, by file, by design
//! id and by file stem; the parsed manifest is cached until the file changes.

use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

/// The manifest's file name.
pub(crate) const MANIFEST: &str = "manifest.json";
/// The largest manifest read, in bytes. It is parsed as a stream into typed rows, so a manifest
/// at the limit costs a fraction of its size in memory; a manifest with the most designs read
/// ([`MAX_ENTRIES`]) is far below it.
const MAX_MANIFEST_BYTES: u64 = 128 << 20;
/// The most designs a manifest may list.
const MAX_ENTRIES: usize = crate::locate::MAX_SEARCHED;
/// How many folders above a design are searched for its manifest.
const LEVELS: usize = 8;

/// A value of type `T`, or any other JSON value: a field of an unexpected type reads as absent.
#[derive(Deserialize)]
#[serde(untagged)]
enum Lenient<T> {
    Value(T),
    Other(serde::de::IgnoredAny),
}

fn lenient<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    Ok(match Lenient::<T>::deserialize(deserializer)? {
        Lenient::Value(value) => value,
        Lenient::Other(_) => T::default(),
    })
}

fn lenient_list<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    let rows: Vec<Lenient<T>> = lenient(deserializer)?;
    Ok(rows
        .into_iter()
        .filter_map(|row| match row {
            Lenient::Value(value) => Some(value),
            Lenient::Other(_) => None,
        })
        .collect())
}

fn non_empty(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.is_empty())
}

#[derive(Deserialize)]
struct RawManifest {
    #[serde(default, deserialize_with = "lenient")]
    entries: Option<Vec<RawEntry>>,
}

#[derive(Default, Deserialize)]
struct RawEntry {
    #[serde(default, deserialize_with = "lenient")]
    id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    version: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    status: Option<String>,
    #[serde(default, deserialize_with = "lenient_list")]
    files: Vec<RawFile>,
    #[serde(default, deserialize_with = "lenient_list")]
    moved_files: Vec<RawFile>,
    #[serde(default, deserialize_with = "lenient_list")]
    references: Vec<RawReference>,
}

#[derive(Default, Deserialize)]
struct RawFile {
    #[serde(default, deserialize_with = "lenient")]
    path: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    moved_to: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct RawReference {
    #[serde(default, deserialize_with = "lenient")]
    id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    version: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    path: Option<String>,
}

/// What became of the files an entry's export wrote.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Status {
    /// Exported and in the folder (also an entry with no status: older manifests).
    Done,
    /// An older version whose files were moved to `Older versions/`.
    Moved,
    /// An older version whose files were deleted.
    Removed,
    /// Anything else: failed, skipped, unsupported, or a status this reader doesn't know.
    Other,
}

/// One design the manifest lists.
#[derive(Debug)]
struct Entry {
    id: String,
    name: Option<String>,
    version: Option<u64>,
    status: Status,
    /// Exported files, relative to the manifest's folder, as written.
    files: Vec<String>,
    /// Where an older version's files went, relative to the manifest's folder.
    moved: Vec<String>,
    references: Vec<RawReference>,
}

/// A manifest, parsed and indexed once.
#[derive(Debug)]
pub(crate) struct Manifest {
    /// The folder the manifest is in, as the real path every exported file must stay under.
    root: PathBuf,
    entries: Vec<Entry>,
    /// The entries that list a file, by the file's real path.
    by_file: HashMap<PathBuf, Vec<usize>>,
    /// The entries that list a `.f3d`, by the real path of the file without its extension.
    by_stem: HashMap<PathBuf, Vec<usize>>,
    /// The entries of a design, by its id.
    by_id: HashMap<String, Vec<usize>>,
}

/// One design an assembly inserts, resolved to the file the export wrote for it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Reference {
    /// The design's id in the manifest, when the reference has it.
    pub id: Option<String>,
    /// The design's name, when the manifest has it.
    pub name: Option<String>,
    /// The design's version, when the manifest has it.
    pub version: Option<u64>,
    /// The exported file, inside the export folder.
    pub file: PathBuf,
}

/// What the manifest says an assembly inserts.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Inserted {
    /// The designs it inserts whose files are in the export.
    pub references: Vec<Reference>,
    /// The references whose file is not in the export, by id, and why (a path inside the
    /// message is the manifest's own).
    pub missing: Vec<(String, String)>,
}

type Cache = Mutex<HashMap<PathBuf, (SystemTime, u64, Arc<Manifest>)>>;

fn cache() -> &'static Cache {
    static CACHE: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The manifest of the export folder `design` is in, read once per change of the file.
fn manifest_for(design: &Path, top: Option<&Path>) -> Option<Result<Arc<Manifest>, String>> {
    let design = design.canonicalize().ok()?;
    let top = top.and_then(|top| top.canonicalize().ok());
    let mut folder = design.parent()?;
    for _ in 0..LEVELS {
        let path = folder.join(MANIFEST);
        if path.is_file() {
            return Some(load(&path));
        }
        if top.as_deref() == Some(folder) {
            break;
        }
        folder = folder.parent()?;
    }
    None
}

fn load(path: &Path) -> Result<Arc<Manifest>, String> {
    let meta = std::fs::metadata(path).map_err(|err| format!("{MANIFEST}: {err}"))?;
    if meta.len() > MAX_MANIFEST_BYTES {
        return Err(format!(
            "{MANIFEST} is {} MB, over Dutchman's limit of {} MB",
            meta.len() >> 20,
            MAX_MANIFEST_BYTES >> 20
        ));
    }
    let stamp = meta.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let key = path.to_path_buf();
    if let Ok(cache) = cache().lock() {
        if let Some((when, len, manifest)) = cache.get(&key) {
            if *when == stamp && *len == meta.len() {
                return Ok(manifest.clone());
            }
        }
    }
    let file = std::fs::File::open(path).map_err(|err| format!("{MANIFEST}: {err}"))?;
    // The size was checked above; the reader is bounded too, in case the file grew since.
    let reader = std::io::BufReader::new(std::io::Read::take(file, MAX_MANIFEST_BYTES + 1));
    let manifest = Arc::new(parse(reader, path.parent().unwrap_or(Path::new(".")))?);
    if let Ok(mut cache) = cache().lock() {
        if cache.len() > 16 {
            cache.clear();
        }
        cache.insert(key, (stamp, meta.len(), manifest.clone()));
    }
    Ok(manifest)
}

fn status_of(status: Option<&str>) -> Status {
    match status {
        None | Some("done") => Status::Done,
        Some("moved") => Status::Moved,
        Some("removed") => Status::Removed,
        Some(_) => Status::Other,
    }
}

fn parse(reader: impl std::io::Read, folder: &Path) -> Result<Manifest, String> {
    let root = folder
        .canonicalize()
        .map_err(|err| format!("{MANIFEST}: {err}"))?;
    let raw: RawManifest = serde_json::from_reader(reader)
        .map_err(|err| format!("{MANIFEST} is not valid JSON: {err}"))?;
    let rows = raw
        .entries
        .ok_or_else(|| format!("{MANIFEST} lists no entries"))?;
    if rows.len() > MAX_ENTRIES {
        return Err(format!(
            "{MANIFEST} lists {} designs, over Dutchman's limit of {MAX_ENTRIES}",
            rows.len()
        ));
    }
    let entries: Vec<Entry> = rows
        .into_iter()
        .map(|row| Entry {
            id: non_empty(row.id).unwrap_or_default(),
            name: non_empty(row.name),
            version: row.version,
            status: status_of(row.status.as_deref()),
            files: row
                .files
                .into_iter()
                .filter_map(|file| non_empty(file.path))
                .collect(),
            moved: row
                .moved_files
                .into_iter()
                .filter_map(|file| non_empty(file.moved_to))
                .collect(),
            references: row
                .references
                .into_iter()
                .map(|reference| RawReference {
                    id: non_empty(reference.id),
                    version: reference.version,
                    path: non_empty(reference.path),
                })
                .collect(),
        })
        .collect();
    let mut by_file: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    let mut by_stem: HashMap<PathBuf, Vec<usize>> = HashMap::new();
    let mut by_id: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, entry) in entries.iter().enumerate() {
        if !entry.id.is_empty() {
            by_id.entry(entry.id.clone()).or_default().push(index);
        }
        for file in entry.files.iter().chain(&entry.moved) {
            let Ok(real) = inside(&root, file) else {
                continue;
            };
            if is_design(file) {
                push_once(by_stem.entry(real.with_extension("")).or_default(), index);
            }
            push_once(by_file.entry(real).or_default(), index);
        }
    }
    Ok(Manifest {
        root,
        entries,
        by_file,
        by_stem,
        by_id,
    })
}

fn push_once(indices: &mut Vec<usize>, index: usize) {
    if indices.last() != Some(&index) {
        indices.push(index);
    }
}

/// The real path of the file a manifest path names, when it names one inside `root`.
fn inside(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let mut path = root.to_path_buf();
    for part in relative.split(['/', '\\']).filter(|part| !part.is_empty()) {
        if part == ".." || part == "." || part.contains(':') || part.contains('\0') {
            return Err(format!("{relative}: a path that leaves the export folder"));
        }
        path.push(part);
    }
    if Path::new(relative)
        .components()
        .any(|part| matches!(part, Component::RootDir | Component::Prefix(_)))
        || relative.starts_with(['/', '\\'])
    {
        return Err(format!("{relative}: a path that leaves the export folder"));
    }
    let real = path
        .canonicalize()
        .map_err(|_| format!("{relative}: the export does not hold this file"))?;
    if !real.starts_with(root) {
        return Err(format!("{relative}: a path that leaves the export folder"));
    }
    Ok(real)
}

fn is_design(path: &str) -> bool {
    path.rsplit_once('.')
        .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("f3d"))
}

impl Manifest {
    /// The entries whose exported file is `design` (a `.f3d`), or, for a rebuilt `.f3z`, whose
    /// `.f3d` has the same name and folder.
    fn entries_of(&self, design: &Path) -> Vec<&Entry> {
        let archive = design
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("f3z"));
        let found = if archive {
            self.by_stem.get(&design.with_extension(""))
        } else {
            self.by_file.get(design)
        };
        found
            .into_iter()
            .flatten()
            .filter_map(|index| self.entries.get(*index))
            .collect()
    }

    /// The `.f3d` of one entry, as the folder holds it now: where the export wrote it, or, for
    /// an older version the export moved aside, where it went.
    fn held(entry: &Entry) -> Result<&str, String> {
        fn design(files: &[String]) -> Option<&str> {
            files
                .iter()
                .map(String::as_str)
                .find(|file| is_design(file))
        }
        match entry.status {
            Status::Done => design(&entry.files).ok_or("the export wrote no .f3d for it"),
            Status::Moved => design(&entry.moved)
                .or_else(|| design(&entry.files))
                .ok_or("the export wrote no .f3d for it"),
            Status::Removed => Err("the export deleted this version"),
            Status::Other => Err("the export did not write it"),
        }
        .map_err(str::to_owned)
    }

    /// The exported `.f3d` of the design with `id`: the exact `version` when one is stated
    /// (never another), else the newest the export holds.
    fn file_of(&self, id: &str, version: Option<u64>) -> Result<(&Entry, &str), String> {
        let versions: Vec<&Entry> = self
            .by_id
            .get(id)
            .into_iter()
            .flatten()
            .filter_map(|index| self.entries.get(*index))
            .filter(|entry| version.is_none() || entry.version == version)
            .collect();
        let mut why = None;
        let mut best: Option<(&Entry, &str)> = None;
        for entry in versions {
            match Self::held(entry) {
                // Prefer a version still in place over one moved aside, then the newer.
                Ok(file) => {
                    let rank = |entry: &Entry| (entry.status == Status::Done, entry.version);
                    if best.is_none_or(|(held, _)| rank(entry) > rank(held)) {
                        best = Some((entry, file));
                    }
                }
                Err(reason) => why = Some(reason),
            }
        }
        match (best, version) {
            (Some(found), _) => Ok(found),
            (None, Some(version)) => Err(format!(
                "the export does not hold version {version} of this design{}",
                why.map(|why| format!(" ({why})")).unwrap_or_default()
            )),
            (None, None) => Err(format!(
                "the export does not hold this design{}",
                why.map(|why| format!(" ({why})")).unwrap_or_default()
            )),
        }
    }

    /// One reference of an assembly: the file to read, with the design's name and version.
    fn resolve(
        &self,
        reference: &RawReference,
    ) -> Result<(PathBuf, Option<String>, Option<u64>), String> {
        let found = match (&reference.id, reference.version, &reference.path) {
            // A stated version is the version placed: look that exact version up, whatever
            // the path (the newest export of the design) says.
            (Some(id), Some(_), _) | (Some(id), None, None) => self
                .file_of(id, reference.version)
                .map(|(entry, file)| (file.to_owned(), entry.name.clone(), entry.version)),
            // No version stated: the manifest's own path (the newest export) is the design.
            (_, None, Some(path)) => self.listed_path(path, None),
            (None, Some(version), Some(path)) => self.listed_path(path, Some(version)),
            (None, _, None) => Err("the reference names no design".to_owned()),
        }?;
        let (file, mut name, version) = found;
        if name.is_none() {
            name = reference
                .id
                .as_ref()
                .and_then(|id| self.by_id.get(id))
                .and_then(|indices| indices.first())
                .and_then(|index| self.entries.get(*index))
                .and_then(|entry| entry.name.clone());
        }
        inside(&self.root, &file).map(|real| (real, name, version))
    }

    /// A design named by the manifest's path: it must be a `.f3d`, and the entry that owns the
    /// file must still hold it and be the version asked for.
    fn listed_path(
        &self,
        path: &str,
        version: Option<u64>,
    ) -> Result<(String, Option<String>, Option<u64>), String> {
        if !is_design(path) {
            return Err(format!("{path}: not a Fusion design (.f3d)"));
        }
        let real = inside(&self.root, path)?;
        let owner = self
            .by_file
            .get(&real)
            .into_iter()
            .flatten()
            .filter_map(|index| self.entries.get(*index))
            .next();
        match owner {
            Some(owner) => {
                if matches!(owner.status, Status::Removed | Status::Other) {
                    return Err(format!("{path}: the export no longer holds this file"));
                }
                if version.is_some() && owner.version != version {
                    return Err(format!(
                        "{path}: the export does not hold version {} of this design",
                        version.unwrap_or_default()
                    ));
                }
                Ok((path.to_owned(), owner.name.clone(), owner.version))
            }
            None => Ok((path.to_owned(), None, version)),
        }
    }
}

/// What the manifest of the export folder `design` is in says it inserts: `None` when no
/// manifest covers it, an error when the manifest is unusable.
pub(crate) fn inserted(design: &Path, top: Option<&Path>) -> Option<Result<Inserted, String>> {
    let manifest = match manifest_for(design, top)? {
        Ok(manifest) => manifest,
        Err(why) => return Some(Err(why)),
    };
    let design = design.canonicalize().ok()?;
    let entries = manifest.entries_of(&design);
    if entries.is_empty() {
        return None;
    }
    let mut out = Inserted::default();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    for entry in entries {
        for reference in &entry.references {
            if out.references.len() + out.missing.len() >= crate::assembly::MAX_PARTS {
                return Some(Err(format!(
                    "the assembly inserts over {} designs, Dutchman's limit",
                    crate::assembly::MAX_PARTS
                )));
            }
            let label = reference.id.clone().unwrap_or_default();
            match manifest.resolve(reference) {
                Ok((file, name, version)) => {
                    if seen.insert(file.clone()) {
                        out.references.push(Reference {
                            id: reference.id.clone(),
                            name,
                            version,
                            file,
                        });
                    }
                }
                Err(why) => out.missing.push((label, why)),
            }
        }
    }
    Some(Ok(out))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use serde_json::json;

    fn export(manifest: serde_json::Value) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in ["Asm v3.f3d", "Screw v2.f3d", "Plate v1.f3d"] {
            std::fs::write(dir.path().join(file), b"x").unwrap();
        }
        std::fs::write(
            dir.path().join(MANIFEST),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        dir
    }

    fn entry(
        id: &str,
        name: &str,
        version: u64,
        references: serde_json::Value,
    ) -> serde_json::Value {
        json!({"id": id, "name": name, "version": version, "references": references,
               "files": [{"path": format!("{name} v{version}.f3d")}, {"path": format!("{name} v{version}.step")}]})
    }

    #[test]
    fn references_resolve_by_id_to_the_exported_design() {
        let dir = export(json!({"entries": [
            entry("urn:a:1", "Asm", 3, json!([{"id": "urn:a:2", "name": "Screw"}, {"id": "urn:a:3"}])),
            entry("urn:a:2", "Screw", 2, json!([])),
            entry("urn:a:3", "Plate", 1, json!([])),
        ]}));
        let inserted = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap();
        let names: Vec<_> = inserted
            .references
            .iter()
            .map(|r| r.name.clone().unwrap())
            .collect();
        assert_eq!(names, ["Screw", "Plate"]);
        assert_eq!(inserted.references[0].version, Some(2));
        assert!(inserted.missing.is_empty());
        // A design that lists nothing, or no manifest, is not an assembly of inserts.
        let plate = self::inserted(&dir.path().join("Plate v1.f3d"), None)
            .unwrap()
            .unwrap();
        assert!(plate.references.is_empty());
    }

    #[test]
    fn a_stated_path_and_version_win_and_a_missing_design_is_named() {
        let dir = export(json!({"entries": [
            entry("urn:a:1", "Asm", 3, json!([
                {"id": "urn:a:2", "version": 2, "path": "Screw v2.f3d"},
                {"id": "urn:a:9", "version": 1, "path": "Gone v1.f3d"},
                {"id": "urn:a:8"}])),
            entry("urn:a:2", "Screw", 2, json!([])),
        ]}));
        let inserted = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap();
        assert_eq!(inserted.references.len(), 1);
        assert_eq!(inserted.references[0].name.as_deref(), Some("Screw"));
        assert_eq!(inserted.references[0].version, Some(2));
        assert_eq!(inserted.missing.len(), 2);
    }

    /// An export holding Screw v1 (kept beside, `status` as given) and the newest Screw v2.
    fn two_versions(older: serde_json::Value, reference: serde_json::Value) -> tempfile::TempDir {
        let dir = export(json!({"entries": [
            entry("urn:a:1", "Asm", 3, json!([reference])),
            entry("urn:a:2", "Screw", 2, json!([])),
            older,
        ]}));
        std::fs::create_dir_all(dir.path().join("Older versions")).unwrap();
        std::fs::write(dir.path().join("Screw v1.f3d"), b"x").unwrap();
        std::fs::write(dir.path().join("Older versions/Screw v1.f3d"), b"x").unwrap();
        dir
    }

    fn file_names(inserted: &Inserted) -> Vec<String> {
        inserted
            .references
            .iter()
            .map(|r| {
                r.file
                    .strip_prefix(r.file.parent().unwrap().parent().unwrap())
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect()
    }

    #[test]
    fn a_stale_path_never_stands_for_the_stated_version() {
        // The reference uses v1; its path is the newest export (v2).
        let reference = json!({"id": "urn:a:2", "version": 1, "path": "Screw v2.f3d"});
        // v1 kept beside: it is v1 that is placed, labelled v1.
        let dir = two_versions(entry("urn:a:2", "Screw", 1, json!([])), reference.clone());
        let got = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap();
        assert_eq!(got.references.len(), 1, "{got:?}");
        assert!(got.references[0].file.ends_with("Screw v1.f3d"));
        assert_eq!(got.references[0].version, Some(1));
        // v1 not in the export at all: missing, v2 is not placed in its stead.
        let dir = export(json!({"entries": [
            entry("urn:a:1", "Asm", 3, json!([reference])),
            entry("urn:a:2", "Screw", 2, json!([])),
        ]}));
        let got = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap();
        assert!(got.references.is_empty(), "{got:?}");
        assert!(got.missing[0].1.contains("version 1"), "{:?}", got.missing);
    }

    #[test]
    fn an_older_version_the_export_moved_is_read_from_where_it_went() {
        let mut older = entry("urn:a:2", "Screw", 1, json!([]));
        older["status"] = json!("moved");
        older["files"] = json!([]);
        older["moved_files"] = json!([{"path": "Screw v1.f3d",
            "moved_to": "Older versions/Screw v1.f3d", "role": "archive"}]);
        let reference = json!({"id": "urn:a:2", "version": 1, "path": "Screw v2.f3d"});
        let dir = two_versions(older, reference);
        let got = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap();
        assert_eq!(got.references.len(), 1, "{got:?}");
        assert!(got.references[0]
            .file
            .ends_with(Path::new("Older versions").join("Screw v1.f3d")));
        assert_eq!(got.references[0].version, Some(1));
        assert_eq!(file_names(&got), ["Older versions/Screw v1.f3d"]);
    }

    #[test]
    fn a_removed_version_is_missing_and_a_path_must_be_a_design() {
        let mut older = entry("urn:a:2", "Screw", 1, json!([]));
        older["status"] = json!("removed");
        older["files"] = json!([]);
        older["removed_files"] = json!([{"path": "Screw v1.f3d"}]);
        let reference = json!({"id": "urn:a:2", "version": 1, "path": "Screw v2.f3d"});
        let dir = two_versions(older, reference);
        let got = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap();
        assert!(got.references.is_empty(), "{got:?}");
        assert!(got.missing[0].1.contains("deleted"), "{:?}", got.missing);
        // A path that is not a .f3d is no design, whatever it holds.
        let dir = export(json!({"entries": [
            entry("urn:a:1", "Asm", 3, json!([{"id": "urn:a:2", "path": "Screw v2.step"}])),
            entry("urn:a:2", "Screw", 2, json!([])),
        ]}));
        std::fs::write(dir.path().join("Screw v2.step"), b"x").unwrap();
        let got = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap();
        assert!(got.references.is_empty());
        assert!(got.missing[0].1.contains("not a Fusion design"));
    }

    #[test]
    fn a_path_whose_entry_was_removed_or_is_another_version_is_missing() {
        let mut gone = entry("urn:a:2", "Screw", 2, json!([]));
        gone["status"] = json!("removed");
        let dir = export(json!({"entries": [
            entry("urn:a:1", "Asm", 3, json!([{"path": "Screw v2.f3d"}])),
            gone,
        ]}));
        let got = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap();
        assert!(got.references.is_empty());
        // Without an id the version is checked against the entry that owns the path.
        let dir = export(json!({"entries": [
            entry("urn:a:1", "Asm", 3, json!([{"version": 1, "path": "Screw v2.f3d"}])),
            entry("urn:a:2", "Screw", 2, json!([])),
        ]}));
        let got = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap();
        assert!(got.references.is_empty());
        assert_eq!(got.missing.len(), 1);
    }

    #[test]
    fn a_manifest_from_before_schema_ids_and_with_odd_fields_is_read() {
        // Older exports: no schema_id, no exporter, no status or kind, a version written as
        // text, an unknown key, a null file list.
        let dir = export(json!({"schema": 2, "future": {"a": [1, 2]}, "entries": [
            {"id": "urn:a:1", "name": "Asm", "version": 3, "files": [{"path": "Asm v3.f3d"}],
             "references": [{"id": "urn:a:2", "version": "2", "extra": true}]},
            {"id": "urn:a:2", "name": "Screw", "version": 2, "files": [{"path": "Screw v2.f3d"}],
             "references": null},
            {"id": 7, "name": ["x"], "files": null},
        ]}));
        let got = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap();
        assert_eq!(got.references.len(), 1, "{got:?}");
        assert_eq!(got.references[0].version, Some(2));
    }

    #[test]
    fn a_manifest_over_the_size_limit_is_refused_unread() {
        let dir = export(json!({"entries": []}));
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(dir.path().join(MANIFEST))
            .unwrap();
        file.set_len(MAX_MANIFEST_BYTES + 1).unwrap();
        let why = inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .unwrap_err();
        assert!(why.contains("over Dutchman's limit of 128 MB"), "{why}");
    }

    #[test]
    fn a_path_that_leaves_the_export_folder_is_refused() {
        let outer = tempfile::tempdir().unwrap();
        std::fs::write(outer.path().join("secret.f3d"), b"x").unwrap();
        let dir = outer.path().join("export");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("Asm v3.f3d"), b"x").unwrap();
        std::fs::write(
            dir.join(MANIFEST),
            serde_json::to_vec(&json!({"entries": [entry("urn:a:1", "Asm", 3, json!([
                {"id": "urn:a:2", "path": "../secret.f3d"},
                {"id": "urn:a:3", "path": "/etc/passwd.f3d"},
                {"id": "urn:a:4", "path": "C:\\x\\y.f3d"}]))]}))
            .unwrap(),
        )
        .unwrap();
        let inserted = inserted(&dir.join("Asm v3.f3d"), None).unwrap().unwrap();
        assert!(inserted.references.is_empty());
        assert_eq!(inserted.missing.len(), 3);
        assert!(inserted
            .missing
            .iter()
            .all(|(_, why)| why.contains("leaves the export folder")));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_export_folder_is_refused() {
        let outer = tempfile::tempdir().unwrap();
        std::fs::write(outer.path().join("secret.f3d"), b"x").unwrap();
        let dir = outer.path().join("export");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("Asm v3.f3d"), b"x").unwrap();
        std::os::unix::fs::symlink(outer.path().join("secret.f3d"), dir.join("Link v1.f3d"))
            .unwrap();
        std::fs::write(
            dir.join(MANIFEST),
            serde_json::to_vec(&json!({"entries": [entry("urn:a:1", "Asm", 3, json!([
                {"id": "urn:a:2", "path": "Link v1.f3d"}]))]}))
            .unwrap(),
        )
        .unwrap();
        let inserted = inserted(&dir.join("Asm v3.f3d"), None).unwrap().unwrap();
        assert!(inserted.references.is_empty());
        assert_eq!(inserted.missing.len(), 1);
    }

    #[test]
    fn a_rebuilt_archive_takes_its_design_s_references() {
        let dir = export(json!({"entries": [
            entry("urn:a:1", "Asm", 3, json!([{"id": "urn:a:2"}])),
            entry("urn:a:2", "Screw", 2, json!([])),
        ]}));
        std::fs::write(dir.path().join("Asm v3.f3z"), b"x").unwrap();
        let inserted = inserted(&dir.path().join("Asm v3.f3z"), None)
            .unwrap()
            .unwrap();
        assert_eq!(inserted.references.len(), 1);
    }

    #[test]
    fn no_manifest_or_an_unlisted_file_is_not_an_export() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.f3d"), b"x").unwrap();
        assert!(inserted(&dir.path().join("a.f3d"), None).is_none());
        let dir = export(json!({"entries": []}));
        assert!(inserted(&dir.path().join("Asm v3.f3d"), None).is_none());
    }

    #[test]
    fn an_unreadable_manifest_is_an_error_and_a_huge_listing_is_refused() {
        let dir = export(json!({"entries": []}));
        std::fs::write(dir.path().join(MANIFEST), b"{not json").unwrap();
        assert!(inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .is_err());
        let many: Vec<_> = (0..MAX_ENTRIES + 1)
            .map(|n| json!({"id": n.to_string()}))
            .collect();
        let dir = export(json!({"entries": many}));
        assert!(inserted(&dir.path().join("Asm v3.f3d"), None)
            .unwrap()
            .is_err());
    }
}
