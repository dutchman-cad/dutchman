//! Solid primitives, as SolidCraft primitive features: spheres, tori about the Z axis, and
//! coils along any axis.
//!
//! Boxes and cylinders are not mapped. cadmpeg gives a box as dimensions and a placement
//! without saying whether the placement is the box's corner or its centre, and a cylinder
//! primitive without a placement at all, so neither can be put where the source has it.

use serde_json::{json, Value};

use super::extrude::{body_names, operation};
use super::sketch::{cross, dot, number, unit, v3};
use super::{name, Context};

/// How close to the Z axis (as a cosine) a torus's axis must be.
const ALONG_Z: f64 = 1.0 - 1e-9;

/// Maps a sphere feature whose operation is `definition`.
pub(crate) fn sphere(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let center = v3(definition.get("center")).ok_or("its centre is malformed")?;
    let radius = length(definition.get("radius"), "radius")?;
    let operation = operation(definition.get("op"))?;
    Ok(primitive(
        feature,
        ctx,
        operation,
        "sphere",
        "Sphere",
        json!({"center": center, "radius": mm(radius)}),
    ))
}

/// Maps a torus feature whose operation is `definition`.
pub(crate) fn torus(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let center = v3(definition.get("center")).ok_or("its centre is malformed")?;
    let axis = v3(definition.get("axis"))
        .and_then(unit)
        .ok_or("its axis is malformed")?;
    // SolidCraft's torus lies about Z; one about -Z is the same solid.
    if axis[2].abs() < ALONG_Z {
        return Err("a torus about an axis other than Z is not mapped yet".to_owned());
    }
    let major = length(definition.get("major_radius"), "major radius")?;
    let minor = length(definition.get("minor_radius"), "minor radius")?;
    let operation = operation(definition.get("op"))?;
    Ok(primitive(
        feature,
        ctx,
        operation,
        "torus",
        "Torus",
        json!({"center": center, "major": mm(major), "minor": mm(minor)}),
    ))
}

/// Maps a coil feature whose operation is `definition`: a section swept along a helix.
///
/// CADIR says which way a coil turns as seen from its base looking along its axis; SolidCraft
/// as seen looking back down the axis at its base. A coil CADIR calls clockwise is therefore
/// one SolidCraft calls anticlockwise. Negative axial travel reverses the mapped axis and
/// the turning flag together, preserving the source's angular travel about its original axis.
pub(crate) fn coil(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let construction = definition
        .get("construction")
        .ok_or("its construction is malformed")?;
    let placement = construction
        .get("placement")
        .ok_or("its placement is malformed")?;
    if placement.get("kind").and_then(Value::as_str) != Some("explicit") {
        return Err(
            "a coil whose placement cadmpeg keeps in native form is not mapped yet".to_owned(),
        );
    }
    let base = v3(placement.get("origin")).ok_or("its base is malformed")?;
    let axis = v3(placement.get("axis"))
        .and_then(unit)
        .ok_or("its axis is malformed")?;
    let radial = v3(placement.get("radial"))
        .and_then(unit)
        .ok_or("its start direction is malformed")?;
    if number(construction.get("taper")).ok_or("its taper is malformed")? != 0.0 {
        return Err(
            "a tapered coil is not mapped yet: SolidCraft's coil has no taper option".to_owned(),
        );
    }
    let diameter = length(construction.get("diameter"), "diameter")?;
    let extent = construction
        .get("extent")
        .ok_or("its extent is malformed")?;
    let signed = |key: &str| {
        number(extent.get(key))
            .filter(|value| *value != 0.0)
            .ok_or_else(|| format!("its {key} is malformed"))
    };
    let (turns, pitch, down) = match extent.get("kind").and_then(Value::as_str) {
        Some("revolutions_pitch") => {
            let turns = length(extent.get("revolutions"), "revolutions")?;
            let pitch = signed("pitch")?;
            (number_text(turns), mm(pitch.abs()), pitch < 0.0)
        }
        Some("revolutions_height") => {
            let turns = length(extent.get("revolutions"), "revolutions")?;
            let height = signed("height")?;
            (
                number_text(turns),
                format!("({} mm) / ({turns})", height.abs()),
                height < 0.0,
            )
        }
        Some("height_pitch") => {
            let height = signed("height")?;
            let pitch = signed("pitch")?;
            if height.is_sign_negative() != pitch.is_sign_negative() {
                return Err("its extent is malformed".to_owned());
            }
            (
                format!("({} mm) / ({} mm)", height.abs(), pitch.abs()),
                mm(pitch.abs()),
                pitch < 0.0,
            )
        }
        Some("spiral") => {
            return Err(
                "a flat spiral coil is not mapped yet: SolidCraft's coil has no spiral mode"
                    .to_owned(),
            )
        }
        _ => return Err("its extent is malformed".to_owned()),
    };
    let axis = if down { axis.map(|v| -v) } else { axis };
    let start = coil_start_angle(axis, radial)?;
    let section = construction
        .get("section")
        .ok_or("its section is malformed")?;
    let (shape, size) = match section.get("kind").and_then(Value::as_str) {
        Some("circular") => (
            "circular",
            length(section.get("diameter"), "section diameter")?,
        ),
        Some("square") => ("square", length(section.get("size"), "section size")?),
        Some("external_triangle" | "internal_triangle") => {
            return Err(
                "a coil of triangular section is not mapped yet: SolidCraft's coil only has \
                 circular and square sections"
                    .to_owned(),
            )
        }
        _ => return Err("its section is malformed".to_owned()),
    };
    let position = match construction
        .get("section_placement")
        .and_then(Value::as_str)
    {
        Some("inside") => Some(-1),
        Some("center") => None,
        Some("outside") => Some(1),
        _ => return Err("its section placement is malformed".to_owned()),
    };
    let clockwise = construction
        .get("clockwise")
        .and_then(Value::as_bool)
        .ok_or("its turning direction is malformed")?;
    let result = definition.get("result").ok_or("its result is malformed")?;
    let operation = match result.get("kind").and_then(Value::as_str) {
        Some("new_body") => "new_body",
        Some("boolean") => operation(result.get("operation"))?,
        _ => return Err("its result is malformed".to_owned()),
    };
    let mut fields = json!({
        "base": base,
        "axis": axis,
        "diameter": mm(diameter),
        "turns": turns,
        "pitch": pitch,
        "section": shape,
        "section_size": mm(size),
    });
    if let Value::Object(fields) = &mut fields {
        if start.abs() > 1e-9 {
            fields.insert("start_angle".to_owned(), json!(format!("{start} deg")));
        }
        if let Some(position) = position {
            fields.insert("position".to_owned(), json!(position));
        }
        if clockwise == down {
            fields.insert("clockwise".to_owned(), json!(true));
        }
    }
    Ok(primitive(feature, ctx, operation, "coil", "Coil", fields))
}

