//! Resolve CADIR external FreeCAD links through the shared project file finder.
use super::*;

pub(super) fn resolve(
    source: &Model,
    input: &Path,
    report: &mut Report,
    decode: &Decode,
    finder: &Finder,
    open: &mut Vec<PathBuf>,
) -> Result<Option<Assembly>, Error> {
    let mut model = source.clone();
    let document = serde_json::to_value(&model.ir).map_err(|err| Error::Decode(err.to_string()))?;
    let objects = document
        .pointer("/native/fcstd/objects")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let nodes = document
        .pointer("/native/fcstd/product_nodes")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    // Array elements are already represented by the array's occurrences. Their document
    // objects are bookkeeping, not another set of placed parts.
    let elements: BTreeSet<&str> = nodes
        .iter()
        .filter(|node| node.get("kind").and_then(Value::as_str) == Some("occurrence"))
        .filter_map(|node| node.get("element_objects").and_then(Value::as_array))
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let joints = document
        .pointer("/native/fcstd/joints")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let bookkeeping: BTreeSet<&str> = objects
        .iter()
        .filter(|object| {
            let id = object.get("id").and_then(Value::as_str);
            let kind = object.get("type_name").and_then(Value::as_str);
            kind == Some("Assembly::JointGroup")
                || (kind == Some("App::LinkElement") && id.is_some_and(|id| elements.contains(id)))
                || joints
                    .iter()
                    .any(|joint| joint.get("object").and_then(Value::as_str) == id)
        })
        .filter_map(|object| object.get("id").and_then(Value::as_str))
        .collect();
    model.ir.model.occurrences.retain(|row| {
        !row.native_ref
            .as_deref()
            .is_some_and(|id| bookkeeping.contains(id))
    });
    if model.ir.model.occurrences.len() > MAX_PLACEMENTS {
        return Err(Error::Assembly(format!(
            "the assembly places over {MAX_PLACEMENTS} components"
        )));
    }
    let folder = input.parent().unwrap_or(Path::new("."));
    let wire =
        serde_json::to_value(&model.ir.model).map_err(|err| Error::Decode(err.to_string()))?;
    let rows = wire
        .get("occurrences")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let mut external = ExternalParts::new();
    let mut selected_parts = BTreeMap::new();
    let mut files: BTreeMap<PathBuf, Result<Model, (Stage, String)>> = BTreeMap::new();
    for row in rows.iter().filter(|row| scope(row) == Some("external")) {
        let id = row.get("id").and_then(Value::as_str).unwrap_or("");
        let stored = row
            .pointer("/prototype/document/path")
            .and_then(Value::as_str);
        let file = stored.map(file_name);
        let name = row.get("name").and_then(Value::as_str).map(str::to_owned);
        let result = (|| {
            if row
                .pointer("/link/members")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .any(|member| {
                    member.get("kind").and_then(Value::as_str) == Some("linked_subelement")
                })
            {
                return Err((
                    Stage::Map,
                    "external links to subelements are not supported yet".to_owned(),
                ));
            }
            let stored = stored.ok_or((
                Stage::Input,
                "the external link names no file path; document identifiers are not supported yet"
                    .to_owned(),
            ))?;
            if Format::from_path(Path::new(stored)) != Some(Format::FreeCadDocument) {
                return Err((Stage::Input, format!("{stored}: not a FreeCAD document")));
            }
            // Prefer a sibling even when the saved link came from another machine.
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
            let object = row
                .pointer("/prototype/object")
                .and_then(Value::as_str)
                .ok_or((
                    Stage::Map,
                    "the external link names no target object".to_owned(),
                ))?;
            let key = (
                found.path.clone(),
                object.to_owned(),
                row.get("linked_prototype").is_some(),
            );
            if let Some(known) = selected_parts.get(&key) {
                return Ok(Clone::clone(known));
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
                        let loaded = read(
                            Format::FreeCadDocument,
                            &found.path,
                            &mut part_report,
                            decode,
                            finder,
                            open,
                        )
                        .map_err(|err| (err.stage(), err.to_string()));
                        report.append_prefixed(part_report, file.unwrap_or(stored));
                        loaded
                    };
                    entry.insert(loaded)
                }
            }
            .as_ref()
            .map_err(Clone::clone)?;
            let (mut selected, placement) =
                select(loaded, object).map_err(|why| (Stage::Map, why))?;
            if row.get("linked_prototype").is_some() && placement != IDENTITY {
                let mut holder = only(
                    &selected,
                    &serde_json::to_value(&selected.ir)
                        .map_err(|err| (Stage::Map, err.to_string()))?,
                    &[],
                    HistoryOwner::Unknown,
                    selected.name.clone(),
                )
                .map_err(|why| (Stage::Map, why))?;
                holder.assembly = Some(Assembly {
                    own_part: None,
                    parts: vec![selected],
                    placements: vec![Placement {
                        hidden: false,
                        name: None,
                        part: 0,
                        transform: placement,
                    }],
                    joints: Vec::new(),
                    left_out: Vec::new(),
                    held: Vec::new(),
                    joint_notes: Vec::new(),
                    stored_placements: Vec::new(),
                });
                selected = holder;
            }
            let selected = (selected_parts.len(), std::rc::Rc::new(selected));
            selected_parts.insert(key, selected.clone());
            Ok(selected)
        })();
        let result = result.map_err(|(stage, reason)| {
            report.push_at(
                stage,
                Severity::Dropped,
                format!("occurrence \"{}\"", name.as_deref().unwrap_or("unnamed")),
                reason.clone(),
            );
            LeftOut {
                name,
                file: file.map(str::to_owned),
                reason,
                stage,
            }
        });
        external.insert(id.to_owned(), result);
    }
    let mut assembly =
        match local_components_with_threads(&model, &external, finder.threads, report) {
            Ok(assembly) => assembly.map(|(assembly, _)| assembly),
            // Preserve the geometry fallback for documents whose local product ownership
            // cannot be mapped. External-only documents have no such fallback.
            Err(_) if external.is_empty() => return Ok(None),
            Err(reason) => return Err(Error::Assembly(reason)),
        };
    if let Some(assembly) = &mut assembly {
        if joints.is_empty()
            && model.ir.model.assembly_joints.is_empty()
            && objects.iter().any(|object| {
                object.get("type_name").and_then(Value::as_str) == Some("Assembly::JointGroup")
                    && object
                        .get("dependencies")
                        .and_then(Value::as_array)
                        .is_some_and(|rows| !rows.is_empty())
            })
        {
            assembly.joint_notes.push(JointNote {
                text: "FreeCAD Assembly joints not carried: neutral joints are not supported yet for these records; stored part placements are retained".to_owned(),
                family: None,
            });
        }
        for joint in joints {
            let id = joint.get("id").and_then(Value::as_str);
            if !model
                .ir
                .model
                .assembly_joints
                .iter()
                .any(|row| row.native_ref.as_deref() == id)
            {
                assembly.joint_notes.push(JointNote {
                    text: "FreeCAD Assembly joint not carried: its neutral operands and frames are not supported yet; stored part placements are retained".to_owned(),
                    family: joint.get("kind").and_then(Value::as_str).map(str::to_owned),
                });
            }
        }
    }
    Ok(assembly)
}

