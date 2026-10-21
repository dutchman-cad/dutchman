//! Solid primitives, as SolidCraft primitive features: spheres, and tori about the Z axis.
//!
//! Boxes and cylinders are not mapped. cadmpeg gives a box as dimensions and a placement
//! without saying whether the placement is the box's corner or its centre, and a cylinder
//! primitive without a placement at all, so neither can be put where the source has it.

use serde_json::{json, Value};

use super::extrude::{body_names, operation};
use super::sketch::{number, unit, v3};
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

fn mm(length: f64) -> String {
    format!("{length} mm")
}
