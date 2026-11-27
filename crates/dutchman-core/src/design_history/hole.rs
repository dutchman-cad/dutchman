//! Holes: positions, the bore with its entry form, the depth, and a stated thread.

use std::f64::consts::PI;

use serde_json::{json, Map, Value};

use super::json::{kind, literal, number, payload, resolved_vector, text, xyz};
use super::ops::{refuse_stated, Built, Note};
use super::CODE_THREAD;

/// The hole operation as the model-history `hole` row. Only a bore with a plain, counterbored
/// or countersunk entry, drilled along a stated direction at stated positions, is projected.
pub(super) fn project(operation: &Value) -> Result<Built, String> {
    refuse_stated(
        operation,
        &[
            "positions",
            "direction",
            "shape",
            "extent",
            "thread",
            "series",
            "native_options",
            "source_settings",
        ],
        &[
            ("start", "a start face"),
            ("standard", "a standard size"),
            ("targets", "target bodies"),
        ],
    )?;
    if operation
        .get("series")
        .and_then(Value::as_array)
        .is_some_and(|series| !series.is_empty())
    {
        return Err("it states a series through several parts, which is not projected yet".into());
    }
    let shape = operation.get("shape").unwrap_or(&Value::Null);
    if kind(shape) != Some("bore") {
        return Err(format!(
            "a {} hole is not projected yet",
            kind(shape).unwrap_or("malformed")
        ));
    }
    refuse_stated(
        shape,
        &["kind", "diameter", "head", "bottom"],
        &[
            ("taper", "a taper"),
            ("near_chamfer", "an entry chamfer"),
            ("neck_chamfer", "a neck chamfer"),
            ("far_chamfer", "an exit chamfer"),
        ],
    )?;
    let diameter = match shape.get("diameter") {
        Some(diameter) => Some(positive(number(diameter)?, "its bore diameter")?),
        None => None,
    };
    let form = form(shape.get("head"), diameter)?;
    let mut notes = Vec::new();
    let specification = thread(operation.get("thread"), &mut notes)?;
    let mut value = Map::new();
    value.insert("kind".to_owned(), form);
    if let Some(specification) = specification {
        value.insert("specification".to_owned(), specification);
    }
    let mut shape_row = Map::new();
    shape_row.insert(
        "construction".to_owned(),
        json!({"kind": "form", "value": value}),
    );
    if let Some(diameter) = diameter {
        shape_row.insert("diameter".to_owned(), json!(diameter));
    }
    let mut row = Map::new();
    row.insert("shape".to_owned(), Value::Object(shape_row));
    if let Some(placements) = placements(operation)? {
        row.insert("placements".to_owned(), placements);
    }
    // The row cannot leave the depth out: it would read as a hole through everything.
    row.insert("extent".to_owned(), extent(operation.get("extent"))?);
    match shape.get("bottom") {
        None => {}
        Some(bottom) if kind(bottom) == Some("flat") => {
            row.insert("bottom".to_owned(), json!({"kind": "flat"}));
        }
        Some(_) => {
            return Err("its bottom is not flat, which is all that is projected yet".to_owned())
        }
    }
    Ok(Built {
        notes,
        ..Built::new(json!({"kind": "hole", "value": row}), Vec::new())
    })
}

fn positive(value: f64, what: &str) -> Result<f64, String> {
    if value > 0.0 {
        Ok(value)
    } else {
        Err(format!("{what} is not positive"))
    }
}

fn angle(value: f64, what: &str) -> Result<f64, String> {
    if value > 0.0 && value < PI {
        Ok(value)
    } else {
        Err(format!("{what} is not between 0 and a half turn"))
    }
}

/// The entry form of the bore. A form the design states in part is the row's own unresolved
/// form, which the mapper refuses by name.
fn form(head: Option<&Value>, bore: Option<f64>) -> Result<Value, String> {
    let unresolved = json!({"kind": "unresolved"});
    let Some(head) = head else {
        return Ok(unresolved);
    };
    let dimension = |value: Option<&Value>| value.map(number).transpose();
    let (kind, diameter, depth, cone_angle) = match kind(head) {
        Some("plain") => return Ok(json!({"kind": "simple"})),
        Some("counterbore") => {
            let body = payload(head);
            (
                "counterbore",
                dimension(body.get("diameter"))?,
                dimension(body.get("depth"))?,
                None,
            )
        }
        Some("countersink") => {
            let body = payload(head);
            if body.get("depth").is_some() {
                return Err("its countersink states a depth, which is not projected yet".to_owned());
            }
            (
                "countersink",
                dimension(body.get("diameter"))?,
                None,
                dimension(body.get("angle"))?,
            )
        }
        Some(other) => return Err(format!("a {other} entry is not projected yet")),
        None => return Err("its entry form is malformed".to_owned()),
    };
    let (Some(bore), Some(diameter)) = (bore, diameter) else {
        return Ok(unresolved);
    };
    positive(diameter, "its entry diameter")?;
    if diameter <= bore {
        return Err("its entry diameter does not exceed its bore diameter".to_owned());
    }
    if kind == "counterbore" {
        let Some(depth) = depth else {
            return Ok(unresolved);
        };
        positive(depth, "its counterbore depth")?;
        return Ok(json!({"kind": "counterbore", "diameter": diameter, "depth": depth}));
    }
    let Some(cone_angle) = cone_angle else {
        return Ok(unresolved);
    };
    angle(cone_angle, "its countersink angle")?;
    Ok(json!({"kind": "countersink", "diameter": diameter, "angle": cone_angle}))
}

