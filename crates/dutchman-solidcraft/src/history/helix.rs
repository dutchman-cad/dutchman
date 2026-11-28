//! Constant-radius helical sweeps, using SolidCraft's sketch-profile coil.

use dutchman_core::{Report, Severity};
use serde_json::{json, Value};

use super::extrude::{body_names, operation};
use super::profile::profile;
use super::sketch::{cross, dot, number, p2, synthesized, unit, v3, Mapped, V3};
use super::{name, Context};

pub(crate) fn map(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    report: &mut Report,
    preceding: &mut [Value],
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
    let signed_axis = if reversed != (height < 0.0) {
        axis.map(|v| -v)
    } else {
        axis
    };
    let (sketch_id, profiles) = profile(construction.get("profile"), ctx)?;
    let mapped = ctx
        .sketches
        .get(&sketch_id)
        .ok_or("its helix profile is not an earlier mapped sketch")?
        .clone();
    let (sketch, moved) = axial_sketch(
        &sketch_id,
        &mapped,
        base,
        signed_axis,
        left_handed,
        pitch,
        feature,
        ctx,
        preceding,
    )?;
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
        "base": base, "axis": signed_axis,
        "pitch": pitch_expr, "turns": turns_expr,
        "clockwise": left_handed,
        "operation": operation,
    });
    if let (Some(out), Some(names)) = (out.as_object_mut(), names) {
        out.insert("body_names".to_owned(), json!(names));
    }
    if moved {
        report.push_coded(
            Severity::Info,
            name(feature).unwrap_or("Helix"),
            "history/coil-profile-moved",
            "coil profile moved into the axis plane for SolidCraft 0.51; its sketch dimensions were not kept",
        );
    }
    Ok(out)
}

/// The sketch the coil takes its profile from. SolidCraft needs the profile on a plane through
/// the coil's axis, while a source can sketch it on a plane across the axis: a section normal
/// to the helix, say, which is tilted by the helix's lead angle. The profile is then carried
/// into the axial plane through its points along the helix's tangent there. A helical sweep
/// of the carried profile fills the same tube as the sweep of the original, whose section
/// along the tangent is the same curve stretched by the lead angle. A sketch only this coil
/// uses is rewritten in place; a shared one is copied.
#[allow(clippy::too_many_arguments)]
fn axial_sketch(
    sketch_id: &str,
    mapped: &Mapped,
    base: V3,
    axis: V3,
    left: bool,
    pitch: f64,
    feature: &Value,
    ctx: &mut Context,
    preceding: &mut [Value],
) -> Result<(u64, bool), String> {
    let sub = |[a0, a1, a2]: V3, [b0, b1, b2]: V3| [a0 - b0, a1 - b1, a2 - b2];
    let lin = |[p0, p1, p2]: V3, [q0, q1, q2]: V3, k: f64| [p0 + k * q0, p1 + k * q1, p2 + k * q2];
    if dot(mapped.normal, axis).abs() <= 1e-9
        && dot(mapped.normal, sub(mapped.origin, base)).abs() <= 1e-7
    {
        return Ok((mapped.feature, false));
    }
    // The profile's own points (circle centers and line ends of curves that are not
    // construction); a sketch's datum point and guides are not part of it.
    let mut own: Vec<[f64; 2]> = Vec::new();
    for entity in super::profile::sketch_entities(ctx, sketch_id) {
        if entity.get("construction").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let geometry = entity.get("geometry").unwrap_or(&Value::Null);
        for key in ["center", "start", "end", "position"] {
            if let Some(at) = p2(geometry.get(key)) {
                own.push(at);
            }
        }
    }
    let count = own.len().max(1) as f64;
    let mut center = [0.0; 2];
    for at in &own {
        center[0] += at[0] / count;
        center[1] += at[1] / count;
    }
    let pivot = lin(
        lin(mapped.origin, mapped.x_dir, center[0]),
        mapped.y_dir,
        center[1],
    );
    let rel = sub(pivot, base);
    let side = lin(rel, axis, -dot(rel, axis));
    let radius = dot(side, side).sqrt();
    let radial = unit(side)
        .filter(|_| radius > 1e-9)
        .ok_or("its profile lies on the coil's axis, so it has no axial plane to move to yet")?;
    // The plane through the axis and the profile, and the helix's tangent in it.
    let normal = cross(axis, radial);
    let around = 2.0 * std::f64::consts::PI * radius;
    // SolidCraft turns a coil right-handed about its axis, left-handed when `clockwise`.
    let turning = if left { -around } else { around };
    let tangent = unit(lin(normal.map(|v| v * turning), axis, pitch))
        .ok_or("its helix tangent is malformed")?;
    let carry = |v: V3| lin(v, tangent, -dot(v, normal) / dot(tangent, normal));
    let (ix, iy) = (carry(mapped.x_dir), carry(mapped.y_dir));
    let x = unit(ix)
        .or_else(|| unit(iy))
        .ok_or("its profile plane holds the helix's tangent")?;
    let mut y = cross(normal, x);
    let mut matrix = [[dot(x, ix), dot(x, iy)], [dot(y, ix), dot(y, iy)]];
    let mut det = matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0];
    if det < 0.0 {
        y = y.map(|v| -v);
        matrix[1] = matrix[1].map(|v| -v);
        det = -det;
    }
    if det <= 1e-9 {
        return Err("its profile plane holds the helix's tangent".to_owned());
    }
    let origin = lin(pivot, carry(sub(mapped.origin, pivot)), 1.0);
    let mut entities = Vec::new();
    for entity in super::profile::sketch_entities(ctx, sketch_id) {
        match affine(entity, matrix) {
            Some(carried) => entities.push(carried),
            // A construction guide of a kind that cannot be carried bounds no profile.
            None if entity.get("construction").and_then(Value::as_bool) == Some(true) => {}
            None => {
                return Err(
                    "its profile is on a plane across the coil's axis, and a curve of it cannot \
                     be carried into the axial plane yet"
                        .to_owned(),
                )
            }
        }
    }
    let uses = ctx
        .arena("features")
        .iter()
        .filter(|row| row.to_string().contains(&format!("\"{sketch_id}\"")))
        .count();
    let in_place = uses <= 2;
    let id = if in_place { mapped.feature } else { ctx.id() };
    let title = if in_place {
        preceding
            .iter()
            .find(|row| row.get("id").and_then(Value::as_u64) == Some(id))
            .and_then(name)
            .map(str::to_owned)
    } else {
        None
    }
    .unwrap_or_else(|| format!("{} profile", name(feature).unwrap_or("Helix")));
    let (written, new) = synthesized(id, &title, (origin, x, y), &entities)?;
    if in_place {
        let slot = preceding
            .iter_mut()
            .find(|row| row.get("id").and_then(Value::as_u64) == Some(id))
            .ok_or("its helix profile is not an earlier mapped sketch")?;
        if let (Some(slot), Some(fields)) = (slot.as_object_mut(), written.as_object()) {
            for key in ["plane", "sketch"] {
                if let Some(value) = fields.get(key) {
                    slot.insert(key.to_owned(), value.clone());
                }
            }
        }
        ctx.sketches.insert(sketch_id.to_owned(), new);
    } else {
        ctx.extra.push(written);
    }
    Ok((id, true))
}

