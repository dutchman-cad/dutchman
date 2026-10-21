//! Holes drilled at the points of a sketch: simple holes, through all or to a depth, flat or
//! with a drill point, as SolidCraft's Hole drills them.

use serde_json::{json, Value};

use super::sketch::{number, unit, v3, Mapped, Shape};
use super::{name, Context};

/// Maps a hole feature whose operation is `definition`.
pub(crate) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    if definition.get("placements").is_some_and(|p| !p.is_null()) {
        return Err("a hole placed without a sketch is not mapped yet".to_owned());
    }
    let profile = definition.get("profile").unwrap_or(&Value::Null);
    if profile.get("kind").and_then(Value::as_str) != Some("sketch") {
        return Err("a hole placed without a sketch is not mapped yet".to_owned());
    }
    let sketch_id = profile
        .get("value")
        .and_then(Value::as_str)
        .ok_or("its sketch is not resolved")?;
    let sketch = ctx
        .sketches
        .get(sketch_id)
        .ok_or("its sketch is not an earlier mapped sketch")?;

    let shape = definition.get("shape").unwrap_or(&Value::Null);
    let construction = shape.get("construction").unwrap_or(&Value::Null);
    match (
        construction.get("construction").and_then(Value::as_str),
        construction.pointer("/kind/kind").and_then(Value::as_str),
    ) {
        (Some("form"), Some("simple")) => {}
        (Some("form"), Some(kind)) => return Err(format!("a {kind} hole is not mapped yet")),
        _ => return Err("a hole of this construction is not mapped yet".to_owned()),
    }
    let diameter = number(shape.get("diameter")).ok_or("its diameter is not given")?;

    let points = locations(sketch, definition.get("profile_filter"))?;
    let first = points
        .first()
        .map(|(_, at)| *at)
        .ok_or("its sketch has no points to drill at")?;

    // The drilling direction, else into the sketch's side, against its normal.
    let direction = match v3(definition.get("direction")) {
        Some(direction) => unit(direction).ok_or("its direction is zero")?,
        None => sketch.normal.map(|c| -c),
    };

    let mut hole = json!({"type": "simple"});
    let mut depth = None;
    let extent = definition.get("extent").unwrap_or(&Value::Null);
    match extent.get("kind").and_then(Value::as_str) {
        None | Some("through_all") => {}
        Some("blind") => {
            let length = number(extent.get("length")).ok_or("its depth is not given")?;
            let bottom = definition.get("bottom").unwrap_or(&Value::Null);
            match bottom.get("kind").and_then(Value::as_str) {
                None | Some("flat") => depth = Some(length),
                Some("angled") => {
                    let angle = number(bottom.get("included_angle"))
                        .filter(|a| *a > 0.0 && *a < std::f64::consts::PI)
                        .ok_or("its drill point's angle is not given")?;
                    // SolidCraft's depth is to the shoulder, where the point begins.
                    let to_tip = bottom.get("depth_to_tip").and_then(Value::as_bool) == Some(true);
                    let point = diameter / 2.0 / (angle / 2.0).tan();
                    let shoulder = if to_tip { length - point } else { length };
                    if shoulder <= 0.0 {
                        return Err("its drill point is deeper than the hole".to_owned());
                    }
                    depth = Some(shoulder);
                    hole =
                        json!({"type": "drilled", "tip_angle": format!("{}", angle.to_degrees())});
                }
                Some(other) => {
                    return Err(format!("a hole with a {other} bottom is not mapped yet"))
                }
            }
        }
        Some(other) => return Err(format!("a hole that ends {other} is not mapped yet")),
    }
    if definition.get("taper_angle").is_some_and(|t| !t.is_null()) {
        return Err("a tapered hole is not mapped yet".to_owned());
    }

    let (sketch_feature, position) = (sketch.feature, sketch.world(first));
    let ids: Vec<String> = points.into_iter().map(|(id, _)| id).collect();
    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Hole{id}")),
        "type": "hole",
        "suppressed": false,
        "diameter": format!("{diameter} mm"),
        "direction": direction,
        "hole": hole,
        "points": {
            "sketch": sketch_feature,
            "ids": ids,
        },
        "position": position,
    });
    if let (Some(depth), Some(fields)) = (depth, out.as_object_mut()) {
        fields.insert("depth".to_owned(), json!(format!("{depth} mm")));
    }
    Ok(out)
}

