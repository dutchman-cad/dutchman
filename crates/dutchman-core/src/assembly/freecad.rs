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
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
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
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
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
}

/// The Bodies a FreeCAD Assembly links place through those links: each Body keeps its own
/// PartDesign history and is placed once, at its link's placement, instead of once more as
/// the document's original Body. cadmpeg does not say which Body a feature belongs to, so a
/// feature is given to the Body it depends on, and a sketch to the Body whose features use it.
fn claim_linked_bodies(model: &mut Model, report: &mut Report) {
    let Ok(mut document) = serde_json::to_value(&model.ir) else {
        return;
    };
    let claimed = claim(&mut document);
    if claimed == Claimed::default() {
        return;
    }
    let text = document.to_string();
    // cadmpeg is third-party code; a panic in it is a reason, not a crash.
    let Ok(Ok(ir)) = std::panic::catch_unwind(|| cadmpeg_ir::CadIr::from_json(&text)) else {
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
    // Definitions a link inside an Assembly places, by definition id.
    let mut linked: BTreeSet<String> = BTreeSet::new();
    for row in rows {
        if scope(row) != Some("local") || row.get("link").is_none() {
            continue;
        }
        let in_assembly = row
            .pointer("/parent/occurrence")
            .and_then(Value::as_str)
            .and_then(|parent| by_id.get(parent))
            .and_then(|parent| parent.get("native_ref")?.as_str())
            .and_then(|object| types.get(object))
            .is_some_and(|kind| kind == "Assembly::AssemblyObject");
        if let (true, Some(definition)) = (
            in_assembly,
            row.pointer("/prototype/definition").and_then(Value::as_str),
        ) {
            linked.insert(definition.to_owned());
        }
    }
    if linked.is_empty() {
        return claimed;
    }
    // The Body object each linked definition stands for.
    let bodies: BTreeMap<String, String> = listed(document.pointer("/model/product_definitions"))
        .iter()
        .filter_map(|row| {
            let id = row.get("id")?.as_str()?;
            let object = row.get("native_ref")?.as_str()?;
            (linked.contains(id)
                && types
                    .get(object)
                    .is_some_and(|kind| kind == "PartDesign::Body"))
            .then(|| (object.to_owned(), id.to_owned()))
        })
        .collect();
    if bodies.is_empty() {
        return claimed;
    }
    // Ownership of features: a Body's own feature, then whatever depends on an owned feature,
    // then the sketches only that Body's features use.
    let features = listed(document.pointer("/model/features"));
    let mut owner: BTreeMap<String, String> = BTreeMap::new();
    for feature in features {
        let (Some(id), Some(object)) = (
            feature.get("id").and_then(Value::as_str),
            feature.get("native_ref").and_then(Value::as_str),
        ) else {
            continue;
        };
        if feature.get("source_tag").and_then(Value::as_str) == Some("PartDesign::Body") {
            if let Some(definition) = bodies.get(object) {
                owner.insert(id.to_owned(), definition.clone());
            }
        }
    }
    let depends = |feature: &Value| -> Vec<String> {
        listed(feature.get("dependencies"))
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect()
    };
    loop {
        let before = owner.len();
        for feature in features {
            let Some(id) = feature.get("id").and_then(Value::as_str) else {
                continue;
            };
            if owner.contains_key(id) {
                continue;
            }
            let owners: BTreeSet<&String> = depends(feature)
                .iter()
                .filter_map(|dependency| owner.get(dependency))
                .collect();
            if let [only] = owners.into_iter().collect::<Vec<_>>().as_slice() {
                owner.insert(id.to_owned(), (*only).clone());
            }
        }
        if owner.len() == before {
            break;
        }
    }
    // A feature the Body's features use and no other Body does: its sketch.
    let mut users: BTreeMap<String, BTreeSet<Option<String>>> = BTreeMap::new();
    for feature in features {
        let id = feature.get("id").and_then(Value::as_str).unwrap_or("");
        for dependency in depends(feature) {
            users
                .entry(dependency)
                .or_default()
                .insert(owner.get(id).cloned());
        }
    }
    let mut sketches: Vec<(String, String)> = Vec::new();
    for feature in features {
        let Some(id) = feature.get("id").and_then(Value::as_str) else {
            continue;
        };
        if owner.contains_key(id) || feature.get("product_definition").is_some() {
            continue;
        }
        if let Some(users) = users.get(id) {
            let owners: Vec<&Option<String>> = users.iter().collect();
            if let [Some(only)] = owners.as_slice() {
                sketches.push((id.to_owned(), only.clone()));
            }
        }
    }
    owner.extend(sketches);
    if let Some(rows) = document
        .pointer_mut("/model/features")
        .and_then(Value::as_array_mut)
    {
        for feature in rows {
            let Some(definition) = feature
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| owner.get(id))
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
    // second copy of the same part.
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
                && row
                    .get("native_ref")
                    .and_then(Value::as_str)
                    .is_some_and(|object| bodies.contains_key(object))
                && row
                    .pointer("/prototype/definition")
                    .and_then(Value::as_str)
                    .is_some_and(|definition| bodies.values().any(|body| body == definition))
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
        let mut document = document("Assembly::AssemblyObject");
        let claimed = claim(&mut document);
        assert_eq!(claimed.dropped, 2);
        for feature in ["Body", "Sketch1", "Pad2", "Pad3", "Pad4"] {
            assert_eq!(owner(&document, feature), Some("d#Body"), "{feature}");
        }
        for feature in ["Body001", "Sketch001", "Revolution2", "Revolution3"] {
            assert_eq!(owner(&document, feature), Some("d#Body001"), "{feature}");
        }
        // Used by features of two Bodies: neither's alone.
        assert_eq!(owner(&document, "Shared"), None);
        assert_eq!(claimed.owned, 9);
    }

    #[test]
    fn each_linked_body_is_placed_once_through_its_link() {
        let mut document = document("Assembly::AssemblyObject");
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
        let mut document = document("App::Part");
        let before = document.clone();
        assert_eq!(claim(&mut document), Claimed::default());
        assert_eq!(document, before);
    }

    #[test]
    fn a_body_no_link_places_stays_placed() {
        let mut document = document("Assembly::AssemblyObject");
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
        let mut document = document("Assembly::AssemblyObject");
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
}
