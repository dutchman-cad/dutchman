//! Resolve SolidWorks references without interpreting native format records.
use super::*;

type LoadedPart = Result<(usize, std::rc::Rc<Model>), (Stage, String)>;

pub(super) fn resolve(
    model: &Model,
    input: &Path,
    report: &mut Report,
    decode: &Decode,
    finder: &Finder,
    open: &mut Vec<PathBuf>,
) -> Result<Option<Assembly>, Error> {
    let wire =
        serde_json::to_value(&model.ir.model).map_err(|err| Error::Decode(err.to_string()))?;
    let rows = wire
        .get("occurrences")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    if rows.len() > MAX_PLACEMENTS {
        return Err(Error::Assembly(format!(
            "the assembly places over {MAX_PLACEMENTS} components"
        )));
    }
    let folder = input.parent().unwrap_or(Path::new("."));
    let mut external = ExternalParts::new();
    let mut files: BTreeMap<PathBuf, LoadedPart> = BTreeMap::new();
    let mut unresolved = Vec::new();
    for row in rows {
        if scope(row) == Some("local") {
            continue;
        }
        let id = row.get("id").and_then(Value::as_str).unwrap_or("");
        let name = row.get("name").and_then(Value::as_str).map(str::to_owned);
        let stored = row
            .pointer("/prototype/document/path")
            .and_then(Value::as_str);
        let file = stored.map(file_name);
        let result = (|| {
            let stored = stored.ok_or((
                Stage::Input,
                "its part is not in a separate file Dutchman can find".to_owned(),
            ))?;
            let format = match Format::from_path(Path::new(file_name(stored))) {
                Some(format @ (Format::SolidWorksPart | Format::SolidWorksAssembly)) => format,
                _ => return Err((Stage::Input, "not a SolidWorks part or assembly".to_owned())),
            };
            let found = finder
                .sibling_first(stored, folder)
                .map_err(|err| (Stage::Input, format!("{stored}: {err}")))?;
            crate::names::saw_path(&found.path, report);
            if let Some((count, used)) = &found.ambiguous {
                crate::names::saw_stored(used, report);
                report.push_at(
                    Stage::Input,
                    Severity::Info,
                    file.unwrap_or(stored),
                    format!("{count} files in the project have this name; used \"{used}\""),
                );
            }
            let count = files.len();
            let loaded = match files.entry(found.path.clone()) {
                std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                std::collections::btree_map::Entry::Vacant(entry) => {
                    let loaded = if count >= MAX_PARTS {
                        Err((
                            Stage::Input,
                            format!("over Dutchman's limit of {MAX_PARTS} part files"),
                        ))
                    } else {
                        let mut part_report = Report::default();
                        let loaded =
                            read(format, &found.path, &mut part_report, decode, finder, open)
                                .map(|part| (count, std::rc::Rc::new(part)))
                                .map_err(|err| (err.stage(), err.to_string()));
                        report.append_prefixed(part_report, file.unwrap_or(stored));
                        loaded
                    };
                    entry.insert(loaded)
                }
            }
            .as_ref()
            .map_err(Clone::clone)?;
            // The reader supplies the saved active configuration, with no option to select
            // another. Only claim the requested configuration when that identity is proven.
            if let Some(requested) = row.pointer("/prototype/object").and_then(Value::as_str) {
                let matched = loaded
                    .1
                    .ir
                    .model
                    .configurations
                    .iter()
                    .any(|config| config.active && config.name.as_deref() == Some(requested));
                if !matched {
                    report.push_at(Stage::Map, Severity::Degraded,
                        format!("occurrence \"{}\"", name.as_deref().unwrap_or("unnamed")),
                        format!("configuration \"{requested}\" could not be selected; placed the file's available active configuration"));
                }
            }
            Ok(loaded.clone())
        })();
        let result = result.map_err(|(stage, reason)| {
            let subject = format!("occurrence \"{}\"", name.as_deref().unwrap_or("unnamed"));
            report.push_at(stage, Severity::Dropped, subject, reason.clone());
            LeftOut {
                name,
                file: file.map(str::to_owned),
                reason,
                stage,
            }
        });
        if scope(row) == Some("external") {
            external.insert(id.to_owned(), result);
        } else if let Err(left) = result {
            unresolved.push(left);
        }
    }
    let mut assembly = local_components_with_threads(model, &external, finder.threads, report)
        .map_err(Error::Assembly)?
        .map(|(assembly, _)| assembly)
        .ok_or_else(|| {
            Error::Assembly("no component placements could be read from the assembly".to_owned())
        })?;
    // Layout numbers are local to this component-tree build. Distinct assembly files
    // must not make unrelated leaf parts share a writer definition just because both
    // were assigned layout 0. Already resolved external trees keep their own namespace.
    fn namespace(assembly: &mut Assembly, source: &str) {
        for identity in assembly.identities.iter_mut().flatten() {
            if identity.starts_with("layout ") {
                *identity = format!("SolidWorks {source}: {identity}");
            }
        }
        for part in &mut assembly.parts {
            if let Some(sub) = &mut part.assembly {
                namespace(sub, source);
            }
        }
    }
    let source = input.canonicalize().unwrap_or_else(|_| input.to_path_buf());
    namespace(&mut assembly, &source.to_string_lossy());
    assembly.left_out.extend(unresolved);
    Ok(Some(assembly))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::cell::Cell;

    fn cube() -> Model {
        crate::read(
            Format::SolidWorksPart,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/solidworks_cube.sldprt"),
            &mut Report::default(),
        )
        .unwrap()
    }

    fn occurrence(key: usize, path: &str, parent: Value) -> Value {
        json!({"id": format!("synthetic:assembly:occurrence#{key}"), "ordinal": key,
            "prototype": {"scope": "external", "document": {"resolution": "path", "path": path}, "object": "Default"},
            "parent": parent, "name": format!("Part {key}"),
            "transform": [[1,0,0,30 * key],[0,1,0,0],[0,0,1,0]], "scale": [1,1,1]})
    }

    fn assembly(cube: &Model, rows: Vec<Value>) -> Model {
        let mut wire = serde_json::to_value(&cube.ir).unwrap();
        wire["model"]["bodies"] = json!([]);
        wire["model"]["occurrences"] = json!(rows);
        Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..cube.clone()
        }
    }

    #[test]
    fn windows_paths_search_repeats_failures_and_configuration_losses() {
        let dir = tempfile::tempdir().unwrap();
        let search = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Pin.SLDPRT"), b"").unwrap();
        std::fs::write(dir.path().join("Bad.sldprt"), b"").unwrap();
        std::fs::write(search.path().join("Other.SLDPRT"), b"").unwrap();
        let cube = cube();
        let mut source = assembly(
            &cube,
            vec![
                occurrence(1, r"C:\old\PIN.sldprt", json!({"kind":"root"})),
                occurrence(2, r"C:\old\pin.SLDPRT", json!({"kind":"root"})),
                occurrence(3, r"C:\old\other.sldprt", json!({"kind":"root"})),
                occurrence(4, "missing.sldprt", json!({"kind":"root"})),
                occurrence(5, "bad.sldprt", json!({"kind":"root"})),
            ],
        );
        let mut wire = serde_json::to_value(&source.ir).unwrap();
        wire["model"]["occurrences"][1]["prototype"]["object"] = json!("Long");
        source.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let mut part = cube.clone();
        part.ir.model.configurations.clear();
        part.ir.model.configurations.push(
            serde_json::from_value(json!({
                "id":"synthetic:part:configuration#default", "name":"Default", "active":true
            }))
            .unwrap(),
        );
        let reads = Cell::new(0);
        let decode = |_: Format, path: &Path, _: &mut Report| {
            reads.set(reads.get() + 1);
            if path.file_name().unwrap() == "Bad.sldprt" {
                Err(Error::Decode("no geometry could be read".to_owned()))
            } else {
                Ok(part.clone())
            }
        };
        let mut report = Report::default();
        let mut settings = PartSearch::project(dir.path());
        settings.also.push(search.path().to_path_buf());
        let resolved = resolve(
            &source,
            &dir.path().join("Top.SLDASM"),
            &mut report,
            &decode,
            &Finder::new(&settings),
            &mut Vec::new(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(reads.get(), 3);
        assert_eq!(resolved.parts.len(), 2);
        assert_eq!(resolved.placements.len(), 3);
        assert_eq!(resolved.placements[0].part, resolved.placements[1].part);
        assert_eq!(resolved.placements[1].transform[0][3], 60.0);
        assert_eq!(resolved.left_out.len(), 2);
        let losses: Vec<_> = report
            .losses()
            .iter()
            .filter(|loss| loss.detail.contains("could not be selected"))
            .collect();
        assert_eq!(losses.len(), 1);
        assert!(losses[0].detail.contains("Long"));
    }

    #[test]
    fn external_subassemblies_keep_parts_when_a_cycle_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["Top.SLDASM", "Sub.SLDASM", "Pin.SLDPRT"] {
            std::fs::write(dir.path().join(name), b"").unwrap();
        }
        let cube = cube();
        let top = assembly(
            &cube,
            vec![occurrence(1, "Sub.SLDASM", json!({"kind":"root"}))],
        );
        let sub = assembly(
            &cube,
            vec![
                occurrence(2, "Pin.SLDPRT", json!({"kind":"root"})),
                occurrence(3, "Top.SLDASM", json!({"kind":"root"})),
            ],
        );
        let decode = |format: Format, path: &Path, _: &mut Report| {
            Ok(if format == Format::SolidWorksPart {
                cube.clone()
            } else if path.file_name().unwrap() == "Sub.SLDASM" {
                sub.clone()
            } else {
                top.clone()
            })
        };
        let mut report = Report::default();
        let model = read(
            Format::SolidWorksAssembly,
            &dir.path().join("Top.SLDASM"),
            &mut report,
            &decode,
            &Finder::new(&PartSearch::project(dir.path())),
            &mut Vec::new(),
        )
        .unwrap();
        let top = model.assembly.unwrap();
        let sub = top.parts[0].assembly.as_ref().unwrap();
        assert_ne!(top.identities[0], sub.identities[0]);
        assert_eq!(sub.placements.len(), 1);
        assert_eq!(sub.left_out.len(), 1);
        assert!(sub.left_out[0].reason.contains("places itself"));
        assert_eq!(top.completeness().placed, 2);
    }

    #[test]
    fn local_subassembly_and_unresolved_virtual_component_are_accounted_for() {
        let cube = cube();
        let mut local = occurrence(1, "unused", json!({"kind":"root"}));
        local["prototype"] =
            json!({"scope":"local", "definition":"synthetic:assembly:component#holder"});
        let mut source = assembly(
            &cube,
            vec![
                local,
                occurrence(
                    2,
                    "Pin.SLDPRT",
                    json!({"kind":"occurrence", "occurrence":"synthetic:assembly:occurrence#1"}),
                ),
                json!({"id":"synthetic:assembly:occurrence#3", "ordinal":3, "parent":{"kind":"root"},
                "prototype":{"scope":"unresolved"}, "transform":IDENTITY, "scale":[1,1,1]}),
            ],
        );
        let mut wire = serde_json::to_value(&source.ir).unwrap();
        wire["model"]["product_definitions"] = json!([{"id":"synthetic:assembly:component#holder", "kind":"part", "source_name":"Holder", "bodies":[]}]);
        source.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Pin.SLDPRT"), b"").unwrap();
        let decode = |_: Format, _: &Path, _: &mut Report| Ok(cube.clone());
        let mut report = Report::default();
        let result = resolve(
            &source,
            &dir.path().join("Top.SLDASM"),
            &mut report,
            &decode,
            &Finder::new(&PartSearch::project(dir.path())),
            &mut Vec::new(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(result.placements.len(), 1);
        assert_eq!(
            result.parts[0].assembly.as_ref().unwrap().placements.len(),
            1
        );
        assert_eq!(result.left_out.len(), 1);
    }
}
