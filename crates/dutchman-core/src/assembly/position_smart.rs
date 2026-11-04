//! Deterministic, monotone admission of current poses. The geometry-free joint trial
//! runs before component extraction; the final layouts and feature mapping happen once.

use super::position_bounds::{self, Bounds};
use super::position_skeleton::Skeleton;
use super::*;
use crate::PartPositionCounts;

#[derive(Clone, Copy)]
enum Rejection {
    Joints,
    Bounds,
    Instances,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn admit(
    model: &Model,
    document: &Value,
    stored: &BTreeMap<String, Placement3x4>,
    proposals: &BTreeMap<String, (Option<String>, Placement3x4)>,
    candidates: &BTreeSet<String>,
    shown: &BTreeSet<String>,
    pinned: &BTreeSet<String>,
    guarded: &BTreeSet<String>,
    references: &BTreeMap<&str, &Value>,
    counts: &mut PartPositionCounts,
) -> Result<(BTreeSet<String>, BTreeSet<String>), String> {
    // Displayed poses bypass the current-pose guards, but a displayed holder (bound or
    // derived) is withdrawn with everything below it when its new pose carries a body
    // outside the envelope.
    let mut fixed = shown.clone();
    fixed.extend(pinned.iter().cloned());
    let mut guarded = guarded.clone();
    let mut below_of: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let rows = document
        .pointer("/model/occurrences")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let joint_count = document
        .pointer("/model/assembly_joints")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    if candidates
        .len()
        .saturating_mul(rows.len().saturating_add(joint_count))
        > 10_000_000
    {
        counts.unsupported = counts.unsupported.saturating_add(candidates.len() as u64);
        return Ok((BTreeSet::new(), fixed));
    }
    let by_id: BTreeMap<_, _> = rows
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    for (id, row) in &by_id {
        if let Some(parent) = row.pointer("/parent/occurrence").and_then(Value::as_str) {
            below_of.entry(parent).or_default().push(id);
        }
    }
    let subtree = |root: &str| {
        let mut all = BTreeSet::new();
        let mut open = vec![root];
        while let Some(at) = open.pop() {
            for child in below_of.get(at).into_iter().flatten() {
                if all.insert((*child).to_owned()) {
                    open.push(child);
                }
            }
        }
        all
    };
    let source = source_posed(document);
    let mut posed = source.clone();
    let mut baseline = Skeleton::build(document, stored)?;
    baseline.solve(&posed);
    let baseline_worlds = baseline.worlds();
    // Displayed poses outrank joint solving as well as the current-pose guards.
    posed.extend(fixed.iter().cloned());
    let definitions = position_bounds::definitions(model);
    let definitions_with_bodies: BTreeSet<_> = model
        .ir
        .model
        .product_definitions
        .iter()
        .filter(|d| !d.bodies.is_empty())
        .map(|d| d.id.as_str())
        .collect();
    let mut envelope: Option<Bounds> = None;
    for (id, row) in &by_id {
        let inner = definition_key(row)
            .and_then(|id| definitions.get(id))
            .and_then(|v| v.0);
        if let (Some(bound), Some(world)) = (inner, baseline_worlds.get(*id)) {
            let bound = bound.placed(world);
            match &mut envelope {
                Some(all) => all.include(bound),
                None => envelope = Some(bound),
            }
        }
    }
    let envelope = envelope.map(Bounds::grown);
    let mut groups: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (id, row) in &by_id {
        if let Some(reference) = row
            .get("native_ref")
            .and_then(Value::as_str)
            .and_then(|native| references.get(native))
        {
            if let Some(key) = definition_key(row)
                .or_else(|| reference.get("relative_path").and_then(Value::as_str))
            {
                groups.entry(key.to_owned()).or_default().push(id);
            }
        }
    }
    let mut accepted = candidates.clone();
    // Rejecting a pose can reveal another conflict. Each pass removes candidates;
    // a fixed cap bounds hostile dependency chains and conservatively keeps the rest stored.
    for _ in 0..MAX_DEPTH {
        if accepted.is_empty() && guarded.is_empty() {
            return Ok((accepted, fixed));
        }
        posed = source.clone();
        posed.extend(fixed.iter().cloned());
        let mut worlds = BTreeMap::new();
        // A proposal stores a desired world pose. Retained stored children follow
        // their parents; explicit current children retain their document-world pose.
        let mut open = BTreeSet::new();
        for id in by_id.keys() {
            let _ = trial_world(
                id,
                &by_id,
                stored,
                proposals,
                &accepted,
                &fixed,
                &mut worlds,
                &mut open,
                0,
            )?;
        }
        let mut trial = Skeleton::build(document, &worlds)?;
        let original_suppressed: Vec<_> =
            trial.assembly.joints.iter().map(|j| j.suppressed).collect();
        trial.solve(&posed);
        let after = trial.worlds();
        let mut rejected: BTreeMap<String, Rejection> = BTreeMap::new();
        for (index, joint) in trial.assembly.joints.iter().enumerate() {
            if original_suppressed.get(index) == Some(&true) {
                continue;
            }
            let closes = match (
                world_at(&trial.assembly, &joint.a),
                world_at(&trial.assembly, &joint.b),
            ) {
                (Some(a), Some(b)) => {
                    !joint.suppressed
                        && (as_built(&a, &b, joint).is_some()
                            || same(
                                &compose(&compose(&a, &joint.frame_a), &joint.meeting()),
                                &compose(&b, &joint.frame_b),
                            ))
                }
                _ => false,
            };
            if !closes {
                for id in &accepted {
                    if let Some(path) = trial.paths.get(id.as_str()).map(|p| &p.path) {
                        if affects(path, &joint.a) || affects(path, &joint.b) {
                            rejected.insert(id.clone(), Rejection::Joints);
                        }
                    }
                }
            }
        }
        // An unsupported touching joint cannot establish that a pose is safe.
        let occurrences: Vec<_> = rows.iter().collect();
        let wire_worlds: BTreeMap<_, _> = worlds.iter().map(|(id, p)| (id.as_str(), *p)).collect();
        for row in document
            .pointer("/model/assembly_joints")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if row.get("suppressed").and_then(Value::as_bool) == Some(true) {
                continue;
            }
            if joint(
                row,
                &trial.paths,
                &wire_worlds,
                &inserted_designs(&occurrences),
            )
            .is_ok()
            {
                continue;
            }
            for connector in row
                .pointer("/operands/connectors")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let Some(path) = connector
                    .pointer("/operand/container/occurrence")
                    .and_then(Value::as_str)
                    .and_then(|id| trial.paths.get(id))
                    .map(|p| &p.path)
                {
                    for id in &accepted {
                        if trial
                            .paths
                            .get(id.as_str())
                            .is_some_and(|p| affects(&p.path, path))
                        {
                            rejected.insert(id.clone(), Rejection::Joints);
                        }
                    }
                }
            }
        }
        // Check the entire placed subtree, including bodies owned by its holder.
        let mut geometry: BTreeMap<&str, (Option<Bounds>, bool)> = BTreeMap::new();
        let mut unsafe_paths = BTreeSet::new();
        let mut unsafe_bodies: Vec<Vec<usize>> = Vec::new();
        for (id, row) in &by_id {
            let has_bodies =
                definition_key(row).is_some_and(|id| definitions_with_bodies.contains(id));
            let bound = definition_key(row)
                .and_then(|id| definitions.get(id))
                .and_then(|v| v.1)
                .zip(after.get(*id))
                .map(|(b, w)| b.placed(w));
            let missing = has_bodies && bound.is_none();
            let moved = baseline_worlds
                .get(*id)
                .zip(after.get(*id))
                .is_some_and(|(a, b)| !super::poses::close(a, b));
            if has_bodies
                && moved
                && (missing
                    || !envelope
                        .zip(bound)
                        .is_some_and(|(all, part)| all.contains(part)))
            {
                if let Some(path) = trial.paths.get(*id) {
                    unsafe_paths.insert(path.path.clone());
                    // A part with a displayed pose of its own is only counted when what is
                    // known of it (its vertices, or its complete bounds) leaves the envelope;
                    // missing bounds cannot be blamed on the holder.
                    let certain = definition_key(row)
                        .and_then(|id| definitions.get(id))
                        .and_then(|v| v.0)
                        .zip(after.get(*id))
                        .map(|(b, w)| b.placed(w));
                    let outside = [bound, certain]
                        .into_iter()
                        .flatten()
                        .any(|part| !envelope.is_some_and(|all| all.contains(part)));
                    if !fixed.contains(*id) || outside {
                        unsafe_bodies.push(path.path.clone());
                    }
                }
            }
            let mut at = Some(*id);
            for _ in 0..=MAX_DEPTH {
                let Some(id) = at else { break };
                let value = geometry.entry(id).or_default();
                value.1 |= missing;
                if let Some(bound) = bound {
                    match &mut value.0 {
                        Some(all) => all.include(bound),
                        None => value.0 = Some(bound),
                    }
                }
                at = by_id
                    .get(id)
                    .and_then(|r| r.pointer("/parent/occurrence"))
                    .and_then(Value::as_str);
            }
        }
        // A joint can move an unposed partner outside the envelope. Attribute that
        // failure to candidates in the same touching joint island, not only its subtree.
        for pass in 0..MAX_DEPTH {
            let mut added = BTreeSet::new();
            for joint in &trial.assembly.joints {
                if unsafe_paths
                    .iter()
                    .any(|p| affects(p, &joint.a) || affects(p, &joint.b))
                {
                    if !joint.a.is_empty() {
                        added.insert(joint.a.clone());
                    }
                    if !joint.b.is_empty() {
                        added.insert(joint.b.clone());
                    }
                }
            }
            let before = unsafe_paths.len();
            unsafe_paths.extend(added);
            if unsafe_paths.len() == before {
                break;
            }
            if pass + 1 == MAX_DEPTH {
                // A dependency chain beyond the bounded trial cannot certify a pose.
                for id in &accepted {
                    rejected.entry(id.clone()).or_insert(Rejection::Bounds);
                }
            }
        }
        for id in &accepted {
            let safe = match (envelope, geometry.get(id.as_str())) {
                (Some(all), Some((Some(part), false))) => all.contains(*part),
                _ => false,
            };
            let unsafe_partner = trial
                .paths
                .get(id.as_str())
                .is_some_and(|p| unsafe_paths.iter().any(|bad| affects(&p.path, bad)));
            if !safe || unsafe_partner {
                rejected.entry(id.clone()).or_insert(Rejection::Bounds);
            }
        }
        // A withdrawn holder takes its whole subtree back to first placement. Only the
        // lowest displayed holder above an unsafe body is withdrawn in each pass.
        let mut withdrawn = BTreeSet::new();
        for holder in &guarded {
            let Some(path) = trial.paths.get(holder.as_str()).map(|p| &p.path) else {
                withdrawn.insert(holder.clone());
                continue;
            };
            let holds = |bad: &Vec<usize>| bad.starts_with(path);
            if !unsafe_bodies.iter().any(holds) {
                continue;
            }
            let lower = guarded.iter().any(|other| {
                other != holder
                    && trial
                        .paths
                        .get(other.as_str())
                        .is_some_and(|p| p.path.len() > path.len() && p.path.starts_with(path))
                    && trial
                        .paths
                        .get(other.as_str())
                        .is_some_and(|p| unsafe_bodies.iter().any(|bad| bad.starts_with(&p.path)))
            });
            if !lower {
                withdrawn.insert(holder.clone());
            }
        }
        for members in groups.values().filter(|members| members.len() > 1) {
            let deltas: Option<Vec<_>> = members
                .iter()
                .map(|id| {
                    Some(compose(
                        after.get(*id)?,
                        &inverse(baseline_worlds.get(*id)?),
                    ))
                })
                .collect();
            let rigid = deltas
                .as_ref()
                .and_then(|d| {
                    d.first()
                        .map(|first| d.iter().all(|delta| same(first, delta)))
                })
                .unwrap_or(false);
            if !rigid {
                for id in &accepted {
                    if let Some(path) = trial.paths.get(id.as_str()).map(|p| &p.path) {
                        if members.iter().any(|member| {
                            trial
                                .paths
                                .get(member)
                                .is_some_and(|p| affects(path, &p.path))
                        }) {
                            rejected.entry(id.clone()).or_insert(Rejection::Instances);
                        }
                    }
                }
            }
        }
        if rejected.is_empty() && withdrawn.is_empty() {
            return Ok((accepted, fixed));
        }
        for holder in &withdrawn {
            guarded.remove(holder);
            fixed.remove(holder);
            counts.bounds += 1;
            for id in subtree(holder) {
                guarded.remove(&id);
                fixed.remove(&id);
                if accepted.remove(&id) {
                    counts.bounds += 1;
                }
            }
        }
        for (id, why) in rejected {
            if accepted.remove(&id) {
                match why {
                    Rejection::Joints => counts.joints += 1,
                    Rejection::Bounds => counts.bounds += 1,
                    Rejection::Instances => counts.instances += 1,
                }
            }
        }
    }
    counts.unsupported = counts.unsupported.saturating_add(accepted.len() as u64);
    // A bounded trial that never settles cannot certify the displayed holders either.
    for holder in &guarded {
        for id in subtree(holder) {
            fixed.remove(&id);
        }
        fixed.remove(holder);
    }
    Ok((BTreeSet::new(), fixed))
}

