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
    reads: &mut SolidWorksReads,
) -> Result<Option<Assembly>, Error> {
    let wire = products::occurrences(&model.ir.model).map_err(Error::Assembly)?;
    let rows = wire.as_array().map_or(&[][..], Vec::as_slice);
    if rows.len() > MAX_PLACEMENTS {
        return Err(Error::Assembly(format!(
            "the assembly places over {MAX_PLACEMENTS} components"
        )));
    }
    let folder = input_folder(input);
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
                        let key = found
                            .path
                            .canonicalize()
                            .unwrap_or_else(|_| found.path.clone());
                        if open.contains(&key) {
                            Err((
                                Stage::Input,
                                "the assembly places itself through its sub-assemblies".to_owned(),
                            ))
                        } else if let Some(Err(error)) = reads.get(&key) {
                            Err(error.clone())
                        } else if !reads.contains_key(&key) && reads.len() >= MAX_PARTS {
                            Err((
                                Stage::Input,
                                format!("over Dutchman's limit of {MAX_PARTS} part files"),
                            ))
                        } else {
                            let mut part_report = Report::default();
                            let loaded = read_cached(
                                format,
                                &found.path,
                                &mut part_report,
                                decode,
                                finder,
                                open,
                                reads,
                            )
                            .map(std::rc::Rc::new)
                            .map_err(|err| (err.stage(), err.to_string()));
                            report.append_prefixed(part_report, file.unwrap_or(stored));
                            if let Err(error) = &loaded {
                                reads.entry(key).or_insert_with(|| Err(error.clone()));
                            }
                            loaded.map(|part| (count, part))
                        }
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
    // Every referenced part file was left out and the assembly holds no bodies of its own:
    // say that the parts were not found, rather than letting the writer report parts
    // without geometry.
    if !external.is_empty()
        && external.values().all(Result::is_err)
        && model.ir.geometry.bodies.is_empty()
    {
        let missing: Vec<&str> = external.keys().take(3).map(String::as_str).collect();
        return Err(Error::Assembly(format!(
            "none of the assembly's parts could be found or read ({}{}); if they are elsewhere, \
             give the project folder that holds them",
            missing.join(", "),
            if external.len() > missing.len() {
                ", ..."
            } else {
                ""
            }
        )));
    }
    let mut assembly = local_components_with_threads(
        model,
        &external,
        finder.threads,
        finder.part_positions,
        report,
    )
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
            "prototype": {"kind": "external", "document": {"kind": "path", "path": path}, "object": "Default"},
            "parent": parent, "name": format!("Part {key}"),
            "transform": [[1,0,0,30 * key],[0,1,0,0],[0,0,1,0]], "scale": [1,1,1]})
    }

    fn assembly_wire(cube: &Model, rows: Vec<Value>) -> Value {
        let mut wire = serde_json::to_value(&cube.ir).unwrap();
        wire["geometry"]["bodies"] = json!([]);
        wire["model"]["product_definitions"] = json!([]);
        wire["model"]["occurrences"] = json!(rows);
        wire
    }

    fn assembly(cube: &Model, rows: Vec<Value>) -> Model {
        Model {
            ir: crate::test_products::read(&assembly_wire(cube, rows)),
            ..cube.clone()
        }
    }

    #[test]
    fn an_assembly_whose_parts_are_all_missing_says_they_were_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let cube = cube();
        let source = assembly(
            &cube,
            vec![
                occurrence(1, "renamed-a.sldprt", json!({"kind":"root"})),
                occurrence(2, r"C:\old\renamed-b.SLDPRT", json!({"kind":"root"})),
            ],
        );
        let decode = |_: Format, _: &Path, _: &mut Report| Ok(cube.clone());
        let mut report = Report::default();
        let settings = PartSearch::project(dir.path());
        let error = resolve(
            &source,
            &dir.path().join("Top.SLDASM"),
            &mut report,
            &decode,
            &Finder::new(&settings),
            &mut Vec::new(),
            &mut SolidWorksReads::new(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("none of the assembly's parts could be found or read"),
            "{error}"
        );
        // Each missing part is still reported, so the user sees which files to look for.
        assert_eq!(
            report
                .losses()
                .iter()
                .filter(|loss| loss.subject.starts_with("occurrence"))
                .count(),
            2
        );
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
        let prototype = wire["model"]["occurrences"][1]["prototype"].clone();
        wire["model"]["product_definitions"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|definition| definition["id"] == prototype)
            .unwrap()["storage"]["object"] = json!("Long");
        source.ir = crate::test_products::read(&wire);
        let mut part = cube.clone();
        part.ir.model.configurations.clear();
        part.ir.model.configurations.push(
            serde_json::from_value(json!({"id": "synthetic:part:configuration#default", "name": "Default", "active": true, "ordinal": 0, "properties": {}, "parameter_overrides": {}, "parameter_values": {}, "feature_states": {}}))
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
            &mut SolidWorksReads::new(),
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
    fn scoped_display_appearances_are_reported_before_their_meshes_are_released() {
        for (key, value, reason) in [
            (
                "path",
                json!([{"occurrence": {"kind": "occurrence", "value": "synthetic:display:occurrence#one"}}]),
                "appearance bindings for one placed instance are not supported yet",
            ),
            (
                "state",
                json!({"of": {"kind": "tool", "value": "synthetic:display:feature#earlier"}}),
                "appearance bindings for an evaluation state are not supported yet",
            ),
        ] {
            let mut source = cube();
            let mut target = json!({"entity": {"kind": "tessellation", "value": "synthetic:display:tessellation#triangle"}});
            target[key] = value;
            source.ir.model.appearance_bindings.push(
                serde_json::from_value(json!({
                    "id": "synthetic:display:binding#scoped", "target": target,
                    "appearance": "synthetic:display:appearance#paint", "channels": {}
                }))
                .unwrap(),
            );
            let mut report = Report::default();
            release_solidworks_display(&mut source, &mut report);
            assert!(source.ir.model.appearance_bindings.is_empty());
            assert_eq!(report.losses().len(), 1);
            assert!(report.losses()[0].detail.ends_with(reason));
        }
    }

    #[test]
    fn display_meshes_are_released_without_changing_exact_geometry_or_history() {
        use cadmpeg_ir::math::Point3;
        use cadmpeg_ir::tessellation::{Tessellation, TessellationMesh};
        let mut source = cube();
        source.ir.model.tessellations.push(
            Tessellation::new(
                "synthetic:display:tessellation#triangle"
                    .try_into()
                    .unwrap(),
                TessellationMesh::List {
                    vertices: vec![
                        Point3::new(0.0, 0.0, 0.0),
                        Point3::new(1.0, 0.0, 0.0),
                        Point3::new(0.0, 1.0, 0.0),
                    ],
                    triangles: vec![[0, 1, 2]],
                },
                Vec::new(),
            )
            .unwrap()
            .with_source_object(Some(serde_json::from_value(json!({
                "format":"sldprt", "object_id":"synthetic-object", "name":"Synthetic display name",
                "instance_path":["Synthetic Folder/Synthetic Part"]
            })).unwrap())),
        );
        for (key, target) in [
            (
                "mesh",
                json!({"kind":"tessellation", "value":"synthetic:display:tessellation#triangle"}),
            ),
            (
                "group",
                json!({"kind":"triangle_group", "value":{"tessellation":"synthetic:display:tessellation#triangle", "group":"front"}}),
            ),
            (
                "body",
                json!({"kind":"body", "value":source.ir.geometry.bodies[0].id}),
            ),
        ] {
            source.ir.model.appearance_bindings.push(
                serde_json::from_value(json!({"id": format!("synthetic:display:binding#{key}"), "target": {"entity": target}, "appearance": "synthetic:display:appearance#paint", "channels": {}}))
                .unwrap(),
            );
        }
        let decode = |_: Format, _: &Path, _: &mut Report| Ok(source.clone());
        let mut report = Report::default();
        let result = read(
            Format::SolidWorksPart,
            Path::new("Synthetic.sldprt"),
            &mut report,
            &decode,
            &Finder::new(&PartSearch::default()),
            &mut Vec::new(),
        )
        .unwrap();
        assert!(report.names().contains("Synthetic display name"));
        assert!(report.names().contains("Synthetic Part"));
        assert!(result.ir.model.tessellations.is_empty());
        assert_eq!(result.ir.model.appearance_bindings.len(), 1);
        source.ir.model.tessellations.clear();
        source.ir.model.appearance_bindings.retain(|binding| {
            matches!(
                binding.target.entity,
                cadmpeg_ir::appearance::AppearanceTarget::Body(_)
            )
        });
        assert_eq!(result.ir, source.ir);
        assert_eq!(result.fidelity, source.fidelity);
    }

    // Entirely synthetic CADIR and empty placeholder paths: no external design is
    // copied or used to derive this graph. Sibling assemblies share one part.
    #[test]
    fn sibling_assemblies_read_a_repeated_part_once() {
        let dir = tempfile::tempdir().unwrap();
        let cube = cube();
        let mut reads = BTreeMap::new();
        let rows: Vec<_> = (1..=12)
            .map(|key| {
                let file = format!("Sub{key}.sldasm");
                std::fs::write(dir.path().join(&file), b"").unwrap();
                occurrence(key, &file, json!({"kind":"root"}))
            })
            .collect();
        std::fs::write(dir.path().join("Pin.sldprt"), b"").unwrap();
        std::fs::write(dir.path().join("Bad.sldprt"), b"").unwrap();
        let top = assembly(&cube, rows);
        let sub = assembly(
            &cube,
            (1..=40)
                .map(|key| {
                    occurrence(
                        key,
                        if key == 40 {
                            "Bad.sldprt"
                        } else {
                            "Pin.sldprt"
                        },
                        json!({"kind":"root"}),
                    )
                })
                .collect(),
        );
        let calls = std::cell::RefCell::new(&mut reads);
        let decode = |format: Format, path: &Path, _: &mut Report| {
            *calls.borrow_mut().entry(path.to_path_buf()).or_insert(0) += 1;
            if path.file_name().unwrap() == "Bad.sldprt" {
                return Err(Error::Decode("synthetic unreadable part".to_owned()));
            }
            Ok(if format == Format::SolidWorksPart {
                cube.clone()
            } else if path.file_name().unwrap() == "Top.sldasm" {
                top.clone()
            } else {
                sub.clone()
            })
        };
        let model = read(
            Format::SolidWorksAssembly,
            &dir.path().join("Top.sldasm"),
            &mut Report::default(),
            &decode,
            &Finder::new(&PartSearch::project(dir.path())),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(reads.len(), 15);
        assert!(reads.values().all(|count| *count == 1));
        let assembly = model.assembly.unwrap();
        assert_eq!(assembly.placements.len(), 12);
        for part in &assembly.parts {
            let sub = part.assembly.as_ref().unwrap();
            assert_eq!(sub.parts.len(), 1);
            assert_eq!(sub.placements.len(), 39);
            assert_eq!(sub.left_out.len(), 1);
        }
    }

    #[test]
    fn cached_assemblies_check_cycles_in_each_ancestor_context() {
        let dir = tempfile::tempdir().unwrap();
        for file in ["Left.sldasm", "Right.sldasm", "Pin.sldprt"] {
            std::fs::write(dir.path().join(file), b"").unwrap();
        }
        let cube = cube();
        let top = assembly(
            &cube,
            vec![
                occurrence(1, "Left.sldasm", json!({"kind":"root"})),
                occurrence(2, "Right.sldasm", json!({"kind":"root"})),
            ],
        );
        let left = assembly(
            &cube,
            vec![
                occurrence(1, "Right.sldasm", json!({"kind":"root"})),
                occurrence(2, "Pin.sldprt", json!({"kind":"root"})),
            ],
        );
        let right = assembly(
            &cube,
            vec![
                occurrence(1, "Left.sldasm", json!({"kind":"root"})),
                occurrence(2, "Pin.sldprt", json!({"kind":"root"})),
            ],
        );
        let calls = Cell::new(0);
        let decode = |_: Format, path: &Path, _: &mut Report| {
            calls.set(calls.get() + 1);
            Ok(match path.file_name().unwrap().to_str().unwrap() {
                "Left.sldasm" => left.clone(),
                "Right.sldasm" => right.clone(),
                "Pin.sldprt" => cube.clone(),
                _ => top.clone(),
            })
        };
        let model = read(
            Format::SolidWorksAssembly,
            &dir.path().join("Top.sldasm"),
            &mut Report::default(),
            &decode,
            &Finder::new(&PartSearch::project(dir.path())),
            &mut Vec::new(),
        )
        .unwrap();
        assert_eq!(calls.get(), 4);
        let top = model.assembly.unwrap();
        for part in &top.parts {
            let sub = part.assembly.as_ref().unwrap();
            assert_eq!(sub.placements.len(), 2);
            assert!(sub.left_out.is_empty());
            let inner = sub.parts[0].assembly.as_ref().unwrap();
            assert_eq!(inner.placements.len(), 1);
            assert_eq!(inner.left_out.len(), 1);
            assert!(inner.left_out[0].reason.contains("places itself"));
        }
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
            json!({"kind":"local", "definition":"synthetic:assembly:component#holder"});
        let mut wire = assembly_wire(
            &cube,
            vec![
                local,
                occurrence(
                    2,
                    "Pin.SLDPRT",
                    json!({"kind":"occurrence", "occurrence":"synthetic:assembly:occurrence#1"}),
                ),
                json!({"id":"synthetic:assembly:occurrence#3", "ordinal":3, "parent":{"kind":"root"},
                "prototype":{"kind":"unresolved"}, "transform":IDENTITY, "scale":[1,1,1]}),
            ],
        );
        wire["model"]["product_definitions"] = json!([{"id": "synthetic:assembly:component#holder", "kind": "part", "source_name": "Holder", "bodies": [], "bom_properties": {}, "sketches": []}]);
        let source = Model {
            ir: crate::test_products::read(&wire),
            ..cube.clone()
        };
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
            &mut SolidWorksReads::new(),
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
