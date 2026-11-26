//! Designs a Fusion assembly inserts, read from a bulk export's folder.
//!
//! The assembly's `.f3d` holds only its own parts; cadmpeg states each insertion of another
//! design as an occurrence whose prototype is unresolved, with its placement. The export wrote
//! every inserted design as a `.f3d` of its own and listed them in `manifest.json`
//! ([`crate::export_folder`]). Each insertion is matched to one of those designs by what cadmpeg
//! states about it, the design is read through the same decoder as any other file, and the
//! assembly places it at the insertion's transform, as it places its own components.

use super::*;
use crate::export_folder::{self, Reference};

/// An insertion of another design that the file doesn't contain, as cadmpeg states it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Insert {
    /// The occurrence's id in the CADIR.
    pub id: String,
    /// The occurrence's name: the insertion's role or label.
    pub name: String,
    /// The identity of the design the reader states the insertion places, when it does.
    pub document_id: Option<String>,
    /// The version of that design, when the reader states it.
    pub version: Option<String>,
}

/// The insertions in `model` whose design the file doesn't hold: the reader states their
/// prototype as unresolved, or (newer readers) as an empty placeholder definition named for
/// the unresolved occurrence.
pub(crate) fn unresolved_inserts(model: &Model) -> Vec<Insert> {
    // The placed view the assembly builder reads: its occurrence ids (one per use of a shared
    // definition) are the keys the placements are filled by, and its parents compose a nested
    // insertion child -> parent -> root.
    let (Ok(definitions), Ok(occurrences)) = (
        serde_json::to_value(&model.ir.model.product_definitions),
        products::occurrences(&model.ir.model),
    ) else {
        return Vec::new();
    };
    // The placeholder is either a definition with no bodies and no children, or no definition
    // at all (readers that name the unresolved prototype without stating it).
    let empty = |id: &str| {
        definitions
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
            .is_none_or(|row| {
                ["bodies", "occurrences"].iter().all(|key| {
                    row.get(*key)
                        .and_then(Value::as_array)
                        .is_none_or(Vec::is_empty)
                })
            })
    };
    // The projection keeps no definition id for an external or unresolved prototype, and names
    // a repeated use of one occurrence `["<original id>", n]`: read the prototype off the
    // stored occurrence the placed row came from.
    let stored: BTreeMap<String, String> = serde_json::to_value(&model.ir.model.occurrences)
        .ok()
        .and_then(|rows| {
            Some(
                rows.as_array()?
                    .iter()
                    .filter_map(|row| {
                        Some((
                            row.get("id")?.as_str()?.to_owned(),
                            row.get("prototype")?.as_str()?.to_owned(),
                        ))
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();
    let prototype = |row: &Value| {
        let id = row.get("id")?.as_str()?;
        let original = serde_json::from_str::<Value>(id)
            .ok()
            .and_then(|pair| pair.get(0)?.as_str().map(str::to_owned))
            .unwrap_or_else(|| id.to_owned());
        stored.get(&original).cloned()
    };
    let stated = |row: &Value, key: &str| {
        let definition = prototype(row)?;
        definitions
            .as_array()?
            .iter()
            .find(|candidate| {
                candidate.get("id").and_then(Value::as_str) == Some(definition.as_str())
            })?
            .pointer(&format!("/storage/{key}"))?
            .as_str()
            .map(str::to_owned)
    };
    occurrences
        .as_array()
        .into_iter()
        .flatten()
        .filter(|row| {
            scope(row) == Some("unresolved")
                || prototype(row).is_some_and(|definition| {
                    definition.ends_with("/unresolved") && empty(&definition)
                })
        })
        .map(|row| Insert {
            id: row
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            name: row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            document_id: stated(row, "document_id"),
            version: stated(row, "version"),
        })
        .collect()
}

/// The design an insertion places, among the designs the manifest lists for the assembly.
///
/// cadmpeg states an insertion's name as `<design name> v<version>:<instance>` (Fusion's own
/// occurrence name) when the file keeps it, and this is the design's name and version in the
/// manifest. Only an exact, unambiguous match counts: a name that fits no listed design, or
/// more than one, matches none.
fn choose<'a>(insert: &Insert, references: &'a [Reference]) -> Option<&'a Reference> {
    // The reader states which design the insertion places: that identity (and version) is the
    // key. A stated identity that fits no listed design matches none, never a guess by name.
    if let Some(document_id) = insert.document_id.as_deref() {
        // A stated version that doesn't read as a number is no match, never "any version".
        let version = match insert.version.as_deref() {
            None => None,
            Some(stated) => Some(stated.trim().trim_start_matches('v').parse::<u64>().ok()?),
        };
        let mut found = references.iter().filter(|reference| {
            reference.id.as_deref() == Some(document_id)
                && version.is_none_or(|stated| reference.version == Some(stated))
        });
        return match (found.next(), found.next()) {
            (Some(only), None) => Some(only),
            _ => None,
        };
    }
    let name = insert.name.trim();
    if name.is_empty() {
        return None;
    }
    // Drop the instance number after the last colon: `Screw v2:3` is the third `Screw v2`.
    let design = match name.rsplit_once(':') {
        Some((design, instance)) if instance.chars().all(|c| c.is_ascii_digit()) => design,
        _ => name,
    };
    let mut found = references.iter().filter(|reference| {
        let Some(own) = &reference.name else {
            return false;
        };
        design.eq_ignore_ascii_case(own)
            || reference
                .version
                .is_some_and(|version| design.eq_ignore_ascii_case(&format!("{own} v{version}")))
    });
    match (found.next(), found.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

type Loaded = Result<(usize, std::rc::Rc<Model>), (Stage, String)>;

/// The most inserted design files read for one assembly, all levels together.
const MAX_TOTAL_READS: usize = 4 * MAX_PARTS;

/// The designs read so far for one assembly and everything nested in it: a design inserted by
/// many parents, at any level, is decoded once.
struct Reads {
    files: BTreeMap<PathBuf, Loaded>,
    /// The most files this may read in all.
    cap: usize,
    /// Files read so far; the next one's key in the placements.
    read: usize,
}

impl Reads {
    fn new(cap: usize) -> Self {
        Self {
            files: BTreeMap::new(),
            cap,
            read: 0,
        }
    }
}

/// Reads the designs an export folder holds for the insertions in `model` (read from
/// `input`), found through its manifest: each insertion that matches a listed design, by the
/// insertion's id. An insertion with no manifest, no match, or a design that cannot be read is
/// left out of the result when the file says nothing more (the caller reports it as missing),
/// and left out with its own reason when the export names a design that cannot be read.
pub(crate) fn resolve(
    model: &Model,
    input: &Path,
    top: Option<&Path>,
    report: &mut Report,
    decode: &Decode,
    finder: &Finder,
    open: &mut Vec<PathBuf>,
) -> ExternalParts {
    resolve_in(
        model,
        input,
        top,
        report,
        decode,
        finder,
        open,
        &mut Reads::new(MAX_TOTAL_READS),
    )
}

/// The files `model` (read from `input`) places from its export folder, one level down, that
/// can be read: what a batch drops from its own results because an assembly places them. A
/// design the manifest lists for the assembly but no insertion matches, or that cannot be
/// read, is not among them.
pub(crate) fn placed_files(
    model: &Model,
    input: &Path,
    top: Option<&Path>,
    decode: &Decode,
) -> Vec<PathBuf> {
    let inserts = unresolved_inserts(model);
    let Some(Ok(listing)) = (!inserts.is_empty())
        .then(|| export_folder::inserted(input, top))
        .flatten()
    else {
        return Vec::new();
    };
    let mut placed: Vec<PathBuf> = Vec::new();
    for insert in &inserts {
        if let Some(reference) = choose(insert, &listing.references) {
            if !placed.contains(&reference.file) && readable(&reference.file, decode) {
                placed.push(reference.file.clone());
            }
        }
    }
    placed
}

/// Whether the decoder reads `file`, remembered until the file changes.
fn readable(file: &Path, decode: &Decode) -> bool {
    type Memo =
        std::sync::Mutex<std::collections::HashMap<PathBuf, (std::time::SystemTime, u64, bool)>>;
    static MEMO: std::sync::OnceLock<Memo> = std::sync::OnceLock::new();
    let memo = MEMO.get_or_init(Default::default);
    let stamp = std::fs::metadata(file)
        .ok()
        .map(|meta| (meta.modified().unwrap_or(std::time::UNIX_EPOCH), meta.len()));
    if let (Some((when, len)), Ok(memo)) = (stamp, memo.lock()) {
        if let Some((seen, size, ok)) = memo.get(file) {
            if *seen == when && *size == len {
                return *ok;
            }
        }
    }
    let ok = decode(Format::FusionArchive, file, &mut Report::default()).is_ok();
    if let (Some((when, len)), Ok(mut memo)) = (stamp, memo.lock()) {
        if memo.len() > 4096 {
            memo.clear();
        }
        memo.insert(file.to_path_buf(), (when, len, ok));
    }
    ok
}

#[allow(clippy::too_many_arguments)]
fn resolve_in(
    model: &Model,
    input: &Path,
    top: Option<&Path>,
    report: &mut Report,
    decode: &Decode,
    finder: &Finder,
    open: &mut Vec<PathBuf>,
    reads: &mut Reads,
) -> ExternalParts {
    let mut external = ExternalParts::new();
    let inserts = unresolved_inserts(model);
    if inserts.is_empty() {
        return external;
    }
    let listing = match export_folder::inserted(input, top) {
        None => return external,
        Some(Ok(listing)) => listing,
        Some(Err(why)) => {
            report.push_at(Stage::Input, Severity::Info, "export manifest", why);
            return external;
        }
    };
    if !listing.missing.is_empty() {
        let shown: Vec<&str> = listing
            .missing
            .iter()
            .take(5)
            .map(|(_, why)| why.as_str())
            .collect();
        report.push_at(
            Stage::Input,
            Severity::Info,
            "export manifest",
            format!(
                "{} designs the manifest lists for this assembly are not in the export ({}{})",
                listing.missing.len(),
                shown.join("; "),
                if listing.missing.len() > shown.len() {
                    "; ..."
                } else {
                    ""
                }
            ),
        );
    }
    let key = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
    if open.contains(&key) {
        report.push_at(
            Stage::Input,
            Severity::Info,
            "export manifest",
            "this design inserts itself, directly or through the designs it inserts; the \
             insertion is not placed again",
        );
        return external;
    }
    if open.len() >= MAX_DEPTH {
        report.push_at(
            Stage::Input,
            Severity::Info,
            "export manifest",
            format!(
                "inserted designs nest over {MAX_DEPTH} deep; the designs inserted below this \
                 level are not read"
            ),
        );
        return external;
    }
    open.push(key);
    for insert in &inserts {
        let Some(reference) = choose(insert, &listing.references) else {
            continue;
        };
        let file = reference
            .file
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let loaded = match reads.files.get(&reference.file) {
            Some(loaded) => loaded.clone(),
            None if open.contains(&reference.file) => Err((
                Stage::Input,
                "the design inserts itself, directly or through the designs it inserts".to_owned(),
            )),
            None if reads.read >= reads.cap => Err((
                Stage::Input,
                format!(
                    "over Dutchman's limit of {} inserted design files read for one assembly",
                    reads.cap
                ),
            )),
            None => {
                let index = reads.read;
                reads.read += 1;
                let mut part_report = Report::default();
                let read = read_design(
                    reference,
                    top,
                    &mut part_report,
                    decode,
                    finder,
                    open,
                    reads,
                )
                .map(|part| (index, std::rc::Rc::new(part)))
                .map_err(|err| (err.stage(), err.to_string()));
                report.append_prefixed(part_report, &file);
                reads.files.insert(reference.file.clone(), read.clone());
                read
            }
        };
        let name = Some(insert.name.clone()).filter(|name| !name.is_empty());
        external.insert(
            insert.id.clone(),
            loaded.map_err(|(stage, why)| {
                let subject = format!("occurrence \"{}\"", name.as_deref().unwrap_or("unnamed"));
                report.push_at(stage, Severity::Dropped, subject, format!("{file}: {why}"));
                LeftOut {
                    name,
                    file: Some(file.clone()),
                    reason: format!("{file} could not be read: {why}"),
                    stage,
                }
            }),
        );
    }
    open.pop();
    let unmatched = inserts
        .iter()
        .filter(|insert| !external.contains_key(&insert.id))
        .count();
    if unmatched > 0 && !listing.references.is_empty() {
        report.push_at(
            Stage::Input,
            Severity::Info,
            "export manifest",
            format!(
                "{unmatched} of {} inserted components could not be matched to the {} designs the \
                 manifest lists for this assembly: the file doesn't state which design each \
                 insertion places, and Dutchman doesn't guess",
                inserts.len(),
                listing.references.len()
            ),
        );
    }
    external
}

/// One inserted design, read, with the designs it inserts in turn and its own components.
fn read_design(
    reference: &Reference,
    top: Option<&Path>,
    report: &mut Report,
    decode: &Decode,
    finder: &Finder,
    open: &mut Vec<PathBuf>,
    reads: &mut Reads,
) -> Result<Model, Error> {
    crate::names::saw_path(&reference.file, report);
    let mut part = decode(Format::FusionArchive, &reference.file, report)?;
    if let Some(name) = &reference.name {
        part.name.clone_from(name);
    }
    let nested = resolve_in(
        &part,
        &reference.file,
        top,
        report,
        decode,
        finder,
        open,
        reads,
    );
    match local_components_with_threads(
        &part,
        &nested,
        finder.threads,
        finder.part_positions,
        report,
    ) {
        Ok(Some((assembly, _))) => part.assembly = Some(assembly),
        Ok(None) => {}
        Err(reason) => report.push_at(
            Stage::Map,
            Severity::Degraded,
            "components",
            format!("the design places components, but {reason}; it converts as stored"),
        ),
    }
    Ok(part)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reference(name: &str, version: u64) -> Reference {
        Reference {
            id: Some(format!("urn:x:{name}")),
            name: Some(name.to_owned()),
            version: Some(version),
            file: PathBuf::from(format!("{name} v{version}.f3d")),
        }
    }

    fn insert(name: &str) -> Insert {
        Insert {
            id: "i".into(),
            name: name.into(),
            document_id: None,
            version: None,
        }
    }

    #[test]
    fn a_stated_design_identity_is_the_key_and_a_misfit_is_never_guessed_by_name() {
        let listed = [reference("Screw", 2), reference("Plate", 1)];
        let stated = |id: &str, version: Option<&str>, name: &str| Insert {
            id: "i".into(),
            name: name.into(),
            document_id: Some(id.into()),
            version: version.map(str::to_owned),
        };
        let file = |insert: &Insert| choose(insert, &listed).map(|r| r.file.clone());
        assert_eq!(
            file(&stated("urn:x:Plate", Some("1"), "role label")),
            Some(PathBuf::from("Plate v1.f3d"))
        );
        assert_eq!(
            file(&stated("urn:x:Screw", None, "")),
            Some(PathBuf::from("Screw v2.f3d"))
        );
        // A stated version the listed design doesn't have, or an identity no design has,
        // matches none even though the name fits.
        assert_eq!(file(&stated("urn:x:Screw", Some("3"), "Screw v2:1")), None);
        assert_eq!(file(&stated("urn:x:Nut", None, "Screw v2:1")), None);
    }

    #[test]
    fn an_insertion_matches_the_design_it_names_without_its_instance_number() {
        let listed = [reference("Screw", 2), reference("Plate", 1)];
        for name in ["Screw v2:1", "Screw v2:12", "Screw v2", "screw", "Screw:4"] {
            assert_eq!(
                choose(&insert(name), &listed).map(|r| r.file.clone()),
                Some(PathBuf::from("Screw v2.f3d")),
                "{name}"
            );
        }
    }

    #[test]
    fn an_insertion_that_fits_no_design_or_two_matches_none() {
        let listed = [reference("Screw", 2), reference("Screw", 3)];
        // The name alone fits both versions; the version separates them.
        assert!(choose(&insert("Screw:1"), &listed).is_none());
        assert_eq!(
            choose(&insert("Screw v3:1"), &listed).map(|r| r.version),
            Some(Some(3))
        );
        assert!(choose(&insert("Nut v1:1"), &listed).is_none());
        // A role that is only an identifier names no design.
        assert!(choose(&insert("1b3f2c7e-0000-4000-8000-000000000000"), &listed).is_none());
        assert!(choose(&insert(""), &listed).is_none());
    }

    fn cube() -> Model {
        crate::read(
            Format::FusionArchive,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generated_unit_cube.f3d"),
            &mut Report::default(),
        )
        .unwrap()
    }

    /// A synthetic export folder: the assembly, the two designs it inserts and a manifest.
    fn export_folder() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for file in ["Asm v1.f3d", "Screw v2.f3d", "Plate v1.f3d"] {
            std::fs::write(dir.path().join(file), b"placeholder").unwrap();
        }
        let entry = |id: &str, name: &str, version: u64, references: Value| {
            json!({"id": id, "name": name, "version": version, "references": references,
                   "files": [{"path": format!("{name} v{version}.f3d")}]})
        };
        let manifest = json!({"schema": 2, "entries": [
            entry("urn:x:asm", "Asm", 1, json!([{"id": "urn:x:screw", "name": "Screw"},
                                                {"id": "urn:x:plate", "name": "Plate"}])),
            entry("urn:x:screw", "Screw", 2, json!([])),
            entry("urn:x:plate", "Plate", 1, json!([])),
        ]});
        std::fs::write(
            dir.path().join(export_folder::MANIFEST),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        dir
    }

    fn insertion(key: usize, name: &str, x: f64) -> Value {
        json!({"id": format!("synthetic:assembly:occurrence#{key}"), "ordinal": key, "name": name,
               "parent": {"kind": "root"},
               "prototype": format!("synthetic:assembly:occurrence#{key}/unresolved"),
               "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
               "scale": [1.0, 1.0, 1.0]})
    }

    #[test]
    fn insertions_are_placed_from_the_export_folder_at_their_transforms() {
        let dir = export_folder();
        let parent = inserting(vec![
            insertion(1, "Screw v2:1", 10.0),
            insertion(2, "Screw v2:2", 20.0),
            insertion(3, "Plate v1:1", 30.0),
            // A role that names no design stays missing.
            insertion(4, "0f1e2d3c-0000-4000-8000-000000000000", 40.0),
        ]);
        let reads = std::cell::Cell::new(0);
        let decode = |_: Format, _: &Path, _: &mut Report| -> Result<Model, Error> {
            reads.set(reads.get() + 1);
            Ok(cube())
        };
        let finder = Finder::default();
        let mut report = Report::default();
        let external = resolve(
            &parent,
            &dir.path().join("Asm v1.f3d"),
            None,
            &mut report,
            &decode,
            &finder,
            &mut Vec::new(),
        );
        assert_eq!(external.len(), 3);
        assert_eq!(
            reads.get(),
            2,
            "each design is read once, however often it is placed"
        );
        assert!(!external.contains_key("synthetic:assembly:occurrence#4"));
        assert_eq!(
            crate::missing_linked_components(&parent, &external),
            ["0f1e2d3c-0000-4000-8000-000000000000"]
        );

        let (assembly, _) = local_components_with_threads(
            &parent,
            &external,
            1,
            crate::PartPositions::LastShown,
            &mut report,
        )
        .unwrap()
        .unwrap();
        let placed: Vec<(Option<&str>, f64)> = assembly
            .placements
            .iter()
            .filter(|placement| placement.name.is_some())
            .map(|placement| (placement.name.as_deref(), placement.transform[0][3]))
            .collect();
        assert_eq!(
            placed,
            [
                (Some("Screw v2:1"), 10.0),
                (Some("Screw v2:2"), 20.0),
                (Some("Plate v1:1"), 30.0)
            ]
        );
        // The two screws share one part; the plate and the assembly's own bodies make three.
        let screw = |name: &str| {
            assembly
                .placements
                .iter()
                .find(|placement| placement.name.as_deref() == Some(name))
                .unwrap()
                .part
        };
        assert_eq!(screw("Screw v2:1"), screw("Screw v2:2"));
        assert_ne!(screw("Screw v2:1"), screw("Plate v1:1"));
        assert_eq!(assembly.parts[screw("Screw v2:1")].name, "Screw");
    }

    #[test]
    fn a_listed_design_that_cannot_be_read_is_left_out_with_its_reason() {
        let dir = export_folder();
        let parent = inserting(vec![insertion(1, "Screw v2:1", 10.0)]);
        let decode = |_: Format, _: &Path, _: &mut Report| -> Result<Model, Error> {
            Err(Error::Decode("not a Fusion design".to_owned()))
        };
        let mut report = Report::default();
        let external = resolve(
            &parent,
            &dir.path().join("Asm v1.f3d"),
            None,
            &mut report,
            &decode,
            &Finder::default(),
            &mut Vec::new(),
        );
        let Some(Err(left)) = external.get("synthetic:assembly:occurrence#1") else {
            panic!("expected a left-out placement");
        };
        assert!(
            left.reason.starts_with("Screw v2.f3d could not be read"),
            "{}",
            left.reason
        );
        // It is accounted for, not reported again as a missing design.
        assert!(crate::missing_linked_components(&parent, &external).is_empty());
    }

    #[test]
    fn without_a_manifest_nothing_is_resolved() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Asm v1.f3d"), b"x").unwrap();
        let parent = inserting(vec![insertion(1, "Screw v2:1", 10.0)]);
        let decode = |_: Format, _: &Path, _: &mut Report| -> Result<Model, Error> { Ok(cube()) };
        let external = resolve(
            &parent,
            &dir.path().join("Asm v1.f3d"),
            None,
            &mut Report::default(),
            &decode,
            &Finder::default(),
            &mut Vec::new(),
        );
        assert!(external.is_empty());
    }

    fn at(key: usize, name: &str, transform: Value) -> Value {
        json!({"id": format!("synthetic:assembly:occurrence#{key}"), "ordinal": key, "name": name,
               "parent": {"kind": "root"},
               "prototype": format!("synthetic:assembly:occurrence#{key}/unresolved"),
               "transform": transform, "scale": [1.0, 1.0, 1.0]})
    }

    /// A cube that inserts the designs in `inserts`.
    fn inserting(inserts: Vec<Value>) -> Model {
        let mut model = cube();
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["occurrences"] = Value::Array(inserts);
        // Each insertion's placeholder definition: empty, naming no stored design.
        wire["model"]["product_definitions"] = Value::Array(
            wire["model"]["occurrences"]
                .as_array()
                .unwrap()
                .iter()
                .map(|row| json!({"id": row["prototype"], "storage": {"kind": "external"}}))
                .collect(),
        );
        model.ir = crate::test_products::read(&wire);
        model
    }

    /// An export folder with the designs `names` (each `<name> v1.f3d`) and a manifest in
    /// which each lists the designs after `->` as references.
    fn folder_of(listing: &[(&str, &[&str])]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let entries: Vec<Value> = listing
            .iter()
            .map(|(name, inserts)| {
                std::fs::write(dir.path().join(format!("{name} v1.f3d")), b"x").unwrap();
                let references: Vec<Value> = inserts
                    .iter()
                    .map(|child| json!({"id": format!("urn:x:{child}"), "version": 1}))
                    .collect();
                json!({"id": format!("urn:x:{name}"), "name": name, "version": 1,
                       "references": references, "files": [{"path": format!("{name} v1.f3d")}]})
            })
            .collect();
        std::fs::write(
            dir.path().join(export_folder::MANIFEST),
            serde_json::to_vec(&json!({"entries": entries})).unwrap(),
        )
        .unwrap();
        dir
    }

    const SHIFT: [[f64; 4]; 3] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];

    #[test]
    fn a_stated_version_that_does_not_read_is_no_match() {
        let listed = [reference("Screw", 2)];
        let stated = |version: &str| Insert {
            id: "i".into(),
            name: String::new(),
            document_id: Some("urn:x:Screw".into()),
            version: Some(version.into()),
        };
        assert!(choose(&stated("2"), &listed).is_some());
        assert!(choose(&stated("v2"), &listed).is_some());
        for version in ["", "two", "2.1", "-2", "latest"] {
            assert!(choose(&stated(version), &listed).is_none(), "{version:?}");
        }
    }

    #[test]
    fn nested_insertions_compose_child_inside_parent_inside_root() {
        // Root places Mid turned a quarter turn about Z at x = 100; Mid places Leaf at x = 10.
        let dir = folder_of(&[("Asm", &["Mid"]), ("Mid", &["Leaf"]), ("Leaf", &[])]);
        let turn = json!([
            [0.0, -1.0, 0.0, 100.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ]);
        let leaf = json!([
            [1.0, 0.0, 0.0, 10.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ]);
        let root = inserting(vec![at(1, "Mid v1:1", turn)]);
        let decode = |_: Format, path: &Path, _: &mut Report| -> Result<Model, Error> {
            Ok(match path.file_name().unwrap().to_str().unwrap() {
                "Mid v1.f3d" => inserting(vec![at(1, "Leaf v1:1", leaf.clone())]),
                _ => cube(),
            })
        };
        let mut report = Report::default();
        let external = resolve(
            &root,
            &dir.path().join("Asm v1.f3d"),
            None,
            &mut report,
            &decode,
            &Finder::default(),
            &mut Vec::new(),
        );
        let (assembly, _) = local_components_with_threads(
            &root,
            &external,
            1,
            crate::PartPositions::LastShown,
            &mut report,
        )
        .unwrap()
        .unwrap();
        let index = |assembly: &Assembly, name: &str| {
            assembly
                .placements
                .iter()
                .position(|placement| placement.name.as_deref() == Some(name))
                .unwrap()
        };
        let mid = index(&assembly, "Mid v1:1");
        let inner = assembly.parts[assembly.placements[mid].part]
            .assembly
            .as_ref()
            .unwrap();
        let leaf = index(inner, "Leaf v1:1");
        // The leaf's own placement is stored relative to Mid, untouched.
        assert_eq!(inner.placements[leaf].transform[0][3], 10.0);
        // In the world: turned by Mid, then moved by Mid: (100, 10, 0), not (110, 0, 0).
        let world = world_at(&assembly, &[mid, leaf]).unwrap();
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(near(world[0][3], 100.0), "{world:?}");
        assert!(near(world[1][3], 10.0), "{world:?}");
        assert!(near(world[2][3], 0.0), "{world:?}");
        assert!(
            near(world[1][0], 1.0) && near(world[0][1], -1.0),
            "{world:?}"
        );
    }

    #[test]
    fn a_design_inserted_by_many_parents_is_read_once_across_levels() {
        let dir = folder_of(&[
            ("Asm", &["A", "B"]),
            ("A", &["Screw"]),
            ("B", &["Screw"]),
            ("Screw", &[]),
        ]);
        let root = inserting(vec![
            at(1, "A v1:1", json!(SHIFT)),
            at(2, "B v1:1", json!(SHIFT)),
        ]);
        let reads = std::cell::RefCell::new(Vec::new());
        let decode = |_: Format, path: &Path, _: &mut Report| -> Result<Model, Error> {
            let name = path.file_name().unwrap().to_str().unwrap().to_owned();
            reads.borrow_mut().push(name.clone());
            Ok(match name.as_str() {
                "A v1.f3d" | "B v1.f3d" => inserting(vec![at(1, "Screw v1:1", json!(SHIFT))]),
                _ => cube(),
            })
        };
        let external = resolve(
            &root,
            &dir.path().join("Asm v1.f3d"),
            None,
            &mut Report::default(),
            &decode,
            &Finder::default(),
            &mut Vec::new(),
        );
        assert_eq!(external.len(), 2);
        let mut got = reads.borrow().clone();
        got.sort();
        assert_eq!(got, ["A v1.f3d", "B v1.f3d", "Screw v1.f3d"]);
    }

    #[test]
    fn the_total_of_files_read_is_capped_across_levels_with_a_clear_message() {
        let dir = folder_of(&[("Asm", &["A", "B"]), ("A", &["C"]), ("B", &[]), ("C", &[])]);
        let root = inserting(vec![
            at(1, "A v1:1", json!(SHIFT)),
            at(2, "B v1:1", json!(SHIFT)),
        ]);
        let decode = |_: Format, path: &Path, _: &mut Report| -> Result<Model, Error> {
            Ok(match path.file_name().unwrap().to_str().unwrap() {
                "A v1.f3d" => inserting(vec![at(1, "C v1:1", json!(SHIFT))]),
                _ => cube(),
            })
        };
        let mut report = Report::default();
        let external = resolve_in(
            &root,
            &dir.path().join("Asm v1.f3d"),
            None,
            &mut report,
            &decode,
            &Finder::default(),
            &mut Vec::new(),
            &mut Reads::new(2),
        );
        // A (1) and C (2) are read; B would be the third.
        assert!(external["synthetic:assembly:occurrence#1"].is_ok());
        let Err(left) = &external["synthetic:assembly:occurrence#2"] else {
            panic!("B should be left out");
        };
        assert!(
            left.reason
                .contains("over Dutchman's limit of 2 inserted design files read for one assembly"),
            "{}",
            left.reason
        );
    }

    #[test]
    fn nesting_stops_at_max_depth_and_says_so() {
        let total = MAX_DEPTH + 8;
        let names: Vec<String> = (0..total).map(|n| format!("N{n}")).collect();
        let listing: Vec<(&str, Vec<&str>)> = names
            .iter()
            .enumerate()
            .map(|(n, name)| {
                (
                    name.as_str(),
                    names.get(n + 1).map(String::as_str).into_iter().collect(),
                )
            })
            .collect();
        let borrowed: Vec<(&str, &[&str])> = listing
            .iter()
            .map(|(name, inserts)| (*name, inserts.as_slice()))
            .collect();
        let dir = folder_of(&borrowed);
        let root = inserting(vec![at(1, "N1 v1:1", json!(SHIFT))]);
        let reads = std::cell::Cell::new(0);
        let decode = |_: Format, path: &Path, _: &mut Report| -> Result<Model, Error> {
            reads.set(reads.get() + 1);
            let name = path.file_stem().unwrap().to_str().unwrap();
            let next: usize = name[1..name.len() - 3].parse::<usize>().unwrap() + 1;
            Ok(inserting(vec![at(
                1,
                &format!("N{next} v1:1"),
                json!(SHIFT),
            )]))
        };
        let mut report = Report::default();
        resolve(
            &root,
            &dir.path().join("N0 v1.f3d"),
            None,
            &mut report,
            &decode,
            &Finder::default(),
            &mut Vec::new(),
        );
        assert_eq!(reads.get(), MAX_DEPTH);
        assert!(
            report
                .losses()
                .iter()
                .any(|loss| loss.detail.contains(&format!("nest over {MAX_DEPTH} deep"))),
            "{report:?}"
        );
    }

    #[test]
    fn a_design_that_inserts_itself_is_left_out_not_read_again() {
        let dir = folder_of(&[("Asm", &["A"]), ("A", &["A"])]);
        let root = inserting(vec![at(1, "A v1:1", json!(SHIFT))]);
        let reads = std::cell::Cell::new(0);
        let decode = |_: Format, _: &Path, _: &mut Report| -> Result<Model, Error> {
            reads.set(reads.get() + 1);
            Ok(inserting(vec![at(1, "A v1:1", json!(SHIFT))]))
        };
        let external = resolve(
            &root,
            &dir.path().join("Asm v1.f3d"),
            None,
            &mut Report::default(),
            &decode,
            &Finder::default(),
            &mut Vec::new(),
        );
        assert_eq!(reads.get(), 1);
        assert!(external["synthetic:assembly:occurrence#1"].is_ok());
    }

    #[test]
    fn only_matched_readable_designs_count_as_placed() {
        let dir = folder_of(&[
            ("Asm", &["Good", "Broken", "Unplaced"]),
            ("Good", &[]),
            ("Broken", &[]),
            ("Unplaced", &[]),
        ]);
        let root = inserting(vec![
            at(1, "Good v1:1", json!(SHIFT)),
            at(2, "Broken v1:1", json!(SHIFT)),
            at(3, "0f1e2d3c-0000-4000-8000-000000000000", json!(SHIFT)),
        ]);
        let decode = |_: Format, path: &Path, _: &mut Report| -> Result<Model, Error> {
            if path.ends_with("Broken v1.f3d") {
                Err(Error::Decode("not a Fusion design".to_owned()))
            } else {
                Ok(cube())
            }
        };
        let placed = placed_files(&root, &dir.path().join("Asm v1.f3d"), None, &decode);
        let names: Vec<_> = placed
            .iter()
            .map(|file| file.file_name().unwrap().to_str().unwrap().to_owned())
            .collect();
        assert_eq!(names, ["Good v1.f3d"]);
    }
}
