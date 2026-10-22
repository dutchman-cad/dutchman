//! Patterns of features, as SolidCraft patterns.
//!
//! A linear pattern (in one or two directions) maps to a rectangular one. A circular pattern
//! maps when it goes all the way round: neither CADIR nor SolidCraft fixes how a partial
//! span spreads its instances, so a partial one stops the history instead of risking a shift.

use std::f64::consts::TAU;

use serde_json::{json, Value};

use super::sketch::{cross, dot, number, unit, v3};
use super::{name, Context};

/// How far from a full turn (radians) a circular pattern may be and still be one.
const FULL_TURN: f64 = 1e-9;

/// Maps a pattern feature whose operation is `definition`.
pub(crate) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let unresolved =
        definition.pointer("/pattern/kind").and_then(Value::as_str) == Some("unresolved");
    let seeds = definition
        .get("seeds")
        .and_then(Value::as_array)
        .filter(|seeds| !seeds.is_empty())
        .ok_or(if unresolved {
            "cadmpeg does not decode what it repeats or how yet"
        } else {
            "it patterns nothing"
        })?;
    let mut features = Vec::with_capacity(seeds.len());
    for seed in seeds {
        match seed.get("kind").and_then(Value::as_str) {
            Some("feature") => {
                let id = seed
                    .get("value")
                    .and_then(Value::as_str)
                    .ok_or("its seed is malformed")?;
                let name = ctx
                    .feature_names
                    .get(id)
                    .ok_or("it patterns a feature that did not map")?;
                features.push(name.clone());
            }
            Some(other)
                if seed.pointer("/value/kind").and_then(Value::as_str) == Some("native") =>
            {
                return Err(format!(
                    "patterns of {other} are not mapped yet: cadmpeg does not resolve which"
                ))
            }
            Some(other) => return Err(format!("patterns of {other} are not mapped yet")),
            None => return Err("its seed is malformed".to_owned()),
        }
    }
    let pattern = definition.get("pattern");
    if pattern.and_then(|p| p.get("kind")).and_then(Value::as_str) == Some("mirror") {
        let plane = mirror_plane(pattern)?;
        let id = ctx.id();
        ctx.builds = true;
        return Ok(json!({
            "id": id,
            "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Mirror{id}")),
            "type": "mirror",
            "suppressed": false,
            "features": features,
            "plane": plane,
        }));
    }
    let layout = layout(pattern)?;
    let id = ctx.id();
    ctx.builds = true;
    Ok(json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Pattern{id}")),
        "type": "pattern",
        "suppressed": false,
        "features": features,
        "pattern": layout,
    }))
}

/// The SolidCraft plane of a CADIR mirror: through its point, square to its normal. Which two
/// directions span it does not matter to a reflection, so any pair square to the normal does.
fn mirror_plane(pattern: Option<&Value>) -> Result<Value, String> {
    let pattern = pattern.ok_or("it has no layout")?;
    let origin = v3(pattern.get("plane_origin")).ok_or("its mirror plane is malformed")?;
    let normal = v3(pattern.get("plane_normal"))
        .and_then(unit)
        .ok_or("its mirror plane is malformed")?;
    let away = if normal[0].abs() < 0.9 {
        [1.0, 0.0, 0.0]
    } else {
        [0.0, 1.0, 0.0]
    };
    let along = dot(away, normal);
    let x = unit([
        away[0] - along * normal[0],
        away[1] - along * normal[1],
        away[2] - along * normal[2],
    ])
    .ok_or("its mirror plane is malformed")?;
    let y = cross(normal, x);
    Ok(json!({"type": "custom", "plane": {"origin": origin, "x_dir": x, "y_dir": y}}))
}

