//! Keep selected edges of stored intermediate results before hidden geometry is removed.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use serde_json::{json, Value};

const LIMIT: usize = 10_000;

/// Resolved edges of a hidden, earlier dependency result become feature input edges.
/// Final-body selections are left alone. The writer still has to prove each pick on replay.
#[cfg(test)]
pub(crate) fn preserve(document: &mut Value, ir: &cadmpeg_ir::CadIr) {
    let changes = plan(document, ir);
    apply_json(document, &changes);
}

/// One feature's stored input edges: its position in the feature list and the feature, the
/// input state that holds them, the selection that now names them, and whether the feature
/// is a sweep (its path) rather than a blend.
pub(crate) struct Change {
    index: usize,
    feature: String,
    state: Value,
    selection: Value,
    path: bool,
}

/// The features whose resolved blend edges or sweep path become stored input edges.
pub(crate) fn plan(document: &Value, ir: &cadmpeg_ir::CadIr) -> Vec<Change> {
    let Some(wire) = document.get("model").and_then(Value::as_object) else {
        return Vec::new();
    };
    let Some(features) = wire.get("features").and_then(Value::as_array) else {
        return Vec::new();
    };
    if features.len() > LIMIT {
        return Vec::new();
    }
    let available = LIMIT.saturating_sub(
        wire.get("feature_input_topologies")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
    );
    let rows: BTreeMap<&str, &Value> = wire
        .values()
        .chain(
            document
                .get("geometry")
                .and_then(Value::as_object)
                .into_iter()
                .flat_map(|home| home.values()),
        )
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let mut children: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for (arena, key) in [("regions", "body"), ("faces", "shell")] {
        for row in document
            .pointer(&format!("/geometry/{arena}"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(parent) = row.get(key).and_then(Value::as_str) {
                children.entry(parent).or_default().push(row);
            }
        }
    }
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
        let Some((edges, native, path)) = selection(feature) else {
            continue;
        };
        if edges.is_empty() || edges.len() > 256 {
            continue;
        }
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
                if let Some(reached) = reachable(body, &rows, &children, &mut remaining_work) {
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
        let bodies: Vec<&str> = if path { vec![body] } else { Vec::new() };
        let state = json!({"id": state_id, "input_of": id, "edges": edges, "edge_geometry": geometry, "bodies": bodies, "faces": [], "face_owners": [], "vertices": []});
        let selection = json!({"kind": if path { "historical_edges" } else { "historical" }, "value": {
            "state": state_id, "edges": edges, "native": native}});
        changes.push(Change {
            index,
            feature: id.to_owned(),
            state,
            selection,
            path,
        });
    }
    changes
}

/// `changes` written into the document: each feature's selection names its stored input edges,
/// and the input state that holds them joins the document's. Only these small rows are read
/// from JSON; the rest of the document stays typed.
pub(crate) fn apply(ir: &mut cadmpeg_ir::CadIr, changes: Vec<Change>) -> Result<(), String> {
    use cadmpeg_ir::features::{
        edge_treatments::{ChamferGroup, FilletGroup},
        evaluation::FeatureInputTopology,
        profiles::PathRef,
        selections::{EdgeSelection, NonEmptyMembers},
        FeatureDefinition, FeatureOperation,
    };
    fn read<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, String> {
        serde_json::from_value(value).map_err(|err| err.to_string())
    }
    for Change {
        index,
        feature,
        state,
        selection,
        path,
    } in changes
    {
        let state: FeatureInputTopology = read(state)?;
        let target = ir
            .model
            .features
            .get_mut(index)
            .filter(|row| row.id.as_str() == feature)
            .ok_or("a feature moved while its input edges were stored")?;
        let mut outcome = Ok(());
        target.evaluation.edit(|definition, _| {
            let operation = match definition {
                FeatureDefinition::PostProcess { operation, .. } => operation,
                FeatureDefinition::Operation(operation) => operation,
            };
            outcome = (|| -> Result<(), String> {
                match operation {
                    FeatureOperation::Fillet { groups } if !path => {
                        let mut group: FilletGroup = groups
                            .first()
                            .cloned()
                            .ok_or("a blend names no edge group")?;
                        group.edges = read::<EdgeSelection>(selection)?;
                        *groups = NonEmptyMembers::one(group);
                    }
                    FeatureOperation::Chamfer { groups, .. } if !path => {
                        let mut group: ChamferGroup = groups
                            .first()
                            .cloned()
                            .ok_or("a blend names no edge group")?;
                        group.edges = read::<EdgeSelection>(selection)?;
                        *groups = NonEmptyMembers::one(group);
                    }
                    FeatureOperation::Sweep { path: along, .. } if path => {
                        *along = Some(read::<PathRef>(selection)?);
                    }
                    _ => {}
                }
                Ok(())
            })();
        });
        outcome?;
        ir.model.feature_input_topologies.push(state);
    }
    Ok(())
}

/// `changes` written into the JSON of a document.
#[cfg(test)]
pub(crate) fn apply_json(document: &mut Value, changes: &[Change]) {
    let Some(wire) = document.get_mut("model").and_then(Value::as_object_mut) else {
        return;
    };
    for Change {
        index,
        state,
        selection,
        path,
        ..
    } in changes
    {
        let (index, state, selection, path) = (*index, state.clone(), selection.clone(), *path);
        if let Some(feature) = wire
            .get_mut("features")
            .and_then(Value::as_array_mut)
            .and_then(|f| f.get_mut(index))
        {
            let wrapped =
                feature.pointer("/definition/kind").and_then(Value::as_str) == Some("post_process");
            let root = if wrapped {
                "/definition/value/operation/value"
            } else {
                "/definition/value/value"
            };
            let path = format!("{root}/{}", if path { "path" } else { "groups/0/edges" });
            if let Some(edges) = feature.pointer_mut(&path) {
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

/// A resolved blend selection or an ordered topological sweep path. These refer only
/// to CADIR geometry; no source-native bytes or edge numbers are interpreted here.
fn selection(feature: &Value) -> Option<(&[Value], &str, bool)> {
    let definition = operation(feature);
    match operation_kind(feature)? {
        "fillet" | "chamfer" => {
            let groups = definition
                .get("groups")?
                .as_array()
                .filter(|g| g.len() == 1)?;
            let selection = groups.first()?.get("edges")?;
            if selection.get("kind")?.as_str()? != "resolved" {
                return None;
            }
            let native = selection
                .pointer("/value/native")?
                .as_str()
                .filter(|native| !native.trim().is_empty())?;
            Some((
                selection.pointer("/value/edges")?.as_array()?,
                native,
                false,
            ))
        }
        "sweep" => {
            let path = definition.get("path")?;
            if path.get("kind")?.as_str()? != "edges" {
                return None;
            }
            // PathRef::Edges already names neutral edge identities in order. The
            // consuming feature id records provenance for the temporary input state.
            Some((
                path.get("value")?.as_array()?,
                feature.get("id")?.as_str()?,
                true,
            ))
        }
        _ => None,
    }
}

fn operation_enum(feature: &Value) -> &Value {
    match feature.pointer("/definition/kind").and_then(Value::as_str) {
        Some("operation") => feature.pointer("/definition/value").unwrap_or(&Value::Null),
        Some("post_process") => feature
            .pointer("/definition/value/operation")
            .unwrap_or(&Value::Null),
        _ => &Value::Null,
    }
}
fn operation_kind(feature: &Value) -> Option<&str> {
    operation_enum(feature).get("kind").and_then(Value::as_str)
}
fn operation(feature: &Value) -> &Value {
    operation_enum(feature).get("value").unwrap_or(&Value::Null)
}

fn reachable<'a>(
    body: &'a Value,
    rows: &BTreeMap<&'a str, &'a Value>,
    children: &BTreeMap<&'a str, Vec<&'a Value>>,
    remaining: &mut usize,
) -> Option<BTreeSet<&'a str>> {
    let mut reached = BTreeSet::new();
    let mut visited = BTreeSet::new();
    let mut queue = vec![(body, 0)];
    while let Some((row, level)) = queue.pop() {
        *remaining = remaining.checked_sub(1)?;
        if level == 5 {
            reached.insert(row.get("edge")?.as_str()?);
            continue;
        }
        let mut ids = Vec::new();
        if level == 0 || level == 2 {
            let id = row.get("id")?.as_str()?;
            for candidate in children.get(id).into_iter().flatten() {
                *remaining = remaining.checked_sub(1)?;
                ids.push(candidate.get("id")?.as_str()?);
            }
            if level == 2 {
                for member in row.get("members")?.as_array()? {
                    if member.get("kind").and_then(Value::as_str) == Some("wire_edge") {
                        reached.insert(member.get("id")?.as_str()?);
                    }
                }
            }
        } else {
            let path = match level {
                1 => "/shells",
                3 => "/loops/loops",
                4 => "/boundary/coedges",
                _ => return None,
            };
            for member in row.pointer(path)?.as_array()? {
                ids.push(member.as_str()?);
            }
        }
        if ids.len() > LIMIT || reached.len() > LIMIT {
            return None;
        }
        for id in ids {
            *remaining = remaining.checked_sub(1)?;
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
            .geometry
            .bodies
            .iter()
            .find(|row| row.id.as_str() == body)?
            .transform
            .unwrap_or_else(cadmpeg_ir::transform::Transform::identity);
        if !rigid(transform) {
            return None;
        }
        let placed = |point: cadmpeg_ir::features::geometry::FinitePoint3| transform.apply_point(point.get());
        let edge = ir
            .geometry
            .edges
            .iter()
            .find(|edge| edge.id.as_str() == id)?;
        let curve = ir
            .geometry
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
            let vertex = ir.geometry.vertices.iter().find(|v| v.id == *id)?;
            ir.geometry
                .points
                .iter()
                .find(|p| p.id == vertex.point)
                .map(|p| p.position())
        };
        let sample = json!({"edge": id, "interior": placed(interior)?, "tangent": tangent,
            "faces": [], "supports": [], "start": placed(position(edge.start()?)?)?, "end": placed(position(edge.end()?)?)?});
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
        sample(ir, edge, ir.geometry.bodies.first()?.id.as_str())
    }

    fn selection(ir: &cadmpeg_ir::CadIr) -> Value {
        let mut wire = serde_json::to_value(ir).unwrap();
        let body = wire["geometry"]["bodies"][0]["id"].clone();
        let edge = wire["geometry"]["edges"][0]["id"].clone();
        wire["geometry"].get_mut("bodies").unwrap()[0]["visible"] = json!(false);
        wire["model"].as_object_mut().unwrap().insert(
            "features".to_owned(),
            json!([
                {"id": "test:model:feature#base", "ordinal": 0, "outputs": [body]},
                {"id": "test:model:feature#blend", "ordinal": 1, "dependencies": ["test:model:feature#base"], "definition": {"kind": "operation", "value": {"kind": "fillet", "value": {"groups": [{"edges": {
                        "kind": "resolved", "value": {"edges": [edge], "native": "base edges"}}}]}}}, "source_properties": {}, "source_content": [], "outputs": []}
            ]),
        );
        wire
    }

    #[test]
    fn final_later_missing_and_repeated_edges_do_not_become_input_edges() {
        let ir = cube();
        let base = selection(&ir);
        let changed = |edit: fn(&mut Value)| {
            let mut wire = base.clone();
            edit(&mut wire);
            preserve(&mut wire, &ir);
            assert_eq!(
                wire["model"]["features"][1]["definition"]["value"]["value"]["groups"][0]["edges"]
                    ["kind"],
                "resolved"
            );
            assert!(wire["model"]
                .get("feature_input_topologies")
                .and_then(Value::as_array)
                .is_none_or(Vec::is_empty));
        };
        changed(|wire| wire["geometry"].get_mut("bodies").unwrap()[0]["visible"] = json!(true));
        changed(|wire| wire["model"].get_mut("features").unwrap()[0]["ordinal"] = json!(2));
        changed(|wire| wire["model"].get_mut("features").unwrap()[1]["dependencies"] = json!([]));
        changed(|wire| wire["model"].get_mut("features").unwrap()[0]["outputs"] = json!([]));
        changed(|wire| {
            let edge = wire["geometry"]["edges"][0]["id"].clone();
            wire["geometry"]["regions"] = json!([]);
            wire["geometry"].get_mut("bodies").unwrap()[0]["name"] = edge;
        });
        changed(|wire| {
            wire["model"].get_mut("features").unwrap()[1]["definition"]["value"]["value"]
                ["groups"][0]["edges"]["value"]["edges"] = json!(["missing"])
        });
        changed(|wire| {
            let edges = &mut wire["model"].get_mut("features").unwrap()[1]["definition"]["value"]
                ["value"]["groups"][0]["edges"]["value"]["edges"];
            *edges = json!([edges[0], edges[0]]);
        });
        changed(|wire| {
            let mut body = wire["geometry"]["bodies"][0].clone();
            body["id"] = json!("test:model:body#other");
            wire["geometry"]
                .get_mut("bodies")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(body);
            let mut region = wire["geometry"]["regions"][0].clone();
            region["id"] = json!("test:model:region#other");
            region["body"] = json!("test:model:body#other");
            wire["geometry"]["regions"]
                .as_array_mut()
                .unwrap()
                .push(region);
            wire["model"].get_mut("features").unwrap()[0]["outputs"]
                .as_array_mut()
                .unwrap()
                .push(json!("test:model:body#other"));
        });
        changed(|wire| {
            let edge = wire["geometry"]["edges"][0]["id"].clone();
            wire["geometry"]
                .get_mut("bodies")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(json!({"id": "test:model:body#wire", "kind": "wire", "visible": false}));
            wire["geometry"]
                .get_mut("regions")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(json!({
                "id": "test:model:region#wire", "body": "test:model:body#wire",
                "shells": ["test:model:shell#wire"]}));
            wire["geometry"]
                .get_mut("shells")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(json!({
                "id": "test:model:shell#wire", "region": "test:model:region#wire",
                "members": [{"kind": "wire_edge", "id": edge}]}));
            wire["model"].get_mut("features").unwrap()[0]["outputs"]
                .as_array_mut()
                .unwrap()
                .push(json!("test:model:body#wire"));
        });
    }

    #[test]
    fn sweep_paths_preserve_only_edges_of_an_earlier_stored_result() {
        let ir = cube();
        let base = selection(&ir);
        for refusal in [
            None,
            Some("visible"),
            Some("later"),
            Some("foreign"),
            Some("repeated"),
        ] {
            let mut wire = base.clone();
            let edge = wire["geometry"]["edges"][0]["id"].clone();
            wire["model"]["features"][1]["definition"]["value"] = json!({"kind": "sweep", "value": {
                "path": {"kind": "edges", "value": [edge]}}});
            match refusal {
                Some("visible") => wire["geometry"]["bodies"][0]["visible"] = json!(true),
                Some("later") => wire["model"]["features"][0]["ordinal"] = json!(2),
                Some("foreign") => wire["model"]["features"][1]["dependencies"] = json!([]),
                Some("repeated") => {
                    wire["model"]["features"][1]["definition"]["value"]["value"]["path"]["value"] =
                        json!([edge, edge])
                }
                _ => {}
            }
            preserve(&mut wire, &ir);
            let path = &wire["model"]["features"][1]["definition"]["value"]["value"]["path"];
            if refusal.is_some() {
                assert_eq!(path["kind"], "edges");
            } else {
                assert_eq!(path["kind"], "historical_edges");
                assert_eq!(path["value"]["edges"], json!([edge]));
                let state = &wire["model"]["feature_input_topologies"][0];
                assert_eq!(state["input_of"], "test:model:feature#blend");
                assert_eq!(state["edge_geometry"][0]["edge"], edge);
                assert_eq!(
                    state["bodies"],
                    json!([wire["geometry"]["bodies"][0]["id"]])
                );
            }
        }
    }

    #[test]
    fn stored_circular_arcs_keep_the_parameter_midpoint_and_tangent() {
        let ir = cube();
        let mut wire = serde_json::to_value(&ir).unwrap();
        let geometry = json!({"kind": "circle", "center": {"x": 0.0, "y": 0.0, "z": 0.0},
            "axis": {"x": 0.0, "y": 0.0, "z": 1.0},
            "ref_direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "radius": 2.0});
        let arc = json!({
            "bodies": [{"id": "test:arc:body#1", "kind": "wire"}],
            "regions": [{"id": "test:arc:region#1", "body": "test:arc:body#1", "shells": ["test:arc:shell#1"]}],
            "shells": [{"id": "test:arc:shell#1", "region": "test:arc:region#1", "members": [{"kind": "wire_edge", "id": "test:arc:edge#1"}]}],
            "edges": [{"id": "test:arc:edge#1", "carrier": {
                "curve": "test:arc:curve#1", "param_range": [0.0, std::f64::consts::FRAC_PI_2]}, "ends": {"start": "test:arc:vertex#0", "end": "test:arc:vertex#1"}}],
            "vertices": [{"id": "test:arc:vertex#0", "point": "test:arc:point#0"}, {"id": "test:arc:vertex#1", "point": "test:arc:point#1"}],
            "points": [{"id": "test:arc:point#0", "position": {"x": 2.0, "y": 0.0, "z": 0.0}},
                {"id": "test:arc:point#1", "position": {"x": 0.0, "y": 2.0, "z": 0.0}}],
            "curves": [{"id": "test:arc:curve#1", "geometry": {"kind":"solved", "value": geometry}}]
        });
        let model = wire["geometry"].as_object_mut().unwrap();
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
        let curve = &mut wire["geometry"]["curves"][0];
        let basis = curve["geometry"]["value"].clone();
        curve["geometry"]["value"] = json!({"kind": "transformed", "basis": basis,
            "transform": [[1.0, 0.0, 0.0, 3.0], [0.0, 1.0, 0.0, -4.0], [0.0, 0.0, 1.0, 2.0]]});
        wire["geometry"]["points"][0]["position"] = json!({"x": 5.0, "y": -4.0, "z": 2.0});
        wire["geometry"]["points"][1]["position"] = json!({"x": 3.0, "y": -2.0, "z": 2.0});
        let placed = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let sampled = sample_edge(&placed, "test:arc:edge#1").unwrap();
        assert!((sampled["interior"]["x"].as_f64().unwrap() - root - 3.0).abs() < 1e-12);
        assert!((sampled["interior"]["y"].as_f64().unwrap() - root + 4.0).abs() < 1e-12);
        assert_eq!(sampled["interior"]["z"], 2.0);
        wire["geometry"]["edges"][0]["carrier"]["param_range"] =
            json!([0.0, 2.0 * std::f64::consts::TAU]);
        wire["geometry"]["edges"][0]["ends"]["end"] = json!("test:arc:vertex#0");
        let ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(sample_edge(&ir, "test:arc:edge#1").is_none());
        wire["geometry"]["edges"][0]["carrier"]["param_range"] =
            json!([0.0, std::f64::consts::TAU]);
        wire["geometry"]["edges"][0]
            .as_object_mut()
            .unwrap()
            .remove("ends");
        let ring = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(ring.geometry.edges[0].vertices().next().is_none());
        assert!(sample_edge(&ring, "test:arc:edge#1").is_none());
    }

    #[test]
    fn missing_or_unsupported_carriers_are_not_sampled() {
        let ir = cube();
        let mut wire = serde_json::to_value(&ir).unwrap();
        let edge = wire["geometry"]["edges"][0].clone();
        wire["geometry"]["curves"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["id"] == edge["carrier"]["curve"])
            .unwrap()["geometry"] = json!({"kind": "solved", "value": {"kind": "unknown"}});
        let unsupported = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(sample_edge(&unsupported, edge["id"].as_str().unwrap()).is_none());
        wire["geometry"]["edges"][0]["carrier"] = json!({});
        let missing = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(sample_edge(&missing, edge["id"].as_str().unwrap()).is_none());
    }

    #[test]
    fn samples_follow_the_stored_body_placement() {
        let ir = cube();
        let id = ir.geometry.edges.first().unwrap().id.as_str();
        let before = sample_edge(&ir, id).unwrap();
        let mut wire = serde_json::to_value(&ir).unwrap();
        wire["geometry"]["bodies"][0]["transform"] = json!([
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
        wire["geometry"]["bodies"][0]["transform"] = json!([
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
        wire["geometry"]["bodies"][0]["transform"][0][0] = json!(2.0);
        let scaled = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        assert!(sample_edge(&scaled, id).is_none());
    }

    #[test]
    fn post_processed_blends_preserve_the_wrapped_selection() {
        let ir = cube();
        let mut wire = selection(&ir);
        let blend = &mut wire["model"].get_mut("features").unwrap()[1];
        let operation = blend["definition"]["value"].clone();
        blend["definition"] = json!({"kind": "post_process", "value": {"operation": operation}});
        preserve(&mut wire, &ir);
        assert_eq!(
            wire["model"]["features"][1]["definition"]["value"]["operation"]["value"]["groups"][0]
                ["edges"]["kind"],
            "historical"
        );
        assert_eq!(
            wire["model"]["feature_input_topologies"][0]["input_of"],
            "test:model:feature#blend"
        );
    }
}