/// Select the linked definition, never the other objects in the same document. Definition
/// geometry is already in its own frame; LinkTransform decides whether to carry its pose.
fn select(model: &Model, object: &str) -> Result<(Model, Placement3x4), String> {
    let document = serde_json::to_value(&model.ir).map_err(|err| err.to_string())?;
    let definitions = document
        .pointer("/model/product_definitions")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let definition = definitions.iter().find(|row| {
        row.get("source_name").and_then(Value::as_str) == Some(object)
            || row.get("native_ref").and_then(Value::as_str) == Some(object)
    });
    let Some(definition) = definition else {
        return select_shape(model, &document, object);
    };
    let id = definition.get("id").and_then(Value::as_str);
    let occurrences: Vec<&Value> = document
        .pointer("/model/occurrences")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|row| row.pointer("/prototype/definition").and_then(Value::as_str) == id)
        .collect();
    // A local link can place the same definition elsewhere. The target object's own
    // occurrence supplies its pose, rather than the first link that happens to use it.
    let native = definition.get("native_ref").and_then(Value::as_str);
    let occurrence = occurrences
        .iter()
        .copied()
        .find(|row| native.is_some() && row.get("native_ref").and_then(Value::as_str) == native)
        .or_else(|| occurrences.first().copied());
    let placement = occurrence.map(transform).transpose()?.unwrap_or(IDENTITY);
    let bodies: Vec<&str> = definition
        .get("bodies")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if !bodies.is_empty() {
        return Ok((
            only(
                model,
                &document,
                &bodies,
                id.map_or(HistoryOwner::Unknown, HistoryOwner::Definition),
                name_of(definition).unwrap_or(object).to_owned(),
            )?,
            placement,
        ));
    }
    if let Some(name) = occurrence
        .and_then(|row| row.get("name"))
        .and_then(Value::as_str)
    {
        if let Some(part) = find_component(model, name, 0) {
            return Ok((part.clone(), placement));
        }
    }
    Err(format!(
        "linked object \"{object}\" has no readable geometry or components"
    ))
}

fn find_component<'a>(model: &'a Model, name: &str, depth: usize) -> Option<&'a Model> {
    if depth >= MAX_DEPTH {
        return None;
    }
    let assembly = model.assembly.as_ref()?;
    for placement in &assembly.placements {
        let part = assembly.parts.get(placement.part)?;
        if placement.name.as_deref() == Some(name) {
            return Some(part);
        }
        if let Some(found) = find_component(part, name, depth + 1) {
            return Some(found);
        }
    }
    None
}

