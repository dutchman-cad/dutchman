//! Revolves, as SolidCraft revolve features.
//!
//! CADIR does not state which way a revolve turns, so only revolves whose result does not
//! depend on it are mapped: symmetric ones, equal two-sided ones, and full turns (unequal
//! sides that add up to one included). Surface revolves map short of a full turn, and an
//! axis chosen as a line of the profile's sketch stays that line.

use std::f64::consts::TAU;

use serde_json::{json, Value};

use super::extrude::{body_names, operation};
use super::profile::profile;
use super::sketch::{dot, number, unit, v3, Mapped};
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
    let axis = sketch_line(construction.get("axis"), &sketch_id, sketch).unwrap_or(placed);
    let (angle, symmetric) = sweep(construction.get("extent"))?;
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
        "angle": format!("{} deg", angle.to_degrees()),
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
                Err(
                    "a one-sided revolve short of a full turn is not mapped yet (CADIR does not \
                     state which way it turns)"
                        .to_owned(),
                )
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
    fn direction_dependent_revolves_stop_the_history() {
        let partial = json!({"kind": "one_sided", "termination": angle(PI)});
        assert!(sweep(Some(&partial))
            .unwrap_err()
            .contains("which way it turns"));
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
