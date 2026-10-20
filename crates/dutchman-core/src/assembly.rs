//! Assemblies: the parts and sub-assemblies an assembly places, read from their own files, and
//! their placements.
//!
//! cadmpeg reads an assembly's occurrences and leaves each external part unresolved, with the
//! path the assembly stored for it. That path is usually absolute on the machine that saved
//! it, so a part is looked for at that path, then by its file name next to the assembly. Each
//! part file is read once, however often it is placed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{Error, Format, Model, Report, Severity};

/// The most part files one assembly may bring in.
pub const MAX_PARTS: usize = 1_000;

/// The most placements one assembly may hold.
pub const MAX_PLACEMENTS: usize = 100_000;

/// How deep sub-assemblies may nest.
pub const MAX_DEPTH: usize = 32;

/// How far from 1 an occurrence's scale may be and still be rigid.
const RIGID: f64 = 1e-9;

/// An assembly's parts and where they are placed.
#[derive(Clone, Debug)]
pub struct Assembly {
    /// Each part, read from its own file, in the order first placed. A sub-assembly is a part
    /// whose model has an assembly of its own.
    pub parts: Vec<Model>,
    /// Each placed part, in the assembly's order.
    pub placements: Vec<Placement>,
}

/// One part placed in an assembly.
#[derive(Clone, Debug, PartialEq)]
pub struct Placement {
    /// The occurrence's name in the source, when it has one.
    pub name: Option<String>,
    /// Which of [`Assembly::parts`] it places.
    pub part: usize,
    /// Its placement in the assembly: a rigid transform, row-major 3 × 4, in millimetres.
    pub transform: [[f64; 4]; 3],
}

/// Decodes one file, without reading the parts an assembly places.
pub(crate) type Decode<'a> = dyn Fn(Format, &Path, &mut Report) -> Result<Model, Error> + 'a;

/// Reads `input` with `decode` and, for an assembly, the parts and sub-assemblies it places,
/// inside the assemblies whose files are in `open` (outermost first), so an assembly that
/// places itself, directly or not, is refused instead of read forever.
pub(crate) fn read(
    format: Format,
    input: &Path,
    report: &mut Report,
    decode: &Decode,
    open: &mut Vec<PathBuf>,
) -> Result<Model, Error> {
    let mut model = decode(format, input, report)?;
    if format.is_assembly() {
        let key = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
        if open.contains(&key) {
            return Err(Error::Decode(
                "the assembly places itself through its sub-assemblies".to_owned(),
            ));
        }
        if open.len() >= MAX_DEPTH {
            return Err(Error::Decode(format!(
                "sub-assemblies nest deeper than Dutchman's limit of {MAX_DEPTH}"
            )));
        }
        open.push(key);
        let resolved = resolve(&model, input, report, decode, open);
        open.pop();
        model.assembly = Some(resolved?);
    }
    Ok(model)
}

