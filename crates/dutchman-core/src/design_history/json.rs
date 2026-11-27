//! Readers of the design layer's JSON wire form.
//!
//! A tagged value is `{"kind": name, "value": payload}`; a unit variant has no payload. A
//! dimension is a tagged literal or a parameter binding. Lengths are millimetres and angles
//! radians, as in the legacy rows.

use serde_json::{json, Value};

/// The rows of a design arena, empty when the design has none.
pub(super) fn rows<'a>(design: &'a Value, arena: &str) -> impl Iterator<Item = &'a Value> {
    design
        .get(arena)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// The text of a field.
pub(super) fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

/// The variant name of a tagged value.
pub(super) fn kind(value: &Value) -> Option<&str> {
    value.get("kind").and_then(Value::as_str)
}

/// The payload of a tagged value, null for a unit variant.
pub(super) fn payload(value: &Value) -> &Value {
    value.get("value").unwrap_or(&Value::Null)
}

/// The kind and id of a container member.
pub(super) fn member(value: &Value) -> Option<(&str, &str)> {
    Some((kind(value)?, payload(value).as_str()?))
}

/// The value of a dimension that states a literal, `None` for a parameter binding.
pub(super) fn literal(dimension: &Value) -> Option<&Value> {
    (kind(dimension) == Some("literal")).then(|| payload(dimension))
}

/// The number of a dimension that states a finite literal.
pub(super) fn number(dimension: &Value) -> Result<f64, String> {
    match literal(dimension) {
        Some(value) => value
            .as_f64()
            .filter(|value| value.is_finite())
            .ok_or_else(|| "a value is not a finite number".to_owned()),
        None => Err("a value is bound to a parameter, which is not projected yet".to_owned()),
    }
}

/// Three coordinates, from `{x, y, z}` or `[x, y, z]`.
pub(super) fn vector(value: &Value) -> Option<[f64; 3]> {
    if let Some(list) = value.as_array() {
        let mut parts = list.iter().map(Value::as_f64);
        let v = [parts.next()??, parts.next()??, parts.next()??];
        return parts.next().is_none().then_some(v);
    }
    Some([
        value.get("x")?.as_f64()?,
        value.get("y")?.as_f64()?,
        value.get("z")?.as_f64()?,
    ])
}

/// A model-history point or vector.
pub(super) fn xyz(v: [f64; 3]) -> Value {
    json!({"x": v[0], "y": v[1], "z": v[2]})
}
