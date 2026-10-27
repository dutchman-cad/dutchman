//! Keep selected edges of stored intermediate results before hidden geometry is removed.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use serde_json::{json, Map, Value};

const LIMIT: usize = 10_000;

/// Resolved edges of a hidden, earlier dependency result become feature input edges.
/// Final-body selections are left alone. The writer still has to prove each pick on replay.
pub(crate) fn preserve(wire: &mut Map<String, Value>, ir: &cadmpeg_ir::CadIr) {
    let Some(features) = wire.get("features").and_then(Value::as_array) else {
        return;
    };
    if features.len() > LIMIT {
        return;
    }
    let available = LIMIT.saturating_sub(
        wire.get("feature_input_topologies")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
    );
    let rows: BTreeMap<&str, &Value> = wire
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let mut changes = Vec::new();
    let mut remaining_edges = LIMIT;
    let mut remaining_work = 100_000usize;
    for (index, feature) in features.iter().enumerate() {
        if remaining_work == 0 || changes.len() == available {
            break;
        }
        let Some(id) = feature.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(ordinal) = feature.get("ordinal").and_then(Value::as_u64) else {
            continue;
        };
        let state_id = format!("{id}:stored-input-edges");
        if rows.contains_key(state_id.as_str()) {
            continue;
        }
        let definition = operation(feature);
        if !matches!(
            definition.get("definition").and_then(Value::as_str),
            Some("fillet" | "chamfer")
        ) {
            continue;
        }
        let Some(groups) = definition
            .get("groups")
            .and_then(Value::as_array)
            .filter(|g| g.len() == 1)
        else {
            continue;
        };
        let Some(selection) = groups.first().and_then(|g| g.get("edges")) else {
            continue;
        };
        if selection.get("kind").and_then(Value::as_str) != Some("resolved") {
            continue;
        }
        let Some(native) = selection
            .pointer("/value/native")
            .and_then(Value::as_str)
            .filter(|native| !native.trim().is_empty())
        else {
            continue;
        };
        let Some(edges) = selection
            .pointer("/value/edges")
            .and_then(Value::as_array)
            .filter(|e| !e.is_empty() && e.len() <= 256)
        else {
            continue;
        };
        if edges
            .iter()
            .filter_map(Value::as_str)
            .collect::<BTreeSet<_>>()
            .len()
            != edges.len()
        {
            continue;
        }
        if edges.len() > remaining_edges {
            continue;
        }
        let dependencies = feature.get("dependencies").and_then(Value::as_array);
        let Some(dependencies) = dependencies.filter(|d| d.len() <= LIMIT) else {
            continue;
        };
        let mut owners = BTreeSet::new();
        let mut bounded = true;
        for dependency in dependencies {
            let Some(remaining) = remaining_work.checked_sub(1) else {
                bounded = false;
                break;
            };
            remaining_work = remaining;
            let Some(base) = dependency.as_str().and_then(|id| rows.get(id)).copied() else {
                continue;
            };
            if base
                .get("ordinal")
                .and_then(Value::as_u64)
                .is_none_or(|base| base >= ordinal)
            {
                continue;
            }
            let Some(outputs) = base
                .get("outputs")
                .and_then(Value::as_array)
                .filter(|o| o.len() <= LIMIT)
            else {
                bounded = false;
                break;
            };
            for body in outputs {
                let Some(remaining) = remaining_work.checked_sub(1) else {
                    bounded = false;
                    break;
                };
                remaining_work = remaining;
                let Some(body) = body
                    .as_str()
                    .and_then(|id| rows.get(id))
                    .copied()
                    .filter(|body| body.get("visible").and_then(Value::as_bool) == Some(false))
                else {
                    continue;
                };
                if let Some(reached) = reachable(body, &rows, &mut remaining_work) {
                    if edges
                        .iter()
                        .all(|edge| edge.as_str().is_some_and(|id| reached.contains(id)))
                    {
                        if let Some(id) = body.get("id").and_then(Value::as_str) {
                            owners.insert(id);
                        }
                    }
                } else {
                    bounded = false;
                }
            }
        }
        if !bounded || owners.len() != 1 {
            continue;
        }
        let Some(body) = owners.first().copied() else {
            continue;
        };
        remaining_edges = remaining_edges.saturating_sub(edges.len());
        let geometry: Vec<Value> = edges
            .iter()
            .filter_map(|edge| sample(ir, edge.as_str()?, body))
            .collect();
        let state =
            json!({"id": state_id, "input_of": id, "edges": edges, "edge_geometry": geometry});
        let selection = json!({"kind": "historical", "value": {
            "state": state_id, "edges": edges, "native": native}});
        changes.push((index, state, selection));
    }
    for (index, state, selection) in changes {
        if let Some(feature) = wire
            .get_mut("features")
            .and_then(Value::as_array_mut)
            .and_then(|f| f.get_mut(index))
        {
            let wrapped = feature
                .pointer("/definition/definition")
                .and_then(Value::as_str)
                == Some("post_process");
            let path = if wrapped {
                "/definition/operation/groups/0/edges"
            } else {
                "/definition/groups/0/edges"
            };
            if let Some(edges) = feature.pointer_mut(path) {
                *edges = selection;
            }
        }
        if let Some(states) = wire
            .entry("feature_input_topologies")
            .or_insert_with(|| json!([]))
            .as_array_mut()
        {
            states.push(state);
        }
    }
}