/// Reads the parts the assembly `model`, read from `input`, places. A placement whose part
/// cannot be found or read is left out and reported; an assembly with none left is an error.
pub(crate) fn resolve(
    model: &Model,
    input: &Path,
    report: &mut Report,
    decode: &Decode,
    open: &mut Vec<PathBuf>,
) -> Result<Assembly, Error> {
    let wire = serde_json::to_value(&model.ir.model)
        .map_err(|err| Error::Decode(format!("cannot read the assembly: {err}")))?;
    let mut occurrences: Vec<&Value> = wire
        .get("occurrences")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().collect())
        .unwrap_or_default();
    if occurrences.len() > MAX_PLACEMENTS {
        return Err(Error::Decode(format!(
            "the assembly places {} parts, over Dutchman's limit of {MAX_PLACEMENTS}",
            occurrences.len()
        )));
    }
    occurrences.sort_by_key(|row| row.get("ordinal").and_then(Value::as_u64));

    let folder = input.parent().unwrap_or(Path::new("."));
    let mut parts: Vec<Model> = Vec::new();
    let mut by_file: BTreeMap<PathBuf, Option<usize>> = BTreeMap::new();
    let mut placements = Vec::new();
    let mut nested = 0usize;
    for occurrence in &occurrences {
        let name = occurrence
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let subject = format!("occurrence \"{}\"", name.as_deref().unwrap_or("unnamed"));
        if occurrence.pointer("/parent/kind").and_then(Value::as_str) != Some("root") {
            nested += 1;
            continue;
        }
        let Some(stored) = occurrence
            .pointer("/prototype/document/path")
            .and_then(Value::as_str)
        else {
            report.push(
                Severity::Dropped,
                subject,
                "its part is not in a separate file Dutchman can find",
            );
            continue;
        };
        let file = file_name(stored);
        let format = match Format::from_path(Path::new(file)) {
            Some(format @ (Format::InventorPart | Format::InventorAssembly)) => format,
            _ => {
                report.push(
                    Severity::Dropped,
                    subject,
                    format!("{file}: not an Inventor part or assembly"),
                );
                continue;
            }
        };
        let Some(path) = locate(stored, file, folder) else {
            report.push(
                Severity::Dropped,
                subject,
                format!("{file}: not found next to the assembly"),
            );
            continue;
        };
        let part = match by_file.get(&path) {
            Some(known) => *known,
            None => {
                let read = if parts.len() >= MAX_PARTS {
                    Err(format!("over Dutchman's limit of {MAX_PARTS} part files"))
                } else {
                    let mut part_report = Report::default();
                    let result = read(format, &path, &mut part_report, decode, open)
                        .map_err(|err| err.to_string());
                    report.append_prefixed(part_report, file);
                    result.map(|part| {
                        parts.push(part);
                        parts.len() - 1
                    })
                };
                let index = match read {
                    Ok(index) => Some(index),
                    Err(why) => {
                        report.push(Severity::Dropped, file, why);
                        None
                    }
                };
                by_file.insert(path, index);
                index
            }
        };
        let Some(part) = part else {
            report.push(
                Severity::Dropped,
                subject,
                format!("{file} could not be read"),
            );
            continue;
        };
        match transform(occurrence) {
            Ok(transform) => placements.push(Placement {
                name,
                part,
                transform,
            }),
            Err(why) => report.push(Severity::Dropped, subject, why),
        }
    }
    if nested > 0 {
        report.push(
            Severity::Info,
            "assembly",
            format!(
                "{nested} occurrences inside sub-assemblies are read from the sub-assembly files"
            ),
        );
    }
    if placements.is_empty() {
        return Err(Error::Decode(
            "none of the assembly's parts could be read; put its part files next to it".to_owned(),
        ));
    }
    Ok(Assembly { parts, placements })
}

/// Arenas a component's own geometry never reaches into: other components' bodies are kept
/// out by not following `bodies`, and these hold structure and history, not geometry.
const NOT_GEOMETRY: [&str; 6] = [
    "bodies",
    "product_definitions",
    "occurrences",
    "assembly_joints",
    "features",
    "configurations",
];

/// What became of a design's own components.
pub(crate) enum Components {
    /// The design places no components of its own.
    None,
    /// They can be written as an assembly.
    Assembly(Assembly),
    /// They cannot, for this reason; the design converts as stored.
    Unplaceable(String),
}

/// The components a design defines and places itself (as Fusion designs do), as an assembly
/// of part models each holding only its component's bodies, plus the design's loose bodies.
pub(crate) fn local(model: &Model) -> Components {
    match local_assembly(model) {
        Ok(Some(assembly)) => Components::Assembly(assembly),
        Ok(None) => Components::None,
        Err(reason) => Components::Unplaceable(reason),
    }
}

