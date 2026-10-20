//! Extrudes, as SolidCraft extrude features.

use serde_json::{json, Map, Value};

use super::sketch::{dot, number, unit, v3};
use super::{name, Context};

/// How close to the sketch normal (as a cosine) an explicit direction must be.
const ALONG_NORMAL: f64 = 1.0 - 1e-9;

/// Maps an extrude feature whose operation is `definition`.
pub(crate) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let (sketch_id, profiles) = profile(definition.get("profile"), ctx)?;
    let sketch = ctx
        .sketches
        .get(&sketch_id)
        .ok_or("its sketch is not an earlier mapped sketch")?;
    let sketch_feature = sketch.feature;
    let forward = forward(definition.get("direction"), sketch.normal)?;
    let mut extent = extent(definition.get("extent"), forward)?;
    if let Some(offset) = start_offset(definition.get("start"))? {
        if !forward {
            return Err("a start offset against the sketch normal is not mapped yet".to_owned());
        }
        extent.insert("start_offset".to_owned(), json!(mm(offset)));
    }
    let operation = operation(definition.get("op"))?;
    let body_names = body_names(feature, ctx, operation);

    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Extrude{id}")),
        "type": "extrude",
        "suppressed": false,
        "sketch": sketch_feature,
        "profiles": profiles,
        "extent": extent,
        "operation": operation,
    });
    if let (Some(names), Some(fields)) = (body_names, out.as_object_mut()) {
        fields.insert("body_names".to_owned(), json!(names));
    }
    Ok(out)
}

