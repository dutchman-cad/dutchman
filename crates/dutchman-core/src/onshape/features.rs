use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

use super::{id, number, push, rows, sketch, text, topology, Mapping};
use crate::Report;

pub(super) fn parameter<'a>(feature: &'a Value, key: &str) -> Mapping<&'a Value> {
    let mut found = rows(feature, "parameters")?
        .iter()
        .filter(|p| p.get("parameterId").and_then(Value::as_str) == Some(key));
    let result = found
        .next()
        .ok_or_else(|| format!("parameter {key} is missing"))?;
    if found.next().is_some() {
        return Err(format!("parameter {key} repeats"));
    }
    Ok(result)
}

pub(super) fn value<'a>(feature: &'a Value, key: &str) -> Option<&'a str> {
    parameter(feature, key).ok()?.get("value")?.as_str()
}

fn enabled(feature: &Value, key: &str) -> bool {
    parameter(feature, key)
        .ok()
        .and_then(|p| p.get("value"))
        .and_then(Value::as_bool)
        == Some(true)
}

/// Quantity values are SI in REST; literal expressions retain explicit units. No
/// FeatureScript evaluation, implicit unit guessing, or string substitution occurs.
pub(super) fn quantity(parameter: &Value, angle: bool) -> Mapping<f64> {
    fn scale(unit: &str, angle: bool) -> Option<f64> {
        match (angle, unit) {
            (true, "radian" | "rad" | "") => Some(1.0),
            (true, "degree" | "deg") => Some(std::f64::consts::PI / 180.0),
            (false, "meter" | "m" | "") => Some(1000.0),
            (false, "millimeter" | "mm") => Some(1.0),
            (false, "centimeter" | "cm") => Some(10.0),
            (false, "inch" | "in") => Some(25.4),
            (false, "foot" | "ft") => Some(304.8),
            _ => None,
        }
    }
    if let Some(expression) = parameter.get("expression").and_then(Value::as_str) {
        let mut parts = expression.split_whitespace();
        if let (Some(Ok(value)), Some(unit), None) = (
            parts.next().map(str::parse::<f64>),
            parts.next(),
            parts.next(),
        ) {
            if let Some(factor) = scale(unit, angle) {
                let result = value * factor;
                if result.is_finite() {
                    return Ok(result);
                }
            }
        }
    }
    let factor = scale(
        parameter.get("units").and_then(Value::as_str).unwrap_or(""),
        angle,
    )
    .ok_or("quantity units are not supported yet")?;
    let result = number(parameter, "value")? * factor;
    if result.is_finite() {
        Ok(result)
    } else {
        Err("quantity exceeds the finite range".into())
    }
}

fn length(feature: &Value, key: &str) -> Mapping<f64> {
    let n =
        quantity(parameter(feature, key)?, false).map_err(|reason| format!("{key}: {reason}"))?;
    if n > 0.0 {
        Ok(n)
    } else {
        Err(format!("{key} must have a positive resolved length"))
    }
}

pub(super) fn queries<'a>(feature: &'a Value, key: &str) -> Mapping<&'a [Value]> {
    let list = rows(parameter(feature, key)?, "queries")?;
    if list.is_empty() {
        Err(format!("{key} has no resolved queries"))
    } else {
        Ok(list)
    }
}

pub(super) fn profile(feature: &Value, key: &str, model: &Model) -> Mapping<Value> {
    let query = queries(feature, key)?;
    let [query] = query else {
        return Err("multiple profile queries are not supported yet".into());
    };
    // A deterministic subset must never silently expand into the entire sketch.
    if query
        .get("deterministicIds")
        .and_then(Value::as_array)
        .is_some_and(|a| !a.is_empty())
    {
        return Err("deterministic sketch-region subsets are not supported yet".into());
    }
    if !matches!(
        query.get("btType").and_then(Value::as_str),
        Some(
            "BTMIndividualSketchRegionQuery-140"
                | "BTMIndividualSketchUniqueVerticesQuery-1472"
                | "BTMIndividualCreatedByQuery-137"
        )
    ) {
        return Err("the sketch query is not supported yet".into());
    }
    if query.get("filterInnerLoops").and_then(Value::as_bool) == Some(true) {
        return Err("filtered inner sketch regions are not supported yet".into());
    }
    let sketch_id = id("sketch", text(query, "featureId")?);
    if !model.sketches.iter().any(|s| s.id.as_str() == sketch_id) {
        return Err("the selected sketch has no resolved geometry and plane".into());
    }
    Ok(json!({"kind": "sketch", "value": sketch_id}))
}

fn operation(feature: &Value) -> Mapping<&'static str> {
    match value(feature, "operationType") {
        Some("NEW") => Ok("new_body"),
        Some("ADD") => Ok("join"),
        Some("REMOVE") => Ok("cut"),
        Some("INTERSECT") => Ok("intersect"),
        _ => Err("the Boolean operation is not supported yet".into()),
    }
}