fn operation(feature: &Value) -> &Value {
    let definition = feature.get("definition").unwrap_or(&Value::Null);
    if definition.get("definition").and_then(Value::as_str) == Some("post_process") {
        definition.get("operation").unwrap_or(&Value::Null)
    } else {
        definition
    }
}

fn reachable<'a>(
    body: &'a Value,
    rows: &BTreeMap<&'a str, &'a Value>,
    remaining: &mut usize,
) -> Option<BTreeSet<&'a str>> {
    let mut reached = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut queue = vec![(body, 0)];
    while let Some((row, level)) = queue.pop() {
        if level == 5 {
            reached.insert(row.get("edge")?.as_str()?);
            continue;
        }
        let path = match level {
            0 => "/regions",
            1 => "/shells",
            2 => "/members",
            3 => "/loops/loops",
            4 => "/boundary/coedges",
            _ => return None,
        };
        let members = row.pointer(path)?.as_array()?;
        if members.len() > LIMIT {
            return None;
        }
        for member in members {
            *remaining = remaining.checked_sub(1)?;
            let id = if level == 2 {
                match member.get("kind").and_then(Value::as_str) {
                    Some("face") => {}
                    Some("wire_edge") => {
                        reached.insert(member.get("id")?.as_str()?);
                        if reached.len() > LIMIT {
                            return None;
                        }
                        continue;
                    }
                    Some("free_vertex") => continue,
                    _ => return None,
                }
                member.get("id")?.as_str()?
            } else {
                member.as_str()?
            };
            if visited.insert(id) {
                if visited.len() > LIMIT {
                    return None;
                }
                queue.push((*rows.get(id)?, level + 1));
            }
        }
    }
    Some(reached)
}

fn sample(ir: &cadmpeg_ir::CadIr, id: &str, body: &str) -> Option<Value> {
    // Evaluation is third-party code on file-controlled geometry, with the decoder's guard
    // and resource policy. Unsupported carriers leave a missing sample, never a guessed one.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let transform = ir
            .model
            .bodies
            .iter()
            .find(|row| row.id.as_str() == body)?
            .transform
            .unwrap_or_else(cadmpeg_ir::transform::Transform::identity);
        if !rigid(transform) {
            return None;
        }
        let placed = |point: cadmpeg_ir::features::FinitePoint3| transform.apply_point(point.get());
        let edge = ir.model.edges.iter().find(|edge| edge.id.as_str() == id)?;
        let curve = ir
            .model
            .curves
            .iter()
            .find(|curve| Some(&curve.id) == edge.curve())?;
        let circular = circular(curve.geometry.solved()?)?;
        let [a, b] = edge.param_range()?.get();
        if circular && b - a > std::f64::consts::TAU {
            return None;
        }
        let mid = a * 0.5 + b * 0.5;
        let arena = DecodeArena::new();
        let policy = DecodePolicy::desktop();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).ok()?;
        let interior = cadmpeg_ir::eval::decode::curve_point(&ctx, &curve.geometry, mid).ok()?;
        let tangent = cadmpeg_ir::eval::decode::curve_tangent(&ctx, &curve.geometry, mid).ok()?;
        let tangent = cadmpeg_ir::units::UnitVector3::normalized(
            transform.apply_vector(tangent.get())?.get(),
        )?;
        let position = |id: &cadmpeg_ir::ids::VertexId| {
            let vertex = ir.model.vertices.iter().find(|v| v.id == *id)?;
            ir.model
                .points
                .iter()
                .find(|p| p.id == vertex.point)
                .map(|p| p.position())
        };
        let sample = json!({"edge": id, "interior": placed(interior)?, "tangent": tangent,
            "start": placed(position(&edge.start)?)?, "end": placed(position(&edge.end)?)?});
        ctx.finish_session().ok()?;
        Some(sample)
    }))
    .ok()
    .flatten()
}