/// A sketch entity with its geometry carried through the plane map `m` (columns: the images
/// of the old u and v axes), or `None` for a curve this cannot carry exactly.
fn affine(entity: &Value, m: [[f64; 2]; 2]) -> Option<Value> {
    let at = |value: Option<&Value>| -> Option<Value> {
        let [u, v] = p2(value)?;
        Some(json!({"u": m[0][0] * u + m[0][1] * v, "v": m[1][0] * u + m[1][1] * v}))
    };
    let mut out = entity.clone();
    let geometry = out.get_mut("geometry")?.as_object_mut()?;
    // m = R(beta) diag(s1, s2) R(gamma).
    let (e, f) = ((m[0][0] + m[1][1]) / 2.0, (m[0][0] - m[1][1]) / 2.0);
    let (g, h) = ((m[1][0] + m[0][1]) / 2.0, (m[1][0] - m[0][1]) / 2.0);
    let (q, r) = (e.hypot(h), f.hypot(g));
    let (s1, s2) = (q + r, q - r);
    let (a1, a2) = (g.atan2(f), h.atan2(e));
    let (beta, gamma) = ((a2 + a1) / 2.0, (a2 - a1) / 2.0);
    let kind = geometry.get("kind")?.as_str()?.to_owned();
    match kind.as_str() {
        "point" => {
            let position = at(geometry.get("position"))?;
            geometry.insert("position".to_owned(), position);
        }
        "line" => {
            let (start, end) = (at(geometry.get("start"))?, at(geometry.get("end"))?);
            geometry.insert("start".to_owned(), start);
            geometry.insert("end".to_owned(), end);
        }
        "circle" | "arc" => {
            let center = at(geometry.get("center"))?;
            let radius = number(geometry.get("radius"))?;
            geometry.insert("center".to_owned(), center);
            if (s1 - s2).abs() <= 1e-12 * s1 {
                geometry.insert("radius".to_owned(), json!(radius * s1));
                for key in ["start_angle", "end_angle"] {
                    if let Some(angle) = number(geometry.get(key)) {
                        geometry.insert(key.to_owned(), json!(angle + beta + gamma));
                    }
                }
            } else {
                let bounds = if kind == "arc" {
                    let start = number(geometry.get("start_angle"))? - gamma;
                    let mut sweep =
                        number(geometry.get("end_angle"))? - number(geometry.get("start_angle"))?;
                    if sweep <= 0.0 {
                        sweep += std::f64::consts::TAU;
                    }
                    Some([start, start + sweep])
                } else {
                    None
                };
                geometry.remove("radius");
                geometry.remove("start_angle");
                geometry.remove("end_angle");
                geometry.insert("kind".to_owned(), json!("ellipse"));
                geometry.insert("major_angle".to_owned(), json!(beta));
                geometry.insert("major_radius".to_owned(), json!(radius * s1));
                geometry.insert("minor_radius".to_owned(), json!(radius * s2));
                if let Some(bounds) = bounds {
                    geometry.insert("bounds".to_owned(), json!(bounds));
                }
            }
        }
        _ => return None,
    }
    Some(out)
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
