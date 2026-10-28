//! Bodies a design keeps hidden. A FreeCAD document stores the shape of every step of a
//! PartDesign Body (each Pad and Pocket result, its tool shapes) beside the Body's own shape,
//! and shows only the last; cadmpeg marks the others `visible: false`. They are not parts of
//! the design, so they are left out rather than converted as bodies of their own.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use crate::assembly::strings;
#[cfg(test)]
use crate::assembly::NOT_GEOMETRY;
use crate::Model;

/// The arenas a body's B-rep is made of. Only rows of these that hidden bodies alone reach are
/// left out: a sketch, a feature or anything else a hidden body names (a sketch's own wire
/// body is hidden, and its sketch is still the history's) is kept.
const BREP: [&str; 15] = [
    "regions",
    "shells",
    "faces",
    "loops",
    "coedges",
    "edges",
    "vertices",
    "points",
    "surfaces",
    "curves",
    "pcurves",
    "procedural_surfaces",
    "procedural_curves",
    "subds",
    "tessellations",
];

/// `model` without its hidden bodies and the geometry only they reach, with how many were
/// left out; `None` when visibility is unchanged, or nothing but hidden bodies remain
/// (then it converts as stored). Ancestor visibility is propagated to occurrences; history,
/// structure and parameters are kept.
pub(crate) fn shown(model: &Model) -> Result<Option<(Model, usize)>, String> {
    let bodies = &model.ir.model.bodies;
    let mut document = serde_json::to_value(&model.ir).map_err(|err| err.to_string())?;
    // FreeCAD hides what a hidden container holds. Other formats say each body's and each
    // occurrence's own visibility, and the assembly writer keeps hidden occurrences hidden.
    let freecad = document.pointer("/native/fcstd").is_some();
    let visibility_changed = freecad && propagate_visibility(&mut document);
    let hidden_ids = if freecad {
        hidden_bodies(&document)
    } else {
        listed(document.pointer("/model/bodies"))
            .iter()
            .filter(|body| body.get("visible").and_then(Value::as_bool) == Some(false))
            .filter_map(|body| body.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect()
    };
    let hidden = hidden_ids.len();
    if (hidden == 0 && !visibility_changed) || hidden == bodies.len() {
        return Ok(None);
    }
    let wire = document
        .get_mut("model")
        .and_then(Value::as_object_mut)
        .ok_or("the model is malformed")?;
    crate::input_edges::preserve(wire, &model.ir);
    let shown_rows: Vec<&Value> = listed(wire.get("bodies"))
        .iter()
        .filter(|body| {
            !hidden_ids.contains(body.get("id").and_then(Value::as_str).unwrap_or_default())
        })
        .collect();
    let geometry: BTreeSet<&str> = wire
        .iter()
        .filter(|(arena, _)| BREP.contains(&arena.as_str()))
        .flat_map(|(_, rows)| listed(Some(rows)))
        .filter_map(|row| row.get("id").and_then(Value::as_str))
        .collect();
    let rows: std::collections::BTreeMap<&str, &Value> = wire
        .values()
        .flat_map(|rows| listed(Some(rows)))
        .filter_map(|row| Some((row.get("id").and_then(Value::as_str)?, row)))
        .collect();
    // A procedural construction names the curve or surface it constructs, not the other way
    // round: reaching the carrier reaches its construction (a trimmed curve's basis, say).
    let mut constructions: std::collections::BTreeMap<&str, Vec<&Value>> =
        std::collections::BTreeMap::new();
    for (arena, field) in [
        ("procedural_curves", "curve"),
        ("procedural_surfaces", "surface"),
    ] {
        for row in listed(wire.get(arena)) {
            if let Some(carrier) = row.get(field).and_then(Value::as_str) {
                constructions.entry(carrier).or_default().push(row);
            }
        }
    }
    // The geometry the shown bodies reach; the rest of the geometry is the hidden bodies'.
    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut queue: Vec<&Value> = shown_rows;
    while let Some(row) = queue.pop() {
        let mut refs = Vec::new();
        strings(row, &mut refs);
        for reference in refs {
            if geometry.contains(reference) && reached.insert(reference.to_owned()) {
                if let Some(next) = rows.get(reference) {
                    queue.push(next);
                }
                for construction in constructions.get(reference).into_iter().flatten() {
                    if let Some(id) = construction.get("id").and_then(Value::as_str) {
                        if reached.insert(id.to_owned()) {
                            queue.push(construction);
                        }
                    }
                }
            }
        }
    }
    let unreached: BTreeSet<String> = geometry
        .iter()
        .filter(|id| !reached.contains(**id))
        .map(|id| (*id).to_owned())
        .collect();
    for (arena, values) in wire.iter_mut() {
        let Some(values) = values.as_array_mut() else {
            continue;
        };
        values.retain(|row| {
            let id = row.get("id").and_then(Value::as_str).unwrap_or_default();
            if arena == "bodies" {
                !hidden_ids.contains(id)
            } else {
                !unreached.contains(id)
            }
        });
    }
    // cadmpeg is third-party code; a panic in it is a reason, not a crash.
    let text = document.to_string();
    let ir = std::panic::catch_unwind(|| cadmpeg_ir::CadIr::from_json(&text))
        .map_err(|_| "cadmpeg failed unexpectedly on the shown bodies".to_owned())?
        .map_err(|err| format!("the shown bodies do not stand alone: {err}"))?;
    Ok(Some((
        Model {
            ir,
            fidelity: model.fidelity.clone(),
            name: model.name.clone(),
            source_file: model.source_file.clone(),
            assembly: model.assembly.clone(),
            // The shown bodies are a subset of the file's: restate them.
            source_step: None,
        },
        hidden,
    )))
}

/// A definition used in any shown occurrence remains available. A hidden occurrence hides
/// its descendants, even when their own visibility is explicitly true.
fn hidden_bodies(document: &Value) -> BTreeSet<String> {
    let model = document.get("model").unwrap_or(&Value::Null);
    let mut hidden: BTreeSet<String> = listed(model.get("bodies"))
        .iter()
        .filter(|body| body.get("visible").and_then(Value::as_bool) == Some(false))
        .filter_map(|body| body.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let concealed_objects = hidden_objects(document);
    let mut inherited = BTreeSet::new();
    for feature in listed(model.get("features")) {
        if feature
            .get("native_ref")
            .and_then(Value::as_str)
            .is_some_and(|id| concealed_objects.contains(id))
        {
            inherited.extend(
                listed(feature.get("outputs"))
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            );
        }
    }
    let occurrences = listed(model.get("occurrences"));
    let concealed = concealed_occurrences(model, &concealed_objects);
    let mut shown_definitions = BTreeSet::new();
    let mut hidden_definitions = BTreeSet::new();
    for occurrence in occurrences {
        let Some(definition) = occurrence
            .pointer("/prototype/definition")
            .and_then(Value::as_str)
        else {
            continue;
        };
        if occurrence
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| concealed.contains(id))
        {
            hidden_definitions.insert(definition);
        } else {
            shown_definitions.insert(definition);
        }
    }
    for definition in listed(model.get("product_definitions")) {
        if definition
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| hidden_definitions.contains(id) && !shown_definitions.contains(id))
        {
            hidden.extend(
                listed(definition.get("bodies"))
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned),
            );
        }
    }
    let shown_bodies: BTreeSet<&str> = listed(model.get("product_definitions"))
        .iter()
        .filter(|definition| {
            definition
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| shown_definitions.contains(id))
        })
        .flat_map(|definition| listed(definition.get("bodies")))
        .filter_map(Value::as_str)
        .collect();
    hidden.extend(
        inherited
            .into_iter()
            .filter(|id| !shown_bodies.contains(id.as_str())),
    );
    let bodies: BTreeSet<&str> = listed(model.get("bodies"))
        .iter()
        .filter_map(|body| body.get("id").and_then(Value::as_str))
        .collect();
    hidden.retain(|id| bodies.contains(id.as_str()));
    hidden
}