/// The SolidCraft pattern layout for a CADIR pattern transform.
fn layout(pattern: Option<&Value>) -> Result<Value, String> {
    let pattern = pattern.ok_or("it has no layout")?;
    let count = |value: Option<&Value>| {
        value
            .and_then(Value::as_u64)
            .filter(|count| *count >= 1)
            .ok_or_else(|| "its count is malformed".to_owned())
    };
    match pattern.get("kind").and_then(Value::as_str) {
        Some("unresolved") => Err("cadmpeg does not decode its layout yet".to_owned()),
        Some("linear") => {
            let dir = v3(pattern.get("direction"))
                .and_then(unit)
                .ok_or("its direction is not resolved")?;
            let spacing = number(pattern.get("spacing"))
                .filter(|s| *s > 0.0)
                .ok_or("its spacing is malformed")?;
            let mut out = json!({
                "type": "rectangular",
                "dir1": dir,
                "count1": count(pattern.get("count"))?.to_string(),
                "spacing1": format!("{spacing} mm"),
            });
            if let Some(second) = pattern.get("second").filter(|s| !s.is_null()) {
                let dir2 = v3(second.get("direction"))
                    .and_then(unit)
                    .ok_or("its second direction is malformed")?;
                let spacing2 = number(second.get("spacing"))
                    .filter(|s| *s > 0.0)
                    .ok_or("its second spacing is malformed")?;
                if let Some(fields) = out.as_object_mut() {
                    fields.insert("dir2".to_owned(), json!(dir2));
                    fields.insert(
                        "count2".to_owned(),
                        json!(count(second.get("count"))?.to_string()),
                    );
                    fields.insert("spacing2".to_owned(), json!(format!("{spacing2} mm")));
                }
            }
            Ok(out)
        }
        Some("circular") => {
            let origin = v3(pattern.get("axis_origin")).ok_or("its axis is malformed")?;
            let axis = v3(pattern.get("axis_dir"))
                .and_then(unit)
                .ok_or("its axis is malformed")?;
            let angle = number(pattern.get("angle")).ok_or("its angle is malformed")?;
            if (TAU - angle).abs() > FULL_TURN {
                return Err("a circular pattern short of a full turn is not mapped yet".to_owned());
            }
            Ok(json!({
                "type": "circular",
                "origin": origin,
                "axis": axis,
                "count": count(pattern.get("count"))?.to_string(),
                "angle": "360 deg",
            }))
        }
        Some(other) => Err(format!("{other} patterns are not mapped yet")),
        None => Err("its layout is malformed".to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_patterns_become_rectangular() {
        let linear = json!({"kind": "linear", "direction": {"x": 2.0, "y": 0.0, "z": 0.0},
            "spacing": 20.0, "count": 3,
            "second": {"direction": {"x": 0.0, "y": 1.0, "z": 0.0}, "spacing": 15.0, "count": 2}});
        assert_eq!(
            layout(Some(&linear)).unwrap(),
            json!({"type": "rectangular", "dir1": [1.0, 0.0, 0.0], "count1": "3",
                   "spacing1": "20 mm", "dir2": [0.0, 1.0, 0.0], "count2": "2",
                   "spacing2": "15 mm"})
        );
    }

    #[test]
    fn only_full_circular_patterns_map() {
        let circular = |angle: f64| {
            json!({"kind": "circular",
            "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0}, "angle": angle, "count": 6})
        };
        assert_eq!(layout(Some(&circular(TAU))).unwrap()["angle"], "360 deg");
        assert!(layout(Some(&circular(TAU / 2.0))).is_err());
        assert!(layout(Some(&json!({"kind": "linear", "spacing": 1.0, "count": 2}))).is_err());
    }
    #[test]
    fn patterns_cadmpeg_does_not_decode_say_so() {
        let model = json!({});
        let reason = |definition: Value| {
            let mut ctx = Context::new(&model);
            map(&json!({"name": "Pattern"}), &definition, &mut ctx).unwrap_err()
        };
        assert_eq!(
            reason(json!({"seeds": [], "pattern": {"kind": "unresolved", "form": "linear"}})),
            "cadmpeg does not decode what it repeats or how yet"
        );
        assert_eq!(
            reason(json!({"seeds": [], "pattern": {"kind": "linear"}})),
            "it patterns nothing"
        );
        assert_eq!(
            reason(
                json!({"seeds": [{"kind": "faces", "value": {"kind": "native", "value": "n"}}],
                "pattern": {"kind": "circular"}})
            ),
            "patterns of faces are not mapped yet: cadmpeg does not resolve which"
        );
        assert_eq!(
            layout(Some(&json!({"kind": "unresolved", "form": "mirror"}))).unwrap_err(),
            "cadmpeg does not decode its layout yet"
        );
    }
}
