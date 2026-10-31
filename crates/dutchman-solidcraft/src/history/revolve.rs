//! Revolves, as SolidCraft revolve features.
//!
//! One-sided solid sweeps follow the oriented CADIR axis. SolidCraft instead sweeps toward
//! the sketch normal, so their angle sign is proven from the selected profile placement.
//! Symmetric, equal two-sided, and full turns need no direction proof.

use std::collections::BTreeSet;
use std::f64::consts::TAU;

use serde_json::{json, Value};

use super::extrude::{body_names, operation};
use super::profile::profile;
use super::sketch::{cross, dot, number, p2, unit, v3, Mapped};
use super::{name, Context};

/// How far from a full turn (radians) a one-sided revolve may be and still be one.
const FULL_TURN: f64 = 1e-9;

/// How far from the sketch plane (mm, and as a cosine for the direction) the axis may lie.
const IN_PLANE: f64 = 1e-6;

/// Maps a revolve feature whose operation is `definition`.
pub(crate) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let construction = definition.get("construction").unwrap_or(&Value::Null);
    if construction.get("state").and_then(Value::as_str) != Some("resolved") {
        let missing = construction
            .get("missing")
            .and_then(Value::as_str)
            .unwrap_or("construction");
        return Err(format!("its revolve {missing} is not resolved"));
    }
    let surface = construction.get("solid").and_then(Value::as_bool) == Some(false);
    match construction.get("face_maker_class").and_then(Value::as_str) {
        // SolidCraft builds profile regions with nested islands, as the bullseye maker does.
        None | Some("Part::FaceMakerBullseye") => {}
        Some(maker) => {
            return Err(format!(
                "revolves whose faces are built by {maker} are not supported yet \
                 (SolidCraft builds profile regions as Part::FaceMakerBullseye does)"
            ))
        }
    }
    // The fuse order and the single-face policy do not change the result of a revolve that
    // was built: the order only swaps the fuse operands, and the policy only rejects a
    // profile of several faces.
    let (sketch_id, profiles) = profile(construction.get("profile"), ctx)?;
    let sketch = ctx
        .sketches
        .get(&sketch_id)
        .ok_or("its sketch is not an earlier mapped sketch")?;
    let sketch_feature = sketch.feature;
    let placed = axis(construction.get("axis"), sketch.normal, sketch.origin)?;
    let axis =
        sketch_line(construction.get("axis"), &sketch_id, sketch).unwrap_or_else(|| placed.clone());
    let (mut angle, symmetric) = sweep(construction.get("extent"))?;
    let mut bound = None;
    if !symmetric && TAU - angle > FULL_TURN {
        if surface {
            return Err("one-sided partial surface revolves are not mapped yet (their sweep side is not proven)".to_owned());
        }
        angle *= sweep_sign(&placed, sketch, &sketch_id, &profiles, ctx)?;
        bound = ctx.owned_parameter(feature, "AlongAngle", angle.abs());
    }
    if surface && TAU - angle <= FULL_TURN {
        return Err(
            "full-turn surface revolves are not supported yet (SolidCraft closes them into a solid)"
                .to_owned(),
        );
    }
    let operation = if surface {
        "new_body"
    } else {
        operation(definition.get("op"))?
    };
    let body_names = body_names(feature, ctx, operation);

    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Revolve{id}")),
        "type": if surface { "surface_revolve" } else { "revolve" },
        "suppressed": false,
        "sketch": sketch_feature,
        "profiles": profiles,
        "axis": axis,
        "angle": bound.map_or_else(
            || format!("{} deg", angle.to_degrees()),
            |name| if angle < 0.0 { format!("-({name})") } else { name },
        ),
        "operation": operation,
        "convention": "sketch_normal",
    });
    if let Some(fields) = out.as_object_mut() {
        if surface {
            fields.remove("operation");
            fields.remove("convention");
        }
        if symmetric {
            fields.insert("symmetric".to_owned(), json!(true));
        }
        if let Some(names) = body_names {
            fields.insert("body_names".to_owned(), json!(names));
        }
    }
    Ok(out)
}

