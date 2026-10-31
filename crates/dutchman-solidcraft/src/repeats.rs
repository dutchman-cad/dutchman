//! Parts of an assembly that repeat a part written earlier, at any level, so each is written
//! once: a part and its feature history are one definition however many sub-assemblies place
//! it. Occurrences are never shared by this for a leaf part. A sub-assembly's own occurrences
//! are, so one is only repeated when nothing reaches inside it: SolidCraft joints name an
//! occurrence, not the path to it, and could no longer tell two placements apart.

use std::collections::{BTreeMap, BTreeSet};

use dutchman_core::{Assembly, Model};

/// Where a part sits: its index in `parts`, level by level from the top assembly.
pub(crate) type PartPath = Vec<usize>;

/// Two placements closer than this in every cell are the same placement, as for the layouts
/// `dutchman-core` tells apart.
const TOLERANCE: f64 = 1e-7;

/// The sub-assemblies a placement path goes through, outermost first: each one's part path,
/// its identity, and the rest of the path from inside it.
fn through<'a>(
    mut assembly: &'a Assembly,
    path: &[usize],
) -> Vec<(PartPath, Option<&'a str>, Vec<usize>)> {
    let mut holders = Vec::new();
    let mut parts = Vec::new();
    for (depth, &index) in path.iter().enumerate() {
        let Some(placement) = assembly.placements.get(index) else {
            break;
        };
        parts.push(placement.part);
        let rest = path.get(depth + 1..).unwrap_or_default();
        let sub = assembly
            .parts
            .get(placement.part)
            .and_then(|part| part.assembly.as_ref());
        let (false, Some(sub)) = (rest.is_empty(), sub) else {
            break;
        };
        holders.push((
            parts.clone(),
            identity(assembly, placement.part),
            rest.to_vec(),
        ));
        assembly = sub;
    }
    holders
}

fn identity(assembly: &Assembly, part: usize) -> Option<&str> {
    assembly.identities.get(part)?.as_deref()
}

/// Whether two sub-assemblies place the same things at the same placements, under the same
/// names, shown or hidden alike: as they are now, whatever the source said of their layouts.
/// An unnamed placement takes its name from its component, so it would change if shared.
fn same_layout(a: &Assembly, b: &Assembly) -> bool {
    let close = |x: &[[f64; 4]; 3], y: &[[f64; 4]; 3]| {
        x.iter()
            .flatten()
            .zip(y.iter().flatten())
            .all(|(p, q)| (p - q).abs() <= TOLERANCE)
    };
    a.own_part == b.own_part
        && a.placements.len() == b.placements.len()
        && a.stored_placements.len() == b.stored_placements.len()
        && a.stored_placements
            .iter()
            .zip(&b.stored_placements)
            .all(|(x, y)| close(x, y))
        && a.placements.iter().zip(&b.placements).all(|(p, q)| {
            let (first, second) = (a.parts.get(p.part), b.parts.get(q.part));
            let inner = match (
                first.and_then(|part| part.assembly.as_ref()),
                second.and_then(|part| part.assembly.as_ref()),
            ) {
                (Some(x), Some(y)) => same_layout(x, y),
                (None, None) => true,
                _ => false,
            };
            p.hidden == q.hidden
                && p.name.is_some()
                && p.name == q.name
                && close(&p.transform, &q.transform)
                && identity(a, p.part).is_some()
                && identity(a, p.part) == identity(b, q.part)
                && inner
        })
}

#[derive(Default)]
struct Walk<'a> {
    /// The sub-assemblies a joint reaches inside.
    joined: BTreeSet<&'a str>,
    /// What is held to its place inside each sub-assembly, by the rest of its path.
    held: BTreeMap<PartPath, BTreeSet<Vec<usize>>>,
    /// The parts written so far, by identity.
    seen: BTreeMap<&'a str, Vec<(PartPath, &'a Model)>>,
    repeats: BTreeMap<PartPath, PartPath>,
}

impl<'a> Walk<'a> {
    fn first(&self, key: &str, path: &PartPath, part: &Model) -> Option<PartPath> {
        if part.assembly.is_some() && self.joined.contains(key) {
            return None;
        }
        let empty = BTreeSet::new();
        let held = |at: &PartPath| self.held.get(at).unwrap_or(&empty);
        let (first, _) = self.seen.get(key)?.iter().find(|(at, other)| {
            held(at) == held(path)
                && match (&other.assembly, &part.assembly) {
                    (Some(a), Some(b)) => same_layout(a, b),
                    (None, None) => true,
                    _ => false,
                }
        })?;
        Some(first.clone())
    }

    fn visit(&mut self, assembly: &'a Assembly, path: &mut PartPath) {
        for (index, part) in assembly.parts.iter().enumerate() {
            path.push(index);
            let key = identity(assembly, index);
            match key.and_then(|key| self.first(key, path, part)) {
                Some(first) => {
                    self.repeats.insert(path.clone(), first);
                }
                None => {
                    if let Some(key) = key {
                        let written = (path.clone(), part);
                        self.seen.entry(key).or_default().push(written);
                    }
                    if let Some(sub) = &part.assembly {
                        self.visit(sub, path);
                    }
                }
            }
            path.pop();
        }
    }
}

/// Each part that repeats one before it, with where that one is. The first is always reached
/// first walking the parts depth first, and is never inside a repeat.
pub(crate) fn find(assembly: &Assembly) -> BTreeMap<PartPath, PartPath> {
    let mut walk = Walk::default();
    for joint in &assembly.joints {
        for path in [&joint.a, &joint.b] {
            let holders = through(assembly, path);
            walk.joined.extend(holders.iter().filter_map(|h| h.1));
        }
    }
    for path in &assembly.held {
        for (holder, _, rest) in through(assembly, path) {
            walk.held.entry(holder).or_default().insert(rest);
        }
    }
    walk.visit(assembly, &mut Vec::new());
    walk.repeats
}

/// The leaf parts that are written, in the order they are written.
pub(crate) fn leaves<'a>(
    assembly: &'a Assembly,
    repeats: &BTreeMap<PartPath, PartPath>,
    path: &mut PartPath,
    parts: &mut Vec<&'a Model>,
) {
    for (index, part) in assembly.parts.iter().enumerate() {
        path.push(index);
        if !repeats.contains_key(path) {
            match &part.assembly {
                Some(sub) => leaves(sub, repeats, path, parts),
                None => parts.push(part),
            }
        }
        path.pop();
    }
}
