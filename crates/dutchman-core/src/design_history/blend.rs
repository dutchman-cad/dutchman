//! Fillets and chamfers of straight edges that the design names by a witness.
//!
//! A witness pick states the end points of the edge it names and, for a straight edge, its
//! direction. The model-history blend selects edges by their identity in a feature input
//! topology, so each selection gets one topology row holding the witnessed edges, and the row
//! carries only what the witnesses state. The mapper proves the edges against the body it
//! rebuilds, so a witness that is not an edge of the input state is refused there.

use serde_json::{json, Map, Value};

use super::json::{kind, literal, midpoint, number, payload, vector, xyz};
use super::ops::{refuse_stated, Built, Note};
use super::CODE_REFERENCE_FACES;

/// The operand keys of a fillet or chamfer that the projection reads or ignores by design.
const BLEND_KEYS: &[&str] = &["sets", "controls", "native_options", "source_settings"];

pub(super) fn fillet(feature: &str, operation: &Value) -> Result<Built, String> {
    refuse_stated(operation, BLEND_KEYS, &[])?;
    default_controls(operation)?;
    let sets = sets(operation)?;
    let mut built = Built::new(json!(null), Vec::new());
    let mut edges = 0_usize;
    let mut groups = Vec::new();
    for set in sets {
        if kind(set) != Some("edges") {
            return Err(format!(
                "a fillet of {} is not projected yet",
                kind(set).unwrap_or("something malformed")
            ));
        }
        refuse_stated(
            set,
            &["kind", "edges", "section", "hold"],
            &[("spine", "a spine")],
        )?;
        refuse_stated(
            set.get("section").unwrap_or(&Value::Null),
            &["size", "profile", "weight"],
            &[],
        )?;
        if set
            .get("hold")
            .and_then(Value::as_array)
            .is_some_and(|hold| hold.iter().any(|held| !held.is_null()))
        {
            return Err("it holds a face, which is not projected yet".to_owned());
        }
        let section = set.get("section").unwrap_or(&Value::Null);
        match section.get("profile") {
            None => {}
            Some(profile) if kind(profile) == Some("circular") => {}
            Some(_) => {
                return Err(
                    "its profile is not circular, which is all that is projected yet".into(),
                )
            }
        }
        let radius = match section.get("size") {
            None => json!({"kind": "unresolved"}),
            Some(size) if kind(size) == Some("radius") => {
                let radius = number(payload(size))?;
                if radius <= 0.0 {
                    return Err("its radius is not positive".to_owned());
                }
                json!({"kind": "constant", "radius": radius})
            }
            Some(size) => {
                return Err(format!(
                    "a {} fillet is not projected yet",
                    kind(size).unwrap_or("malformed")
                ))
            }
        };
        let selection = selection(
            feature,
            set.get("edges").unwrap_or(&Value::Null),
            &mut edges,
            &mut built,
        )?;
        let mut group = json!({"edges": selection, "radius": radius});
        if let (Some(weight), Some(fields)) = (section.get("weight"), group.as_object_mut()) {
            fields.insert("tangency_weight".to_owned(), json!(number(weight)?));
        }
        groups.push(group);
    }
    built.definition = json!({"kind": "fillet", "value": {"groups": groups}});
    Ok(built)
}

pub(super) fn chamfer(feature: &str, operation: &Value) -> Result<Built, String> {
    refuse_stated(operation, BLEND_KEYS, &[])?;
    default_controls(operation)?;
    let sets = sets(operation)?;
    let mut built = Built::new(json!(null), Vec::new());
    let mut edges = 0_usize;
    let mut groups = Vec::new();
    let mut flipped = None;
    let mut reference_unread = false;
    for set in sets {
        refuse_stated(
            set,
            &["edges", "size", "reference", "flip"],
            &[("measure", "a measure")],
        )?;
        if let Some(reference) = set.get("reference") {
            // The mapper infers the setback faces of an unequal chamfer from the final body.
            if reference.pointer("/picks/kind").and_then(Value::as_str) != Some("unread") {
                return Err(
                    "it names the face its first setback lies on, which is not projected yet"
                        .to_owned(),
                );
            }
            reference_unread = true;
        }
        if let Some(flip) = set.get("flip") {
            let flip = literal(flip)
                .and_then(Value::as_bool)
                .ok_or("its flip is bound to a parameter, which is not projected yet")?;
            if flipped.is_some_and(|seen| seen != flip) {
                return Err("its sets state different flips, which is not projected yet".into());
            }
            flipped = Some(flip);
        }
        let spec = match set.get("size") {
            None => json!({"kind": "unresolved"}),
            Some(size) => match kind(size) {
                Some("distance") => json!({"kind": "distance", "distance": length(payload(size))?}),
                Some("distances") => {
                    let size = payload(size);
                    json!({"kind": "two_distances",
                        "first": length(size.get("first").ok_or("it has no first distance")?)?,
                        "second": length(size.get("second").ok_or("it has no second distance")?)?})
                }
                Some("distance_angle") => {
                    let size = payload(size);
                    let angle = number(size.get("angle").ok_or("it has no angle")?)?;
                    if !(angle > 0.0 && angle < std::f64::consts::PI) {
                        return Err("its angle is not between 0 and a half turn".to_owned());
                    }
                    json!({"kind": "distance_angle",
                        "distance": length(size.get("distance").ok_or("it has no distance")?)?,
                        "angle": angle})
                }
                Some(other) => return Err(format!("a {other} chamfer is not projected yet")),
                None => return Err("its size is malformed".to_owned()),
            },
        };
        let selection = selection(
            feature,
            set.get("edges").unwrap_or(&Value::Null),
            &mut edges,
            &mut built,
        )?;
        groups.push(json!({"edges": selection, "spec": spec}));
    }
    if reference_unread {
        built.notes.push(Note {
            code: CODE_REFERENCE_FACES,
            message:
                "the faces its setbacks lie on are not stated; they are read from the rebuilt body"
                    .to_owned(),
        });
    }
    built.definition = json!({"kind": "chamfer", "value": {
        "groups": groups, "flip_direction": flipped.unwrap_or(false)}});
    Ok(built)
}

