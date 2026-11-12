use std::collections::BTreeMap;

use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

use super::{
    coordinate,
    features::{parameter, quantity, queries},
    id, number, optional_rows, push, rows, text, topology, Mapping,
};
use crate::{Report, Severity};

/// Built-in planes have a fixed source frame. Other face attachments cannot supply
/// the sketch's in-plane rotation from bodydetails alone and are refused explicitly.
fn frame(feature: &Value) -> Mapping<Value> {
    let [query] = queries(feature, "sketchPlane")? else {
        return Err("multiple sketch planes are not supported yet".into());
    };
    if query
        .get("deterministicIds")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty())
    {
        return Err(
            "a deterministic sketch-plane attachment has no resolved in-plane frame".into(),
        );
    }
    let plane = if let Some(feature_id) = query.get("featureId").and_then(Value::as_str) {
        feature_id.to_owned()
    } else {
        let source_query = text(query, "queryString")?;
        let query_string = source_query
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>();
        ["Top", "Front", "Right"]
            .into_iter()
            .find(|name| {
                source_query.contains(&format!("\"{name}\""))
                    && query_string.trim_end_matches(';')
                        == format!("qCreatedBy(makeId(\"{name}\"),EntityType.FACE)")
            })
            .ok_or("the sketch plane query is not supported yet")?
            .to_owned()
    };
    let (normal, u_axis) = match plane.as_str() {
        "Top" => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        "Front" => ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
        "Right" => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        _ => return Err("this sketch plane has no resolved in-plane frame".into()),
    };
    Ok(
        json!({"origin": topology::point([0.0; 3]), "normal": topology::point(normal), "u_axis": topology::point(u_axis)}),
    )
}

fn point(u: f64, v: f64) -> Value {
    json!({"u": u, "v": v})
}

fn geometry(entity: &Value) -> Mapping<Value> {
    if entity.get("btType").and_then(Value::as_str) == Some("BTMSketchPoint-158") {
        return Ok(
            json!({"kind": "point", "position": point(coordinate(entity, "x")? * 1000.0, coordinate(entity, "y")? * 1000.0)}),
        );
    }
    let geometry = entity
        .get("geometry")
        .ok_or("a sketch entity has no solved geometry")?;
    match text(geometry, "btType")? {
        "BTCurveGeometryLine-117" => {
            if entity.get("btType").and_then(Value::as_str) != Some("BTMSketchCurveSegment-155") {
                return Err("unbounded sketch lines are not supported yet".into());
            }
            let x = coordinate(geometry, "pntX")?;
            let y = coordinate(geometry, "pntY")?;
            let dx = coordinate(geometry, "dirX")?;
            let dy = coordinate(geometry, "dirY")?;
            let start = coordinate(entity, "startParam")?;
            let end = coordinate(entity, "endParam")?;
            Ok(
                json!({"kind": "line", "start": point((x + start * dx) * 1000.0, (y + start * dy) * 1000.0),
                "end": point((x + end * dx) * 1000.0, (y + end * dy) * 1000.0)}),
            )
        }
        "BTCurveGeometryCircle-115" => {
            let center = point(
                coordinate(geometry, "xCenter")? * 1000.0,
                coordinate(geometry, "yCenter")? * 1000.0,
            );
            let radius = number(geometry, "radius")? * 1000.0;
            if entity.get("btType").and_then(Value::as_str) == Some("BTMSketchCurve-4") {
                return Ok(json!({"kind": "circle", "center": center, "radius": radius}));
            }
            let direction = coordinate(geometry, "yDir")?.atan2(coordinate(geometry, "xDir")?);
            let sign = if geometry.get("clockwise").and_then(Value::as_bool) == Some(true) {
                -1.0
            } else {
                1.0
            };
            let a = direction + sign * coordinate(entity, "startParam")?;
            let b = direction + sign * coordinate(entity, "endParam")?;
            // CADIR arcs traverse counterclockwise. Preserve native endpoints separately
            // for constraints, swapping their loci when the source arc runs clockwise.
            let (a, b) = if sign < 0.0 { (b, a) } else { (a, b) };
            Ok(
                json!({"kind": "arc", "center": center, "radius": radius, "start_angle": a, "end_angle": b}),
            )
        }
        "BTCurveGeometrySpline-118" | "BTCurveGeometryControlPointSpline-2197" => {
            if entity.get("btType").and_then(Value::as_str) != Some("BTMSketchCurve-4") {
                return Err("trimmed sketch splines are not supported yet".into());
            }
            if geometry.get("isRational").and_then(Value::as_bool) == Some(true)
                || geometry.get("isPeriodic").and_then(Value::as_bool) == Some(true)
                || geometry.get("dimension").and_then(Value::as_u64) != Some(2)
            {
                return Err(
                    "rational, periodic, or non-planar sketch splines are not supported yet".into(),
                );
            }
            let controls = rows(geometry, "controlPoints")?;
            if controls.len() % 2 != 0
                || geometry.get("controlPointCount").and_then(Value::as_u64)
                    != Some((controls.len() / 2) as u64)
            {
                return Err("spline control point count does not match its coordinates".into());
            }
            let mut points = Vec::new();
            for [u, v] in controls.as_chunks::<2>().0 {
                points.push(point(
                    u.as_f64().ok_or("spline coordinate is missing")? * 1000.0,
                    v.as_f64().ok_or("spline coordinate is missing")? * 1000.0,
                ));
            }
            Ok(
                json!({"kind": "nurbs", "curve": {"degree": geometry.get("degree"), "knots": rows(geometry, "knots")?,
                "poles": {"form": "polynomial", "points": points}, "periodic": false}}),
            )
        }
        other => Err(format!("{other} sketch geometry is not supported yet")),
    }
}

