//! The order of the history: one scope, one root timeline, nested timelines flattened.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use super::json::{kind, member, payload, rows, text};

/// How deep timelines nest before the projection declines.
const MAX_DEPTH: usize = 16;

/// The features of the history in order.
pub(super) struct Order {
    pub members: Vec<String>,
    /// The members the source rolled back: the one the rollback names and every later one.
    pub rolled_back: BTreeSet<String>,
}

pub(super) fn order(design: &Value) -> Result<Order, String> {
    let scopes: Vec<&Value> = rows(design, "scopes").collect();
    let [scope] = scopes.as_slice() else {
        return Err(format!(
            "the design states {} scopes and only one is projected",
            scopes.len()
        ));
    };
    if scope.get("active_configuration").is_some() {
        return Err(
            "the scope has an active configuration, whose overrides are not projected yet"
                .to_owned(),
        );
    }
    let roots = scope
        .get("roots")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let [root] = roots else {
        return Err(format!(
            "the scope has {} root containers and only one timeline is projected",
            roots.len()
        ));
    };
    let containers: BTreeMap<&str, &Value> = rows(design, "containers")
        .filter_map(|container| Some((text(payload(container), "id")?, container)))
        .collect();
    let root_id = root.as_str().ok_or("the scope's root is not an id")?;
    let mut flat = Flat::default();
    let rollback = timeline(&containers, root_id, 0, &mut flat)?;
    let mut rolled_back = BTreeSet::new();
    if let Some(start) = rollback {
        rolled_back.extend(flat.members.iter().skip(start).cloned());
    }
    Ok(Order {
        members: flat.members,
        rolled_back,
    })
}

#[derive(Default)]
struct Flat {
    members: Vec<String>,
}

/// Flattens a timeline. The result is the position the root's rollback names, when it names
/// one.
fn timeline(
    containers: &BTreeMap<&str, &Value>,
    id: &str,
    depth: usize,
    flat: &mut Flat,
) -> Result<Option<usize>, String> {
    if depth > MAX_DEPTH {
        return Err("the timelines nest too deeply".to_owned());
    }
    let container = containers
        .get(id)
        .ok_or_else(|| format!("the container {id} is not stated"))?;
    let body = payload(container);
    if kind(container) != Some("ordered") || text(body, "kind") != Some("timeline") {
        return Err(format!(
            "the container {id} is not a timeline, and only timelines are projected"
        ));
    }
    let members = body
        .get("members")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let mut local = BTreeMap::new();
    for entry in members {
        let (what, target) =
            member(entry).ok_or_else(|| format!("a member of {id} is malformed"))?;
        let position = flat.members.len();
        local.insert((what.to_owned(), target.to_owned()), position);
        match what {
            "feature" => {
                if flat.members.iter().any(|seen| seen == target) {
                    return Err(format!("the feature {target} appears twice in the history"));
                }
                flat.members.push(target.to_owned());
            }
            "container" => {
                if timeline(containers, target, depth + 1, flat)?.is_some() {
                    return Err(format!("the nested timeline {target} is rolled back"));
                }
            }
            other => return Err(format!("a member of {id} is a {other}")),
        }
    }
    let Some(rollback) = body.get("rollback") else {
        return Ok(None);
    };
    match kind(rollback) {
        Some("end") => Ok(None),
        Some("before") => {
            let (what, target) = member(payload(rollback)).ok_or("the rollback names no member")?;
            let at = local
                .get(&(what.to_owned(), target.to_owned()))
                .copied()
                .ok_or("the rollback names a member that is not in the timeline")?;
            if depth > 0 {
                Err("a nested timeline is rolled back".to_owned())
            } else {
                Ok(Some(at))
            }
        }
        _ => Err(format!("the rollback of {id} is malformed")),
    }
}