/// The components a design defines and places itself, as an assembly of part models each
/// holding only its component's bodies, plus the design's loose bodies; `None` when it places
/// no components of its own, `Err` with the reason when they cannot be placed.
pub fn local_assembly(model: &Model) -> Result<Option<Assembly>, String> {
    let document = serde_json::to_value(&model.ir).map_err(|err| err.to_string())?;
    let wire = document.get("model").unwrap_or(&Value::Null);
    let arena = |name: &str| {
        wire.get(name)
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
    };
    let mut occurrences: Vec<&Value> = arena("occurrences")
        .iter()
        .filter(|row| row.pointer("/prototype/scope").and_then(Value::as_str) == Some("local"))
        .collect();
    if occurrences.is_empty() {
        return Ok(None);
    }
    if occurrences.len() > MAX_PLACEMENTS {
        return Err(format!("it places over {MAX_PLACEMENTS} components"));
    }
    occurrences.sort_by_key(|row| row.get("ordinal").and_then(Value::as_u64));
    if occurrences
        .iter()
        .any(|row| row.pointer("/parent/kind").and_then(Value::as_str) != Some("root"))
    {
        return Err("components inside components are not converted yet".to_owned());
    }

    let definitions: BTreeMap<&str, &Value> = arena("product_definitions")
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let owned: Vec<&str> = definitions
        .values()
        .filter_map(|row| row.get("bodies").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect();

    let mut parts = Vec::new();
    let mut part_of: BTreeMap<&str, usize> = BTreeMap::new();
    let mut placements = Vec::new();
    for occurrence in &occurrences {
        let definition = occurrence
            .pointer("/prototype/definition")
            .and_then(Value::as_str)
            .ok_or("a component placement names no component")?;
        let part = match part_of.get(definition) {
            Some(part) => *part,
            None => {
                let row = definitions
                    .get(definition)
                    .ok_or("a component placement names a component the design lacks")?;
                let bodies: Vec<&str> = row
                    .get("bodies")
                    .and_then(Value::as_array)
                    .map(|bodies| bodies.iter().filter_map(Value::as_str).collect())
                    .unwrap_or_default();
                if bodies.is_empty() {
                    return Err(format!(
                        "cadmpeg does not say which bodies belong to component {}",
                        name_of(row).unwrap_or(definition)
                    ));
                }
                let name = name_of(row)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("{} component {}", model.name, parts.len() + 1));
                parts.push(only(model, &document, &bodies, name)?);
                part_of.insert(definition, parts.len() - 1);
                parts.len() - 1
            }
        };
        placements.push(Placement {
            name: occurrence
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_owned),
            part,
            transform: transform(occurrence)?,
        });
    }

    // Bodies no component owns are the design's own, placed where they are.
    let loose: Vec<&str> = arena("bodies")
        .iter()
        .filter_map(|row| row.get("id")?.as_str())
        .filter(|id| !owned.contains(id))
        .collect();
    if !loose.is_empty() {
        parts.push(only(model, &document, &loose, model.name.clone())?);
        placements.push(Placement {
            name: None,
            part: parts.len() - 1,
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
        });
    }
    Ok(Some(Assembly { parts, placements }))
}

fn name_of(definition: &Value) -> Option<&str> {
    ["label", "source_name"]
        .into_iter()
        .find_map(|key| definition.get(key).and_then(Value::as_str))
        .filter(|name| !name.trim().is_empty())
}

