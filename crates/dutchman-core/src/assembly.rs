//! Assemblies: the parts an assembly places, read from their own files, and their placements.
//!
//! cadmpeg reads an assembly's occurrences and leaves each external part unresolved, with the
//! path the assembly stored for it. That path is usually absolute on the machine that saved
//! it, so a part is looked for at that path, then by its file name next to the assembly. Each
//! part file is read once, however often it is placed.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::{read, Error, Format, Model, Report, Severity};

/// The most part files one assembly may bring in.
pub const MAX_PARTS: usize = 1_000;

/// The most placements one assembly may hold.
pub const MAX_PLACEMENTS: usize = 100_000;

/// How far from 1 an occurrence's scale may be and still be rigid.
const RIGID: f64 = 1e-9;

/// An assembly's parts and where they are placed.
#[derive(Clone, Debug)]
pub struct Assembly {
    /// Each part, read from its own file, in the order first placed.
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

/// Reads the parts the assembly `model`, read from `input`, places. A placement whose part
/// cannot be found or read is left out and reported; an assembly with none left is an error.
pub(crate) fn resolve(model: &Model, input: &Path, report: &mut Report) -> Result<Assembly, Error> {
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
        match Format::from_path(Path::new(file)) {
            Some(Format::InventorPart) => {}
            Some(format) if format.is_assembly() => {
                report.push(
                    Severity::Dropped,
                    subject,
                    format!("{file}: sub-assemblies are not converted yet"),
                );
                continue;
            }
            _ => {
                report.push(
                    Severity::Dropped,
                    subject,
                    format!("{file}: not an Inventor part"),
                );
                continue;
            }
        }
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
                    let result = read(Format::InventorPart, &path, &mut part_report)
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
            Severity::Dropped,
            "assembly",
            format!("{nested} occurrences inside sub-assemblies are not converted yet"),
        );
    }
    if placements.is_empty() {
        return Err(Error::Decode(
            "none of the assembly's parts could be read; put its part files next to it".to_owned(),
        ));
    }
    Ok(Assembly { parts, placements })
}

/// The file name in a stored path, whichever separators the saving machine used.
fn file_name(stored: &str) -> &str {
    stored.rsplit(['/', '\\']).next().unwrap_or(stored)
}

/// The part file: at its stored path when that exists, else by name next to the assembly
/// (ignoring case, as the saving system may have).
fn locate(stored: &str, file: &str, folder: &Path) -> Option<PathBuf> {
    let as_stored = Path::new(stored);
    let as_stored = if as_stored.is_absolute() {
        as_stored.to_path_buf()
    } else {
        folder.join(as_stored)
    };
    if as_stored.is_file() {
        return Some(as_stored);
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

    fn occurrence(key: u32, path: &str, parent: Value, x: f64) -> Value {
        json!({
            "id": format!("inventor:assembly:occurrence#{key}"),
            "prototype": {"scope": "external", "document": {"resolution": "path", "path": path}},
            "parent": parent,
            "ordinal": key,
            "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
            "scale": [1.0, 1.0, 1.0],
            "name": format!("Part:{key}"),
        })
    }

    /// An assembly (CADIR built here) in a fresh folder holding a copy of the synthetic
    /// Inventor cube, placing it twice by a path from another machine, plus a part that is
    /// missing, a sub-assembly and an occurrence inside a sub-assembly.
    fn assembly() -> (tempfile::TempDir, Model) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::copy(
            fixture("synthetic_cube.ipt"),
            dir.path().join("Synthetic_Cube.ipt"),
        )
        .unwrap();
        let part = read(
            Format::InventorPart,
            &fixture("synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        let mut wire = serde_json::to_value(&part.ir).unwrap();
        let root = json!({"kind": "root"});
        wire["model"]["occurrences"] = json!([
            occurrence(1, r"C:\Work\Parts\synthetic_cube.ipt", root.clone(), 0.0),
            occurrence(2, r"C:\Work\Parts\synthetic_cube.ipt", root.clone(), 30.0),
            occurrence(3, r"C:\Work\Parts\gone.ipt", root.clone(), 0.0),
            occurrence(4, r"C:\Work\Sub.iam", root, 0.0),
            occurrence(
                5,
                r"C:\Work\Parts\synthetic_cube.ipt",
                json!({"kind": "occurrence", "occurrence": "inventor:assembly:occurrence#4"}),
                0.0
            ),
        ]);
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            fidelity: None,
            name: "asm".to_owned(),
            source_file: Some("asm.iam".to_owned()),
            assembly: None,
        };
        (dir, model)
    }

    #[test]
    fn parts_are_found_beside_the_assembly_and_read_once() {
        let (dir, model) = assembly();
        let mut report = Report::default();
        let assembly = resolve(&model, &dir.path().join("asm.iam"), &mut report).unwrap();
        assert_eq!(assembly.parts.len(), 1);
        assert_eq!(assembly.placements.len(), 2);
        assert_eq!(assembly.placements[1].transform[0][3], 30.0);
        assert_eq!(assembly.placements[1].name.as_deref(), Some("Part:2"));
        let dropped: Vec<String> = report
            .losses()
            .iter()
            .filter(|loss| loss.severity == Severity::Dropped)
            .map(|loss| format!("{}: {}", loss.subject, loss.detail))
            .collect();
        assert_eq!(
            dropped,
            [
                "occurrence \"Part:3\": gone.ipt: not found next to the assembly",
                "occurrence \"Part:4\": Sub.iam: sub-assemblies are not converted yet",
                "assembly: 1 occurrences inside sub-assemblies are not converted yet",
            ]
        );
    }

    #[test]
    fn an_assembly_with_no_readable_part_is_an_error() {
        let (_dir, model) = assembly();
        let elsewhere = tempfile::tempdir().unwrap();
        let err = resolve(
            &model,
            &elsewhere.path().join("asm.iam"),
            &mut Report::default(),
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("put its part files next to it"),
            "{err}"
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
