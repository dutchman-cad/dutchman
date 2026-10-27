//! Bodies a design keeps hidden. A FreeCAD document stores the shape of every step of a
//! PartDesign Body (each Pad and Pocket result, its tool shapes) beside the Body's own shape,
//! and shows only the last; cadmpeg marks the others `visible: false`. They are not parts of
//! the design, so they are left out rather than converted as bodies of their own.

use std::collections::BTreeSet;

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
/// left out; `None` when it hides none, or nothing but hidden bodies (then what it hides is
/// all there is, so it converts as stored). History, structure and parameters are kept.
pub(crate) fn shown(model: &Model) -> Result<Option<(Model, usize)>, String> {
    let bodies = &model.ir.model.bodies;
    let hidden = bodies.iter().filter(|b| b.visible == Some(false)).count();
    if hidden == 0 || hidden == bodies.len() {
        return Ok(None);
    }
    let mut document = serde_json::to_value(&model.ir).map_err(|err| err.to_string())?;
    let wire = document
        .get_mut("model")
        .and_then(Value::as_object_mut)
        .ok_or("the model is malformed")?;
    crate::input_edges::preserve(wire, &model.ir);
    let shown_rows: Vec<&Value> = listed(wire.get("bodies"))
        .iter()
        .filter(|body| body.get("visible").and_then(Value::as_bool) != Some(false))
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
                row.get("visible").and_then(Value::as_bool) != Some(false)
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
        assert!(curves.iter().any(|id| *id == procedural[0].replace(":construction", ":basis")));
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