fn concealed_occurrences<'a>(
    model: &'a Value,
    concealed_objects: &BTreeSet<&str>,
) -> BTreeSet<&'a str> {
    let occurrences = listed(model.get("occurrences"));
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut concealed = BTreeSet::new();
    let mut queue = Vec::new();
    for occurrence in occurrences {
        let Some(id) = occurrence.get("id").and_then(Value::as_str) else {
            continue;
        };
        if let Some(parent) = occurrence
            .pointer("/parent/occurrence")
            .and_then(Value::as_str)
        {
            children.entry(parent).or_default().push(id);
        }
        if occurrence.get("visible").and_then(Value::as_bool) == Some(false)
            || occurrence
                .get("native_ref")
                .and_then(Value::as_str)
                .is_some_and(|id| concealed_objects.contains(id))
        {
            queue.push(id);
        }
    }
    while let Some(id) = queue.pop() {
        if concealed.insert(id) {
            queue.extend(children.get(id).into_iter().flatten().copied());
        }
    }
    concealed
}

fn propagate_visibility(document: &mut Value) -> bool {
    let objects = hidden_objects(document);
    let concealed: BTreeSet<String> =
        concealed_occurrences(document.get("model").unwrap_or(&Value::Null), &objects)
            .into_iter()
            .map(str::to_owned)
            .collect();
    let Some(occurrences) = document
        .pointer_mut("/model/occurrences")
        .and_then(Value::as_array_mut)
    else {
        return false;
    };
    let mut changed = false;
    for occurrence in occurrences {
        if occurrence
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| concealed.contains(id))
            && occurrence.get("visible").and_then(Value::as_bool) != Some(false)
        {
            if let Some(fields) = occurrence.as_object_mut() {
                fields.insert("visible".to_owned(), Value::Bool(false));
                changed = true;
            }
        }
    }
    changed
}