/// SolidCraft 0.19 measures start angles from world X projected normal to the axis.
/// Within 1e-6 of either X direction it instead starts at axis × Y (±Z).
/// CLI quarter-turn and short-arc probes confirm both branches and their turning sense.
fn coil_start_angle(axis: [f64; 3], radial: [f64; 3]) -> Result<f64, String> {
    let world_x = [1.0, 0.0, 0.0];
    let perpendicular = cross(axis, world_x);
    let x = if dot(perpendicular, perpendicular).sqrt() > 1e-6 {
        let projection = dot(axis, world_x);
        unit([
            1.0 - axis[0] * projection,
            -axis[1] * projection,
            -axis[2] * projection,
        ])
    } else {
        unit(cross(axis, [0.0, 1.0, 0.0]))
    }
    .ok_or("its axis is malformed")?;
    Ok(dot(radial, cross(axis, x))
        .atan2(dot(radial, x))
        .to_degrees())
}

/// Why a box or cylinder primitive is not mapped.
pub(crate) fn unplaced(kind: &str) -> String {
    match kind {
        "block" => "boxes are not mapped yet: cadmpeg does not say whether a box's placement \
                    is its corner or its centre"
            .to_owned(),
        _ => "cylinder and cone primitives are not mapped yet: cadmpeg gives them no placement"
            .to_owned(),
    }
}

fn primitive(
    feature: &Value,
    ctx: &mut Context,
    operation: &'static str,
    kind: &str,
    default_name: &str,
    fields: Value,
) -> Value {
    let body_names = body_names(feature, ctx, operation);
    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("{default_name}{id}")),
        "type": kind,
        "suppressed": false,
        "operation": operation,
    });
    if let (Some(out), Value::Object(fields)) = (out.as_object_mut(), fields) {
        out.extend(fields);
        if let Some(names) = body_names {
            out.insert("body_names".to_owned(), json!(names));
        }
    }
    out
}

fn length(value: Option<&Value>, what: &str) -> Result<f64, String> {
    number(value)
        .filter(|length| *length > 0.0)
        .ok_or_else(|| format!("its {what} is malformed"))
}

fn number_text(value: f64) -> String {
    format!("{value}")
}

fn mm(length: f64) -> String {
    format!("{length} mm")
}
