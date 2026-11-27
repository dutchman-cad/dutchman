//! Body mirror, boolean, split and removal operations.
//!
//! They act on whole bodies named by earlier mapped features, never on faces of the final
//! body, which a later feature may already have removed or moved. A generated body selection
//! names a body by the feature that made it; a selection in the feature's own input state
//! names the one body that state holds. A generated result can also resolve through its
//! producer's single named output, or captured result identities with unique earlier mapped
//! names. Native selections and ambiguous or unproven identities are refused.

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

pub(super) fn body_names(
    source: &Value,
    selection: &Value,
    ctx: &Context,
) -> Result<Vec<String>, String> {
    Ok(bodies(source, selection, ctx)?
        .into_iter()
        .map(|(_, name)| name)
        .collect())
}

/// The bodies a selection names, each as (CADIR body id when the final model keeps it, name).
fn bodies(
    source: &Value,
    selection: &Value,
    ctx: &Context,
) -> Result<Vec<(Option<String>, String)>, String> {
    match selection.get("kind").and_then(Value::as_str) {
        Some("generated") => return generated_names(source, selection, ctx),
        Some("historical") => return input_state_body(source, selection, ctx),
        _ => {}
    }
    let mut names = Vec::new();
    for value in members(selection)? {
        let id = value.as_str().ok_or("its body reference is malformed")?;
        let (_, name) = ctx
            .named_bodies
            .iter()
            .find(|(known, _)| known == id)
            .ok_or("it selects a body that an earlier feature has not named")?;
        if ctx.body_name_shared(id, name) {
            return Err("its body name is not unique".to_owned());
        }
        names.push((Some(id.to_owned()), name.clone()));
    }
    if names.is_empty() {
        return Err("it selects no bodies".to_owned());
    }
    Ok(names)
}

/// Captured identities resolve only through a stored result and a unique earlier name.
fn captured_names(
    selection: &Value,
    ctx: &Context,
) -> Result<Vec<(Option<String>, String)>, String> {
    let bodies = selection
        .pointer("/value/bodies")
        .and_then(Value::as_array)
        .ok_or("its generated bodies are missing")?;
    let mut names: Vec<(Option<String>, String)> = Vec::new();
    for body in bodies {
        let feature = body
            .get("feature")
            .and_then(Value::as_str)
            .filter(|id| ctx.feature_names.contains_key(*id))
            .ok_or("its body creator is missing")?;
        let local = body
            .get("local_id")
            .and_then(Value::as_str)
            .ok_or("its body result identity is missing")?;
        let owner = ctx
            .row("features", feature)
            .ok_or("its body creator is missing")?;
        let known = super::reference::body_bindings(owner)?
            .is_some_and(|bindings| bindings.iter().any(|(id, _)| id == local));
        let proven = ctx.source.is_some_and(|source| {
            source
                .ir
                .model
                .feature_result_topologies
                .iter()
                .any(|state| {
                    state.output_of.as_str() == feature
                        && state.bodies().iter().any(|body| body.as_str() == local)
                })
        });
        if !known || !proven {
            return Err("its generated body is not a captured earlier feature result".into());
        }
        let matches: Vec<_> = ctx
            .named_bodies
            .iter()
            .filter(|(id, _)| id == local)
            .collect();
        let [(_, name)] = matches.as_slice() else {
            return Err("its generated body has no unique earlier name".into());
        };
        if ctx.body_name_shared(local, name) || names.iter().any(|(_, known)| known == name) {
            return Err("its generated body name is not unique".into());
        }
        names.push((Some(local.to_owned()), name.clone()));
    }
    if names.is_empty() {
        return Err("it selects no generated bodies".into());
    }
    Ok(names)
}