/// SolidCraft's positive angle turns each profile toward the sketch normal, regardless
/// of axis sign. Bound every selected curve on one side of the oriented source axis;
/// its right-hand rotation then either agrees with that normal or opposes it.
fn sweep_sign(
    axis: &Value,
    sketch: &Mapped,
    sketch_id: &str,
    profiles: &Value,
    ctx: &Context,
) -> Result<f64, String> {
    let missing = "its one-sided revolve sweep side is not proven (selected profile curves must lie on one side of the oriented axis)";
    let vector = |key| -> Option<[f64; 3]> {
        let values = axis.get(key)?.as_array()?;
        Some([
            number(values.first())?,
            number(values.get(1))?,
            number(values.get(2))?,
        ])
    };
    let origin = vector("origin").ok_or(missing)?;
    let direction = vector("dir").ok_or(missing)?;
    let radial = cross(sketch.normal, direction);
    let at = |point| {
        let world = sketch.world(point);
        dot(
            radial,
            [
                world[0] - origin[0],
                world[1] - origin[1],
                world[2] - origin[2],
            ],
        )
    };
    let all = profiles.get("type").and_then(Value::as_str) == Some("all");
    let mut selected = BTreeSet::new();
    selected_curves(profiles, &mut selected);
    let mut sign: f64 = 0.0;
    for entity in ctx
        .arena("sketch_entities")
        .iter()
        .filter(|entity| entity.get("sketch").and_then(Value::as_str) == Some(sketch_id))
    {
        let selected = if all {
            entity.get("construction").and_then(Value::as_bool) != Some(true)
        } else {
            entity
                .get("id")
                .and_then(Value::as_str)
                .and_then(|id| sketch.curve(id))
                .is_some_and(|curve| selected.contains(curve.id.as_str()))
        };
        if !selected {
            continue;
        }
        let geometry = entity.get("geometry").ok_or(missing)?;
        let (low, high) = match geometry.get("kind").and_then(Value::as_str) {
            Some("point" | "reference_line") => continue,
            Some("line") => {
                let a = at(p2(geometry.get("start")).ok_or(missing)?);
                let b = at(p2(geometry.get("end")).ok_or(missing)?);
                (a.min(b), a.max(b))
            }
            // A whole carrier disk is a conservative bound even for a trimmed arc.
            Some("circle" | "arc" | "ellipse") => {
                let center = at(p2(geometry.get("center")).ok_or(missing)?);
                let radius = if geometry.get("kind").and_then(Value::as_str) == Some("ellipse") {
                    number(geometry.get("major_radius"))
                        .zip(number(geometry.get("minor_radius")))
                        .map(|(major, minor)| major.max(minor))
                } else {
                    number(geometry.get("radius"))
                }.filter(|radius| *radius > 0.0).ok_or(missing)?;
                (center - radius, center + radius)
            }
            Some(kind) => return Err(format!(
                "its one-sided revolve sweep side is not proven (bounds for {kind} profile curves are not mapped yet)"
            )),
            None => return Err(missing.to_owned()),
        };
        if !low.is_finite() || !high.is_finite() {
            return Err(missing.to_owned());
        }
        let side = if low >= -IN_PLANE && high > IN_PLANE {
            1.0
        } else if high <= IN_PLANE && low < -IN_PLANE {
            -1.0
        } else if low.abs() <= IN_PLANE && high.abs() <= IN_PLANE {
            continue;
        } else {
            return Err(missing.to_owned());
        };
        if sign != 0.0 && sign != side {
            return Err(missing.to_owned());
        }
        sign = side;
    }
    if sign == 0.0 {
        return Err(missing.to_owned());
    }
    Ok(sign)
}

/// Selections produced by `profile` contain curve ids in bounded nested arrays/objects.
fn selected_curves<'a>(selection: &'a Value, ids: &mut BTreeSet<&'a str>) {
    match selection {
        Value::String(value) => {
            ids.insert(value);
        }
        Value::Array(values) => {
            for value in values {
                selected_curves(value, ids);
            }
        }
        Value::Object(fields) => {
            for value in fields.values() {
                selected_curves(value, ids);
            }
        }
        _ => {}
    }
}

/// The axis as a line of the profile's own sketch, when the source chose one.
fn sketch_line(axis: Option<&Value>, sketch_id: &str, sketch: &Mapped) -> Option<Value> {
    let reference = axis?.get("reference")?;
    if reference.get("kind").and_then(Value::as_str) != Some("sketch_curves") {
        return None;
    }
    let value = reference.get("value")?;
    if value.get("sketch").and_then(Value::as_str) != Some(sketch_id) {
        return None;
    }
    let [entity] = value.get("curves")?.as_array()?.as_slice() else {
        return None;
    };
    let curve = sketch.curve(entity.as_str()?)?;
    (curve.kind == "line").then(|| json!({"type": "sketch_line", "curve": curve.id}))
}

/// The axis as a SolidCraft line in model space, which must lie in the sketch plane.
fn axis(axis: Option<&Value>, normal: [f64; 3], plane_origin: [f64; 3]) -> Result<Value, String> {
    let axis = axis.ok_or("it has no axis")?;
    let origin = v3(axis.get("origin")).ok_or("its axis is malformed")?;
    let direction = v3(axis.get("direction"))
        .and_then(unit)
        .ok_or("its axis is malformed")?;
    let offset = [
        origin[0] - plane_origin[0],
        origin[1] - plane_origin[1],
        origin[2] - plane_origin[2],
    ];
    if dot(direction, normal).abs() > IN_PLANE || dot(offset, normal).abs() > IN_PLANE {
        return Err("its axis is not in the sketch plane".to_owned());
    }
    Ok(json!({"type": "line", "origin": origin, "dir": direction}))
}

