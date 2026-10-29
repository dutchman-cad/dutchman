//! Body mirror, boolean and split operations.
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

/// The topology arenas a body's vertices are reached through.
const TOPOLOGY: [&str; 8] = [
    "regions", "shells", "faces", "loops", "coedges", "edges", "vertices", "points",
];

/// Most vertices looked at to place a body on one side of a plane.
const VERTEX_LIMIT: usize = 100_000;

/// The positions of a body's vertices, in its own frame; `None` when the body is moved by a
/// transform of its own, or its topology does not reach its vertices.
fn vertices(ctx: &Context, body: &str) -> Option<Vec<[f64; 3]>> {
    let row = ctx.row("bodies", body)?;
    if row.get("transform").is_some_and(|t| !t.is_null()) {
        return None;
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut queue = vec![row];
    let mut points = Vec::new();
    while let Some(row) = queue.pop() {
        if let Some(position) = super::sketch::v3(row.get("position")) {
            points.push(position);
        }
        let mut refs = Vec::new();
        strings(row, &mut refs);
        for id in refs {
            if seen.len() >= VERTEX_LIMIT || !seen.insert(id) {
                continue;
            }
            if let Some(next) = TOPOLOGY.iter().find_map(|arena| ctx.row(arena, id)) {
                queue.push(next);
            }
        }
    }
    (!points.is_empty()).then_some(points)
}

/// Every string inside `value`.
fn strings<'v>(value: &'v Value, out: &mut Vec<&'v str>) {
    match value {
        Value::String(s) => out.push(s),
        Value::Array(values) => values.iter().for_each(|v| strings(v, out)),
        Value::Object(fields) => fields.values().for_each(|v| strings(v, out)),
        _ => {}
    }
}

/// Which side of the plane through `origin` with `normal` a body lies on: `Some(true)` on the
/// side the normal points to, `Some(false)` on the other, `None` when it is on both or unknown.
fn side(ctx: &Context, body: &str, origin: [f64; 3], normal: [f64; 3]) -> Option<bool> {
    const TOLERANCE: f64 = 1e-6;
    let distances: Vec<f64> = vertices(ctx, body)?
        .into_iter()
        .map(|p| {
            dot(
                [p[0] - origin[0], p[1] - origin[1], p[2] - origin[2]],
                normal,
            )
        })
        .collect();
    if distances.iter().all(|d| *d >= -TOLERANCE) && distances.iter().any(|d| *d > TOLERANCE) {
        Some(true)
    } else if distances.iter().all(|d| *d <= TOLERANCE) && distances.iter().any(|d| *d < -TOLERANCE)
    {
        Some(false)
    } else {
        None
    }
}

/// Splits one body by an earlier datum plane. SolidCraft keeps the body's name for the piece
/// on the side the plane's normal points to and names the piece on the other side; the
/// source's two result bodies are matched to them by the side their vertices lie on. When they
/// cannot be, neither piece keeps a source identity, so a later feature selecting one is
/// refused rather than given the wrong piece.
pub(super) fn split(
    source: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let targets = body_names(definition.get("targets").unwrap_or(&Value::Null), ctx)?;
    let target = match targets.as_slice() {
        [target] => target.clone(),
        _ => return Err("splitting several bodies at once is not supported yet".to_owned()),
    };
    let tool = definition.get("tool").unwrap_or(&Value::Null);
    match tool.get("kind").and_then(Value::as_str) {
        Some("plane") => {}
        Some("faces") => return Err("splitting a body by faces is not supported yet".to_owned()),
        _ => return Err("its splitting tool is not resolved".to_owned()),
    }
    let datum = tool
        .pointer("/value/plane")
        .and_then(Value::as_str)
        .and_then(|id| ctx.row("features", id))
        .ok_or("its splitting plane is not an earlier feature")?;
    let frame = super::operation(datum)
        .get("frame")
        .ok_or("its splitting plane is not placed")?;
    let origin = v3(frame.get("origin")).ok_or("its splitting plane is not placed")?;
    let normal = v3(frame.get("normal"))
        .and_then(unit)
        .ok_or("its splitting plane is not placed")?;
    let plane = plane(origin, normal)?;
    let outputs: Vec<String> = source
        .get("outputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let sides: Vec<Option<bool>> = outputs
        .iter()
        .map(|body| side(ctx, body, origin, normal))
        .collect();
    let kept = outputs
        .iter()
        .zip(&sides)
        .find(|(_, side)| **side == Some(true));
    let cut = outputs
        .iter()
        .zip(&sides)
        .find(|(_, side)| **side == Some(false));
    let base = cut
        .and_then(|(body, _)| ctx.row("bodies", body))
        .and_then(|row| row.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty() && *name != target)
        .map_or_else(|| format!("{target} (split)"), str::to_owned);
    let mut piece = base.clone();
    let mut n = 2;
    while ctx.named_bodies.iter().any(|(_, known)| *known == piece) {
        piece = format!("{base} ({n})");
        n += 1;
    }
    ctx.named_bodies.retain(|(_, name)| *name != target);
    if let (2, Some((kept, _)), Some((cut, _))) = (outputs.len(), kept, cut) {
        ctx.named_bodies.push((kept.clone(), target.clone()));
        ctx.named_bodies.push((cut.clone(), piece.clone()));
    }
    Ok(feature(
        source,
        ctx,
        "split",
        json!({"body": target, "plane": plane, "body_names": [piece]}),
    ))
}