/// `model` reduced to `bodies` and the geometry they reach, named `name`.
fn only(model: &Model, document: &Value, bodies: &[&str], name: String) -> Result<Model, String> {
    let wire = document
        .get("model")
        .and_then(Value::as_object)
        .ok_or("the model is malformed")?;
    // Every row by id, with its arena.
    let mut rows: BTreeMap<&str, (&str, &Value)> = BTreeMap::new();
    for (arena, values) in wire {
        for row in values.as_array().map_or(&[][..], Vec::as_slice) {
            if let Some(id) = row.get("id").and_then(Value::as_str) {
                rows.insert(id, (arena.as_str(), row));
            }
        }
    }
    let mut keep: BTreeMap<&str, ()> = BTreeMap::new();
    let mut queue: Vec<&str> = Vec::new();
    for body in bodies {
        if rows.contains_key(body) && keep.insert(body, ()).is_none() {
            queue.push(body);
        }
    }
    while let Some(id) = queue.pop() {
        let Some((_, row)) = rows.get(id) else {
            continue;
        };
        let mut refs = Vec::new();
        strings(row, &mut refs);
        for reference in refs {
            if let Some((arena, _)) = rows.get(reference) {
                if !NOT_GEOMETRY.contains(arena) && keep.insert(reference, ()).is_none() {
                    queue.push(reference);
                }
            }
        }
    }
    let mut reduced = serde_json::Map::new();
    for (arena, values) in wire {
        let kept: Vec<Value> = values
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .filter(|row| {
                row.get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| keep.contains_key(id))
            })
            .cloned()
            .collect();
        reduced.insert(arena.clone(), Value::Array(kept));
    }
    let mut part = document.clone();
    if let Some(fields) = part.as_object_mut() {
        fields.insert("model".to_owned(), Value::Object(reduced));
        fields.remove("native");
    }
    let ir = cadmpeg_ir::CadIr::from_json(&part.to_string())
        .map_err(|err| format!("a component's geometry does not stand alone: {err}"))?;
    Ok(Model {
        ir,
        fidelity: None,
        name,
        source_file: model.source_file.clone(),
        assembly: None,
    })
}

/// Every string inside `value`.
fn strings<'v>(value: &'v Value, out: &mut Vec<&'v str>) {
    match value {
        Value::String(s) => out.push(s),
        Value::Array(values) => values.iter().for_each(|v| strings(v, out)),
        Value::Object(fields) => fields.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// The file name in a stored path, whichever separators the saving machine used.
fn file_name(stored: &str) -> &str {
    stored.rsplit(['/', '\\']).next().unwrap_or(stored)
}

/// The part file: at its stored path when that exists (relative to the assembly, and with
/// either separator), else by name next to the assembly (ignoring case, as the saving system
/// may have).
fn locate(stored: &str, file: &str, folder: &Path) -> Option<PathBuf> {
    let candidates = [stored.to_owned(), stored.replace('\\', "/")];
    for candidate in &candidates {
        let path = Path::new(candidate);
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            folder.join(path)
        };
        if path.is_file() {
            return Some(path);
        }
    }
    let beside = folder.join(file);
    if beside.is_file() {
        return Some(beside);
    }
    std::fs::read_dir(folder)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| {
            path.is_file()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case(file))
        })
}

