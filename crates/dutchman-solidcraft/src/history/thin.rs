//! A typed design-layer thin output paired with its model-history projection.
//! No native-format options cross the writer seam.
use serde_json::{json, Value};

use super::{profile::planar, Context};

pub(super) fn map(
    feature: &Value,
    model: &Value,
    ctx: &Context,
) -> Result<Option<(String, Value)>, String> {
    let Some(id) = feature.get("id").and_then(Value::as_str) else {
        return Ok(None);
    };
    let mut records = ctx
        .design
        .get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|f| f.get("id").and_then(Value::as_str) == Some(id));
    let Some(record) = records.next() else {
        return Ok(None);
    };
    let refused="the typed extrusion and its model-history projection disagree; this thin extrude is not supported yet";
    if records.next().is_some() {
        return Err(refused.to_owned());
    }
    let op = record.pointer("/definition/operation").ok_or(refused)?;
    if op.get("kind").and_then(Value::as_str) != Some("extrude") {
        return Err(refused.to_owned());
    }
    let design = op.get("value").ok_or(refused)?;
    let body = design.pointer("/output/body").ok_or(refused)?;
    if body.get("kind").and_then(Value::as_str) != Some("thin") {
        return Err(refused.to_owned());
    }
    if !crate::VersionGate::ThinExtrude.supported(ctx.target_version) {
        return Err(
            "thin extrudes need SolidCraft 0.44 or newer; kept the final geometry".to_owned(),
        );
    }
    for field in ["draft", "fit", "outside", "sheet_cut"] {
        if design.get(field).is_some() {
            return Err(refused.to_owned());
        }
    }
    for field in ["native_options", "source_settings"] {
        if design
            .get(field)
            .and_then(Value::as_object)
            .is_none_or(|v| !v.is_empty())
        {
            return Err(refused.to_owned());
        }
    }
    if design.pointer("/origin/kind").and_then(Value::as_str) != Some("profile_plane")
        || model.pointer("/start/kind").and_then(Value::as_str) != Some("profile_plane")
        || design.pointer("/direction/kind").and_then(Value::as_str) != Some("profile_normal")
        || design.pointer("/direction/reversed")
            != Some(
                &json!({"kind":"literal","value":model.pointer("/direction/kind").and_then(Value::as_str)==Some("reversed_profile_normal")}),
            )
    {
        return Err(refused.to_owned());
    }
    let extent = model.get("extent").ok_or(refused)?;
    let termination = extent.pointer("/side/termination").ok_or(refused)?;
    let literal = |value: &Value| json!({"kind":"literal","value":value});
    let end = match termination.get("kind").and_then(Value::as_str) {
        Some("blind") => {
            json!({"kind":"amount","value":literal(termination.get("length").ok_or(refused)?)})
        }
        Some("through_all") => json!({"kind":"through_all"}),
        _ => return Err(refused.to_owned()),
    };
    let span = match extent.get("kind").and_then(Value::as_str) {
        Some("one_sided") => {
            json!({"kind":"between","start":{"kind":"at_origin"},"end":end})
        }
        Some("symmetric") => {
            json!({"kind":"symmetric","amount":literal(termination.get("length").ok_or(refused)?),"measure":"total"})
        }
        _ => return Err(refused.to_owned()),
    };
    let boolean = match model.get("op").and_then(Value::as_str) {
        Some("new_body") => json!({"kind":"new_body"}),
        Some(op @ ("join" | "cut" | "intersect")) => {
            json!({"kind":"apply","op":{"kind":op},"targets":{"kind":"all"}})
        }
        _ => return Err(refused.to_owned()),
    };
    if design.get("span") != Some(&span) || design.pointer("/output/boolean") != Some(&boolean) {
        return Err(refused.to_owned());
    }
    let profile = planar(model.get("profile").ok_or(refused)?);
    if profile.get("kind").and_then(Value::as_str) != Some("sketch_entities") {
        return Err(refused.to_owned());
    }
    let sketch = profile
        .pointer("/value/sketch")
        .and_then(Value::as_str)
        .ok_or(refused)?;
    let entities = profile
        .pointer("/value/entities")
        .and_then(Value::as_array)
        .ok_or(refused)?;
    let picks: Vec<_> = entities
        .iter()
        .map(|e| json!({"target":{"kind":"sketch_entity","value":e}}))
        .collect();
    if design.get("profile")
        != Some(&json!({"at":{"kind":"input"},"picks":{"kind":"named","value":picks}}))
    {
        return Err(refused.to_owned());
    }
    let mapped = ctx
        .sketches
        .get(sketch)
        .ok_or("the thin extrude sketch is not mapped yet")?;
    let mut curves = Vec::new();
    for entity in entities {
        let curve = mapped
            .curve(entity.as_str().ok_or(refused)?)
            .ok_or("the thin extrude curve is not mapped yet")?;
        curves.extend(curve.ids());
    }
    if curves.is_empty() {
        return Err("the thin extrude selects no curves".to_owned());
    }
    if body.get("ends").is_some() || body.get("corner_round").is_some() {
        return Err("thin-wall end and corner treatments are not supported yet".to_owned());
    }
    let thickness = body.get("thickness").ok_or(refused)?;
    let mm = |value: Option<&Value>, positive: bool| -> Result<String, String> {
        let value = value
            .filter(|v| v.get("kind").and_then(Value::as_str) == Some("literal"))
            .and_then(|v| v.get("value"))
            .and_then(Value::as_f64)
            .filter(|v| v.is_finite() && if positive { *v > 0.0 } else { *v >= 0.0 })
            .ok_or("the thin-wall thickness has no supported literal length")?;
        Ok(format!("{value} mm"))
    };
    let mut thin = json!({"curves":curves});
    let fields = thin.as_object_mut().ok_or(refused)?;
    match thickness.get("kind").and_then(Value::as_str) {
        Some("symmetric") => {
            fields.insert("thickness".into(), json!(mm(thickness.get("value"), true)?));
        }
        Some("one_side") => {
            let side = thickness
                .pointer("/value/side/kind")
                .and_then(Value::as_str)
                .ok_or("the thin-wall side is not resolved")?;
            if !matches!(side, "inside" | "outside") {
                return Err("directional left/right thin walls are not supported yet".to_owned());
            }
            fields.insert("side".into(), json!(side));
            fields.insert(
                "thickness".into(),
                json!(mm(thickness.pointer("/value/value"), true)?),
            );
        }
        Some("offsets") => {
            fields.insert("side".into(), json!("two_sides"));
            fields.insert(
                "thickness".into(),
                json!(mm(thickness.pointer("/value/inside"), false)?),
            );
            fields.insert(
                "thickness2".into(),
                json!(mm(thickness.pointer("/value/outside"), false)?),
            );
        }
        _ => return Err("this thin-wall thickness is not supported yet".to_owned()),
    }
    Ok(Some((sketch.to_owned(), thin)))
}