/// cadmpeg retains decoded container membership and GUI properties even for a container
/// with no shape. Use those records, never source XML, to propagate visibility to its shapes.
fn hidden_objects(document: &Value) -> BTreeSet<&str> {
    let native = document.pointer("/native/fcstd").unwrap_or(&Value::Null);
    let providers: BTreeMap<&str, &str> = listed(native.get("gui_view_providers"))
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row.get("object")?.as_str()?)))
        .collect();
    let mut visibility = BTreeMap::new();
    for (arena, gui) in [("properties", false), ("gui_properties", true)] {
        for property in listed(native.get(arena)) {
            if property.get("name").and_then(Value::as_str) != Some("Visibility")
                || property.get("type_name").and_then(Value::as_str) != Some("App::PropertyBool")
            {
                continue;
            }
            let owner = property.get("owner").and_then(Value::as_str);
            let owner = if gui {
                owner.and_then(|id| providers.get(id).copied())
            } else {
                owner
            };
            let value = listed(property.get("values"))
                .iter()
                .find(|value| value.get("tag").and_then(Value::as_str) == Some("Bool"))
                .and_then(|value| value.pointer("/attributes/value"))
                .and_then(Value::as_str);
            if let (Some(owner), Some(value @ ("true" | "false" | "0" | "1"))) = (owner, value) {
                visibility.insert(owner, matches!(value, "true" | "1"));
            }
        }
    }
    let containers: BTreeSet<&str> = listed(native.get("objects"))
        .iter()
        .filter(|row| {
            matches!(
                row.get("type_name").and_then(Value::as_str),
                Some("PartDesign::Body" | "App::Part" | "App::DocumentObjectGroup")
            )
        })
        .filter_map(|row| row.get("id").and_then(Value::as_str))
        .collect();
    let mut children: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for property in listed(native.get("properties")) {
        let Some(owner) = property
            .get("owner")
            .and_then(Value::as_str)
            .filter(|id| containers.contains(id))
        else {
            continue;
        };
        if property.get("name").and_then(Value::as_str) == Some("Group") {
            for link in listed(property.get("links")) {
                if link.get("document").is_none_or(Value::is_null) {
                    if let Some(child) = link.get("object").and_then(Value::as_str) {
                        children.entry(owner).or_default().push(child);
                    }
                }
            }
        }
    }
    let mut hidden = BTreeSet::new();
    let mut queue: Vec<&str> = visibility
        .into_iter()
        .filter_map(|(id, visible)| (!visible).then_some(id))
        .collect();
    while let Some(id) = queue.pop() {
        if hidden.insert(id) {
            queue.extend(children.get(id).into_iter().flatten().copied());
        }
    }
    hidden
}

