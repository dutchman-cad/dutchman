//! Sweeps, as SolidCraft sweep features.
//!
//! A solid sweep of one sketch profile along curves of a planar sketch maps when SolidCraft
//! builds the same shape from it: the profile square to the path as it turns (or held as
//! drawn), optionally twisted, along the whole path, optionally guided by one rail of sketch
//! curves. Sweeps of surfaces, of several sections, along part of a path, with a taper or a
//! scale, or oriented by a guide surface or a direction are not mapped.

use serde_json::{json, Map, Value};

use super::extrude::{body_names, operation};
use super::profile::profile;
use super::sketch::number;
use super::{name, Context};

/// How far from the whole path (as a fraction of it) a sweep's extent may be and still be it.
const WHOLE: f64 = 1e-9;

/// Maps a sweep feature whose operation is `definition`.
pub(crate) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let shape = definition.get("shape").unwrap_or(&Value::Null);
    match shape.get("mode").and_then(Value::as_str) {
        Some("solid") => {}
        Some("surface") => return Err("surface sweeps are not mapped yet".to_owned()),
        _ => return Err("its swept shape is not resolved".to_owned()),
    }
    if shape
        .get("sections")
        .and_then(Value::as_array)
        .is_some_and(|sections| !sections.is_empty())
    {
        return Err("sweeps through several sections are not mapped yet".to_owned());
    }
    let section = shape.get("section").unwrap_or(&Value::Null);
    if section.get("kind").and_then(Value::as_str) != Some("profile") {
        return Err("its profile is not a sketch profile".to_owned());
    }
    let (sketch_id, profiles) = profile(section.get("value"), ctx)?;
    let sketch = ctx
        .sketches
        .get(&sketch_id)
        .ok_or("its sketch is not an earlier mapped sketch")?
        .feature;

    let (path_sketch, path) = curves(definition.get("path"), ctx, "path")?;
    whole(definition.get("path_extent"), "path")?;

    let mut extra = Map::new();
    match definition
        .pointer("/orientation/kind")
        .and_then(Value::as_str)
    {
        None | Some("corrected_frenet") => {}
        Some("fixed") => {
            extra.insert("orientation".to_owned(), json!("parallel"));
        }
        Some(other) => {
            return Err(format!(
                "sweeps oriented by {} are not mapped yet",
                other.replace('_', " ")
            ))
        }
    }
    match definition.get("transition").and_then(Value::as_str) {
        None | Some("transformed") => {}
        Some(_) => return Err("sweeps with corner transitions are not mapped yet".to_owned()),
    }
    match definition.get("transformation").and_then(Value::as_str) {
        None | Some("constant") => {}
        Some(_) => {
            return Err(
                "sweeps that change their profile along the path are not mapped yet".to_owned(),
            )
        }
    }
    if number(definition.get("taper")).is_some_and(|taper| taper != 0.0) {
        return Err("tapered sweeps are not mapped yet".to_owned());
    }
    if number(definition.get("scale")).is_some_and(|scale| (scale - 1.0).abs() > WHOLE) {
        return Err("scaled sweeps are not mapped yet".to_owned());
    }
    if let Some(twist) = number(definition.get("twist")).filter(|twist| *twist != 0.0) {
        extra.insert(
            "twist".to_owned(),
            json!(format!("{} deg", twist.to_degrees())),
        );
    }
    if let Some(rail) = definition.get("guide_rail").filter(|rail| !rail.is_null()) {
        let (rail_sketch, rail_curves) = curves(rail.get("path"), ctx, "guide rail")?;
        whole(rail.get("extent"), "guide rail")?;
        extra.insert(
            "rails".to_owned(),
            json!([{"sketch": rail_sketch, "curves": rail_curves}]),
        );
    }
    let operation = operation(shape.get("op"))?;
    let body_names = body_names(feature, ctx, operation);

    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Sweep{id}")),
        "type": "sweep",
        "suppressed": false,
        "sketch": sketch,
        "profiles": profiles,
        "path_sketch": path_sketch,
        "path": path,
        "operation": operation,
    });
    if let Some(fields) = out.as_object_mut() {
        fields.extend(extra);
        if let Some(names) = body_names {
            fields.insert("body_names".to_owned(), json!(names));
        }
    }
    Ok(out)
}

/// The SolidCraft sketch feature and curve ids a path of sketch curves uses; `what` names it
/// in the reason when it cannot be mapped.
fn curves(path: Option<&Value>, ctx: &Context, what: &str) -> Result<(u64, Vec<String>), String> {
    let path = path.ok_or_else(|| format!("it has no {what}"))?;
    match path.get("kind").and_then(Value::as_str) {
        Some("sketch_curves") => {}
        Some("spatial_sketch_curves" | "spatial_sketch_selection") => {
            return Err(format!("a {what} in a 3D sketch is not mapped yet"))
        }
        _ => return Err(format!("its {what} is not a set of sketch curves")),
    }
    let value = path.get("value").unwrap_or(&Value::Null);
    let sketch = value
        .get("sketch")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("its {what} is malformed"))?;
    let mapped = ctx
        .sketches
        .get(sketch)
        .ok_or_else(|| format!("its {what} sketch is not an earlier mapped sketch"))?;
    let ids = value
        .get("curves")
        .and_then(Value::as_array)
        .filter(|curves| !curves.is_empty())
        .ok_or_else(|| format!("its {what} is malformed"))?
        .iter()
        .map(|entity| {
            entity
                .as_str()
                .and_then(|entity| mapped.curve(entity))
                .map(|curve| curve.id.clone())
                .ok_or_else(|| format!("its {what} uses a curve the sketch does not carry"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok((mapped.feature, ids))
}

/// Refuses an extent that is not the whole of a path or rail.
fn whole(extent: Option<&Value>, what: &str) -> Result<(), String> {
    let Some(extent) = extent.filter(|extent| !extent.is_null()) else {
        return Ok(());
    };
    let along = number(extent.get("along_fraction")).unwrap_or(1.0);
    let against = number(extent.get("against_fraction")).unwrap_or(0.0);
    if (along - 1.0).abs() > WHOLE || against.abs() > WHOLE {
        return Err(format!(
            "sweeps along part of their {what} are not mapped yet"
        ));
    }
    Ok(())
}
