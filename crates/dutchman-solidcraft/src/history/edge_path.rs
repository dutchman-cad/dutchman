//! Resolve a path's stored wire to its own earlier sketch, without replacing a body-edge
//! selection with a coincident but unrelated sketch. The core retained the owning hidden
//! result before dropping its geometry; each selected edge must match one entire emitted
//! sketch curve, including its arc midpoint, tangent and length.

use serde_json::Value;

use super::blend::Input;
use super::{operation, operation_kind, Context};

pub(super) fn sketch(
    feature: &Value,
    selection: &Value,
    inputs: &[Input],
    ctx: &Context,
) -> Result<Option<(u64, Vec<String>)>, String> {
    let owner = selection
        .pointer("/value/state")
        .and_then(Value::as_str)
        .and_then(|id| ctx.row("feature_input_topologies", id))
        .and_then(|state| state.get("bodies"))
        .and_then(Value::as_array)
        .filter(|bodies| bodies.len() == 1)
        .and_then(|bodies| bodies.first());
    let Some(owner) = owner else { return Ok(None) };
    let dependencies = feature
        .get("dependencies")
        .and_then(Value::as_array)
        .filter(|dependencies| dependencies.len() <= 10_000)
        .ok_or("its input edge path dependencies exceed the sketch resolution limit")?;
    let mut remaining = 100_000usize;
    let mut selected = None;
    for dependency in dependencies {
        let Some(producer) = dependency
            .as_str()
            .and_then(|id| ctx.row("features", id))
            .filter(|producer| operation_kind(producer) == Some("sketch"))
            .filter(|producer| {
                producer
                    .get("outputs")
                    .and_then(Value::as_array)
                    .is_some_and(|outputs| outputs.contains(owner))
            })
        else {
            continue;
        };
        let Some(mapped) = operation(producer)
            .pointer("/sketch/sketch")
            .and_then(Value::as_str)
            .and_then(|id| ctx.sketches.get(id))
        else {
            continue;
        };
        let mut curves = Vec::new();
        let mut complete = true;
        for input in inputs {
            let mut found = Vec::new();
            for (entity, shape) in &mapped.shapes {
                remaining = remaining
                    .checked_sub(1)
                    .ok_or("its input edge path exceeds the sketch resolution limit")?;
                let kind = ctx
                    .row("sketch_entities", entity)
                    .and_then(|row| row.pointer("/geometry/kind"))
                    .and_then(Value::as_str);
                if !matches!(kind, Some("line" | "arc")) {
                    continue;
                }
                let super::sketch::Shape::Curve(curve) = shape else {
                    continue;
                };
                if input.matches_sketch_curve(mapped, curve) {
                    found.push(curve.id.clone());
                }
            }
            match found.as_slice() {
                [curve] if !curves.contains(curve) => curves.push(curve.clone()),
                [] => {
                    complete = false;
                    break;
                }
                [_] => return Err("its input edge path repeats a source sketch curve".to_owned()),
                _ => {
                    return Err(
                        "its input edge path is ambiguous within its source sketch".to_owned()
                    )
                }
            }
        }
        if complete {
            match &selected {
                Some((id, _)) if *id != mapped.feature => {
                    return Err(
                        "its input edge path matches more than one earlier dependency sketch"
                            .to_owned(),
                    )
                }
                _ => selected = Some((mapped.feature, curves)),
            }
        }
    }
    Ok(selected)
}