fn listed(value: Option<&Value>) -> &[Value] {
    value
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::{Format, Report};

    /// cadmpeg's example 10 mm cube twice over: the second a copy of every geometry row under
    /// new ids, its body `visible` as given.
    fn two_cubes(second_visible: Option<bool>) -> Model {
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic_cube.ipt");
        let part = crate::read(Format::InventorPart, &fixture, &mut Report::default()).unwrap();
        let mut wire = serde_json::to_value(&part.ir).unwrap();
        let model = wire["model"].as_object_mut().unwrap();
        let ids: BTreeMap<String, String> = model
            .iter()
            .filter(|(arena, _)| {
                arena.as_str() == "bodies" || !NOT_GEOMETRY.contains(&arena.as_str())
            })
            .flat_map(|(_, rows)| listed(Some(rows)))
            .filter_map(|row| row.get("id").and_then(Value::as_str))
            .map(|id| (id.to_owned(), format!("{id}~copy")))
            .collect();
        fn renamed(value: &Value, ids: &BTreeMap<String, String>) -> Value {
            match value {
                Value::String(s) => json!(ids.get(s).unwrap_or(s)),
                Value::Array(values) => values.iter().map(|v| renamed(v, ids)).collect(),
                Value::Object(fields) => fields
                    .iter()
                    .map(|(k, v)| (k.clone(), renamed(v, ids)))
                    .collect(),
                other => other.clone(),
            }
        }
        for (arena, rows) in model.iter_mut() {
            if arena != "bodies" && NOT_GEOMETRY.contains(&arena.as_str()) {
                continue;
            }
            let rows = rows.as_array_mut().unwrap();
            let copies: Vec<Value> = rows
                .iter()
                .map(|row| {
                    let mut copy = renamed(row, &ids);
                    if arena == "bodies" {
                        match second_visible {
                            Some(visible) => copy["visible"] = json!(visible),
                            None => {
                                if let Some(fields) = copy.as_object_mut() {
                                    fields.remove("visible");
                                }
                            }
                        }
                    }
                    copy
                })
                .collect();
            rows.extend(copies);
        }
        Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..part
        }
    }

    #[test]
    fn decoded_container_membership_propagates_gui_visibility() {
        let document = json!({"native": {"fcstd": {
            "objects": [{"id": "Part", "type_name": "App::Part"},
                {"id": "Body", "type_name": "PartDesign::Body"}],
            "gui_view_providers": [{"id": "part-view", "object": "Part"},
                {"id": "body-view", "object": "Body"}, {"id": "pad-view", "object": "Pad"}],
            "gui_properties": [
                {"owner": "part-view", "name": "Visibility", "type_name": "App::PropertyBool", "values": [{"tag": "Bool", "attributes": {"value": "false"}}]},
                {"owner": "body-view", "name": "Visibility", "type_name": "App::PropertyBool", "values": [{"tag": "Bool", "attributes": {"value": "true"}}]},
                {"owner": "pad-view", "name": "Visibility", "type_name": "App::PropertyBool", "values": [{"tag": "Bool", "attributes": {"value": "true"}}]}],
            "properties": [
                {"owner": "Part", "name": "Visibility", "type_name": "App::PropertyBool", "values": [{"tag": "Bool", "attributes": {"value": "false"}}]},
                {"owner": "Part", "name": "Group", "links": [{"object": "Body"}]},
                {"owner": "Body", "name": "Group", "links": [{"object": "Pad"}]}]
        }}});
        assert_eq!(
            hidden_objects(&document),
            BTreeSet::from(["Part", "Body", "Pad"])
        );
        let mut own_visibility = document.clone();
        own_visibility["native"]["fcstd"]["gui_properties"][0]["values"][0]["attributes"]
            ["value"] = json!("true");
        assert!(hidden_objects(&own_visibility).is_empty());
        // A cycle is bounded by visited identities rather than the number of paths.
        let mut cycle = document;
        cycle["native"]["fcstd"]["properties"][2]["links"]
            .as_array_mut()
            .unwrap()
            .push(json!({"object": "Part"}));
        assert_eq!(hidden_objects(&cycle).len(), 3);
    }

    #[test]
    fn a_shown_link_keeps_geometry_hidden_in_the_source_container() {
        let document = json!({"model": {
            "bodies": [{"id": "b", "visible": true}],
            "features": [{"native_ref": "Body", "outputs": ["b"]}],
            "product_definitions": [{"id": "part", "kind": "part", "bodies": ["b"]}],
            "occurrences": [
                {"id": "source", "prototype": {"definition": "part"}, "native_ref": "Body"},
                {"id": "link", "prototype": {"definition": "part"}, "native_ref": "Link"}]
        }, "native": {"fcstd": {
            "objects": [{"id": "Part", "type_name": "App::Part"}],
            "properties": [
                {"owner": "Part", "name": "Visibility", "type_name": "App::PropertyBool", "values": [{"tag": "Bool", "attributes": {"value": "false"}}]},
                {"owner": "Part", "name": "Group", "links": [{"object": "Body"}]}]
        }}});
        assert!(hidden_bodies(&document).is_empty());
        let mut shown_link = document.clone();
        assert!(propagate_visibility(&mut shown_link));
        assert_eq!(
            shown_link["model"]["occurrences"][0]["visible"],
            json!(false)
        );
        assert!(shown_link["model"]["occurrences"][1]
            .get("visible")
            .is_none());
        assert!(hidden_bodies(&shown_link).is_empty());
        let mut without_link = document;
        let _ = without_link["model"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert_eq!(
            hidden_bodies(&without_link),
            BTreeSet::from(["b".to_owned()])
        );
    }

    #[test]
    fn hidden_ancestors_override_visible_children_but_shown_instances_survive() {
        let mut model = two_cubes(Some(true));
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let second = wire["model"]["bodies"][1]["id"].clone();
        wire["model"]["product_definitions"] = json!([
            {"id": "test:model:component#container", "kind": "group"},
            {"id": "test:model:component#nested", "kind": "group"},
            {"id": "test:model:component#part", "kind": "part", "bodies": [second]},
        ]);
        let occurrence = |id: &str, definition: &str, parent: Value, visible: bool| {
            json!({
            "id": id, "prototype": {"scope": "local", "definition": definition},
            "parent": parent, "ordinal": 0, "transform": crate::placement::IDENTITY,
            "scale": [1, 1, 1], "visible": visible})
        };
        wire["model"]["occurrences"] = json!([
            occurrence(
                "test:model:occurrence#container",
                "test:model:component#container",
                json!({"kind": "root"}),
                false
            ),
            occurrence(
                "test:model:occurrence#nested",
                "test:model:component#nested",
                json!({"kind": "occurrence", "occurrence": "test:model:occurrence#container"}),
                true
            ),
            occurrence(
                "test:model:occurrence#part",
                "test:model:component#part",
                json!({"kind": "occurrence", "occurrence": "test:model:occurrence#nested"}),
                true
            ),
        ]);
        // The filtering policy consumes the neutral occurrence graph, including deeply nested
        // containers. Keep this independent of source-format visibility projection.
        assert!(hidden_bodies(&wire).contains(second.as_str().unwrap()));
        wire["model"]["occurrences"]
            .as_array_mut()
            .unwrap()
            .push(occurrence(
                "test:model:occurrence#shown",
                "test:model:component#part",
                json!({"kind": "root"}),
                true,
            ));
        assert!(!hidden_bodies(&wire).contains(second.as_str().unwrap()));
        let _ = wire["model"]["occurrences"].as_array_mut().unwrap().pop();
        // Other formats state each occurrence's own visibility, so nothing is propagated.
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(shown(&model).unwrap().is_none());
        wire["native"]["fcstd"] = json!({});
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (shown, count) = shown(&model).unwrap().unwrap();
        assert_eq!(count, 1);
        assert_eq!(shown.ir.model.bodies.len(), 1);
    }

    #[test]
    fn a_shown_curve_keeps_its_construction_and_basis() {
        let model = two_cubes(Some(false));
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let m = wire["model"].as_object_mut().unwrap();
        // A subset construction of one shown and one hidden edge curve, each over a basis curve
        // of its own: the construction names the curve, nothing names the construction.
        let mut constructions = Vec::new();
        let mut bases = Vec::new();
        for hidden in [false, true] {
            let edge = m["edges"]
                .as_array()
                .unwrap()
                .iter()
                .find(|e| e["id"].as_str().unwrap().ends_with("~copy") == hidden)
                .unwrap()
                .clone();
            let curve = edge["carrier"]["curve"].as_str().unwrap().to_owned();
            let mut basis = m["curves"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == json!(curve))
                .unwrap()
                .clone();
            basis["id"] = json!(format!("{curve}:basis"));
            bases.push(basis);
            constructions.push(json!({
                "id": format!("{curve}:construction"), "curve": curve,
                "definition": {"kind": "subset", "source": format!("{curve}:basis"),
                    "parameter_range": edge["carrier"]["param_range"], "sense": true}}));
        }
        m["curves"].as_array_mut().unwrap().extend(bases);
        m.insert("procedural_curves".to_owned(), json!(constructions));
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model
        };
        let (shown, _) = shown(&model).unwrap().unwrap();
        let kept = serde_json::to_value(&shown.ir).unwrap();
        let ids = |arena: &str| -> Vec<String> {
            listed(kept["model"].get(arena))
                .iter()
                .filter_map(|row| row["id"].as_str().map(str::to_owned))
                .collect()
        };
        let procedural = ids("procedural_curves");
        assert_eq!(procedural.len(), 1, "{procedural:?}");
        assert!(!procedural[0].contains("~copy"));
        let curves = ids("curves");
        assert!(curves
            .iter()
            .any(|id| *id == procedural[0].replace(":construction", ":basis")));
        assert!(!curves.iter().any(|id| id.contains("~copy")));
    }

    #[test]
    fn hidden_bodies_and_their_geometry_are_left_out() {
        let model = two_cubes(Some(false));
        assert_eq!(model.ir.model.bodies.len(), 2);
        assert_eq!(model.ir.model.faces.len(), 12);
        let (shown, hidden) = shown(&model).unwrap().unwrap();
        assert_eq!(hidden, 1);
        assert_eq!(shown.ir.model.bodies.len(), 1);
        assert_eq!(shown.ir.model.faces.len(), 6);
        assert_eq!(shown.ir.model.edges.len(), 12);
        assert_eq!(shown.ir.model.bodies[0].id, model.ir.model.bodies[0].id);
    }

    #[test]
    fn blends_keep_their_stored_base_edges_when_hidden_geometry_is_removed() {
        for kind in ["fillet", "chamfer"] {
            let mut model = two_cubes(Some(false));
            let mut wire = serde_json::to_value(&model.ir).unwrap();
            // The stored base is elsewhere: its sample cannot come from the visible cube.
            for (arena, path) in [
                ("points", "/position/x"),
                ("curves", "/geometry/origin/x"),
                ("surfaces", "/geometry/origin/x"),
            ] {
                for row in wire["model"][arena].as_array_mut().unwrap() {
                    if row["id"].as_str().unwrap().ends_with("~copy") {
                        let x = row.pointer_mut(path).unwrap();
                        *x = json!(x.as_f64().unwrap() + 30.0);
                    }
                }
            }
            let body = wire["model"]["bodies"][1]["id"].clone();
            let edge = wire["model"]["edges"]
                .as_array()
                .unwrap()
                .iter()
                .find(|edge| edge["id"].as_str().unwrap().ends_with("~copy"))
                .unwrap()["id"]
                .clone();
            let dimension = if kind == "fillet" {
                json!({"radius": {"kind": "constant", "radius": 0.5}})
            } else {
                json!({"spec": {"kind": "distance", "distance": 0.5}})
            };
            let mut group = dimension;
            group["edges"] = json!({"kind": "resolved", "value": {
                "edges": [edge], "native": "synthetic base selection"}});
            wire["model"]["features"] = json!([
                {"id": "test:model:feature#base", "ordinal": 0, "outputs": [body], "suppressed": false,
                    "definition": {"definition": "native", "kind": "synthetic"}},
                {"id": "test:model:feature#blend", "ordinal": 1, "dependencies": ["test:model:feature#base"], "suppressed": false,
                    "definition": {"definition": kind, "groups": [group]}}
            ]);
            model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
            let (shown, _) = shown(&model).unwrap().unwrap();
            assert_eq!(shown.ir.model.bodies.len(), 1);
            assert!(shown
                .ir
                .model
                .edges
                .iter()
                .all(|e| e.id.as_str() != edge.as_str().unwrap()));
            let kept = serde_json::to_value(&shown.ir).unwrap();
            let selection = &kept["model"]["features"][1]["definition"]["groups"][0]["edges"];
            assert_eq!(selection["kind"], "historical");
            let state = &kept["model"]["feature_input_topologies"][0];
            assert_eq!(state["input_of"], "test:model:feature#blend");
            assert_eq!(state["edges"], json!([edge]));
            assert_eq!(state["edge_geometry"].as_array().unwrap().len(), 1);
            assert!(state["edge_geometry"][0]["interior"].is_object());
            assert!(state["edge_geometry"][0]["interior"]["x"].as_f64().unwrap() >= 30.0);
            assert!(state["edge_geometry"][0]["tangent"].is_object());
        }
    }

    #[test]
    fn structure_naming_a_hidden_body_still_reads() {
        let mut model = two_cubes(Some(false));
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let ids: Vec<Value> = listed(wire["model"].get("bodies"))
            .iter()
            .map(|body| body["id"].clone())
            .collect();
        wire["model"]["product_definitions"] = json!([{
            "id": "fcstd:model:component#body", "kind": "part", "label": "Body", "bodies": ids,
        }]);
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (shown, _) = shown(&model).unwrap().unwrap();
        assert_eq!(shown.ir.model.bodies.len(), 1);
    }

    #[test]
    fn sketches_and_their_curves_are_kept_with_the_history() {
        // A FreeCAD sketch's own wire body is hidden; the sketch is still the history's.
        let mut model = two_cubes(Some(false));
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let sketch = "fcstd:model:sketch#Sketch";
        wire["model"]["sketches"] = json!([{"id": sketch, "name": "Sketch",
            "placement": {"kind": "resolved", "frame": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}}}}]);
        wire["model"]["sketch_entities"] = json!([{"id": "fcstd:model:sketch-entity#1",
            "sketch": sketch, "geometry": {"kind": "line",
            "start": {"u": 0.0, "v": 0.0}, "end": {"u": 10.0, "v": 0.0}}}]);
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (shown, _) = shown(&model).unwrap().unwrap();
        assert_eq!(shown.ir.model.bodies.len(), 1);
        assert_eq!(shown.ir.model.faces.len(), 6);
        assert_eq!(shown.ir.model.sketches.len(), 1);
        assert_eq!(shown.ir.model.sketch_entities.len(), 1);
    }

    #[test]
    fn bodies_shown_or_unstated_are_all_kept() {
        assert!(shown(&two_cubes(Some(true))).unwrap().is_none());
        assert!(shown(&two_cubes(None)).unwrap().is_none());
    }

    #[test]
    fn a_design_hiding_every_body_converts_as_stored() {
        let mut model = two_cubes(Some(false));
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["bodies"][0]["visible"] = json!(false);
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(shown(&model).unwrap().is_none());
    }
}
