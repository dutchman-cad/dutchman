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
    claim_linked_bodies(&mut model, report);
    if model.ir.model.occurrences.len() > MAX_PLACEMENTS {
        return Err(Error::Assembly(format!(
            "the assembly places over {MAX_PLACEMENTS} components"
        )));
    }
    let folder = input_folder(input);
    let wire = products::occurrences(&model.ir.model).map_err(Error::Assembly)?;
    let rows = wire.as_array().map_or(&[][..], Vec::as_slice);
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
                        unproven: false,
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
                    identities: Vec::new(),
                    definitions: Vec::new(),
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
    let mut assembly = match local_components_with_threads(
        &model,
        &external,
        finder.threads,
        finder.part_positions,
        report,
    ) {
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
    let mut document = serde_json::to_value(&model.ir).map_err(|err| err.to_string())?;
    if let Some(model) = document.get_mut("model") {
        products::project(model)?;
    }
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
    let geometry = document
        .get("geometry")
        .and_then(Value::as_object)
        .ok_or("the geometry is malformed")?;
    let wire = document
        .get("model")
        .and_then(Value::as_object)
        .ok_or("the model is malformed")?;
    let rows: BTreeMap<&str, &Value> = geometry
        .iter()
        .chain(wire)
        .filter(|(arena, _)| !NOT_GEOMETRY.contains(&arena.as_str()) || arena.as_str() == "bodies")
        .flat_map(|(_, rows)| rows.as_array().into_iter().flatten())
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (arena, parent) in [("regions", "body"), ("faces", "shell")] {
        for row in geometry
            .get(arena)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let (Some(parent), Some(id)) = (
                row.get(parent).and_then(Value::as_str),
                row.get("id").and_then(Value::as_str),
            ) {
                children.entry(parent).or_default().push(id);
            }
        }
    }
    let mut bodies = Vec::new();
    for body in &model.ir.geometry.bodies {
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
            pending.extend(children.get(id).into_iter().flatten().copied());
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
        .geometry
        .bodies
        .iter()
        .filter_map(|body| body.transform)
        .any(|transform| !transform.is_proper_rigid())
    {
        return Err("scaled linked object placements are not supported yet".to_owned());
    }
    let placement = selected
        .ir
        .geometry
        .bodies
        .first()
        .and_then(|body| body.transform)
        .map(|transform| transform.affine_rows())
        .unwrap_or(IDENTITY);
    for body in &mut selected.ir.geometry.bodies {
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
    fn a_standalone_shape_follows_v11_parents_to_point_provenance() {
        let mut model = crate::read(
            Format::InventorPart,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        for point in &mut model.ir.geometry.points {
            point.source_object = Some(
                serde_json::from_value(json!({
                    "format": "fcstd", "object_id": "test:source:object#body", "instance_path": []
                }))
                .unwrap(),
            );
        }
        model.ir.model.product_definitions.clear();
        model.ir.model.occurrences.clear();
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["native"]["fcstd"] = json!({"objects": [{
            "id": "test:source:object#body", "name": "Standalone"
        }]});
        model.ir = crate::test_products::read(&wire);
        let (selected, _) = select(&model, "Standalone").unwrap();
        assert_eq!(selected.ir.geometry.bodies.len(), 1);
        assert_eq!(
            selected.ir.geometry.faces.len(),
            model.ir.geometry.faces.len()
        );
        assert_eq!(
            selected.ir.geometry.points.len(),
            model.ir.geometry.points.len()
        );
    }

    #[test]
    fn selecting_a_part_keeps_its_children_and_excludes_other_document_objects() {
        let mut model = crate::read(
            Format::InventorPart,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        let body = model.ir.geometry.bodies[0].id.as_str();
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["product_definitions"] = json!([
            {"id": "test:assembly:product_definition#part", "kind": "part", "source_name": "TargetPart", "bom_properties": {}, "bodies": [], "sketches": []},
            {"id": "test:assembly:product_definition#body", "kind": "object", "source_name": "Body", "bodies": [body], "bom_properties": {}, "sketches": []},
        ]);
        let occurrence = |id: &str, definition: &str, parent: Value, x: f64| {
            json!({
                "id": id, "prototype": {"kind": "local", "definition": definition},
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
        model.ir = crate::test_products::read(&wire);
        model.assembly = local_assembly(&model).unwrap();
        let (selected, placement) = select(&model, "TargetPart").unwrap();
        assert_eq!(placement[0][3], 10.0);
        let selected = selected.assembly.unwrap();
        assert_eq!(selected.parts.len(), 1);
        assert_eq!(selected.placements.len(), 1);
        assert_eq!(selected.placements[0].transform[0][3], 2.0);
        assert_eq!(selected.parts[0].ir.geometry.bodies.len(), 1);
    }
}

/// What [`claim`] changed.
#[derive(Debug, Default, PartialEq, Eq)]
struct Claimed {
    /// Original Bodies no longer placed on their own: their links in a FreeCAD Assembly place them.
    dropped: usize,
    /// Features given to the Body they build.
    owned: usize,
    /// What could not be attributed, for the report.
    warnings: Vec<String>,
}

/// The Bodies a FreeCAD Assembly links place through those links: each Body keeps its own
/// PartDesign history and is placed once, at its link's placement, instead of once more as
/// the document's original Body. cadmpeg does not say which Body a feature belongs to, so a
/// feature is given to the Body it depends on, and a sketch to the Body whose features use it.
fn claim_linked_bodies(model: &mut Model, report: &mut Report) {
    let Ok(mut document) = serde_json::to_value(&model.ir) else {
        report.push(
            Severity::Degraded,
            "assembly",
            "the FreeCAD document could not be read back; the history of the Bodies its \
             Assembly links is not mapped"
                .to_owned(),
        );
        return;
    };
    let claimed = claim(&mut document);
    for warning in &claimed.warnings {
        report.push(Severity::Degraded, "assembly", warning.clone());
    }
    if claimed.dropped == 0 && claimed.owned == 0 {
        return;
    }
    let text = document.to_string();
    // cadmpeg is third-party code; a panic in it is a reason, not a crash.
    let Ok(Ok(ir)) = std::panic::catch_unwind(|| cadmpeg_ir::CadIr::from_json(&text)) else {
        report.push(
            Severity::Degraded,
            "assembly",
            "the FreeCAD document did not survive attributing features to the Bodies its \
             Assembly links; their history is not mapped and the original Bodies stay placed"
                .to_owned(),
        );
        return;
    };
    model.ir = ir;
    if claimed.dropped > 0 {
        report.push(
            Severity::Info,
            "assembly",
            format!(
                "{} Body(ies) the FreeCAD Assembly links are placed once, through their links, \
                 not again as the document's original Bodies",
                claimed.dropped
            ),
        );
    }
}

/// The object a feature row stands for, by its name.
fn feature_name(feature: &Value) -> String {
    feature
        .get("name")
        .and_then(Value::as_str)
        .or_else(|| {
            feature
                .get("id")
                .and_then(Value::as_str)
                .map(|id| id.rsplit('#').next().unwrap_or(id))
        })
        .unwrap_or("unnamed")
        .to_owned()
}

fn claim(document: &mut Value) -> Claimed {
    let mut claimed = Claimed::default();
    let types: BTreeMap<String, String> = listed(document.pointer("/native/fcstd/objects"))
        .iter()
        .filter_map(|object| {
            Some((
                object.get("id")?.as_str()?.to_owned(),
                object.get("type_name")?.as_str()?.to_owned(),
            ))
        })
        .collect();
    let rows = listed(document.pointer("/model/occurrences"));
    let by_id: BTreeMap<&str, &Value> = rows
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let definitions = listed(document.pointer("/model/product_definitions"));
    let object_of: BTreeMap<&str, &str> = definitions
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row.get("native_ref")?.as_str()?)))
        .collect();
    let definition_of: BTreeMap<&str, &str> = object_of
        .iter()
        .map(|(definition, object)| (*object, *definition))
        .collect();
    let is_body = |object: &str| {
        types
            .get(object)
            .is_some_and(|kind| kind == "PartDesign::Body")
    };
    // The Body a link ends at: a link to a link is followed to the object it finally shows.
    let target = |start: &str| -> Option<String> {
        let mut definition = start.to_owned();
        for _ in 0..MAX_DEPTH {
            let object = *object_of.get(definition.as_str())?;
            if is_body(object) {
                return Some(object.to_owned());
            }
            if types.get(object).map(String::as_str) != Some("App::Link") {
                return None;
            }
            definition = rows
                .iter()
                .find(|row| {
                    scope(row) == Some("local")
                        && row.get("link").is_some()
                        && row.get("native_ref").and_then(Value::as_str) == Some(object)
                })?
                .pointer("/prototype/definition")?
                .as_str()?
                .to_owned();
        }
        None
    };
    // Bodies an unsuppressed link inside an Assembly places, by object. A hidden link still
    // places its Body (hidden), so the original is not placed again; a suppressed one places
    // nothing.
    let mut placed: BTreeSet<String> = BTreeSet::new();
    for row in rows {
        if scope(row) != Some("local")
            || row.get("link").is_none()
            || row.get("suppressed").and_then(Value::as_bool) == Some(true)
        {
            continue;
        }
        let in_assembly = row
            .pointer("/parent/occurrence")
            .and_then(Value::as_str)
            .and_then(|parent| by_id.get(parent))
            .and_then(|parent| parent.get("native_ref")?.as_str())
            .and_then(|object| types.get(object))
            .is_some_and(|kind| kind == "Assembly::AssemblyObject");
        if let (true, Some(body)) = (
            in_assembly,
            row.pointer("/prototype/definition")
                .and_then(Value::as_str)
                .and_then(target),
        ) {
            placed.insert(body);
        }
    }
    if placed.is_empty() {
        return claimed;
    }
    // Which Body each feature belongs to, over every Body of the document, so a feature
    // reaching into another Body is seen even when that Body is not linked.
    let features = listed(document.pointer("/model/features"));
    let tag = |feature: &Value| -> String {
        feature
            .get("source_tag")
            .and_then(Value::as_str)
            .or_else(|| {
                feature
                    .get("native_ref")
                    .and_then(Value::as_str)
                    .and_then(|object| types.get(object).map(String::as_str))
            })
            .unwrap_or("")
            .to_owned()
    };
    let id_of = |feature: &Value| feature.get("id").and_then(Value::as_str).map(str::to_owned);
    let depends = |feature: &Value| -> Vec<String> {
        listed(feature.get("dependencies"))
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    // Body feature id -> its Body object.
    let mut body_of: BTreeMap<String, String> = BTreeMap::new();
    // Features by kind: PartDesign features take part in the dependency walk, sketches only
    // by who uses them.
    let mut modelled: Vec<&Value> = Vec::new();
    let mut sketches: Vec<&Value> = Vec::new();
    for feature in features {
        let Some(id) = id_of(feature) else { continue };
        let kind = tag(feature);
        if kind == "PartDesign::Body" {
            if let Some(object) = feature.get("native_ref").and_then(Value::as_str) {
                body_of.insert(id, object.to_owned());
            }
        } else if kind.starts_with("PartDesign::") {
            modelled.push(feature);
        } else if kind.starts_with("Sketcher::") {
            sketches.push(feature);
        }
    }
    let mut set: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // A Body's own row belongs to that Body.
    for (id, body) in &body_of {
        set.entry(id.clone()).or_default().insert(body.clone());
    }
    // What the Body rows state themselves (a Body listing its features), then what the
    // features state (a feature listing its Body).
    for feature in features {
        let Some(body) = id_of(feature).and_then(|id| body_of.get(&id)) else {
            continue;
        };
        for member in depends(feature) {
            set.entry(member).or_default().insert(body.clone());
        }
    }
    for feature in modelled.iter().chain(&sketches) {
        let Some(id) = id_of(feature) else { continue };
        for dependency in depends(feature) {
            if let Some(body) = body_of.get(&dependency) {
                set.entry(id.clone()).or_default().insert(body.clone());
            }
        }
    }
    // PartDesign features follow the features they build on; a sketch's own attachments say
    // nothing about the Body that uses it.
    loop {
        let mut changed = false;
        for feature in &modelled {
            let Some(id) = id_of(feature) else { continue };
            let mut bodies = set.get(&id).cloned().unwrap_or_default();
            for dependency in depends(feature) {
                if modelled
                    .iter()
                    .any(|row| id_of(row).as_ref() == Some(&dependency))
                {
                    bodies.extend(set.get(&dependency).into_iter().flatten().cloned());
                }
            }
            if bodies.len() > set.get(&id).map_or(0, BTreeSet::len) {
                set.insert(id, bodies);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    // A sketch belongs to the Body whose features use it.
    for feature in &modelled {
        let Some(id) = id_of(feature) else { continue };
        let bodies = set.get(&id).cloned().unwrap_or_default();
        for dependency in depends(feature) {
            if sketches
                .iter()
                .any(|row| id_of(row).as_ref() == Some(&dependency))
            {
                set.entry(dependency)
                    .or_default()
                    .extend(bodies.iter().cloned());
            }
        }
    }
    let ambiguous: BTreeSet<String> = modelled
        .iter()
        .chain(&sketches)
        .filter_map(|feature| id_of(feature))
        .filter(|id| set.get(id).is_some_and(|bodies| bodies.len() > 1))
        .collect();
    let name_of_feature: BTreeMap<String, String> = features
        .iter()
        .filter_map(|feature| Some((id_of(feature)?, feature_name(feature))))
        .collect();
    let named = |ids: &mut dyn Iterator<Item = &String>| -> String {
        ids.map(|id| format!("\"{}\"", name_of_feature.get(id).unwrap_or(id)))
            .collect::<Vec<_>>()
            .join(", ")
    };
    // A Body whose history cannot be attributed with certainty keeps today's behaviour:
    // its original Body stays placed and its features stay unowned.
    let mut clean: BTreeMap<String, String> = BTreeMap::new();
    for object in &placed {
        let Some(definition) = definition_of.get(object.as_str()) else {
            continue;
        };
        let doubtful: Vec<&String> = ambiguous
            .iter()
            .filter(|id| set.get(*id).is_some_and(|bodies| bodies.contains(object)))
            .collect();
        if doubtful.is_empty() {
            clean.insert(object.clone(), (*definition).to_owned());
        } else {
            let name = features
                .iter()
                .find(|feature| feature.get("native_ref").and_then(Value::as_str) == Some(object))
                .map_or_else(|| object.clone(), feature_name);
            claimed.warnings.push(format!(
                "Body \"{name}\" is placed through its Assembly link as stored: features {} \
                 belong to more than one Body, so its history is not mapped",
                named(&mut doubtful.into_iter())
            ));
        }
    }
    let unused: Vec<&String> = modelled
        .iter()
        .chain(&sketches)
        .filter_map(|feature| id_of(feature))
        .filter(|id| set.get(id).is_none_or(BTreeSet::is_empty))
        .filter_map(|id| name_of_feature.get_key_value(&id).map(|(key, _)| key))
        .collect();
    if !unused.is_empty() && !clean.is_empty() {
        claimed.warnings.push(format!(
            "features {} are used by no Body, so no part carries them",
            named(&mut unused.into_iter())
        ));
    }
    if clean.is_empty() {
        return claimed;
    }
    if let Some(rows) = document
        .pointer_mut("/model/features")
        .and_then(Value::as_array_mut)
    {
        for feature in rows {
            let Some(definition) = feature
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| set.get(id))
                .filter(|bodies| bodies.len() == 1)
                .and_then(|bodies| bodies.iter().next())
                .and_then(|body| clean.get(body))
                .cloned()
            else {
                continue;
            };
            if feature.get("product_definition").is_none() {
                feature["product_definition"] = Value::String(definition);
                claimed.owned += 1;
            }
        }
    }
    // The document's own Body, once a link places it: what the original Body would add is a
    // second copy of the same part. A Body a Part container places stays where it is.
    let parents: BTreeSet<String> = listed(document.pointer("/model/occurrences"))
        .iter()
        .filter_map(|row| {
            row.pointer("/parent/occurrence")?
                .as_str()
                .map(str::to_owned)
        })
        .collect();
    if let Some(rows) = document
        .pointer_mut("/model/occurrences")
        .and_then(Value::as_array_mut)
    {
        let before = rows.len();
        rows.retain(|row| {
            let original = scope(row) == Some("local")
                && row.get("link").is_none()
                && row.pointer("/parent/kind").and_then(Value::as_str) == Some("root")
                && row
                    .get("native_ref")
                    .and_then(Value::as_str)
                    .is_some_and(|object| clean.contains_key(object))
                && row
                    .pointer("/prototype/definition")
                    .and_then(Value::as_str)
                    .is_some_and(|definition| clean.values().any(|body| body == definition))
                && !row
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| parents.contains(id));
            !original
        });
        claimed.dropped = before - rows.len();
    }
    claimed
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod linked_bodies {
    use serde_json::json;

    use super::*;

    /// Two PartDesign Bodies, each with a sketch and a pad, a document-level sketch no Body
    /// owns, and an Assembly that links each Body.
    fn document(container: &str) -> Value {
        let occurrence = |id: &str, definition: &str, native: &str, parent: Value, link: bool| {
            let mut row = json!({
                "id": id, "native_ref": native, "ordinal": 0, "scale": [1, 1, 1],
                "prototype": {"kind": "local", "definition": definition}, "parent": parent});
            if link {
                row["link"] = json!({"members": []});
            }
            row
        };
        let feature = |id: &str, tag: &str, dependencies: &[&str]| {
            json!({"id": id, "native_ref": format!("o#{id}"), "source_tag": tag,
                   "dependencies": dependencies})
        };
        json!({"native": {"fcstd": {"objects": [
            {"id": "o#Assembly", "type_name": container},
            {"id": "o#Body", "type_name": "PartDesign::Body"},
            {"id": "o#Body001", "type_name": "PartDesign::Body"}]}},
        "model": {
            "product_definitions": [
                {"id": "d#Body", "native_ref": "o#Body"},
                {"id": "d#Body001", "native_ref": "o#Body001"}],
            "occurrences": [
                occurrence("c#Assembly", "d#Assembly", "o#Assembly", json!({"kind": "root"}), false),
                occurrence("c#Body", "d#Body", "o#Body", json!({"kind": "root"}), false),
                occurrence("c#Body001", "d#Body001", "o#Body001", json!({"kind": "root"}), false),
                occurrence("i#block", "d#Body", "o#block",
                    json!({"kind": "occurrence", "occurrence": "c#Assembly"}), true),
                occurrence("i#ring", "d#Body001", "o#ring",
                    json!({"kind": "occurrence", "occurrence": "c#Assembly"}), true)],
            "features": [
                feature("Body", "PartDesign::Body", &[]),
                feature("Sketch1", "Sketcher::SketchObject", &[]),
                feature("Pad2", "PartDesign::Pad", &["Sketch1", "Body"]),
                feature("Pad3", "PartDesign::Pad", &["Pad2", "Body"]),
                feature("Body001", "PartDesign::Body", &[]),
                feature("Sketch001", "Sketcher::SketchObject", &[]),
                feature("Revolution2", "PartDesign::Revolution", &["Sketch001", "Body001"]),
                feature("Shared", "Sketcher::SketchObject", &[]),
                feature("Pad4", "PartDesign::Pad", &["Shared", "Pad3"]),
                feature("Revolution3", "PartDesign::Revolution", &["Shared", "Revolution2"])]}})
    }

    fn owner<'a>(document: &'a Value, feature: &str) -> Option<&'a str> {
        document["model"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["id"] == feature)
            .unwrap()
            .get("product_definition")
            .and_then(Value::as_str)
    }

    #[test]
    fn a_linked_body_owns_its_features_and_sketches() {
        let mut document = plain("Assembly::AssemblyObject");
        let claimed = claim(&mut document);
        assert_eq!(claimed.dropped, 2);
        for feature in ["Body", "Sketch1", "Pad2", "Pad3", "Pad4"] {
            assert_eq!(owner(&document, feature), Some("d#Body"), "{feature}");
        }
        for feature in ["Body001", "Sketch001", "Revolution2", "Revolution3"] {
            assert_eq!(owner(&document, feature), Some("d#Body001"), "{feature}");
        }
        assert_eq!(claimed.owned, 9);
    }

    #[test]
    fn each_linked_body_is_placed_once_through_its_link() {
        let mut document = plain("Assembly::AssemblyObject");
        claim(&mut document);
        let ids: Vec<&str> = document["model"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["id"].as_str())
            .collect();
        assert_eq!(ids, ["c#Assembly", "i#block", "i#ring"]);
    }

    #[test]
    fn links_outside_an_assembly_leave_the_document_as_stored() {
        let mut document = plain("App::Part");
        let before = document.clone();
        assert_eq!(claim(&mut document), Claimed::default());
        assert_eq!(document, before);
    }

    #[test]
    fn a_body_no_link_places_stays_placed() {
        let mut document = plain("Assembly::AssemblyObject");
        let rows = document["model"]["occurrences"].as_array_mut().unwrap();
        rows.retain(|row| row["id"] != "i#ring");
        let claimed = claim(&mut document);
        assert_eq!(claimed.dropped, 1);
        assert_eq!(owner(&document, "Revolution2"), None);
        assert!(document["model"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "c#Body001"));
    }

    #[test]
    fn a_body_holding_placed_children_is_kept() {
        let mut document = plain("Assembly::AssemblyObject");
        document["model"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .push(json!({
            "id": "c#Child", "native_ref": "o#Child", "ordinal": 0, "scale": [1, 1, 1],
            "prototype": {"kind": "local", "definition": "d#Child"},
            "parent": {"kind": "occurrence", "occurrence": "c#Body"}}));
        let claimed = claim(&mut document);
        assert_eq!(claimed.dropped, 1);
        assert!(document["model"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| row["id"] == "c#Body"));
    }

    /// The same two Bodies as [`document`] without the sketch both of them use.
    fn plain(container: &str) -> Value {
        let mut document = document(container);
        let features = document["model"]["features"].as_array_mut().unwrap();
        features.retain(|row| row["id"] != "Shared");
        for row in features {
            if row["id"] == "Pad4" {
                row["dependencies"] = json!(["Pad3", "Body"]);
            }
            if row["id"] == "Revolution3" {
                row["dependencies"] = json!(["Revolution2", "Body001"]);
            }
        }
        document
    }

    fn add_feature(document: &mut Value, id: &str, tag: &str, dependencies: &[&str]) {
        document["model"]["features"].as_array_mut().unwrap().push(
            json!({"id": id, "native_ref": format!("o#{id}"), "source_tag": tag,
                   "dependencies": dependencies}),
        );
    }

    fn occurrence_mut<'a>(document: &'a mut Value, id: &str) -> &'a mut Value {
        document["model"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["id"] == id)
            .unwrap()
    }

    fn placed(document: &Value) -> Vec<&str> {
        document["model"]["occurrences"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|row| row["id"].as_str())
            .collect()
    }

    #[test]
    fn a_sketch_two_bodies_use_leaves_both_bodies_as_stored() {
        let mut document = plain("Assembly::AssemblyObject");
        add_feature(&mut document, "Shared", "Sketcher::SketchObject", &[]);
        add_feature(
            &mut document,
            "Pad5",
            "PartDesign::Pad",
            &["Shared", "Pad3", "Body"],
        );
        add_feature(
            &mut document,
            "Revolution4",
            "PartDesign::Revolution",
            &["Shared", "Revolution3", "Body001"],
        );
        let before = document["model"]["occurrences"].clone();
        let claimed = claim(&mut document);
        assert_eq!((claimed.dropped, claimed.owned), (0, 0));
        assert_eq!(document["model"]["occurrences"], before);
        assert_eq!(owner(&document, "Pad2"), None);
        assert_eq!(claimed.warnings.len(), 2, "{:?}", claimed.warnings);
        assert!(claimed
            .warnings
            .iter()
            .all(|text| text.contains("\"Shared\"")));
        assert!(
            claimed.warnings[0].contains("Body \"Body\""),
            "{:?}",
            claimed.warnings
        );
    }

    #[test]
    fn a_boolean_across_two_bodies_is_named_and_keeps_both_bodies() {
        let mut document = plain("Assembly::AssemblyObject");
        add_feature(
            &mut document,
            "Boolean",
            "PartDesign::Boolean",
            &["Pad4", "Revolution3"],
        );
        let claimed = claim(&mut document);
        assert_eq!((claimed.dropped, claimed.owned), (0, 0));
        assert_eq!(claimed.warnings.len(), 2);
        assert!(claimed
            .warnings
            .iter()
            .all(|text| text.contains("\"Boolean\"")));
        assert!(placed(&document).contains(&"c#Body"));
        assert!(placed(&document).contains(&"c#Body001"));
    }

    #[test]
    fn a_sketch_goes_to_the_body_that_uses_it_not_the_one_it_is_attached_to() {
        let mut document = plain("Assembly::AssemblyObject");
        // Attached to a face of the first Body's Pad, used by the second Body's Pad.
        add_feature(
            &mut document,
            "Sketch9",
            "Sketcher::SketchObject",
            &["Pad4"],
        );
        add_feature(
            &mut document,
            "Pad9",
            "PartDesign::Pad",
            &["Sketch9", "Revolution3", "Body001"],
        );
        let claimed = claim(&mut document);
        assert_eq!(claimed.dropped, 2);
        assert!(claimed.warnings.is_empty(), "{:?}", claimed.warnings);
        assert_eq!(owner(&document, "Sketch9"), Some("d#Body001"));
        assert_eq!(owner(&document, "Pad9"), Some("d#Body001"));
        assert_eq!(owner(&document, "Pad4"), Some("d#Body"));
    }

    #[test]
    fn a_sketch_nothing_uses_is_reported_and_the_rest_is_still_claimed() {
        let mut document = plain("Assembly::AssemblyObject");
        add_feature(&mut document, "Orphan", "Sketcher::SketchObject", &[]);
        let claimed = claim(&mut document);
        assert_eq!(claimed.dropped, 2);
        assert_eq!(owner(&document, "Orphan"), None);
        assert_eq!(claimed.warnings.len(), 1);
        assert!(claimed.warnings[0].contains("\"Orphan\""));
    }

    #[test]
    fn a_body_that_lists_its_features_owns_them() {
        let mut document = plain("Assembly::AssemblyObject");
        let features = document["model"]["features"].as_array_mut().unwrap();
        for row in features.iter_mut() {
            match row["id"].as_str().unwrap() {
                "Body" => row["dependencies"] = json!(["Sketch1", "Pad2"]),
                "Pad2" => row["dependencies"] = json!(["Sketch1"]),
                _ => {}
            }
        }
        claim(&mut document);
        assert_eq!(owner(&document, "Pad2"), Some("d#Body"));
        assert_eq!(owner(&document, "Sketch1"), Some("d#Body"));
        // Pad3 still builds on Pad2 and lists its Body.
        assert_eq!(owner(&document, "Pad3"), Some("d#Body"));
    }

    #[test]
    fn a_suppressed_link_places_nothing() {
        let mut document = plain("Assembly::AssemblyObject");
        occurrence_mut(&mut document, "i#ring")["suppressed"] = json!(true);
        let claimed = claim(&mut document);
        assert_eq!(claimed.dropped, 1);
        assert!(placed(&document).contains(&"c#Body001"));
        assert!(!placed(&document).contains(&"c#Body"));
        assert_eq!(owner(&document, "Revolution2"), None);
    }

    #[test]
    fn a_hidden_link_still_places_its_body() {
        let mut document = plain("Assembly::AssemblyObject");
        occurrence_mut(&mut document, "i#ring")["visible"] = json!(false);
        let claimed = claim(&mut document);
        assert_eq!(claimed.dropped, 2);
        assert!(!placed(&document).contains(&"c#Body001"));
        assert_eq!(owner(&document, "Revolution2"), Some("d#Body001"));
    }

    #[test]
    fn a_link_to_a_link_places_the_body_it_finally_shows() {
        let mut document = plain("Assembly::AssemblyObject");
        document["native"]["fcstd"]["objects"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "o#alias", "type_name": "App::Link"}));
        document["model"]["product_definitions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "d#alias", "native_ref": "o#alias"}));
        let ring = occurrence_mut(&mut document, "i#ring");
        ring["prototype"]["definition"] = json!("d#alias");
        let mut alias = ring.clone();
        alias["id"] = json!("c#alias");
        alias["native_ref"] = json!("o#alias");
        alias["prototype"]["definition"] = json!("d#Body001");
        alias["parent"] = json!({"kind": "root"});
        document["model"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .push(alias);
        let claimed = claim(&mut document);
        assert_eq!(claimed.dropped, 2);
        assert_eq!(owner(&document, "Revolution2"), Some("d#Body001"));
        assert!(!placed(&document).contains(&"c#Body001"));
    }

    #[test]
    fn a_body_a_part_container_places_stays_placed() {
        let mut document = plain("Assembly::AssemblyObject");
        document["native"]["fcstd"]["objects"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "o#Part", "type_name": "App::Part"}));
        document["model"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": "c#Part", "native_ref": "o#Part", "ordinal": 0, "scale": [1, 1, 1],
                "prototype": {"kind": "local", "definition": "d#Part"},
                "parent": {"kind": "root"}}));
        occurrence_mut(&mut document, "c#Body")["parent"] =
            json!({"kind": "occurrence", "occurrence": "c#Part"});
        let claimed = claim(&mut document);
        assert_eq!(claimed.dropped, 1);
        assert!(placed(&document).contains(&"c#Body"));
        assert!(!placed(&document).contains(&"c#Body001"));
        assert_eq!(owner(&document, "Pad2"), Some("d#Body"));
    }
}