/// The sketch a profile reference uses, and the SolidCraft profile selection for it.
pub(super) fn profile(profile: Option<&Value>, ctx: &Context) -> Result<(String, Value), String> {
    let profile = profile.ok_or("it has no profile")?;
    let kind = profile
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let value = profile.get("value").unwrap_or(&Value::Null);
    match kind {
        "sketch" => {
            let sketch = value.as_str().ok_or("its profile sketch is malformed")?;
            Ok((sketch.to_owned(), json!({"type": "all"})))
        }
        "sketch_profiles" => {
            let sketch = value
                .get("sketch")
                .and_then(Value::as_str)
                .ok_or("its profile sketch is malformed")?;
            let mapped = ctx
                .sketches
                .get(sketch)
                .ok_or("its sketch is not an earlier mapped sketch")?;
            let loops = ctx
                .row("sketches", sketch)
                .and_then(|row| row.get("profiles"))
                .and_then(Value::as_array)
                .ok_or("its sketch has no profiles")?;
            let indices = value
                .get("profiles")
                .and_then(Value::as_array)
                .ok_or("its profile selection is malformed")?;
            let mut selected = Vec::with_capacity(indices.len());
            for index in indices {
                let lp = index
                    .as_u64()
                    .and_then(|i| usize::try_from(i).ok())
                    .and_then(|i| loops.get(i))
                    .and_then(Value::as_array)
                    .ok_or("it selects a sketch profile that does not exist")?;
                let curves = lp
                    .iter()
                    .map(|uses| {
                        uses.get("entity")
                            .and_then(Value::as_str)
                            .and_then(|entity| mapped.curves.get(entity))
                            .cloned()
                            .ok_or("its profile uses a curve the sketch does not carry")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                selected.push(curves);
            }
            if selected.is_empty() {
                return Err("it selects no profile".to_owned());
            }
            Ok((
                sketch.to_owned(),
                json!({"type": "curves", "loops": selected}),
            ))
        }
        other => Err(format!("{other} profiles are not mapped yet")),
    }
}

/// Whether the extrude runs along the sketch normal (`false`: against it).
fn forward(direction: Option<&Value>, normal: [f64; 3]) -> Result<bool, String> {
    let direction = direction.unwrap_or(&Value::Null);
    match direction.get("kind").and_then(Value::as_str) {
        None | Some("profile_normal") => Ok(true),
        Some("reversed_profile_normal") => Ok(false),
        Some("explicit") => {
            let vector = v3(direction.get("vector"))
                .and_then(unit)
                .ok_or("its direction is malformed")?;
            let cos = dot(vector, normal);
            if cos >= ALONG_NORMAL {
                Ok(true)
            } else if cos <= -ALONG_NORMAL {
                Ok(false)
            } else {
                Err("an extrude oblique to its sketch is not mapped yet".to_owned())
            }
        }
        Some(other) => Err(format!("its direction is {other}")),
    }
}

/// The SolidCraft extent, with `forward` saying which way the first side runs.
fn extent(extent: Option<&Value>, forward: bool) -> Result<Map<String, Value>, String> {
    let extent = extent.ok_or("it has no extent")?;
    let way = if forward { "positive" } else { "negative" };
    let mut out = Map::new();
    match extent.get("kind").and_then(Value::as_str) {
        Some("one_sided") => {
            let side = side(extent.get("side"))?;
            out.insert("direction".to_owned(), json!(way));
            put(&mut out, side, "distance");
        }
        Some("symmetric") => {
            let side = side(extent.get("side"))?;
            out.insert("direction".to_owned(), json!("symmetric"));
            out.insert("convention".to_owned(), json!("total_distance"));
            put(&mut out, side, "distance");
        }
        Some("two_sided") => {
            let first = side(extent.get("first"))?;
            let second = side(extent.get("second"))?;
            let (Side::Blind(_), Side::Blind(length)) = (first, second) else {
                return Err(
                    "a two-sided extrude that is not blind on both sides is not mapped yet"
                        .to_owned(),
                );
            };
            out.insert("direction".to_owned(), json!(way));
            put(&mut out, first, "distance");
            out.insert("distance2".to_owned(), json!(mm(length)));
        }
        Some(other) => return Err(format!("its extent is {other}")),
        None => return Err("its extent is malformed".to_owned()),
    }
    Ok(out)
}

/// One side's travel, as far as SolidCraft can follow it.
#[derive(Clone, Copy)]
enum Side {
    Blind(f64),
    ThroughAll,
}

fn side(side: Option<&Value>) -> Result<Side, String> {
    let side = side.ok_or("its extent has no side")?;
    if number(side.get("draft")).is_some_and(|draft| draft != 0.0) {
        return Err("drafted extrudes are not mapped yet".to_owned());
    }
    let termination = side.get("termination").unwrap_or(&Value::Null);
    match termination.get("kind").and_then(Value::as_str) {
        Some("blind") => number(termination.get("length"))
            .filter(|length| *length > 0.0)
            .map(Side::Blind)
            .ok_or_else(|| "its blind length is malformed".to_owned()),
        Some("through_all") => Ok(Side::ThroughAll),
        Some(other) => Err(format!("{other} extrude ends are not mapped yet")),
        None => Err("its extent is malformed".to_owned()),
    }
}

fn put(out: &mut Map<String, Value>, side: Side, key: &str) {
    match side {
        Side::Blind(length) => {
            out.insert(key.to_owned(), json!(mm(length)));
        }
        Side::ThroughAll => {
            out.insert(key.to_owned(), json!("0 mm"));
            out.insert("through_all".to_owned(), json!(true));
        }
    }
}

/// The start offset from the sketch plane, if any.
fn start_offset(start: Option<&Value>) -> Result<Option<f64>, String> {
    let start = start.unwrap_or(&Value::Null);
    match start.get("kind").and_then(Value::as_str) {
        None | Some("profile_plane") => Ok(None),
        Some("offset_profile_plane") => number(start.get("offset"))
            .map(|offset| (offset != 0.0).then_some(offset))
            .ok_or_else(|| "its start offset is malformed".to_owned()),
        Some(other) => Err(format!("{other} extrude starts are not mapped yet")),
    }
}

/// SolidCraft's operation for a CADIR boolean.
pub(super) fn operation(op: Option<&Value>) -> Result<&'static str, String> {
    match op.and_then(Value::as_str) {
        Some("new_body") => Ok("new_body"),
        Some("join") => Ok("join"),
        Some("cut") => Ok("cut"),
        Some("intersect") => Ok("intersect"),
        Some(other) => Err(format!("its boolean is {other}")),
        None => Err("its boolean is malformed".to_owned()),
    }
}

/// The names of the bodies a new-body extrude makes, when the source names them all.
pub(super) fn body_names(feature: &Value, ctx: &Context, operation: &str) -> Option<Vec<String>> {
    if operation != "new_body" {
        return None;
    }
    let outputs = feature.get("outputs").and_then(Value::as_array)?;
    if outputs.is_empty() {
        return None;
    }
    outputs
        .iter()
        .map(|id| {
            let row = ctx.row("bodies", id.as_str()?)?;
            row.get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .map(str::to_owned)
        })
        .collect()
}

/// A length as a SolidCraft expression.
fn mm(length: f64) -> String {
    format!("{length} mm")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blind(length: f64) -> Value {
        json!({"termination": {"kind": "blind", "length": length}})
    }

    #[test]
    fn directions_follow_or_oppose_the_sketch_normal() {
        let n = [0.0, 0.0, 1.0];
        assert_eq!(forward(None, n), Ok(true));
        assert_eq!(
            forward(Some(&json!({"kind": "profile_normal"})), n),
            Ok(true)
        );
        assert_eq!(
            forward(Some(&json!({"kind": "reversed_profile_normal"})), n),
            Ok(false)
        );
        let explicit =
            |z: f64, x: f64| json!({"kind": "explicit", "vector": {"x": x, "y": 0.0, "z": z}});
        assert_eq!(forward(Some(&explicit(-2.0, 0.0)), n), Ok(false));
        assert!(forward(Some(&explicit(1.0, 1.0)), n).is_err());
        assert!(forward(Some(&json!({"kind": "unresolved"})), n).is_err());
    }

    #[test]
    fn extents_map_to_solidcraft_extents() {
        let one = json!({"kind": "one_sided", "side": blind(5.0)});
        assert_eq!(
            Value::Object(extent(Some(&one), false).unwrap()),
            json!({"direction": "negative", "distance": "5 mm"})
        );
        let symmetric = json!({"kind": "symmetric", "side": blind(8.0)});
        assert_eq!(
            Value::Object(extent(Some(&symmetric), true).unwrap()),
            json!({"direction": "symmetric", "convention": "total_distance", "distance": "8 mm"})
        );
        let two = json!({"kind": "two_sided", "first": blind(3.0), "second": blind(2.0)});
        assert_eq!(
            Value::Object(extent(Some(&two), true).unwrap()),
            json!({"direction": "positive", "distance": "3 mm", "distance2": "2 mm"})
        );
    }

    #[test]
    fn unsupported_extents_stop_the_history() {
        let drafted = json!({"kind": "one_sided",
            "side": {"termination": {"kind": "blind", "length": 5.0}, "draft": 0.1}});
        assert_eq!(
            extent(Some(&drafted), true).unwrap_err(),
            "drafted extrudes are not mapped yet"
        );
        let to_face = json!({"kind": "one_sided", "side": {"termination": {"kind": "to_face"}}});
        assert_eq!(
            extent(Some(&to_face), true).unwrap_err(),
            "to_face extrude ends are not mapped yet"
        );
        let mixed = json!({"kind": "two_sided", "first": blind(3.0),
            "second": {"termination": {"kind": "through_all"}}});
        assert!(extent(Some(&mixed), true).is_err());
    }

    #[test]
    fn starts_and_booleans() {
        assert_eq!(start_offset(None), Ok(None));
        assert_eq!(
            start_offset(Some(&json!({"kind": "profile_plane"}))),
            Ok(None)
        );
        assert_eq!(
            start_offset(Some(
                &json!({"kind": "offset_profile_plane", "offset": 2.5})
            )),
            Ok(Some(2.5))
        );
        assert!(start_offset(Some(&json!({"kind": "from_face"}))).is_err());
        for op in ["new_body", "join", "cut", "intersect"] {
            assert_eq!(operation(Some(&json!(op))), Ok(op));
        }
        assert!(operation(Some(&json!("unresolved"))).is_err());
    }
}