/// Standalone PartDesign Bodies need not have product definitions. Follow neutral geometry
/// provenance to their saved object, rather than taking every body in the part document.
fn select_shape(
    model: &Model,
    document: &Value,
    object: &str,
) -> Result<(Model, Placement3x4), String> {
    let owner = document
        .pointer("/native/fcstd/objects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|row| {
            row.get("name").and_then(Value::as_str) == Some(object)
                || row.get("id").and_then(Value::as_str) == Some(object)
        })
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .ok_or_else(|| {
            format!("linked object \"{object}\" was not found in the referenced document")
        })?;
    let wire = document
        .get("model")
        .and_then(Value::as_object)
        .ok_or("the model is malformed")?;
    let rows: BTreeMap<&str, &Value> = wire
        .iter()
        .filter(|(arena, _)| !NOT_GEOMETRY.contains(&arena.as_str()) || arena.as_str() == "bodies")
        .flat_map(|(_, rows)| rows.as_array().into_iter().flatten())
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let mut bodies = Vec::new();
    for body in &model.ir.model.bodies {
        let mut seen = BTreeSet::new();
        let mut pending = vec![body.id.as_str()];
        let mut owners = BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(row) = rows.get(id) else {
                continue;
            };
            if let Some(owner) = row
                .pointer("/source_object/object_id")
                .and_then(Value::as_str)
            {
                owners.insert(owner);
            }
            let mut references = Vec::new();
            strings(row, &mut references);
            pending.extend(references.into_iter().filter(|id| rows.contains_key(id)));
        }
        if owners.len() == 1 && owners.contains(owner) {
            bodies.push(body.id.as_str());
        }
    }
    if bodies.is_empty() {
        return Err(format!(
            "linked object \"{object}\" has no readable geometry or components"
        ));
    }
    let mut selected = only(
        model,
        document,
        &bodies,
        HistoryOwner::Unknown,
        object.to_owned(),
    )?;
    if selected
        .ir
        .model
        .bodies
        .iter()
        .filter_map(|body| body.transform)
        .any(|transform| !transform.is_proper_rigid())
    {
        return Err("scaled linked object placements are not supported yet".to_owned());
    }
    let placement = selected
        .ir
        .model
        .bodies
        .first()
        .and_then(|body| body.transform)
        .map(|transform| transform.affine_rows())
        .unwrap_or(IDENTITY);
    for body in &mut selected.ir.model.bodies {
        let at = body
            .transform
            .map(|transform| transform.affine_rows())
            .unwrap_or(IDENTITY);
        let local = compose(&inverse(&placement), &at);
        body.transform = Some(
            cadmpeg_ir::transform::Transform::affine(local)
                .ok_or("the linked object placement is not finite")?,
        );
    }
    Ok((selected, placement))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn selecting_a_part_keeps_its_children_and_excludes_other_document_objects() {
        let mut model = crate::read(
            Format::InventorPart,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        let body = model.ir.model.bodies[0].id.as_str();
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["product_definitions"] = json!([
            {"id": "test:assembly:product_definition#part", "kind": "part", "source_name": "TargetPart"},
            {"id": "test:assembly:product_definition#body", "kind": "object", "source_name": "Body", "bodies": [body]},
        ]);
        let occurrence = |id: &str, definition: &str, parent: Value, x: f64| {
            json!({
                "id": id, "prototype": {"scope": "local", "definition": definition},
                "parent": parent, "name": id, "ordinal": 0,
                "transform": [[1.0,0.0,0.0,x],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0]],
                "scale": [1.0,1.0,1.0]
            })
        };
        wire["model"]["occurrences"] = json!([
            occurrence(
                "test:assembly:occurrence#part",
                "test:assembly:product_definition#part",
                json!({"kind":"root"}),
                10.0
            ),
            occurrence(
                "test:assembly:occurrence#inside",
                "test:assembly:product_definition#body",
                json!({"kind":"occurrence", "occurrence":"test:assembly:occurrence#part"}),
                2.0
            ),
            occurrence(
                "test:assembly:occurrence#other",
                "test:assembly:product_definition#body",
                json!({"kind":"root"}),
                100.0
            ),
        ]);
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        model.assembly = local_assembly(&model).unwrap();
        let (selected, placement) = select(&model, "TargetPart").unwrap();
        assert_eq!(placement[0][3], 10.0);
        let selected = selected.assembly.unwrap();
        assert_eq!(selected.parts.len(), 1);
        assert_eq!(selected.placements.len(), 1);
        assert_eq!(selected.placements[0].transform[0][3], 2.0);
        assert_eq!(selected.parts[0].ir.model.bodies.len(), 1);
    }
}