/// Bodies named by their producer, its sole output, or proven captured result identities.
/// Results absent from the final model can use producer names without a CADIR body id.
fn generated_names(
    source: &Value,
    selection: &Value,
    ctx: &Context,
) -> Result<Vec<(Option<String>, String)>, String> {
    let refs = selection
        .pointer("/value/bodies")
        .and_then(Value::as_array)
        .filter(|refs| !refs.is_empty())
        .ok_or("its generated body selection is malformed")?;
    let mut names: Vec<(Option<String>, String)> = Vec::new();
    for body in refs {
        let (Some(feature), Some(local)) = (
            body.get("feature").and_then(Value::as_str),
            body.get("local_id").and_then(Value::as_str),
        ) else {
            return Err("its generated body reference is malformed".to_owned());
        };
        let resolved = if let Some((_, name)) = ctx
            .generated_bodies
            .iter()
            .find(|((known, id), _)| known == feature && id == local)
        {
            (None, name.clone())
        } else if ctx
            .row("features", feature)
            .map(super::reference::body_bindings)
            .transpose()?
            .flatten()
            .is_some()
        {
            let selected = json!({"kind": "generated", "value": {"bodies": [body]}});
            captured_names(&selected, ctx)?
                .into_iter()
                .next()
                .ok_or("it selects no generated bodies")?
        } else if crate::VersionGate::ScopedBodies.supported(ctx.target_version) {
            let selected = json!({"kind": "generated", "value": {"bodies": [body]}});
            generated_output_bodies(source, &selected, ctx)?
                .into_iter()
                .next()
                .ok_or("its generated body has no single named output")?
        } else {
            return Err(
                "it selects a body made by an earlier feature that did not name it".to_owned(),
            );
        };
        if names.iter().any(|(_, known)| known == &resolved.1) {
            return Err("it selects one body twice".to_owned());
        }
        names.push(resolved);
    }
    Ok(names)
}