pub(super) fn map(feature: &Value, model: &mut Model, report: &mut Report) -> Mapping<Value> {
    if feature.get("btType").and_then(Value::as_str) != Some("BTMSketch-151") {
        return Err("only planar sketches are supported yet".into());
    }
    let native = text(feature, "featureId")?;
    let sketch_id = id("sketch", native);
    let plane = frame(feature)?;
    let mut loci = BTreeMap::new();
    for entity in rows(feature, "entities")? {
        let native_entity = text(entity, "entityId")?;
        let entity_id = id("entity", &format!("{native}/{native_entity}"));
        if loci
            .insert(
                native_entity.to_owned(),
                json!({"kind": "entity", "entity": entity_id}),
            )
            .is_some()
        {
            return Err("sketch entity ids repeat".into());
        }
        let clockwise = entity
            .pointer("/geometry/clockwise")
            .and_then(Value::as_bool)
            == Some(true);
        for (key, kind) in [
            ("startPointId", if clockwise { "end" } else { "start" }),
            ("endPointId", if clockwise { "start" } else { "end" }),
            ("centerId", "center"),
        ] {
            if let Some(endpoint) = entity
                .get(key)
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                if loci
                    .insert(
                        endpoint.to_owned(),
                        json!({"kind": kind, "entity": entity_id}),
                    )
                    .is_some()
                {
                    return Err("sketch endpoint ids repeat".into());
                }
            }
        }
        push(
            &mut model.sketch_entities,
            json!({"id": entity_id, "sketch": sketch_id,
            "construction": entity.get("isConstruction").and_then(Value::as_bool).unwrap_or(false), "geometry": geometry(entity)?}),
        )?;
    }
    push(
        &mut model.sketches,
        json!({"id": sketch_id, "name": feature.get("name"), "placement": {"kind": "resolved", "frame": plane}, "profiles": []}),
    )?;
    for constraint in optional_rows(feature, "constraints")? {
        let parameter_count = model.parameters.len();
        let result = constraint_row(constraint, native, &sketch_id, &loci, model)
            .and_then(|row| push(&mut model.sketch_constraints, row));
        if let Err(reason) = result {
            model.parameters.truncate(parameter_count);
            report.push_coded(
                Severity::Degraded,
                feature
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(native),
                "onshape/sketch.constraint-not-supported",
                format!("{reason}; kept the solved sketch geometry"),
            );
        }
    }
    Ok(json!({"definition": "sketch", "sketch": {"space": "planar", "sketch": sketch_id}}))
}

