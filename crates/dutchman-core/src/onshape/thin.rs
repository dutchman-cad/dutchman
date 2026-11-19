//! Thin-wall output in CADIR's typed design vocabulary. The model projection
//! retains the same feature identity, profile and extent for existing readers.
use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

use super::{features, id, rows, text, topology, Mapping};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn profile(profile: Value, model: &Model) -> Mapping<Value> {
    if profile.get("kind").and_then(Value::as_str) != Some("sketch") {
        return Ok(profile);
    }
    let sketch = text(&profile, "value")?;
    let mut entities = Vec::new();
    for entity in &model.sketch_entities {
        if entity.sketch.as_str() == sketch && !entity.construction {
            let value =
                serde_json::to_value(entity).map_err(|_| "the thin extrude curve is unreadable")?;
            if value.pointer("/geometry/kind").and_then(Value::as_str) != Some("point") {
                entities.push(entity.id().as_str());
            }
        }
    }
    Ok(json!({"kind":"sketch_entities","value":{"sketch":sketch,"entities":entities}}))
}

pub(super) fn feature(
    source: &Value,
    definition: &Value,
    model: &Model,
    inputs: &topology::Inputs<'_>,
    ordinal: usize,
    unresolved: &BTreeSet<String>,
) -> Mapping<Value> {
    if features::value(source, "domain") != Some("MODEL") {
        return Err("the thin extrusion domain is missing or not MODEL".into());
    }
    // The writer's all-targets projection is exact only for this proven sole body.
    // NEW has no Boolean targets. Never infer a scope from the final STEP bodies.
    if features::value(source, "operationType") != Some("NEW") {
        inputs.load(ordinal, source)?.whole_body_scope(source)?;
    }
    let midplane = features::required_boolean(source, "midplane")?;
    let flipped = if midplane {
        false
    } else {
        features::required_boolean(source, "flipWall")?
    };
    features::required_boolean(source, "oppositeDirection")?;
    for key in ["hasExtrudeDirection", "startOffset"] {
        if features::enabled(source, key)? {
            return Err(format!("thin extrusion {key} is not supported yet"));
        }
    }
    let extrude = definition
        .pointer("/value/value")
        .ok_or("the thin extrude definition is missing")?;
    let profile = extrude
        .pointer("/profile/value/value")
        .ok_or("the thin extrude needs explicit sketch curves")?;
    let entities = rows(profile, "entities")?;
    let mut picks = Vec::new();
    for entity in entities {
        picks.push(json!({"target":{"kind":"sketch_entity","value":entity}}));
    }
    if picks.is_empty() {
        return Err("the thin extrude has no selected sketch curves".into());
    }
    let sketch = text(profile, "sketch")?;
    let mut reversed = None;
    let queries = rows(features::parameter(source, "wallShape")?, "queries")?
        .iter()
        .map(super::query::resolved)
        .collect::<Mapping<Vec<_>>>()?;
    let mut sources = BTreeMap::new();
    for query in &queries {
        let [operation] = rows(query, "operationId")? else {
            return Err("the thin curve has multiple source operations".into());
        };
        let native_sketch = operation
            .as_str()
            .and_then(|s| s.strip_suffix(".wireOp"))
            .ok_or("the thin curve source operation is missing")?;
        let native_entity = text(query, "sketchEntityId")?;
        if sources
            .insert(
                id("entity", &format!("{native_sketch}/{native_entity}")),
                (native_sketch, native_entity),
            )
            .is_some()
        {
            return Err("the thin profile repeats a stored directed curve selection".into());
        }
    }
    for selected in entities {
        let selected = selected.as_str().ok_or("a thin curve id is missing")?;
        let entity = model
            .sketch_entities
            .iter()
            .find(|e| e.id().as_str() == selected)
            .ok_or("a thin curve has no solved geometry")?;
        if entity.sketch.as_str() != sketch || entity.construction {
            return Err("the thin profile selects another sketch or construction geometry".into());
        }
        let (native_sketch, native_entity) = sources
            .get(selected)
            .ok_or("the thin curve has no stored directed sketch selection")?;
        let source_entity = inputs.sketch_entity(native_sketch, native_entity)?;
        let geometry = source_entity
            .get("geometry")
            .ok_or("the thin curve direction is missing")?;
        let clockwise = super::boolean_field(geometry, "clockwise")?;
        if reversed.is_some_and(|previous| previous != clockwise) && !midplane {
            return Err("mixed thin curve directions cannot share one stored wall side yet".into());
        }
        reversed = Some(clockwise);
    }
    // Circle/ellipse canonicalization reverses clockwise source curves. Preserve
    // the stored side relative to those curves; no area chooses the wall side.
    let flipped = flipped ^ reversed.unwrap_or(false);
    let length = |key| -> Mapping<f64> {
        let n = features::quantity(features::parameter(source, key)?, false, model, unresolved)?;
        if n < 0.0 {
            return Err(format!("thin extrusion {key} must not be negative"));
        }
        Ok(n)
    };
    let literal = |value| json!({"kind":"literal","value":value});
    let thickness = if midplane {
        let thickness = length("thickness")?;
        if thickness == 0.0 {
            return Err("the thin extrusion has no positive wall thickness".into());
        }
        json!({"kind":"symmetric","value":literal(thickness)})
    } else {
        let first = length("thickness1")?;
        let second = length("thickness2")?;
        if first == 0.0 && second == 0.0 {
            return Err("the thin extrusion has no positive wall thickness".into());
        }
        if second == 0.0 {
            json!({"kind":"one_side","value":{"value":literal(first),"side":{"kind":if flipped {"inside"} else {"outside"}}}})
        } else {
            let (outside, inside) = if flipped {
                (second, first)
            } else {
                (first, second)
            };
            json!({"kind":"offsets","value":{"outside":literal(outside),"inside":literal(inside)}})
        }
    };
    let extent = extrude
        .get("extent")
        .ok_or("the thin extrude extent is missing")?;
    let termination = extent
        .pointer("/side/termination")
        .ok_or("the thin extrude termination is missing")?;
    let limit = match text(termination, "kind")? {
        "blind" => json!({"kind":"amount","value":literal(super::number(termination,"length")?)}),
        "through_all" => json!({"kind":"through_all"}),
        _ => return Err("thin extrudes with this bound are not supported yet".into()),
    };
    let span = if text(extent, "kind")? == "symmetric" {
        json!({"kind":"symmetric","amount":literal(super::number(termination,"length")?),"measure":"total"})
    } else {
        json!({"kind":"between","start":{"kind":"at_origin"},"end":limit})
    };
    let op = text(extrude, "op")?;
    let boolean = if op == "new_body" {
        json!({"kind":"new_body"})
    } else {
        json!({"kind":"apply","op":{"kind":op},"targets":{"kind":"all"}})
    };
    Ok(
        json!({"id":id("feature",text(source,"featureId")?),"definition":{"operation":{"kind":"extrude","value":{
        "profile":{"at":{"kind":"input"},"picks":{"kind":"named","value":picks}},
        "direction":{"kind":"profile_normal","reversed":{"kind":"literal","value":extrude.pointer("/direction/kind").and_then(Value::as_str)==Some("reversed_profile_normal")}},
        "origin":{"kind":"profile_plane"},"span":span,
        "output":{"body":{"kind":"thin","thickness":thickness},"boolean":boolean},"native_options":{},"source_settings":{}}}}}),
    )
}
