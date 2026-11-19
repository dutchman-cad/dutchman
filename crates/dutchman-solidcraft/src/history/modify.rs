//! Body mirror, boolean and split operations.
//!
//! Both act on whole bodies named by earlier mapped features, never on faces of the final
//! body, which a later feature may already have removed or moved. Native and historical body
//! selections remain unresolved; a single generated result can resolve through its producer.

use serde_json::{json, Value};

use super::sketch::{cross, dot, unit, v3};
use super::{name, Context};

/// Only resolved operands have identities in the model's arenas: plain body lists, resolved
/// bodies with one native expression, and resolved rows that each keep their native member.
fn members(selection: &Value) -> Result<Vec<&Value>, String> {
    let value = selection.get("value");
    let rows = match selection.get("kind").and_then(Value::as_str) {
        Some("bodies") => value
            .and_then(Value::as_array)
            .map(|rows| rows.iter().collect()),
        Some("resolved") => value
            .and_then(|v| v.get("bodies"))
            .and_then(Value::as_array)
            .map(|rows| rows.iter().collect()),
        Some("resolved_set") => value
            .and_then(|v| v.get("members"))
            .and_then(Value::as_array)
            .and_then(|rows| rows.iter().map(|row| row.get("body")).collect()),
        found => return Err(super::unsupported_selection("body", found)),
    };
    rows.ok_or_else(|| "its bodies are not resolved".to_owned())
}