fn length(dimension: &Value) -> Result<f64, String> {
    let length = number(dimension)?;
    if length > 0.0 {
        Ok(length)
    } else {
        Err("a setback is not positive".to_owned())
    }
}

fn sets(operation: &Value) -> Result<&[Value], String> {
    let sets = operation
        .get("sets")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    if sets.is_empty() {
        return Err("it states no edge set".to_owned());
    }
    Ok(sets)
}

/// Refuses controls that change the blend: every control is at its default or absent.
fn default_controls(operation: &Value) -> Result<(), String> {
    let Some(controls) = operation.get("controls").and_then(Value::as_object) else {
        return Ok(());
    };
    for (key, value) in controls {
        let empty = value.is_null() || value.as_array().is_some_and(Vec::is_empty);
        if !empty {
            return Err(format!(
                "it states the control {key}, which is not projected yet"
            ));
        }
    }
    Ok(())
}

/// The model-history selection of the edges a design selection names, and the topology row
/// that holds them. `next` counts the edges written for the feature so far.
fn selection(
    feature: &str,
    selection: &Value,
    next: &mut usize,
    built: &mut Built,
) -> Result<Value, String> {
    if selection.pointer("/at/kind").and_then(Value::as_str) != Some("input") {
        return Err("its edges are read in a state other than its input".to_owned());
    }
    if selection.get("native").is_some() {
        return Err("its edges have a native selection, which is not projected yet".to_owned());
    }
    let picks = selection.get("picks").unwrap_or(&Value::Null);
    match kind(picks) {
        // The model row keeps a selection it cannot resolve as one the mapper refuses by name.
        Some("unread") => return Ok(json!({"kind": "unresolved"})),
        Some("named") => {}
        Some("rule") => {
            return Err("its edges are selected by a rule, which is not evaluated".into())
        }
        _ => return Err("its edge selection is malformed".to_owned()),
    }
    let named = payload(picks).as_array().map_or(&[][..], Vec::as_slice);
    if named.is_empty() {
        return Err("it names no edge".to_owned());
    }
    let state = format!("{feature}/edge/{next}");
    let mut ids = Vec::new();
    let mut geometry = Vec::new();
    for pick in named {
        let id = format!("{feature}/edge/{next}");
        geometry.push(edge_geometry(&id, pick)?);
        ids.push(id);
        *next = next.checked_add(1).ok_or("too many edges")?;
    }
    built.topologies.push(json!({
        "id": state,
        "input_of": feature,
        "bodies": [],
        "faces": [],
        "face_owners": [],
        "edges": ids,
        "vertices": [],
        "edge_geometry": geometry,
    }));
    Ok(json!({"kind": "historical", "value": {
        "state": state, "edges": ids, "native": state}}))
}

/// The geometry of one witnessed edge, from its witness alone.
fn edge_geometry(id: &str, pick: &Value) -> Result<Value, String> {
    let target = pick.get("target").unwrap_or(&Value::Null);
    let plain =
        kind(target) == Some("native") && payload(target).get("within").is_none_or(Value::is_null);
    if !plain {
        return Err("an edge is named in some way other than a witness".to_owned());
    }
    for key in ["context", "native", "domain", "resolved"] {
        if pick.get(key).is_some_and(|value| !value.is_null()) {
            return Err(format!(
                "an edge pick states {key}, which is not projected yet"
            ));
        }
    }
    let witness = pick
        .get("witness")
        .ok_or("an edge pick states no witness")?;
    let ends = witness
        .get("endpoints")
        .and_then(Value::as_array)
        .map(|ends| ends.iter().map(vector).collect::<Option<Vec<_>>>())
        .ok_or("a witness does not state both edge end points")?
        .ok_or("a witness end point is malformed")?;
    let [start, end] = ends.as_slice() else {
        return Err("a witness does not state both edge end points".to_owned());
    };
    let interior = witness
        .get("point")
        .and_then(vector)
        .unwrap_or_else(|| midpoint(*start, *end));
    let mut row = Map::new();
    row.insert("edge".to_owned(), json!(id));
    row.insert("interior".to_owned(), xyz(interior));
    if let Some(direction) = witness.get("direction").filter(|d| !d.is_null()) {
        row.insert("tangent".to_owned(), direction.clone());
    }
    row.insert("start".to_owned(), xyz(*start));
    row.insert("end".to_owned(), xyz(*end));
    row.insert("faces".to_owned(), json!([]));
    row.insert("supports".to_owned(), json!([]));
    Ok(Value::Object(row))
}
