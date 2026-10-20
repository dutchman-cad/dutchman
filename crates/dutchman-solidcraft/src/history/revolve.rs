//! Revolves, as SolidCraft revolve features.
//!
//! CADIR does not state which way a revolve turns, so only revolves whose result does not
//! depend on it are mapped: symmetric ones, and one-sided full turns.

use std::f64::consts::TAU;

use serde_json::{json, Value};

use super::extrude::{body_names, operation};
use super::profile::profile;
use super::sketch::{dot, number, unit, v3};
use super::{name, Context};

/// How far from a full turn (radians) a one-sided revolve may be and still be one.
const FULL_TURN: f64 = 1e-9;

/// How far from the sketch plane (mm, and as a cosine for the direction) the axis may lie.
const IN_PLANE: f64 = 1e-6;

/// Maps a revolve feature whose operation is `definition`.
pub(crate) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let construction = definition.get("construction").unwrap_or(&Value::Null);
    if construction.get("state").and_then(Value::as_str) != Some("resolved") {
        return Err("its construction is not resolved".to_owned());
    }
    let (sketch_id, profiles) = profile(construction.get("profile"), ctx)?;
    let sketch = ctx
        .sketches
        .get(&sketch_id)
        .ok_or("its sketch is not an earlier mapped sketch")?;
    let sketch_feature = sketch.feature;
    let axis = axis(construction.get("axis"), sketch.normal, sketch.origin)?;
    let (angle, symmetric) = sweep(construction.get("extent"))?;
    let operation = operation(definition.get("op"))?;
    let body_names = body_names(feature, ctx, operation);

    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Revolve{id}")),
        "type": "revolve",
        "suppressed": false,
        "sketch": sketch_feature,
        "profiles": profiles,
        "axis": axis,
        "angle": format!("{} deg", angle.to_degrees()),
        "operation": operation,
        "convention": "sketch_normal",
    });
    if let Some(fields) = out.as_object_mut() {
        if symmetric {
            fields.insert("symmetric".to_owned(), json!(true));
        }
        if let Some(names) = body_names {
            fields.insert("body_names".to_owned(), json!(names));
        }
    }
    Ok(out)
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
            Some(other) => Err(format!("{other} revolve ends are not mapped yet")),
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
        Some("two_sided") => Err(
            "two-sided revolves are not mapped yet (CADIR does not state \
                                  which way each side turns)"
                .to_owned(),
        ),
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
        let two = json!({"kind": "two_sided", "first": angle(1.0), "second": angle(1.0)});
        assert!(sweep(Some(&two)).is_err());
        let to_face = json!({"kind": "symmetric", "termination": {"kind": "to_face"}});
        assert_eq!(
            sweep(Some(&to_face)).unwrap_err(),
            "to_face revolve ends are not mapped yet"
        );
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