pub(super) fn map(
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    model: &mut Model,
    report: &mut Report,
) -> Mapping<Value> {
    if feature
        .get("namespace")
        .and_then(Value::as_str)
        .is_some_and(|s| !s.is_empty())
    {
        return Err("custom FeatureScript features are not supported yet".into());
    }
    for key in [
        "hasSecondDirection",
        "hasDraft",
        "hasSecondDirectionDraft",
        "hasOffset",
        "isVariable",
        "isPartial",
        "allowEdgeOverflow",
        "symmetric",
    ] {
        if enabled(feature, key) {
            return Err(format!("{key} is not supported yet"));
        }
    }
    let scoped = if parameter(feature, "booleanScope")
        .ok()
        .and_then(|p| p.get("queries"))
        .and_then(Value::as_array)
        .is_some_and(|queries| !queries.is_empty())
    {
        let state = inputs.load(ordinal, feature)?;
        state.whole_body_scope(feature)?;
        Some(state)
    } else {
        None
    };
    match text(feature, "featureType")? {
        "newSketch" => sketch::map(feature, model, report),
        "extrude" => {
            if value(feature, "bodyType") != Some("SOLID") {
                return Err("non-solid extrudes are not supported yet".into());
            }
            let profile = profile(feature, "entities", model)?;
            let termination = match value(feature, "endBound") {
                Some("BLIND") => json!({"kind": "blind", "length": length(feature, "depth")?}),
                Some("THROUGH_ALL") => json!({"kind": "through_all"}),
                Some("UP_TO_SURFACE") => {
                    let state = scoped
                        .map(Ok)
                        .unwrap_or_else(|| inputs.load(ordinal, feature))?;
                    let face = state.selection(feature, "endBoundEntityFace", "faces")?;
                    state.require_planes(&face)?;
                    push(&mut model.feature_input_topologies, state.row.clone())?;
                    json!({"kind": "to_face", "face": face})
                }
                _ => return Err("this extrude bound is not supported yet".into()),
            };
            let direction = if enabled(feature, "oppositeDirection") {
                json!({"kind": "reversed_profile_normal"})
            } else {
                json!({"kind": "profile_normal"})
            };
            Ok(
                json!({"definition": "extrude", "profile": profile, "direction": direction,
                "start": {"kind": "profile_plane"}, "extent": {"kind": "one_sided", "side": {"termination": termination}}, "op": operation(feature)?, "solid": true}),
            )
        }
        "revolve" => {
            if value(feature, "bodyType") != Some("SOLID") {
                return Err("non-solid revolves are not supported yet".into());
            }
            let profile = profile(feature, "entities", model)?;
            let state = scoped
                .map(Ok)
                .unwrap_or_else(|| inputs.load(ordinal, feature))?;
            let axis = state.axis(feature, "axis")?;
            let angle = match value(feature, "revolveType") {
                Some("FULL") => std::f64::consts::TAU,
                Some("ONE_DIRECTION") => quantity(parameter(feature, "angle")?, true)?,
                _ => return Err("this revolve extent is not supported yet".into()),
            };
            let angle = if enabled(feature, "oppositeDirection") {
                -angle
            } else {
                angle
            };
            Ok(
                json!({"definition": "revolve", "construction": {"state": "resolved", "profile": profile, "axis": axis,
                "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": angle}}}, "op": operation(feature)?}),
            )
        }
        kind @ ("fillet" | "chamfer") => {
            if enabled(feature, "tangentPropagation") {
                return Err("tangent-chain expansion is not supported yet".into());
            }
            if kind == "fillet"
                && (value(feature, "crossSection") != Some("CIRCULAR")
                    || value(feature, "filletType").is_some_and(|s| s != "EDGE"))
            {
                return Err("only circular constant-radius edge fillets are supported yet".into());
            }
            if kind == "chamfer" && value(feature, "chamferType") != Some("EQUAL_OFFSETS") {
                return Err("only equal-distance chamfers are supported yet".into());
            }
            let state = scoped
                .map(Ok)
                .unwrap_or_else(|| inputs.load(ordinal, feature))?;
            let edges = state.selection(feature, "entities", "edges")?;
            state.require_edges(&edges)?;
            let mut group = json!({"edges": edges});
            let (key, spec) = if kind == "fillet" {
                (
                    "radius",
                    json!({"kind": "constant", "radius": length(feature, "radius")?}),
                )
            } else {
                (
                    "spec",
                    json!({"kind": "distance", "distance": length(feature, "width")?}),
                )
            };
            group
                .as_object_mut()
                .ok_or("blend group is missing")?
                .insert(key.into(), spec);
            push(&mut model.feature_input_topologies, state.row)?;
            Ok(json!({"definition": kind, "groups": [group]}))
        }
        "hole" => {
            if value(feature, "style") != Some("SIMPLE") || enabled(feature, "isTapped") {
                return Err("this hole style or thread is not supported yet".into());
            }
            if enabled(feature, "oppositeDirection") {
                return Err("reversed hole drilling is not supported yet".into());
            }
            let profile = profile(feature, "entities", model)?;
            let extent = match value(feature, "endStyle") {
                Some("THROUGH") | Some("THROUGH_ALL") => json!({"kind": "through_all"}),
                Some("BLIND") => {
                    if let Ok(tip) = parameter(feature, "tipAngle") {
                        if quantity(tip, true)? != std::f64::consts::PI {
                            return Err(
                                "blind holes with a drill point are not supported yet".into()
                            );
                        }
                    }
                    json!({"kind": "blind", "length": length(feature, "holeDepth")?})
                }
                _ => return Err("this hole end is not supported yet".into()),
            };
            Ok(
                json!({"definition": "hole", "profile": profile, "profile_filter": "points",
                "shape": {"diameter": length(feature, "holeDiameter")?, "construction": {"construction": "form", "kind": {"kind": "simple"}}},
                "extent": extent}),
            )
        }
        other => Err(format!("{other} features are not supported yet")),
    }
}
