//! Planar sketches: the placement frame and the free curves a sketch states.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use super::json::{kind, payload, rows, text, vector, xyz};

/// The planar sketches of a design, projected on demand.
pub(super) struct Sketches<'a> {
    sketches: BTreeMap<&'a str, &'a Value>,
    entities: BTreeMap<&'a str, Vec<&'a Value>>,
}

/// The sketch row, its entity rows, and whether the design left construction unstated for
/// any entity.
type Built = (Value, Vec<Value>, bool);

impl<'a> Sketches<'a> {
    pub(super) fn new(design: &'a Value) -> Self {
        let sketches = rows(design, "sketches")
            .filter_map(|sketch| Some((text(sketch, "id")?, sketch)))
            .collect();
        let mut entities: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
        for entity in rows(design, "sketch_entities") {
            if let Some(sketch) = text(entity, "sketch") {
                entities.entry(sketch).or_default().push(entity);
            }
        }
        Self { sketches, entities }
    }

    /// The name the design gives a sketch.
    pub(super) fn name(&self, sketch: &str) -> Option<&'a str> {
        text(self.sketches.get(sketch)?, "name")
    }

    /// The `model.sketches` row and `model.sketch_entities` rows of a sketch.
    pub(super) fn build(&self, sketch: &str) -> Result<Built, String> {
        let record = self
            .sketches
            .get(sketch)
            .ok_or_else(|| format!("the sketch {sketch} is not stated"))?;
        let mut rows = Vec::new();
        let mut unstated = false;
        for entity in self.entities.get(sketch).into_iter().flatten() {
            unstated |= entity
                .get("construction")
                .and_then(Value::as_bool)
                .is_none();
            rows.push(entity_row(entity, sketch)?);
        }
        let mut row = json!({"id": sketch, "placement": placement(record), "profiles": []});
        if let (Some(name), Some(object)) = (text(record, "name"), row.as_object_mut()) {
            object.insert("name".to_owned(), json!(name));
        }
        Ok((row, rows, unstated))
    }
}

/// The resolved frame of a placed sketch. A plane the design does not resolve stays
/// unresolved, and the mapper refuses it with its own reason.
fn placement(sketch: &Value) -> Value {
    let unresolved = json!({"kind": "unresolved"});
    let Some(plane) = sketch.get("plane").filter(|p| kind(p) == Some("placed")) else {
        return unresolved;
    };
    let Some(frame) = plane.get("resolved") else {
        return unresolved;
    };
    let axes = (
        frame.get("origin").and_then(vector),
        frame.get("x_axis").and_then(vector),
        frame.get("z_axis").and_then(vector),
    );
    let (Some(origin), Some(u_axis), Some(normal)) = axes else {
        return unresolved;
    };
    json!({"kind": "resolved", "frame": {
        "origin": xyz(origin), "normal": xyz(normal), "u_axis": xyz(u_axis),
    }})
}

fn entity_row(entity: &Value, sketch: &str) -> Result<Value, String> {
    let id = text(entity, "id").ok_or("a sketch entity has no id")?;
    let definition = entity.get("definition").unwrap_or(&Value::Null);
    if kind(definition) != Some("free") {
        return Err(format!(
            "the entity {id} is defined by another entity, and defined entities are not projected yet"
        ));
    }
    let mut row = json!({
        "id": id,
        "sketch": sketch,
        "endpoint_refs": [],
        "geometry": curve(payload(definition)).map_err(|reason| format!("the entity {id}: {reason}"))?,
    });
    // The model row cannot leave construction unstated; a design that does not state it
    // states an ordinary entity.
    let construction = entity
        .get("construction")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if let Some(object) = row.as_object_mut() {
        object.insert("construction".to_owned(), json!(construction));
    }
    Ok(row)
}

fn point(value: &Value) -> Result<Value, String> {
    let u = value.get("u").and_then(Value::as_f64);
    let v = value.get("v").and_then(Value::as_f64);
    match (u, v) {
        (Some(u), Some(v)) => Ok(json!({"u": u, "v": v})),
        _ => Err("a point is malformed".to_owned()),
    }
}

fn pair(value: Option<&Value>) -> Option<[f64; 2]> {
    let list = value?.as_array()?;
    let [a, b] = list.as_slice() else {
        return None;
    };
    Some([a.as_f64()?, b.as_f64()?])
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    value
        .get(key)
        .ok_or_else(|| format!("a curve has no {key}"))
}

fn scalar(value: &Value, key: &str) -> Result<f64, String> {
    field(value, key)?
        .as_f64()
        .ok_or_else(|| format!("the curve's {key} is malformed"))
}

fn curve(curve: &Value) -> Result<Value, String> {
    let body = payload(curve);
    match kind(curve) {
        Some("point") => Ok(json!({"kind": "point", "position": point(body)?})),
        Some("line") => Ok(json!({
            "kind": "line",
            "start": point(field(body, "start")?)?,
            "end": point(field(body, "end")?)?,
        })),
        Some("infinite_line") => {
            let [u, v] = pair(body.get("direction")).ok_or("the line's direction is malformed")?;
            Ok(json!({
                "kind": "reference_line",
                "origin": point(field(body, "through")?)?,
                "direction": {"u": u, "v": v},
            }))
        }
        Some("circle") => Ok(json!({
            "kind": "circle",
            "center": point(field(body, "center")?)?,
            "radius": scalar(body, "radius")?,
        })),
        Some("arc") => Ok(json!({
            "kind": "arc",
            "center": point(field(body, "center")?)?,
            "radius": scalar(body, "radius")?,
            "start_angle": scalar(body, "start")?,
            "end_angle": scalar(body, "end")?,
        })),
        Some("ellipse") => {
            let [x, y] = pair(body.get("major")).ok_or("the ellipse's major axis is malformed")?;
            let [major, minor] =
                pair(body.get("radii")).ok_or("the ellipse's radii are malformed")?;
            let mut row = json!({
                "kind": "ellipse",
                "center": point(field(body, "center")?)?,
                "major_angle": y.atan2(x),
                "major_radius": major,
                "minor_radius": minor,
            });
            if let (Some(bounds), Some(object)) = (body.get("range"), row.as_object_mut()) {
                let [start, end] = pair(Some(bounds)).ok_or("the ellipse's range is malformed")?;
                object.insert("bounds".to_owned(), json!([start, end]));
            }
            Ok(row)
        }
        Some(other) => Err(format!("a {other} curve is not projected yet")),
        None => Err("a curve is malformed".to_owned()),
    }
}