pub(super) fn body_names(selection: &Value, ctx: &Context) -> Result<Vec<String>, String> {
    if crate::VersionGate::ScopedBodies.supported(ctx.target_version)
        && selection.get("kind").and_then(Value::as_str) == Some("generated")
    {
        return generated_names(selection, ctx);
    }
    let mut names = Vec::new();
    for value in members(selection)? {
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

/// A single generated body can be identified with its producer's single named output.
/// Multi-body result order alone cannot establish that correspondence. The declared result
/// state must contain exactly that local body and belong to the mapped producer.
fn generated_names(selection: &Value, ctx: &Context) -> Result<Vec<String>, String> {
    let bodies = selection
        .pointer("/value/bodies")
        .and_then(Value::as_array)
        .ok_or("its generated bodies are not resolved")?;
    let mut names = Vec::new();
    for body in bodies {
        let producer = body
            .get("feature")
            .and_then(Value::as_str)
            .filter(|id| ctx.feature_names.contains_key(*id))
            .ok_or("its generated body has no earlier mapped producer")?;
        let local = body
            .get("local_id")
            .and_then(Value::as_str)
            .ok_or("its generated body identity is missing")?;
        let mut result = ctx
            .arena("feature_result_topologies")
            .iter()
            .filter(|row| row.get("output_of").and_then(Value::as_str) == Some(producer))
            .flat_map(|row| {
                row.get("members")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
            })
            .filter(|member| member.get("kind").and_then(Value::as_str) == Some("body"));
        if !result
            .next()
            .is_some_and(|member| member.get("id").and_then(Value::as_str) == Some(local))
            || result.next().is_some()
        {
            return Err("its generated body is not the producer's sole result body".to_owned());
        }
        let outputs = ctx
            .row("features", producer)
            .and_then(|row| row.get("outputs"))
            .and_then(Value::as_array)
            .filter(|outputs| outputs.len() == 1)
            .ok_or("its generated body has no single named output")?;
        let selected = json!({"kind": "bodies", "value": outputs});
        let resolved = body_names(&selected, ctx)?;
        names.extend(resolved);
    }
    if names.is_empty() {
        return Err("it selects no bodies".to_owned());
    }
    if names
        .iter()
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != names.len()
    {
        return Err("its generated selections repeat a body".to_owned());
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
    // One copied body and one new output have an unambiguous correspondence. A result
    // already named before this mirror is not a new copy, and duplicate display names
    // cannot identify the copy in SolidCraft.
    let names = if crate::VersionGate::ScopedBodies.supported(ctx.target_version)
        && bodies.len() == 1
        && source
            .get("outputs")
            .and_then(Value::as_array)
            .is_some_and(|outputs| {
                outputs.len() == 1
                    && outputs.iter().all(|output| {
                        output.as_str().is_some_and(|id| {
                            !ctx.named_bodies.iter().any(|(known, _)| known == id)
                                && ctx
                                    .row("bodies", id)
                                    .and_then(|row| row.get("name"))
                                    .and_then(Value::as_str)
                                    .is_none_or(|name| {
                                        !ctx.named_bodies.iter().any(|(_, known)| known == name)
                                    })
                        })
                    })
            }) {
        super::extrude::body_names(source, ctx, "new_body")
    } else {
        None
    };
    let mut mapped = feature(
        source,
        ctx,
        "mirror",
        json!({"features": [], "bodies": bodies, "plane": plane, "combine": false}),
    );
    if let (Some(names), Some(fields)) = (names, mapped.as_object_mut()) {
        fields.insert("body_names".to_owned(), json!(names));
    }
    Ok(mapped)
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
        if let Some(id) = row.get("id").and_then(Value::as_str) {
            for (arena, parent) in [("regions", "body"), ("faces", "shell")] {
                refs.extend(
                    ctx.children(arena, parent, id)
                        .iter()
                        .filter_map(|row| row.get("id").and_then(Value::as_str)),
                );
            }
        }
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

/// The body a split works on when it exists only earlier in the timeline (a historical
/// selection of one body, or a body no earlier feature has named): imported from the history
/// body the source keeps for it. None when the selection is any other.
fn consumed_target(
    source: &Value,
    selection: &Value,
    ctx: &mut Context,
) -> Result<Option<String>, String> {
    let historical = selection.get("kind").and_then(Value::as_str) == Some("historical");
    let refs = if historical {
        selection
            .pointer("/value/bodies")
            .and_then(Value::as_array)
            .map(|ids| ids.iter().filter_map(Value::as_str).collect::<Vec<_>>())
    } else {
        members(selection)
            .ok()
            .map(|ids| ids.into_iter().filter_map(Value::as_str).collect())
    };
    let [target] = refs.as_deref().unwrap_or_default() else {
        // A native target the source does not resolve: the body this split consumed, kept by
        // the source as its history body, is still the body it works on.
        if selection.get("kind").and_then(Value::as_str) == Some("native") {
            return super::base::import_consumed(source, "", ctx);
        }
        return Ok(None);
    };
    if !historical && ctx.named_bodies.iter().any(|(known, _)| known == target) {
        return Ok(None);
    }
    super::base::import_consumed(source, target, ctx)
}

/// Splits one body by a plane or coplanar input faces. SolidCraft keeps the body's name for the piece
/// on the side the plane's normal points to and names the piece on the other side; the
/// source's two result bodies are matched to them by the side their vertices lie on. When they
/// cannot be, neither piece keeps a source identity, so a later feature selecting one is
/// refused rather than given the wrong piece.
pub(super) fn split(
    source: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let names = ctx.named_bodies.clone();
    let mapped = split_bodies(source, definition, ctx);
    if mapped.is_err() {
        ctx.named_bodies = names;
    }
    mapped
}

/// The plane of a splitting tool given as faces, as a point on it and its unit normal. SolidCraft
/// splits by a plane, or by a face of a body it names; the source says which faces split the
/// body but not which body holds them, so only a planar face, whose surface extended is the
/// plane, is mapped. Faces that are not coplanar and selections the source does not resolve
/// to input faces are refused.
fn face_tool(source: &Value, value: &Value, ctx: &Context) -> Result<([f64; 3], [f64; 3]), String> {
    let found = value.get("kind").and_then(Value::as_str);
    let selection = match found {
        Some("historical") => value.get("value").unwrap_or(&Value::Null),
        Some("historical_partial") => {
            return Err(
                "some of its splitting faces are not resolved in its input topology".to_owned(),
            )
        }
        _ => return Err(super::unsupported_selection("splitting face", found)),
    };
    if !crate::VersionGate::SplitFaces.supported(ctx.target_version) {
        return Err(format!(
            "splitting by input faces needs SolidCraft 0.40 or newer (target {})",
            ctx.target_version.minor()
        ));
    }
    let several = selection
        .get("faces")
        .and_then(Value::as_array)
        .is_some_and(|faces| faces.len() > 1);
    // Several faces split as one plane when they all lie on it (the source often gives the
    // tool as the faces where the results meet).
    if several {
        let (origin, normal) = super::face_profile::coplanar_faces(source, selection, ctx)
            .map_err(|why| {
                if why.contains("not planar") {
                    "splitting a body by faces that do not lie on one plane is not supported yet"
                        .to_owned()
                } else {
                    why
                }
            })?;
        let normal = unit(normal).ok_or("its splitting faces are not placed")?;
        return Ok((origin, normal));
    }
    let (origin, normal) = super::face_profile::planar_face(source, selection, ctx).map_err(|why| {
        if why.contains("not planar") {
            "splitting a body by a face that is not flat is not supported yet: the source does not say which body holds the face".to_owned()
        } else {
            why
        }
    })?;
    let normal = unit(normal).ok_or("its splitting face is not placed")?;
    Ok((origin, normal))
}

fn split_bodies(source: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let selection = definition.get("targets").unwrap_or(&Value::Null);
    let targets = match consumed_target(source, selection, ctx)? {
        Some(name) => vec![name],
        None => body_names(selection, ctx)?,
    };
    let target = match targets.as_slice() {
        [target] => target.clone(),
        _ => return Err("splitting several bodies at once is not supported yet".to_owned()),
    };
    let tool = definition.get("tool").unwrap_or(&Value::Null);
    let (origin, normal) = match tool.get("kind").and_then(Value::as_str) {
        Some("plane") => {
            let datum = tool
                .pointer("/value/plane")
                .and_then(Value::as_str)
                .and_then(|id| ctx.planes.get(id))
                .ok_or("its splitting plane is not an earlier resolved datum plane")?;
            let frame = super::datum::value(datum);
            let origin = v3(frame.get("origin")).ok_or("its splitting plane is not placed")?;
            let normal = v3(frame.get("normal"))
                .and_then(unit)
                .ok_or("its splitting plane is not placed")?;
            (origin, normal)
        }
        Some("faces") => face_tool(source, tool.get("value").unwrap_or(&Value::Null), ctx)?,
        _ => return Err("its splitting tool is not resolved".to_owned()),
    };
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