/// The sketch points holes are drilled at, with their positions in the sketch: the centres of
/// its circles and arcs, and its points, as far as `filter` takes each, in the sketch's order.
fn locations(sketch: &Mapped, filter: Option<&Value>) -> Result<Vec<(String, [f64; 2])>, String> {
    let filter = filter.and_then(Value::as_str).unwrap_or("all");
    let (points, circles, arcs) = match filter {
        "points" => (true, false, false),
        "circles" => (false, true, false),
        "points_and_circles" => (true, true, false),
        "arcs" => (false, false, true),
        "points_and_arcs" => (true, false, true),
        "circles_and_arcs" => (false, true, true),
        "all" => (true, true, true),
        other => return Err(format!("holes at a sketch's {other} are not mapped yet")),
    };
    let mut indices: Vec<usize> = sketch
        .shapes
        .values()
        .filter_map(|shape| match shape {
            Shape::Point(index) if points => Some(*index),
            Shape::Curve(curve)
                if (circles && curve.kind == "circle") || (arcs && curve.kind == "arc") =>
            {
                curve.center
            }
            _ => None,
        })
        .collect();
    indices.sort_unstable();
    indices.dedup();
    indices
        .into_iter()
        .map(|index| {
            sketch
                .points
                .get(index)
                .cloned()
                .ok_or_else(|| "its sketch's points are malformed".to_owned())
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::sketch::Curve;
    use super::*;

    /// A sketch 10 mm up on XY with a circle about (3, 4) and a lone point at (-1, 0).
    fn sketch() -> Mapped {
        let mut shapes = BTreeMap::new();
        shapes.insert(
            "circle".to_owned(),
            Shape::Curve(Curve {
                index: 0,
                id: "c1".to_owned(),
                kind: "circle",
                start: None,
                end: None,
                center: Some(1),
            }),
        );
        shapes.insert("point".to_owned(), Shape::Point(2));
        Mapped {
            feature: 7,
            normal: [0.0, 0.0, 1.0],
            origin: [0.0, 0.0, 10.0],
            x_dir: [1.0, 0.0, 0.0],
            y_dir: [0.0, 1.0, 0.0],
            points: vec![
                ("origin".to_owned(), [0.0, 0.0]),
                ("p1".to_owned(), [3.0, 4.0]),
                ("p2".to_owned(), [-1.0, 0.0]),
            ],
            shapes,
        }
    }

    fn hole(extra: Value) -> Value {
        let mut hole = json!({
            "definition": "hole",
            "profile": {"kind": "sketch", "value": "s"},
            "profile_filter": "circles",
            "direction": {"x": 0.0, "y": 0.0, "z": -1.0},
            "shape": {"construction": {"construction": "form", "kind": {"kind": "simple"}}, "diameter": 5.0},
            "extent": {"kind": "through_all"},
        });
        if let (Some(fields), Some(more)) = (hole.as_object_mut(), extra.as_object()) {
            fields.extend(more.clone());
        }
        hole
    }

    fn mapped(definition: &Value) -> Result<Value, String> {
        let model = json!({});
        let mut ctx = Context::new(&model);
        ctx.sketches.insert("s".to_owned(), sketch());
        map(&json!({"name": "Hole"}), definition, &mut ctx)
    }

    #[test]
    fn a_simple_hole_through_all_drills_at_each_circle() {
        let out = mapped(&hole(json!({}))).unwrap();
        assert_eq!(out["type"], "hole");
        assert_eq!(out["diameter"], "5 mm");
        assert_eq!(out["direction"], json!([0.0, 0.0, -1.0]));
        assert_eq!(out["hole"], json!({"type": "simple"}));
        assert_eq!(out["points"], json!({"sketch": 7, "ids": ["p1"]}));
        assert_eq!(out["position"], json!([3.0, 4.0, 10.0]));
        assert!(out.get("depth").is_none(), "through all");
    }

    #[test]
    fn every_location_the_filter_takes_is_drilled() {
        let out = mapped(&hole(json!({"profile_filter": "all"}))).unwrap();
        assert_eq!(out["points"]["ids"], json!(["p1", "p2"]));
        let none = mapped(&hole(json!({"profile_filter": "arcs"}))).unwrap_err();
        assert_eq!(none, "its sketch has no points to drill at");
    }

    #[test]
    fn a_blind_hole_with_a_drill_point_is_drilled_to_its_shoulder() {
        let angle = 118_f64.to_radians();
        let flat = mapped(&hole(json!({"extent": {"kind": "blind", "length": 6.0}}))).unwrap();
        assert_eq!(flat["depth"], "6 mm");
        let to_shoulder = mapped(&hole(json!({
            "extent": {"kind": "blind", "length": 6.0},
            "bottom": {"kind": "angled", "included_angle": angle, "depth_to_tip": false},
        })))
        .unwrap();
        assert_eq!(to_shoulder["depth"], "6 mm");
        assert_eq!(to_shoulder["hole"]["type"], "drilled");
        let to_tip = mapped(&hole(json!({
            "extent": {"kind": "blind", "length": 6.0},
            "bottom": {"kind": "angled", "included_angle": angle, "depth_to_tip": true},
        })))
        .unwrap();
        let depth: f64 = to_tip["depth"]
            .as_str()
            .unwrap()
            .trim_end_matches(" mm")
            .parse()
            .unwrap();
        assert!((depth - (6.0 - 2.5 / (59_f64.to_radians()).tan())).abs() < 1e-9);
    }

    #[test]
    fn other_holes_say_why_they_are_not_mapped() {
        let counterbore = hole(json!({"shape": {"construction": {"construction": "form",
            "kind": {"kind": "counterbore"}}, "diameter": 5.0}}));
        assert_eq!(
            mapped(&counterbore).unwrap_err(),
            "a counterbore hole is not mapped yet"
        );
        let to_face = hole(json!({"extent": {"kind": "to_face"}}));
        assert_eq!(
            mapped(&to_face).unwrap_err(),
            "a hole that ends to_face is not mapped yet"
        );
    }
}
