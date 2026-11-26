//! Spans and limits of extrusions and revolutions as the extents the mappers read.

use serde_json::{json, Value};

use super::json::{kind, number, payload};

/// One end of a span.
#[derive(Debug, Clone, Copy, PartialEq)]
enum End {
    AtOrigin,
    Amount(f64),
    ThroughAll,
}

fn end(limit: &Value) -> Result<End, String> {
    match kind(limit) {
        Some("at_origin") => Ok(End::AtOrigin),
        Some("amount") => Ok(End::Amount(number(payload(limit))?)),
        Some("through_all") => Ok(End::ThroughAll),
        Some(other) => Err(format!("a {other} limit is not projected yet")),
        None => Err("a limit is malformed".to_owned()),
    }
}

/// How much a symmetric amount spans in all, from what the source says it measures.
fn total(span: &Value) -> Result<f64, String> {
    let amount = number(span.get("amount").ok_or("the span has no amount")?)?;
    if amount <= 0.0 {
        return Err("a symmetric amount is not positive".to_owned());
    }
    match span.get("measure").and_then(Value::as_str) {
        Some("total") => Ok(amount),
        Some("per_side") => Ok(2.0 * amount),
        _ => Err("the source does not state what a symmetric amount measures".to_owned()),
    }
}

fn blind(length: f64) -> Value {
    json!({"termination": {"kind": "blind", "length": length}})
}

fn through_all() -> Value {
    json!({"termination": {"kind": "through_all"}})
}

/// The extent of an extrusion, and whether it runs against the direction the design states.
pub(super) fn extrude(span: &Value) -> Result<(Value, bool), String> {
    if kind(span) == Some("symmetric") {
        return Ok((
            json!({"kind": "symmetric", "side": blind(total(span)?)}),
            false,
        ));
    }
    if kind(span) != Some("between") {
        return Err("its span is malformed".to_owned());
    }
    let start = end(span.get("start").ok_or("the span has no start")?)?;
    let stop = end(span.get("end").ok_or("the span has no end")?)?;
    if matches!(start, End::Amount(a) if a == 0.0) || matches!(stop, End::Amount(a) if a == 0.0) {
        return Err("an extent of zero length is not projected".to_owned());
    }
    let one =
        |side: Value, against: bool| Ok((json!({"kind": "one_sided", "side": side}), against));
    match (start, stop) {
        (End::AtOrigin, End::Amount(length)) if length > 0.0 => one(blind(length), false),
        (End::AtOrigin, End::Amount(length)) if length < 0.0 => one(blind(-length), true),
        (End::Amount(length), End::AtOrigin) if length > 0.0 => one(blind(length), true),
        (End::Amount(length), End::AtOrigin) if length < 0.0 => one(blind(-length), false),
        (End::AtOrigin, End::ThroughAll) => one(through_all(), false),
        (End::ThroughAll, End::AtOrigin) => one(through_all(), true),
        // The first side runs along the direction and the second against it.
        (End::Amount(against), End::Amount(along)) if along > 0.0 => Ok((
            json!({"kind": "two_sided", "first": blind(along), "second": blind(against)}),
            false,
        )),
        (End::Amount(against), End::ThroughAll) => Ok((
            json!({"kind": "two_sided", "first": through_all(), "second": blind(against)}),
            false,
        )),
        (End::ThroughAll, End::Amount(along)) if along > 0.0 => Ok((
            json!({"kind": "two_sided", "first": blind(along), "second": through_all()}),
            false,
        )),
        (End::ThroughAll, End::ThroughAll) => Ok((
            json!({"kind": "two_sided", "first": through_all(), "second": through_all()}),
            false,
        )),
        _ => Err("this combination of span ends is not projected yet".to_owned()),
    }
}

/// The extent of a revolution. A positive angle turns right-handed about the axis; the mapper
/// proves which side that is, so a negative angle is not projected.
pub(super) fn revolve(span: &Value) -> Result<Value, String> {
    let angle = |amount: f64| -> Result<Value, String> {
        if amount > 0.0 {
            Ok(json!({"kind": "angle", "angle": amount}))
        } else {
            Err("an angle that is not positive is not projected".to_owned())
        }
    };
    if kind(span) == Some("symmetric") {
        return Ok(json!({"kind": "symmetric", "termination": angle(total(span)?)?}));
    }
    if kind(span) != Some("between") {
        return Err("its span is malformed".to_owned());
    }
    let start = end(span.get("start").ok_or("the span has no start")?)?;
    let stop = end(span.get("end").ok_or("the span has no end")?)?;
    match (start, stop) {
        (End::AtOrigin, End::Amount(amount)) => {
            Ok(json!({"kind": "one_sided", "termination": angle(amount)?}))
        }
        _ => Err("this span of a revolution is not projected yet".to_owned()),
    }
}
