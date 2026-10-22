//! Body mirror and boolean operations.
//!
//! Both act on whole bodies named by earlier mapped features, never on faces of the final
//! body, which a later feature may already have removed or moved. Native and generated body
//! selections are refused.

use serde_json::{json, Value};

use super::sketch::{cross, dot, unit, v3};
use super::{name, Context};

/// Only resolved operands have identities in the model's arenas.
fn members<'a>(selection: &'a Value, kind: &str) -> Result<&'a [Value], String> {
    let values = match selection.get("kind").and_then(Value::as_str) {
        Some(found) if found == kind => selection.get("value"),
        Some("resolved") => selection.get("value").and_then(|v| v.get(kind)),
        _ => return Err(format!("its {kind} are not resolved")),
    };
    values
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("its {kind} are not resolved"))
}

fn body_names(selection: &Value, ctx: &Context) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for value in members(selection, "bodies")? {
        let id = value.as_str().ok_or("its body reference is malformed")?;
        let (_, name) = ctx
            .named_bodies
            .iter()
            .find(|(known, _)| known == id)
            .ok_or("it selects a body that an earlier feature has not named")?;
        if ctx
            .named_bodies
            .iter()
            .any(|(other, n)| other != id && n == name)
        {
            return Err("its body name is not unique".to_owned());
        }
        names.push(name.clone());
    }
    if names.is_empty() {
        return Err("it selects no bodies".to_owned());
    }
    Ok(names)
}

fn feature(source: &Value, ctx: &mut Context, kind: &str, fields: Value) -> Value {
    let id = ctx.id();
    let mut out = serde_json::Map::new();
    out.insert("id".to_owned(), json!(id));
    out.insert(
        "name".to_owned(),
        json!(name(source)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{kind}{id}"))),
    );
    out.insert("type".to_owned(), json!(kind));
    out.insert("suppressed".to_owned(), json!(false));
    if let Value::Object(fields) = fields {
        out.extend(fields);
    }
    Value::Object(out)
}

/// Builds the stored custom plane, whose in-plane axes do not affect a reflection.
fn plane(origin: [f64; 3], normal: [f64; 3]) -> Result<Value, String> {
    let normal = unit(normal).ok_or("its plane normal is not resolved")?;
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
    .ok_or("its plane axes are not resolved")?;
    Ok(
        json!({"type": "custom", "plane": {"origin": origin, "x_dir": x, "y_dir": cross(normal, x)}}),
    )
}

pub(super) fn combine(
    source: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let operands = definition
        .get("operands")
        .ok_or("its boolean operands are missing")?;
    let target = body_names(operands.get("target").unwrap_or(&Value::Null), ctx)?;
    let target = match target.as_slice() {
        [target] => target.clone(),
        _ => return Err("combining several target bodies is not supported yet".to_owned()),
    };
    let tools = body_names(operands.get("tools").unwrap_or(&Value::Null), ctx)?;
    if tools.contains(&target) {
        return Err("its target is also a tool body".to_owned());
    }
    let operation = definition
        .get("op")
        .and_then(Value::as_str)
        .filter(|op| matches!(*op, "join" | "cut" | "intersect"))
        .ok_or("its boolean operation is not supported yet")?;
    let keep_tools = definition
        .get("keep_tools")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let out = feature(
        source,
        ctx,
        "combine",
        json!({"target": target, "tools": tools, "operation": operation, "keep_tools": keep_tools}),
    );
    if !keep_tools {
        ctx.named_bodies.retain(|(_, name)| !tools.contains(name));
    }
    Ok(out)
}

/// Body seeds of a mirror pattern; feature seeds keep the existing pattern mapping.
pub(super) fn mirror(
    source: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let seeds = definition
        .get("seeds")
        .and_then(Value::as_array)
        .ok_or("its mirror seeds are missing")?;
    let mut bodies = Vec::new();
    let mut features = Vec::new();
    for seed in seeds {
        match seed.get("kind").and_then(Value::as_str) {
            Some("bodies") => {
                bodies.extend(body_names(seed.get("value").unwrap_or(&Value::Null), ctx)?);
            }
            Some("feature") => {
                let mapped = seed
                    .get("value")
                    .and_then(Value::as_str)
                    .and_then(|id| ctx.feature_names.get(id))
                    .ok_or("it mirrors a feature that did not map")?;
                features.push(mapped.clone());
            }
            _ => {
                return Err("mirrors of faces or unresolved seeds are not supported yet".to_owned());
            }
        }
    }
    if bodies.is_empty() || !features.is_empty() {
        return Err("mixing body and feature mirror seeds is not supported yet".to_owned());
    }
    let pattern = definition
        .get("pattern")
        .ok_or("its mirror plane is missing")?;
    let origin =
        v3(pattern.get("plane_origin")).ok_or("its mirror plane origin is not resolved")?;
    let normal =
        v3(pattern.get("plane_normal")).ok_or("its mirror plane normal is not resolved")?;
    let plane = plane(origin, normal)?;
    Ok(feature(
        source,
        ctx,
        "mirror",
        json!({"features": [], "bodies": bodies, "plane": plane, "combine": false}),
    ))
}