fn affects(a: &[usize], b: &[usize]) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

#[allow(clippy::too_many_arguments)]
fn trial_world(
    id: &str,
    rows: &BTreeMap<&str, &Value>,
    stored: &BTreeMap<String, Placement3x4>,
    proposals: &BTreeMap<String, (Option<String>, Placement3x4)>,
    accepted: &BTreeSet<String>,
    fixed: &BTreeSet<String>,
    cache: &mut BTreeMap<String, Placement3x4>,
    open: &mut BTreeSet<String>,
    depth: usize,
) -> Result<Placement3x4, String> {
    if let Some(value) = cache.get(id) {
        return Ok(*value);
    }
    if depth > MAX_DEPTH || !open.insert(id.to_owned()) {
        return Err("part-position parents form a cycle or nest too deeply".to_owned());
    }
    let result = if accepted.contains(id) || fixed.contains(id) {
        let (member, pose) = proposals.get(id).ok_or("a current pose is missing")?;
        let insertion = match member {
            Some(member) => trial_world(
                member,
                rows,
                stored,
                proposals,
                accepted,
                fixed,
                cache,
                open,
                depth + 1,
            )?,
            None => IDENTITY,
        };
        compose(&insertion, pose)
    } else {
        let row = rows
            .get(id)
            .ok_or("a positioned occurrence has no parent")?;
        let original = *stored.get(id).ok_or("a stored pose is missing")?;
        match row.pointer("/parent/occurrence").and_then(Value::as_str) {
            Some(parent) => {
                let above = trial_world(
                    parent,
                    rows,
                    stored,
                    proposals,
                    accepted,
                    fixed,
                    cache,
                    open,
                    depth + 1,
                )?;
                compose(
                    &above,
                    &compose(
                        &inverse(stored.get(parent).ok_or("a stored parent is missing")?),
                        &original,
                    ),
                )
            }
            None => original,
        }
    };
    open.remove(id);
    cache.insert(id.to_owned(), result);
    Ok(result)
}