fn constraint_row(
    constraint: &Value,
    native: &str,
    sketch_id: &str,
    loci: &BTreeMap<String, Value>,
    model: &mut Model,
) -> Mapping<Value> {
    let kind = text(constraint, "constraintType")?;
    let cid = id(
        "constraint",
        &format!("{native}/{}", text(constraint, "entityId")?),
    );
    let locus = |key: &str| -> Mapping<Value> {
        let operand = parameter(constraint, key)?
            .get("value")
            .and_then(Value::as_str)
            .ok_or("a constraint operand is missing")?;
        loci.get(operand)
            .cloned()
            .ok_or_else(|| format!("constraint operand {operand} has no local geometry"))
    };
    let entity = |key: &str| -> Mapping<Value> {
        let locus = locus(key)?;
        if locus.get("kind").and_then(Value::as_str) != Some("entity") {
            return Err("an endpoint constraint needs explicit loci".into());
        }
        locus
            .get("entity")
            .cloned()
            .ok_or("constraint entity is missing".into())
    };
    let definition = match kind {
        "HORIZONTAL" | "VERTICAL" | "FIX" => {
            json!({"kind": if kind == "FIX" { "fixed" } else if kind == "HORIZONTAL" { "horizontal" } else { "vertical" }, "entity": entity("localFirst")?})
        }
        "PARALLEL" | "PERPENDICULAR" | "TANGENT" | "EQUAL" | "CONCENTRIC" => {
            json!({"kind": kind.to_ascii_lowercase(), "first": entity("localFirst")?, "second": entity("localSecond")?})
        }
        "COINCIDENT" => {
            json!({"kind": "coincident_loci", "loci": [locus("localFirst")?, locus("localSecond")?]})
        }
        "LENGTH" | "DISTANCE" | "RADIUS" | "DIAMETER" | "ANGLE" => {
            if constraint.get("drivenDimension").and_then(Value::as_bool) == Some(true) {
                return Err("driven sketch dimensions are not supported yet".into());
            }
            let key = match kind {
                "LENGTH" => "length",
                "DISTANCE" => "distance",
                "RADIUS" => "radius",
                "DIAMETER" => "diameter",
                _ => "angle",
            };
            let dimension = parameter(constraint, key)?;
            let value = quantity(dimension, kind == "ANGLE")?;
            let pid = id(
                "parameter",
                &format!("{native}/{}", text(constraint, "entityId")?),
            );
            let parameter_ordinal = model.parameters.len();
            push(
                &mut model.parameters,
                json!({"id": pid, "owner": id("feature", native), "name": format!("dimension{parameter_ordinal}"),
                "expression": format!("{value} {}", if kind == "ANGLE" { "rad" } else { "mm" }),
                "value": {"kind": if kind == "ANGLE" { "angle" } else { "length" }, "value": value}}),
            )?;
            match kind {
                "LENGTH" => {
                    json!({"kind": "distance", "entities": [entity("localFirst")?], "parameter": pid})
                }
                "DISTANCE" => {
                    json!({"kind": "distance_loci", "first": locus("localFirst")?, "second": locus("localSecond")?, "parameter": pid})
                }
                "ANGLE" => {
                    json!({"kind": "angle", "first": entity("localFirst")?, "second": entity("localSecond")?, "parameter": pid})
                }
                _ => {
                    json!({"kind": kind.to_ascii_lowercase(), "entity": entity("localFirst")?, "parameter": pid})
                }
            }
        }
        _ => return Err(format!("{kind} sketch constraints are not supported yet")),
    };
    Ok(json!({"id": cid, "sketch": sketch_id, "definition": definition}))
}
