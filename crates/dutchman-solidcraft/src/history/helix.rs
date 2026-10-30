//! Constant-radius helical sweeps, using SolidCraft's sketch-profile coil.

use dutchman_core::{Report, Severity};
use serde_json::{json, Value};

use super::extrude::{body_names, operation};
use super::profile::profile;
use super::sketch::{number, unit, v3};
use super::{name, Context};

pub(crate) fn map(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    report: &mut Report,
) -> Result<Value, String> {
    if !crate::VersionGate::SketchProfileCoil.supported(ctx.target_version) {
        return Err(format!(
            "needs SolidCraft 0.25 patch 1 or newer (target {})",
            ctx.target_version.minor()
        ));
    }
    let construction = definition
        .get("construction")
        .ok_or("its helical construction is not resolved")?;
    let base = v3(construction.get("axis_origin")).ok_or("its helix axis origin is malformed")?;
    let axis = v3(construction.get("axis_direction"))
        .and_then(unit)
        .ok_or("its helix axis direction is malformed")?;
    // Stored principal axes can carry rotation round-off. Keep those exact in SolidCraft.
    let axis = unit(axis.map(|v| if v.abs() < 1e-12 { 0.0 } else { v }))
        .ok_or("its helix axis direction is malformed")?;
    let travel = construction
        .get("travel")
        .ok_or("its helix travel is missing")?;
    if travel.get("kind").and_then(Value::as_str) != Some("axial")
        || number(construction.get("cone_angle")) != Some(0.0)
    {
        return Err(
            "conical and spiral helical sweeps are not supported yet: SolidCraft's sketch-profile coil has no radial growth or cone angle".to_owned(),
        );
    }
    let positive = |value| number(value).filter(|v| *v > 0.0);
    let pitch = positive(construction.get("pitch")).ok_or("its helix pitch is malformed")?;
    let height = number(travel.get("height"))
        .filter(|v| *v != 0.0)
        .ok_or("its helix height is malformed")?;
    let turns = positive(construction.get("turns")).ok_or("its helix turns are malformed")?;
    let flag = |key| {
        construction
            .get(key)
            .and_then(Value::as_bool)
            .ok_or_else(|| format!("its helix {key} flag is missing"))
    };
    let reversed = flag("reversed")?;
    let left_handed = flag("left_handed")?;
    let (sketch_id, profiles) = profile(construction.get("profile"), ctx)?;
    let sketch = ctx
        .sketches
        .get(&sketch_id)
        .ok_or("its helix profile is not an earlier mapped sketch")?
        .feature;
    let pitch_expr = dimension(feature, ctx, "Pitch", pitch, "length", " mm");
    let height_expr = dimension(feature, ctx, "Height", height.abs(), "length", " mm");
    let turns_expr = dimension(feature, ctx, "Turns", turns, "real", "");
    let independent = match construction.get("law").and_then(Value::as_str) {
        Some("pitch_height_angle") => [&pitch_expr, &height_expr],
        Some("pitch_turns_angle") => [&pitch_expr, &turns_expr],
        _ => [&height_expr, &turns_expr],
    };
    if independent
        .iter()
        .any(|expr| !ctx.params.values().any(|name| name == *expr))
    {
        report.push(
            Severity::Degraded,
            name(feature).unwrap_or("Helix"),
            "helix dimensions without feature parameter bindings kept as their resolved values",
        );
    }
    let (pitch_expr, turns_expr) = match construction.get("law").and_then(Value::as_str) {
        Some("pitch_height_angle") => (
            pitch_expr.clone(),
            format!("({height_expr}) / ({pitch_expr})"),
        ),
        Some("pitch_turns_angle") => (pitch_expr, turns_expr),
        Some("height_turns_angle" | "height_turns_growth") => {
            (format!("({height_expr}) / ({turns_expr})"), turns_expr)
        }
        _ => return Err("its helix parameter law is not supported yet".to_owned()),
    };
    let operation = operation(definition.get("op"))?;
    let names = body_names(feature, ctx, operation);
    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Helix{id}")),
        "type": "coil", "suppressed": false,
        "sketch": sketch, "profiles": profiles,
        "base": base, "axis": if reversed != (height < 0.0) { axis.map(|v| -v) } else { axis },
        "pitch": pitch_expr, "turns": turns_expr,
        "clockwise": left_handed,
        "operation": operation,
    });
    if let (Some(out), Some(names)) = (out.as_object_mut(), names) {
        out.insert("body_names".to_owned(), json!(names));
    }
    Ok(out)
}

/// Bind only a uniquely named property of this feature with the resolved value and unit.
/// Spreadsheet expressions use the ordinary parameter mapper's evaluated-value fallback.
fn dimension(
    feature: &Value,
    ctx: &Context,
    property: &str,
    value: f64,
    kind: &str,
    suffix: &str,
) -> String {
    let owner = feature.get("id").and_then(Value::as_str);
    let mut matching = ctx.arena("parameters").iter().filter(|p| {
        owner.is_some()
            && p.get("owner").and_then(Value::as_str) == owner
            && p.get("name")
                .and_then(Value::as_str)
                .is_some_and(|n| n.eq_ignore_ascii_case(property))
            && p.pointer("/value/kind").and_then(Value::as_str) == Some(kind)
            && number(p.get("value")) == Some(value)
    });
    if let Some(parameter) = matching.next() {
        if matching.next().is_none() {
            if let Some(name) = parameter
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| ctx.params.get(id))
            {
                return name.clone();
            }
        }
    }
    format!("{value}{suffix}")
}