/// The total angle (radians) and whether it is split evenly either side of the sketch.
fn sweep(extent: Option<&Value>) -> Result<(f64, bool), String> {
    let extent = extent.ok_or("it has no extent")?;
    let angle = |termination: Option<&Value>| -> Result<f64, String> {
        let termination = termination.unwrap_or(&Value::Null);
        match termination.get("kind").and_then(Value::as_str) {
            Some("angle") => number(termination.get("angle"))
                .filter(|angle| *angle > 0.0 && *angle <= TAU + FULL_TURN)
                .map(|angle| angle.min(TAU))
                .ok_or_else(|| "its angle is malformed".to_owned()),
            Some("to_face") if number(termination.get("offset")).is_some_and(|v| v != 0.0) => Err(
                "to-face revolves with an offset are not supported yet (SolidCraft has no angular end offset option)"
                    .to_owned(),
            ),
            Some("to_face") => Err(
                "to-face revolves are not supported yet (CADIR does not state which way it turns; \
                 SolidCraft's to option turns on one side only)"
                    .to_owned(),
            ),
            Some("to_vertex") => Err(
                "to-vertex revolves are not supported yet (CADIR does not state which way it turns; \
                 SolidCraft's to option turns on one side only)"
                    .to_owned(),
            ),
            Some("offset_from_face") => Err(
                "offset-from-face revolves are not supported yet (SolidCraft has no angular end offset option)"
                    .to_owned(),
            ),
            Some("to_shape") => Err(
                "to-shape revolves are not supported yet (SolidCraft cannot terminate a revolve at the first face of a target body)"
                    .to_owned(),
            ),
            Some(kind @ ("through_all" | "through_next" | "to_first" | "to_last")) => Err(format!(
                "{kind} revolve ends are not supported yet (SolidCraft has no material-search termination option)"
            )),
            Some("unresolved") => Err("its revolve termination is not resolved".to_owned()),
            Some(other) => Err(format!("{other} revolve ends are not supported yet")),
            None => Err("its extent is malformed".to_owned()),
        }
    };
    match extent.get("kind").and_then(Value::as_str) {
        Some("symmetric") => Ok((angle(extent.get("termination"))?, true)),
        Some("one_sided") => {
            let turn = angle(extent.get("termination"))?;
            if TAU - turn <= FULL_TURN {
                Ok((TAU, false))
            } else {
                Ok((turn, false))
            }
        }
        Some("two_sided") => {
            let first = angle(extent.get("first"))?;
            let second = angle(extent.get("second"))?;
            if first + second > TAU + FULL_TURN {
                return Err(
                    "a two-sided revolve beyond a full turn is not supported yet".to_owned(),
                );
            }
            if first == second {
                Ok(((first + second).min(TAU), true))
            } else if TAU - (first + second) <= FULL_TURN {
                // The angular start is immaterial once the two sides cover a full turn.
                Ok((TAU, false))
            } else {
                Err(
                    "unequal two-sided revolves are not mapped yet (CADIR does not state \
                     which way each side turns)"
                        .to_owned(),
                )
            }
        }
        Some(other) => Err(format!("its extent is {other}")),
        None => Err("its extent is malformed".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use std::f64::consts::PI;

    use super::*;

    fn angle(a: f64) -> Value {
        json!({"kind": "angle", "angle": a})
    }

    #[test]
    fn symmetric_and_full_turns_map() {
        let symmetric = json!({"kind": "symmetric", "termination": angle(PI / 2.0)});
        assert_eq!(sweep(Some(&symmetric)), Ok((PI / 2.0, true)));
        let full = json!({"kind": "one_sided", "termination": angle(TAU)});
        assert_eq!(sweep(Some(&full)), Ok((TAU, false)));
    }

    #[test]
    fn partial_angles_map_and_unproven_ends_stop_the_history() {
        let partial = json!({"kind": "one_sided", "termination": angle(PI)});
        assert_eq!(sweep(Some(&partial)), Ok((PI, false)));
        let two = json!({"kind": "two_sided", "first": angle(1.0), "second": angle(2.0)});
        assert!(sweep(Some(&two)).is_err());
        let to_face = json!({"kind": "symmetric", "termination": {"kind": "to_face"}});
        assert!(sweep(Some(&to_face))
            .unwrap_err()
            .contains("to-face revolves"));
    }

    #[test]
    fn the_axis_must_lie_in_the_sketch_plane() {
        let n = [0.0, 0.0, 1.0];
        let o = [0.0, 0.0, 0.0];
        let v = |c: [f64; 3]| json!({"x": c[0], "y": c[1], "z": c[2]});
        let in_plane = json!({"origin": v([5.0, 0.0, 0.0]), "direction": v([0.0, 2.0, 0.0])});
        assert_eq!(
            axis(Some(&in_plane), n, o).unwrap(),
            json!({"type": "line", "origin": [5.0, 0.0, 0.0], "dir": [0.0, 1.0, 0.0]})
        );
        let normal = json!({"origin": v([0.0; 3]), "direction": v([0.0, 0.0, 1.0])});
        assert!(axis(Some(&normal), n, o).is_err());
        let lifted = json!({"origin": v([0.0, 0.0, 3.0]), "direction": v([1.0, 0.0, 0.0])});
        assert!(axis(Some(&lifted), n, o).is_err());
    }
}