/// The body of a selection in the feature's own input state, when that state holds exactly
/// one body and the history so far has named exactly one: the two are then the same body.
/// A captured import can register final and historical ids under the same body name;
/// these aliases describe one live SolidCraft body, not two input candidates.
/// A state with several bodies says nothing about which named body each one is.
fn input_state_body(
    source: &Value,
    selection: &Value,
    ctx: &Context,
) -> Result<Vec<(Option<String>, String)>, String> {
    const UNSUPPORTED: &str = "a body from an earlier history state is not supported yet \
        when that state holds several bodies";
    let state = selection
        .pointer("/value/state")
        .and_then(Value::as_str)
        .and_then(|id| ctx.row("feature_input_topologies", id))
        .ok_or("its input body state is missing")?;
    let own = source.get("id").and_then(Value::as_str);
    if own.is_none() || state.get("input_of").and_then(Value::as_str) != own {
        return Err("its input body state belongs to another feature".to_owned());
    }
    let held_bodies = state
        .get("bodies")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    if held_bodies.len() > 1 {
        return slot_bodies(source, held_bodies, selection, ctx)
            .ok_or_else(|| UNSUPPORTED.to_owned());
    }
    let [held] = held_bodies else {
        return Err(UNSUPPORTED.to_owned());
    };
    let [selected] = selection
        .pointer("/value/bodies")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    else {
        return Err(UNSUPPORTED.to_owned());
    };
    if selected != held {
        return Err("it selects a body its input state does not hold".to_owned());
    }
    if ctx.unnamed_bodies {
        return Err(
            "its one input body cannot be matched to a named body: an earlier feature made a body the history could not name"
                .to_owned(),
        );
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut live = ctx
        .named_bodies
        .iter()
        .map(|(id, name)| (Some(id.clone()), name.clone()))
        .chain(
            ctx.generated_bodies
                .iter()
                .map(|(_, name)| (None, name.clone())),
        )
        .filter(|(_, name)| seen.insert(name.clone()));
    match (live.next(), live.next()) {
        (Some(body), None) => Ok(vec![body]),
        _ => Err("its one input body is not the one body an earlier feature named".to_owned()),
    }
}

/// The earlier-named bodies that the selected bodies of a several-body input state are.
/// A state body's id ends in its history slot, and the feature that first output a body
/// stores the slots it affected: a slot names the one body whose creating feature affected
/// exactly that slot. Any slot that is not held, repeated, or matched by other than one named
/// body leaves the whole selection unresolved.
fn slot_bodies(
    source: &Value,
    held: &[Value],
    selection: &Value,
    ctx: &Context,
) -> Option<Vec<(Option<String>, String)>> {
    let own_ordinal = source.get("ordinal").and_then(Value::as_u64)?;
    let slot_of = |id: &str| id.rsplit(':').next()?.parse::<u64>().ok();
    let affected = |feature: &Value| -> Option<Vec<u64>> {
        feature
            .pointer("/source_properties/affected_bodies")?
            .as_str()?
            .split(',')
            .map(|slot| slot.trim().parse().ok())
            .collect()
    };
    let selected = selection.pointer("/value/bodies")?.as_array()?;
    let mut found: Vec<(Option<String>, String)> = Vec::new();
    for body in selected {
        if !held.contains(body) {
            return None;
        }
        let slot = slot_of(body.as_str()?)?;
        let mut matches = Vec::new();
        for (id, name) in &ctx.named_bodies {
            let creator = ctx
                .arena("features")
                .iter()
                .filter(|row| {
                    row.get("ordinal")
                        .and_then(Value::as_u64)
                        .is_some_and(|ordinal| ordinal < own_ordinal)
                        && row
                            .get("outputs")
                            .and_then(Value::as_array)
                            .is_some_and(|outputs| {
                                outputs.iter().any(|out| out.as_str() == Some(id))
                            })
                })
                .min_by_key(|row| row.get("ordinal").and_then(Value::as_u64));
            if creator.and_then(affected) == Some(vec![slot]) {
                matches.push((Some(id.clone()), name.clone()));
            }
        }
        matches.dedup_by(|a, b| a.1 == b.1);
        let [one] = matches.as_slice() else {
            return None;
        };
        if found.iter().any(|(_, name)| name == &one.1) {
            return None;
        }
        found.push(one.clone());
    }
    (!found.is_empty()).then_some(found)
}

/// A removal of selected bodies from the model.
pub(super) fn remove(
    source: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    match definition.get("mode").and_then(Value::as_str) {
        Some("delete_selected") => {}
        Some("keep_selected") => {
            return Err(
                "removing every body except the selected ones is not supported yet".to_owned(),
            )
        }
        _ => return Err("its removal mode is not resolved".to_owned()),
    }
    if !crate::VersionGate::RemoveBody.supported(ctx.target_version) {
        return Err(format!(
            "removing a body is written for SolidCraft 0.40 or newer (target {})",
            ctx.target_version.minor()
        ));
    }
    let bodies = body_names(
        source,
        definition.get("bodies").unwrap_or(&Value::Null),
        ctx,
    )?;
    let out = feature(source, ctx, "remove", json!({"bodies": bodies}));
    ctx.named_bodies.retain(|(_, name)| !bodies.contains(name));
    ctx.generated_bodies
        .retain(|(_, name)| !bodies.contains(name));
    Ok(out)
}

/// A single generated body can be identified with its producer's single named output.
/// Multi-body result order alone cannot establish that correspondence. The declared result
/// state must contain exactly that local body and belong to the mapped producer.
fn generated_output_bodies(
    source: &Value,
    selection: &Value,
    ctx: &Context,
) -> Result<Vec<(Option<String>, String)>, String> {
    let refs = selection
        .pointer("/value/bodies")
        .and_then(Value::as_array)
        .ok_or("its generated bodies are not resolved")?;
    let mut names = Vec::new();
    for body in refs {
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
        let resolved = bodies(source, &selected, ctx)?;
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
    let target = body_names(source, operands.get("target").unwrap_or(&Value::Null), ctx)?;
    let target = match target.as_slice() {
        [target] => target.clone(),
        _ => return Err("combining several target bodies is not supported yet".to_owned()),
    };
    let tools = body_names(source, operands.get("tools").unwrap_or(&Value::Null), ctx)?;
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
        ctx.generated_bodies
            .retain(|(_, name)| !tools.contains(name));
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
    let mut seeds_found = Vec::new();
    let mut features = Vec::new();
    for seed in seeds {
        match seed.get("kind").and_then(Value::as_str) {
            Some("bodies") => {
                seeds_found.extend(bodies(
                    source,
                    seed.get("value").unwrap_or(&Value::Null),
                    ctx,
                )?);
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
    if seeds_found.is_empty() || !features.is_empty() {
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
    let combine = joins_its_seeds(source, &seeds_found);
    let bodies: Vec<String> = seeds_found.into_iter().map(|(_, name)| name).collect();
    // One copied body and one new output have an unambiguous correspondence. A result
    // already named before this mirror is not a new copy, and duplicate display names
    // cannot identify the copy in SolidCraft.
    let names = if crate::VersionGate::ScopedBodies.supported(ctx.target_version)
        && !combine
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
        json!({"features": [], "bodies": bodies, "plane": plane, "combine": combine}),
    );
    match (names, mapped.as_object_mut()) {
        (Some(names), Some(fields)) => {
            fields.insert("body_names".to_owned(), json!(names));
        }
        // A copy kept beside its seed is a body the history did not name.
        (None, _) if !combine => ctx.unnamed_bodies = true,
        _ => {}
    }
    Ok(mapped)
}

/// Whether a body mirror joins its copies to the bodies it mirrors: the bodies it outputs
/// are exactly its seed bodies, so it changed them instead of making new ones.
fn joins_its_seeds(source: &Value, seeds: &[(Option<String>, String)]) -> bool {
    let outputs: Vec<&str> = source
        .get("outputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let Some(seeds) = seeds
        .iter()
        .map(|(id, _)| id.as_deref())
        .collect::<Option<Vec<&str>>>()
    else {
        return false;
    };
    !outputs.is_empty()
        && outputs.len() == seeds.len()
        && seeds.iter().all(|id| outputs.contains(id))
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
    let generated = ctx.generated_bodies.clone();
    let mapped = split_bodies(source, definition, ctx);
    if mapped.is_err() {
        ctx.named_bodies = names;
        ctx.generated_bodies = generated;
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
        None => body_names(source, selection, ctx)?,
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
    while ctx.body_name_taken(&piece) {
        piece = format!("{base} ({n})");
        n += 1;
    }
    ctx.named_bodies.retain(|(_, name)| *name != target);
    ctx.generated_bodies.retain(|(_, name)| *name != target);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_several_body_state_resolves_a_body_by_its_history_slot() {
        let feature = |id: &str, ordinal: u64, out: &str, affected: &str| json!({"id": id, "ordinal": ordinal, "outputs": [out], "source_properties": {"affected_bodies": affected}});
        let state = json!({"id": "s", "input_of": "f:move",
            "bodies": ["s:1", "s:2"], "faces": [], "edges": [], "vertices": [], "edge_geometry": []});
        let model = json!({"features": [
            feature("f:a", 2, "a", "2"), feature("f:b", 1, "b", "1"),
            json!({"id": "f:move", "ordinal": 3, "outputs": ["a"], "source_properties": {"affected_bodies": "2"}})],
            "feature_input_topologies": [state]});
        let mut ctx = Context::testing(&model);
        ctx.named_bodies
            .extend([("a".into(), "Body A".into()), ("b".into(), "Body B".into())]);
        let pick = |slot: &str| {
            json!({"kind": "historical",
            "value": {"state": "s", "bodies": [slot], "native": "n"}})
        };
        let source = json!({"id": "f:move", "ordinal": 3});
        let got = input_state_body(&source, &pick("s:2"), &ctx).unwrap();
        assert_eq!(got, vec![(Some("a".to_owned()), "Body A".to_owned())]);
        let got = input_state_body(&source, &pick("s:1"), &ctx).unwrap();
        assert_eq!(got[0].1, "Body B");
        // A slot no earlier feature affected alone stays refused.
        let mut other = ctx.named_bodies.clone();
        other.push(("c".into(), "Body C".into()));
        ctx.named_bodies = other;
        assert!(input_state_body(&source, &pick("s:3"), &ctx).is_err());
        assert!(input_state_body(&source, &pick("s:1"), &ctx).is_ok());
    }

    #[test]
    fn a_single_historical_input_accepts_aliases_of_one_imported_body() {
        let model = json!({"feature_input_topologies": [{
            "id": "input", "input_of": "move", "bodies": ["input-body"]
        }]});
        let mut ctx = Context::testing(&model);
        // A captured base import names its final output and its historical topology
        // with the same SolidCraft body name, as import_history_body does.
        ctx.named_bodies.extend([
            ("final-body".into(), "Imported body".into()),
            ("captured-body".into(), "Imported body".into()),
        ]);
        let source = json!({"id": "move"});
        let selection = json!({"kind": "historical", "value": {
            "state": "input", "bodies": ["input-body"], "native": "retained"
        }});
        assert_eq!(
            body_names(&source, &selection, &ctx).unwrap(),
            ["Imported body"]
        );
        ctx.named_bodies
            .push(("other-body".into(), "Other body".into()));
        assert!(body_names(&source, &selection, &ctx)
            .unwrap_err()
            .contains("one body an earlier feature named"));
    }

    #[test]
    fn a_single_historical_input_is_not_the_named_body_beside_an_unnamed_one() {
        let model = json!({"feature_input_topologies": [{
            "id": "input", "input_of": "mirror", "bodies": ["input-body"]
        }]});
        let mut ctx = Context::testing(&model);
        ctx.named_bodies
            .push(("imported".into(), "Imported body".into()));
        let source = json!({"id": "mirror"});
        let selection = json!({"kind": "historical", "value": {
            "state": "input", "bodies": ["input-body"], "native": "seed"
        }});
        assert!(body_names(&source, &selection, &ctx).is_ok());
        // An earlier new body the history could not name may be the one the state holds.
        ctx.unnamed_bodies = true;
        assert!(body_names(&source, &selection, &ctx)
            .unwrap_err()
            .contains("could not name"));
        // A new body with no outputs and no stated result is such a body.
        let mut ctx = Context::testing(&model);
        let unnamed = json!({"id": "extrude", "outputs": []});
        assert!(super::super::extrude::body_names(&unnamed, &mut ctx, "new_body").is_none());
        assert!(ctx.unnamed_bodies);
        let mut ctx = Context::testing(&model);
        assert!(super::super::extrude::body_names(&unnamed, &mut ctx, "join").is_none());
        assert!(!ctx.unnamed_bodies);
    }

    #[test]
    fn removal_resolves_generated_bodies_from_both_identity_sources() {
        let model = json!({
            "features": [{"id": "producer", "outputs": ["body"]}],
            "feature_result_topologies": [{"id": "result", "output_of": "producer",
                "members": [{"kind": "body", "id": "local"}]}]
        });
        let mut ctx = Context::testing(&model);
        ctx.feature_names
            .insert("producer".into(), "Extrude".into());
        ctx.named_bodies.push(("body".into(), "Kept".into()));
        ctx.generated_bodies
            .push((("omitted".into(), "local".into()), "Omitted".into()));
        let selected = json!({"kind": "generated", "value": {"bodies": [
            {"feature": "producer", "local_id": "local"},
            {"feature": "omitted", "local_id": "local"}
        ]}});
        let removed = remove(
            &json!({"id": "remove"}),
            &json!({"mode": "delete_selected", "bodies": selected}),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(removed["bodies"], json!(["Kept", "Omitted"]));
        assert!(ctx.named_bodies.is_empty());
        assert!(ctx.generated_bodies.is_empty());
    }
}