/// An occurrence's rigid placement, or why it is not one.
fn transform(occurrence: &Value) -> Result<[[f64; 4]; 3], String> {
    let malformed = || "its placement is malformed".to_owned();
    let rows = occurrence
        .get("transform")
        .and_then(Value::as_array)
        .ok_or_else(malformed)?;
    let mut out = [[0.0; 4]; 3];
    for (row, values) in out.iter_mut().zip(rows) {
        let values = values.as_array().ok_or_else(malformed)?;
        for (cell, value) in row.iter_mut().zip(values) {
            *cell = value
                .as_f64()
                .filter(|v| v.is_finite())
                .ok_or_else(malformed)?;
        }
    }
    if rows.len() != 3
        || rows
            .iter()
            .any(|row| row.as_array().map(Vec::len) != Some(4))
    {
        return Err(malformed());
    }
    let scaled = occurrence
        .get("scale")
        .and_then(Value::as_array)
        .is_some_and(|scale| {
            scale
                .iter()
                .any(|s| s.as_f64().is_none_or(|s| (s - 1.0).abs() > RIGID))
        });
    if scaled {
        return Err("scaled placements are not carried".to_owned());
    }
    Ok(out)
}

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use serde_json::json;

    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn occurrence(key: usize, path: &str, parent: Value, x: f64) -> Value {
        json!({
            "id": format!("inventor:assembly:occurrence#{key}"),
            "prototype": {"scope": "external", "document": {"resolution": "path", "path": path}},
            "parent": parent,
            "ordinal": key,
            "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
            "scale": [1.0, 1.0, 1.0],
            "name": format!("Occurrence:{key}"),
        })
    }

    fn root() -> Value {
        json!({"kind": "root"})
    }

    /// A folder of placeholder files and a decoder that gives each its model: the synthetic
    /// Inventor cube for `cube.ipt`, and for an assembly the occurrences `assemblies` lists
    /// for its file name (as `(stored path, x)`).
    struct Project {
        dir: tempfile::TempDir,
        cube: Model,
        assemblies: BTreeMap<String, Vec<Value>>,
    }

    impl Project {
        fn new(assemblies: &[(&str, Vec<Value>)]) -> Project {
            let dir = tempfile::tempdir().unwrap();
            let cube = crate::read(
                Format::InventorPart,
                &fixture("synthetic_cube.ipt"),
                &mut Report::default(),
            )
            .unwrap();
            std::fs::write(dir.path().join("Cube.IPT"), b"").unwrap();
            for (name, _) in assemblies {
                std::fs::write(dir.path().join(name), b"").unwrap();
            }
            let assemblies = assemblies
                .iter()
                .map(|(name, rows)| ((*name).to_owned(), rows.clone()))
                .collect();
            Project {
                dir,
                cube,
                assemblies,
            }
        }

        fn decode(&self, _format: Format, path: &Path, _: &mut Report) -> Result<Model, Error> {
            let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
            if name == "cube.ipt" {
                return Ok(self.cube.clone());
            }
            let rows = self.assemblies.get(&name).cloned().unwrap_or_default();
            let mut wire = serde_json::to_value(&self.cube.ir).unwrap();
            wire["model"]["occurrences"] = Value::Array(rows);
            Ok(Model {
                ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
                fidelity: None,
                name: name.trim_end_matches(".iam").to_owned(),
                source_file: Some(name),
                assembly: None,
            })
        }

        fn read(&self, name: &str, report: &mut Report) -> Result<Model, Error> {
            let decode = |format: Format, path: &Path, report: &mut Report| {
                self.decode(format, path, report)
            };
            read(
                Format::InventorAssembly,
                &self.dir.path().join(name),
                report,
                &decode,
                &mut Vec::new(),
            )
        }
    }

    fn dropped(report: &Report) -> Vec<String> {
        report
            .losses()
            .iter()
            .filter(|loss| loss.severity == Severity::Dropped)
            .map(|loss| format!("{}: {}", loss.subject, loss.detail))
            .collect()
    }

    #[test]
    fn parts_and_sub_assemblies_are_read_from_their_files() {
        let nested = json!({"kind": "occurrence", "occurrence": "inventor:assembly:occurrence#2"});
        let project = Project::new(&[
            (
                "top.iam",
                vec![
                    occurrence(0, r"C:\Work\Parts\cube.ipt", root(), 0.0),
                    occurrence(1, r"C:\Work\Parts\cube.ipt", root(), 30.0),
                    occurrence(2, r"C:\Work\sub.iam", root(), 100.0),
                    occurrence(3, r"C:\Work\Parts\gone.ipt", root(), 0.0),
                    occurrence(4, r"C:\Work\Parts\cube.ipt", nested, 0.0),
                ],
            ),
            (
                "sub.iam",
                vec![
                    occurrence(0, "cube.ipt", root(), 0.0),
                    occurrence(1, "cube.ipt", root(), 15.0),
                ],
            ),
        ]);
        let mut report = Report::default();
        let top = project.read("top.iam", &mut report).unwrap();
        let assembly = top.assembly.unwrap();
        assert_eq!(
            assembly.parts.len(),
            2,
            "the cube once, and the sub-assembly"
        );
        assert_eq!(assembly.placements.len(), 3);
        assert_eq!(assembly.placements[2].transform[0][3], 100.0);
        let sub = assembly.parts[1].assembly.as_ref().unwrap();
        assert_eq!(sub.parts.len(), 1);
        assert_eq!(sub.placements.len(), 2);
        assert_eq!(
            dropped(&report),
            ["occurrence \"Occurrence:3\": gone.ipt: not found next to the assembly"]
        );
    }

    #[test]
    fn an_assembly_that_places_itself_is_refused() {
        let project = Project::new(&[
            (
                "a.iam",
                vec![
                    occurrence(0, "b.iam", root(), 0.0),
                    occurrence(1, "cube.ipt", root(), 0.0),
                ],
            ),
            ("b.iam", vec![occurrence(0, "a.iam", root(), 0.0)]),
        ]);
        let mut report = Report::default();
        let a = project.read("a.iam", &mut report).unwrap();
        assert_eq!(
            a.assembly.unwrap().placements.len(),
            1,
            "the cube is still placed"
        );
        assert!(
            dropped(&report)
                .iter()
                .any(|loss| loss.contains("places itself through its sub-assemblies")),
            "{:?}",
            dropped(&report)
        );
    }

    #[test]
    fn sub_assemblies_nest_only_so_deep() {
        let chain: Vec<(String, Vec<Value>)> = (0..=MAX_DEPTH + 2)
            .map(|n| {
                (
                    format!("d{n}.iam"),
                    vec![
                        occurrence(0, &format!("d{}.iam", n + 1), root(), 0.0),
                        occurrence(1, "cube.ipt", root(), 0.0),
                    ],
                )
            })
            .collect();
        let named: Vec<(&str, Vec<Value>)> = chain
            .iter()
            .map(|(name, rows)| (name.as_str(), rows.clone()))
            .collect();
        let project = Project::new(&named);
        let mut report = Report::default();
        project.read("d0.iam", &mut report).unwrap();
        assert!(
            dropped(&report)
                .iter()
                .any(|loss| loss.contains("nest deeper")),
            "{:?}",
            dropped(&report)
        );
    }

    #[test]
    fn an_assembly_with_no_readable_part_is_an_error() {
        let project = Project::new(&[("lonely.iam", vec![occurrence(0, "gone.ipt", root(), 0.0)])]);
        let err = project
            .read("lonely.iam", &mut Report::default())
            .unwrap_err();
        assert!(
            err.to_string().contains("put its part files next to it"),
            "{err}"
        );
    }

    #[test]
    fn parts_are_found_as_stored_or_beside_the_assembly() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Assemblies");
        std::fs::create_dir_all(dir.path().join("Parts")).unwrap();
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("Bracket.ipt"), b"").unwrap();
        std::fs::write(dir.path().join("Parts").join("Plate.ipt"), b"").unwrap();
        let find = |stored: &str| locate(stored, file_name(stored), &folder);

        // An absolute path from another machine: found by name beside the assembly.
        assert_eq!(
            find(r"C:\Users\Someone\Work\Bracket.ipt"),
            Some(folder.join("Bracket.ipt"))
        );
        assert_eq!(
            find("/home/someone/work/Bracket.ipt"),
            Some(folder.join("Bracket.ipt"))
        );
        // A different case, as Windows saves it.
        assert!(find(r"C:\Work\BRACKET.IPT").is_some());
        // A relative path, with either separator.
        assert!(find(r"..\Parts\Plate.ipt").is_some());
        assert!(find("../Parts/Plate.ipt").is_some());
        assert_eq!(find(r"C:\Work\Missing.ipt"), None);
    }

    /// cadmpeg's example 10 mm cube in a design whose component `Block` owns it, placed at
    /// the origin and 30 mm along X, plus `loose` (a copy of the cube no component owns) when
    /// asked.
    fn with_components(owned: bool, nested: bool) -> Model {
        let part = crate::read(
            Format::InventorPart,
            &fixture("synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        let mut wire = serde_json::to_value(&part.ir).unwrap();
        let body = wire["model"]["bodies"][0]["id"]
            .as_str()
            .unwrap()
            .to_owned();
        let bodies = if owned { json!([body]) } else { json!([]) };
        wire["model"]["product_definitions"] = json!([{
            "id": "f3d:model:component#block", "kind": "part", "label": "Block", "bodies": bodies,
        }]);
        let placed = |key: u32, x: f64, parent: Value| {
            json!({
                "id": format!("f3d:model:occurrence#{key}"),
                "prototype": {"scope": "local", "definition": "f3d:model:component#block"},
                "parent": parent,
                "ordinal": key,
                "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
                "scale": [1.0, 1.0, 1.0],
            })
        };
        let second_parent = if nested {
            json!({"kind": "occurrence", "occurrence": "f3d:model:occurrence#0"})
        } else {
            root()
        };
        wire["model"]["occurrences"] =
            json!([placed(0, 0.0, root()), placed(1, 30.0, second_parent)]);
        Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..part
        }
    }

    #[test]
    fn components_that_own_their_bodies_become_an_assembly() {
        let assembly = local_assembly(&with_components(true, false))
            .unwrap()
            .unwrap();
        assert_eq!(assembly.parts.len(), 1);
        let part = &assembly.parts[0];
        assert_eq!(part.name, "Block");
        assert_eq!(part.ir.model.bodies.len(), 1);
        assert_eq!(part.ir.model.faces.len(), 6);
        assert!(part.ir.model.occurrences.is_empty());
        assert_eq!(assembly.placements.len(), 2);
        assert_eq!(assembly.placements[1].transform[0][3], 30.0);
    }

    #[test]
    fn components_that_do_not_say_what_they_own_convert_as_stored() {
        let err = local_assembly(&with_components(false, false)).unwrap_err();
        assert_eq!(
            err,
            "cadmpeg does not say which bodies belong to component Block"
        );
        let err = local_assembly(&with_components(true, true)).unwrap_err();
        assert_eq!(err, "components inside components are not converted yet");
    }

    #[test]
    fn a_design_without_components_is_not_an_assembly() {
        let part = crate::read(
            Format::InventorPart,
            &fixture("synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        assert!(local_assembly(&part).unwrap().is_none());
    }

    #[test]
    fn fusion_components_without_bodies_are_reported() {
        let mut report = Report::default();
        let model = crate::read(
            Format::FusionArchive,
            &fixture("design_copy_paste.f3d"),
            &mut report,
        )
        .unwrap();
        assert!(model.assembly.is_none());
        let loss = report
            .losses()
            .iter()
            .find(|loss| loss.subject == "components")
            .unwrap();
        assert_eq!(loss.severity, Severity::Degraded);
        assert!(
            loss.detail
                .contains("cadmpeg does not say which bodies belong to component"),
            "{}",
            loss.detail
        );
    }

    #[test]
    fn file_names_come_from_either_separator() {
        assert_eq!(file_name(r"C:\Work\Parts\Bracket.ipt"), "Bracket.ipt");
        assert_eq!(file_name("/home/a/Bracket.ipt"), "Bracket.ipt");
        assert_eq!(file_name("Bracket.ipt"), "Bracket.ipt");
    }

    #[test]
    fn placements_must_be_rigid_and_whole() {
        let identity = serde_json::json!([[1, 0, 0, 5], [0, 1, 0, 0], [0, 0, 1, 0]]);
        let row =
            |scale: serde_json::Value| serde_json::json!({"transform": identity, "scale": scale});
        assert_eq!(
            transform(&row(serde_json::json!([1, 1, 1]))).unwrap()[0],
            [1.0, 0.0, 0.0, 5.0]
        );
        assert!(transform(&row(serde_json::json!([2, 1, 1]))).is_err());
        assert!(transform(&serde_json::json!({"transform": [[1, 0, 0]]})).is_err());
    }
}