fn circular(mut geometry: &SolvedCurveGeometry) -> Option<bool> {
    for _ in 0..64 {
        match geometry {
            SolvedCurveGeometry::Line(_) => return Some(false),
            SolvedCurveGeometry::Circle(_) => return Some(true),
            SolvedCurveGeometry::Transformed(placed) if rigid(*placed.transform()) => {
                geometry = placed.basis();
            }
            _ => return None,
        }
    }
    None
}

/// Reflections preserve lines and circles too; neither scale nor shear does in general.
fn rigid(transform: cadmpeg_ir::transform::Transform) -> bool {
    if transform.is_proper_rigid() {
        return true;
    }
    let mut rows = transform.affine_rows();
    for row in &mut rows {
        if let Some(x) = row.first_mut() {
            *x = -*x;
        }
    }
    cadmpeg_ir::transform::Transform::affine(rows).is_some_and(|t| t.is_proper_rigid())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::{Format, Report};
    use std::path::Path;

    fn cube() -> cadmpeg_ir::CadIr {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic_cube.ipt");
        crate::read(Format::InventorPart, &path, &mut Report::default())
            .unwrap()
            .ir
    }

    fn sample_edge(ir: &cadmpeg_ir::CadIr, edge: &str) -> Option<Value> {
        sample(ir, edge, ir.model.bodies.first()?.id.as_str())
    }

    fn selection(ir: &cadmpeg_ir::CadIr) -> Map<String, Value> {
        let mut wire = serde_json::to_value(ir).unwrap()["model"]
            .as_object()
            .unwrap()
            .clone();
        let body = wire["bodies"][0]["id"].clone();
        let edge = wire["edges"][0]["id"].clone();
        wire.get_mut("bodies").unwrap()[0]["visible"] = json!(false);
        wire.insert(
            "features".to_owned(),
            json!([
                {"id": "test:model:feature#base", "ordinal": 0, "outputs": [body]},
                {"id": "test:model:feature#blend", "ordinal": 1, "dependencies": ["test:model:feature#base"],
                    "definition": {"definition": "fillet", "groups": [{"edges": {
                        "kind": "resolved", "value": {"edges": [edge], "native": "base edges"}}}]}}
            ]),
        );
        wire
    }

    #[test]
    fn final_later_missing_and_repeated_edges_do_not_become_input_edges() {
        let ir = cube();
        let base = selection(&ir);
        let changed = |edit: fn(&mut Map<String, Value>)| {
            let mut wire = base.clone();
            edit(&mut wire);
            preserve(&mut wire, &ir);
            assert_eq!(
                wire["features"][1]["definition"]["groups"][0]["edges"]["kind"],
                "resolved"
            );
            assert!(wire
                .get("feature_input_topologies")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty));
        };
        changed(|wire| wire.get_mut("bodies").unwrap()[0]["visible"] = json!(true));
        changed(|wire| wire.get_mut("features").unwrap()[0]["ordinal"] = json!(2));
        changed(|wire| wire.get_mut("features").unwrap()[1]["dependencies"] = json!([]));
        changed(|wire| wire.get_mut("features").unwrap()[0]["outputs"] = json!([]));
        changed(|wire| {
            let edge = wire["edges"][0]["id"].clone();
            wire.get_mut("bodies").unwrap()[0]["regions"] = json!([]);
            wire.get_mut("bodies").unwrap()[0]["name"] = edge;
        });
        changed(|wire| {
            wire.get_mut("features").unwrap()[1]["definition"]["groups"][0]["edges"]["value"]
                ["edges"] = json!(["missing"])
        });
        changed(|wire| {
            let edges = &mut wire.get_mut("features").unwrap()[1]["definition"]["groups"][0]
                ["edges"]["value"]["edges"];
            *edges = json!([edges[0], edges[0]]);
        });
        changed(|wire| {
            let mut body = wire["bodies"][0].clone();
            body["id"] = json!("test:model:body#other");
            wire.get_mut("bodies")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(body);
            wire.get_mut("features").unwrap()[0]["outputs"]
                .as_array_mut()
                .unwrap()
                .push(json!("test:model:body#other"));
        });
        changed(|wire| {
            let edge = wire["edges"][0]["id"].clone();
            wire.get_mut("bodies")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(json!({
                "id": "test:model:body#wire", "kind": "wire", "visible": false,
                "regions": ["test:model:region#wire"]}));
            wire.get_mut("regions")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(json!({
                "id": "test:model:region#wire", "body": "test:model:body#wire",
                "shells": ["test:model:shell#wire"]}));
            wire.get_mut("shells")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(json!({
                "id": "test:model:shell#wire", "region": "test:model:region#wire",
                "members": [{"kind": "wire_edge", "id": edge}]}));
            wire.get_mut("features").unwrap()[0]["outputs"]
                .as_array_mut()
                .unwrap()
                .push(json!("test:model:body#wire"));
        });
    }

    #[test]
    fn stored_circular_arcs_keep_the_parameter_midpoint_and_tangent() {
        let ir = cube();
        let mut wire = serde_json::to_value(&ir).unwrap();
        let geometry = json!({"kind": "circle", "center": {"x": 0.0, "y": 0.0, "z": 0.0},
            "axis": {"x": 0.0, "y": 0.0, "z": 1.0},
            "ref_direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "radius": 2.0});
        let arc = json!({
            "bodies": [{"id": "test:arc:body#1", "kind": "wire", "regions": ["test:arc:region#1"]}],
            "regions": [{"id": "test:arc:region#1", "body": "test:arc:body#1", "shells": ["test:arc:shell#1"]}],
            "shells": [{"id": "test:arc:shell#1", "region": "test:arc:region#1", "members": [{"kind": "wire_edge", "id": "test:arc:edge#1"}]}],
            "edges": [{"id": "test:arc:edge#1", "start": "test:arc:vertex#0", "end": "test:arc:vertex#1", "carrier": {
                "curve": "test:arc:curve#1", "param_range": [0.0, std::f64::consts::FRAC_PI_2]}}],
            "vertices": [{"id": "test:arc:vertex#0", "point": "test:arc:point#0"}, {"id": "test:arc:vertex#1", "point": "test:arc:point#1"}],
            "points": [{"id": "test:arc:point#0", "position": {"x": 2.0, "y": 0.0, "z": 0.0}},
                {"id": "test:arc:point#1", "position": {"x": 0.0, "y": 2.0, "z": 0.0}}],
            "curves": [{"id": "test:arc:curve#1", "geometry": geometry}]
        });
        let model = wire["model"].as_object_mut().unwrap();
        for rows in model.values_mut() {
            if let Some(rows) = rows.as_array_mut() {
                rows.clear();
            }
        }
        model.extend(arc.as_object().unwrap().clone());
        let ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let sampled = sample_edge(&ir, "test:arc:edge#1").unwrap();
        let root = 2.0_f64.sqrt();
        assert!((sampled["interior"]["x"].as_f64().unwrap() - root).abs() < 1e-12);
        assert!((sampled["interior"]["y"].as_f64().unwrap() - root).abs() < 1e-12);
        assert!((sampled["tangent"]["x"].as_f64().unwrap() + 1.0 / root).abs() < 1e-12);
        assert!((sampled["tangent"]["y"].as_f64().unwrap() - 1.0 / root).abs() < 1e-12);
        assert_eq!(sampled["start"], json!({"x": 2.0, "y": 0.0, "z": 0.0}));
        assert_eq!(sampled["end"], json!({"x": 0.0, "y": 2.0, "z": 0.0}));
        let curve = &mut wire["model"]["curves"][0];
        let basis = curve["geometry"].clone();
        curve["geometry"] = json!({"kind": "transformed", "basis": basis,
            "transform": [[1.0, 0.0, 0.0, 3.0], [0.0, 1.0, 0.0, -4.0], [0.0, 0.0, 1.0, 2.0]]});
        wire["model"]["points"][0]["position"] = json!({"x": 5.0, "y": -4.0, "z": 2.0});
        wire["model"]["points"][1]["position"] = json!({"x": 3.0, "y": -2.0, "z": 2.0});
        let placed = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let sampled = sample_edge(&placed, "test:arc:edge#1").unwrap();
        assert!((sampled["interior"]["x"].as_f64().unwrap() - root - 3.0).abs() < 1e-12);
        assert!((sampled["interior"]["y"].as_f64().unwrap() - root + 4.0).abs() < 1e-12);
        assert_eq!(sampled["interior"]["z"], 2.0);
        wire["model"]["edges"][0]["carrier"]["param_range"] =
            json!([0.0, 2.0 * std::f64::consts::TAU]);
        wire["model"]["edges"][0]["end"] = json!("test:arc:vertex#0");
        let ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(sample_edge(&ir, "test:arc:edge#1").is_none());
    }

    #[test]
    fn missing_or_unsupported_carriers_are_not_sampled() {
        let ir = cube();
        let mut wire = serde_json::to_value(&ir).unwrap();
        let edge = wire["model"]["edges"][0].clone();
        wire["model"]["curves"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["id"] == edge["carrier"]["curve"])
            .unwrap()["geometry"] = json!({"kind": "unknown"});
        let unsupported = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(sample_edge(&unsupported, edge["id"].as_str().unwrap()).is_none());
        wire["model"]["edges"][0]["carrier"] = json!({});
        let missing = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(sample_edge(&missing, edge["id"].as_str().unwrap()).is_none());
    }

    #[test]
    fn samples_follow_the_stored_body_placement() {
        let ir = cube();
        let id = ir.model.edges.first().unwrap().id.as_str();
        let before = sample_edge(&ir, id).unwrap();
        let mut wire = serde_json::to_value(&ir).unwrap();
        wire["model"]["bodies"][0]["transform"] = json!([
            [0.0, -1.0, 0.0, 3.0],
            [1.0, 0.0, 0.0, -4.0],
            [0.0, 0.0, 1.0, 2.0]
        ]);
        let placed = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let after = sample_edge(&placed, id).unwrap();
        for key in ["interior", "start", "end"] {
            assert_eq!(
                after[key]["x"].as_f64().unwrap(),
                3.0 - before[key]["y"].as_f64().unwrap()
            );
            assert_eq!(
                after[key]["y"].as_f64().unwrap(),
                before[key]["x"].as_f64().unwrap() - 4.0
            );
            assert_eq!(
                after[key]["z"].as_f64().unwrap(),
                before[key]["z"].as_f64().unwrap() + 2.0
            );
        }
        assert_eq!(
            after["tangent"]["x"].as_f64().unwrap(),
            -before["tangent"]["y"].as_f64().unwrap()
        );
        assert_eq!(
            after["tangent"]["y"].as_f64().unwrap(),
            before["tangent"]["x"].as_f64().unwrap()
        );
        wire["model"]["bodies"][0]["transform"] = json!([
            [-1.0, 0.0, 0.0, 3.0],
            [0.0, 1.0, 0.0, -4.0],
            [0.0, 0.0, 1.0, 2.0]
        ]);
        let mirrored = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let after = sample_edge(&mirrored, id).unwrap();
        assert_eq!(
            after["interior"]["x"].as_f64().unwrap(),
            3.0 - before["interior"]["x"].as_f64().unwrap()
        );
        assert_eq!(
            after["tangent"]["x"].as_f64().unwrap(),
            -before["tangent"]["x"].as_f64().unwrap()
        );
        wire["model"]["bodies"][0]["transform"][0][0] = json!(2.0);
        let scaled = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(sample_edge(&scaled, id).is_none());
    }

    #[test]
    fn post_processed_blends_preserve_the_wrapped_selection() {
        let ir = cube();
        let mut wire = selection(&ir);
        let blend = &mut wire.get_mut("features").unwrap()[1];
        let operation = blend["definition"].clone();
        blend["definition"] = json!({"definition": "post_process", "operation": operation});
        preserve(&mut wire, &ir);
        assert_eq!(
            wire["features"][1]["definition"]["operation"]["groups"][0]["edges"]["kind"],
            "historical"
        );
        assert_eq!(
            wire["feature_input_topologies"][0]["input_of"],
            "test:model:feature#blend"
        );
    }
}