/// The depth. A depth the design leaves unstated is the row's unresolved termination.
fn extent(limit: Option<&Value>) -> Result<Value, String> {
    let Some(limit) = limit else {
        return Ok(json!({"kind": "unresolved"}));
    };
    match kind(limit) {
        Some("through_all") => Ok(json!({"kind": "through_all"})),
        Some("amount") => {
            let length = number(payload(limit))?;
            Ok(json!({"kind": "blind", "length": positive(length, "its depth")?}))
        }
        Some(other) => Err(format!("a {other} depth is not projected yet")),
        None => Err("its depth is malformed".to_owned()),
    }
}

/// The directed placements: one per stated position, drilled along the stated direction.
/// A position or the direction left unresolved leaves the placements out, and the mapper
/// refuses the hole as one placed without resolved points.
fn placements(operation: &Value) -> Result<Option<Value>, String> {
    let positions = operation.get("positions").unwrap_or(&Value::Null);
    if kind(positions) != Some("placed") {
        return Err(
            "its positions are picked from a sketch or vertices, which is not projected yet"
                .to_owned(),
        );
    }
    let direction = operation.get("direction").and_then(resolved_vector);
    let points: Option<Vec<[f64; 3]>> = payload(positions)
        .as_array()
        .map_or(&[][..], Vec::as_slice)
        .iter()
        .map(resolved_vector)
        .collect();
    let (Some(direction), Some(points)) = (direction, points.filter(|p| !p.is_empty())) else {
        return Ok(None);
    };
    Ok(Some(Value::Array(
        points
            .into_iter()
            .map(|position| {
                json!({"kind": "directed", "position": xyz(position),
                    "direction": xyz(direction)})
            })
            .collect(),
    )))
}

/// The hole's thread as the model-history specification. A thread without the facts the
/// specification needs is left out with a note, and the bore stays.
fn thread(thread: Option<&Value>, notes: &mut Vec<Note>) -> Result<Option<Value>, String> {
    let Some(thread) = thread else {
        return Ok(None);
    };
    let standard = thread.get("standard").unwrap_or(&Value::Null);
    let name = text(standard, "standard").unwrap_or("unnamed");
    let mut left_out = |why: &str| {
        notes.push(Note {
            code: CODE_THREAD,
            message: format!(
                "the {name} thread {why}; the hole is projected as a plain hole of its bore diameter"
            ),
        });
        Ok(None)
    };
    if standard.is_null() {
        return left_out("states no standard");
    }
    let Some(pitch) = thread
        .get("pitch")
        .and_then(literal)
        .and_then(Value::as_f64)
    else {
        return left_out("states no pitch");
    };
    let Some(hand) = thread.get("hand").and_then(Value::as_str) else {
        return left_out("states no hand");
    };
    let Some(modeled) = thread
        .get("modeled")
        .and_then(literal)
        .and_then(Value::as_bool)
    else {
        return left_out("does not state whether it is modeled");
    };
    let depth = match thread.get("length") {
        None => json!({"kind": "hole_depth"}),
        Some(length) => match kind(length) {
            Some("full") => json!({"kind": "hole_depth"}),
            Some("standard") => json!({"kind": "tapped_standard"}),
            Some("blind") => match number(payload(length)) {
                Ok(depth) if depth > 0.0 => json!({"kind": "blind", "depth": depth}),
                _ => return left_out("states a length that is not a positive number"),
            },
            _ => return left_out("states a length that is not projected yet"),
        },
    };
    if pitch <= 0.0 {
        return left_out("states a pitch that is not positive");
    }
    let designation = standard
        .get("size")
        .and_then(literal)
        .and_then(Value::as_str)
        .unwrap_or(name);
    let mut row = Map::new();
    row.insert("kind".to_owned(), json!("threaded"));
    row.insert("standard".to_owned(), json!(name));
    row.insert("designation".to_owned(), json!(designation));
    if let Some(class) = text(standard, "class") {
        row.insert("class".to_owned(), json!(class));
    }
    row.insert("modeled".to_owned(), json!(modeled));
    // A thread is either in the geometry or an annotation on it.
    row.insert("cosmetic".to_owned(), json!(!modeled));
    row.insert("pitch".to_owned(), json!(pitch));
    if let Some(major) = thread
        .get("major_diameter")
        .and_then(literal)
        .and_then(Value::as_f64)
        .filter(|major| *major > 0.0)
    {
        row.insert("major_diameter".to_owned(), json!(major));
    }
    row.insert("hand".to_owned(), json!(hand));
    row.insert("depth".to_owned(), depth);
    Ok(Some(Value::Object(row)))
}
