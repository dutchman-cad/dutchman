//! The parts of a decoded design, numbered, so one can be looked at, converted or reported on
//! alone.
//!
//! A design that places components is read as an [`Assembly`] of part models; a sub-assembly
//! is a part whose model holds an assembly of its own. Every part gets a number, counted from
//! 1 in the order [`catalog`] lists them: a part, then (when it is a sub-assembly) the parts
//! inside it, then the next part beside it. The number depends only on the design's
//! structure, so it is the same on every run and every machine, and it says nothing about
//! the part: it is what `dutchman diagnose` reports and `dutchman convert --part` takes.
//!
//! A design that places no components is one part, number 1.
//!
//! A part's history can be cut short too: [`stop_at_feature`] keeps its first N features and
//! leaves its stored shape alone, so writing it again tries the history up to feature N. N
//! counts the features in timeline order, leaving out those the source suppresses
//! ([`history_length`] says how many there are).
//!
//! Part names are private. [`PartInfo::name`] is for the key that stays on the user's
//! machine, never for anything shared.

use crate::{Assembly, Model};

/// One numbered part of a design.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartInfo {
    /// Its number, from 1.
    pub number: usize,
    /// The number of the sub-assembly that holds it, when one does.
    pub parent: Option<usize>,
    /// How many sub-assemblies it sits inside.
    pub depth: usize,
    /// Whether it is a sub-assembly: it holds other parts.
    pub assembly: bool,
    /// How many bodies it holds itself.
    pub bodies: usize,
    /// How many entities (bodies, faces, edges, features, sketches…) it holds itself.
    pub entities: u64,
    /// The same for it and everything inside it.
    pub subtree_entities: u64,
    /// Its name in the design. Private: keep it on this machine.
    pub name: String,
}

/// Why a part number cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectError {
    /// The design has no such part.
    Missing { number: usize, total: usize },
    /// The part only holds other parts, and has no geometry of its own.
    HoldsOnly { number: usize },
}

impl std::fmt::Display for SelectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing { number, total } if *total == 0 => {
                write!(f, "there is no part {number}: this design has no parts")
            }
            Self::Missing { number, total } => write!(
                f,
                "there is no part {number}: this design has {total} {} (numbered 1 to {total}, \
                 as `dutchman diagnose` numbers them)",
                if *total == 1 { "part" } else { "parts" }
            ),
            Self::HoldsOnly { number } => write!(
                f,
                "part {number} only holds other parts and has no geometry of its own; add \
                 --with-children to convert it with the parts inside it, or choose one of them"
            ),
        }
    }
}

/// The entities a model holds (its bodies, faces, edges, features, sketches and the rest of
/// CADIR's arenas), as a size to rank parts by.
pub fn entity_count(model: &Model) -> u64 {
    let m = &model.ir.model;
    let mut total = 0usize;
    macro_rules! count { ($($arena:ident),+ $(,)?) => { $( total = total.saturating_add(m.$arena.len()); )+ }; }
    count!(
        bodies,
        regions,
        shells,
        faces,
        loops,
        coedges,
        edges,
        vertices,
        points,
        curves,
        surfaces,
        pcurves,
        features,
        sketches,
        parameters,
        product_definitions,
        occurrences
    );
    total as u64
}

/// Every part of `model`, in number order.
pub fn catalog(model: &Model) -> Vec<PartInfo> {
    let mut out = Vec::new();
    match &model.assembly {
        None => out.push(PartInfo {
            number: 1,
            parent: None,
            depth: 0,
            assembly: false,
            bodies: model.ir.model.bodies.len(),
            entities: entity_count(model),
            subtree_entities: entity_count(model),
            name: model.name.clone(),
        }),
        Some(assembly) => list(assembly, None, 0, &mut out),
    }
    out
}

fn list(assembly: &Assembly, parent: Option<usize>, depth: usize, out: &mut Vec<PartInfo>) {
    for part in &assembly.parts {
        let number = out.len() + 1;
        let at = out.len();
        out.push(PartInfo {
            number,
            parent,
            depth,
            assembly: part.assembly.is_some(),
            bodies: part.ir.model.bodies.len(),
            entities: entity_count(part),
            subtree_entities: 0,
            name: part.name.clone(),
        });
        if let Some(inner) = &part.assembly {
            list(inner, Some(number), depth + 1, out);
        }
        let inside: u64 = out.iter().skip(at + 1).map(|p| p.entities).sum();
        if let Some(this) = out.get_mut(at) {
            this.subtree_entities = this.entities.saturating_add(inside);
        }
    }
}

/// The model of part `number` alone: its bodies and their history. With `children`, a
/// sub-assembly is kept with the parts inside it; without, only what it holds itself.
pub fn select(model: &Model, number: usize, children: bool) -> Result<Model, SelectError> {
    let total = catalog(model).len();
    let missing = SelectError::Missing { number, total };
    if number == 0 || number > total {
        return Err(missing);
    }
    let Some(assembly) = &model.assembly else {
        return Ok(model.clone());
    };
    let mut seen = 0usize;
    let part = walk(assembly, number, &mut seen).ok_or(missing)?;
    let Some(inner) = &part.assembly else {
        let mut alone = part.clone();
        alone.assembly = None;
        return Ok(alone);
    };
    if children {
        return Ok(part.clone());
    }
    match inner.own_part.and_then(|at| inner.parts.get(at)) {
        Some(own) => {
            let mut alone = own.clone();
            alone.assembly = None;
            Ok(alone)
        }
        None => Err(SelectError::HoldsOnly { number }),
    }
}

/// How many features of `model`'s history a conversion would try: those not suppressed.
pub fn history_length(model: &Model) -> usize {
    model
        .ir
        .model
        .features
        .iter()
        .filter(|feature| feature.suppressed != Some(true))
        .count()
}

/// The arena positions of `model`'s features, in timeline order.
fn timeline(model: &Model) -> Vec<usize> {
    let features = &model.ir.model.features;
    let mut order: Vec<usize> = (0..features.len()).collect();
    order.sort_by_key(|&at| features.get(at).map(|feature| feature.ordinal));
    order
}

/// Where feature `number` (from 1, counted as [`history_length`] counts) is in `model`'s
/// feature arena.
pub fn feature_position(model: &Model, number: usize) -> Option<usize> {
    let features = &model.ir.model.features;
    timeline(model)
        .into_iter()
        .filter(|&at| features.get(at).is_some_and(|f| f.suppressed != Some(true)))
        .nth(number.checked_sub(1)?)
}

/// Why a history cannot be cut at a feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PastTheEnd {
    /// The feature asked for.
    pub number: usize,
    /// How many features the history has.
    pub total: usize,
}

impl std::fmt::Display for PastTheEnd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "there is no feature {}: this history has {} {}",
            self.number,
            self.total,
            if self.total == 1 {
                "feature"
            } else {
                "features"
            }
        )
    }
}

impl std::error::Error for PastTheEnd {}

/// `model` with its history cut after feature `number`: the features before it and that one
/// stay (with any suppressed feature among them), and those after it go. 0 leaves no history.
/// The stored shape stays as it is, so a failure in the stored shape fails at every `number`.
pub fn stop_at_feature(model: &Model, number: usize) -> Result<Model, PastTheEnd> {
    let mut cut = model.clone();
    cut_history(&mut cut, number)?;
    Ok(cut)
}

fn cut_history(model: &mut Model, number: usize) -> Result<(), PastTheEnd> {
    let total = history_length(model);
    if number > total {
        return Err(PastTheEnd { number, total });
    }
    let last = feature_position(model, number);
    let mut keep = vec![false; model.ir.model.features.len()];
    if let Some(last) = last {
        for at in timeline(model) {
            if let Some(slot) = keep.get_mut(at) {
                *slot = true;
            }
            if at == last {
                break;
            }
        }
    }
    let mut flags = keep.into_iter();
    model
        .ir
        .model
        .features
        .retain(|_| flags.next().unwrap_or(false));
    Ok(())
}

/// A sub-assembly's parts, or the design's, around part `number`.
fn holder<'a>(
    owner: &'a Model,
    assembly: &'a Assembly,
    number: usize,
    seen: &mut usize,
) -> Option<(&'a Model, &'a Assembly, usize)> {
    for (at, part) in assembly.parts.iter().enumerate() {
        *seen += 1;
        if *seen == number {
            return Some((owner, assembly, at));
        }
        if let Some(inner) = &part.assembly {
            if let Some(found) = holder(part, inner, number, seen) {
                return Some(found);
            }
        }
    }
    None
}

/// Part `number` with the parts listed either side of it in what holds it, each where it is
/// placed and with no joints between them, its history cut after feature `stop_at` when given.
/// A design of one part is that part.
pub fn with_neighbours(
    model: &Model,
    number: usize,
    stop_at: Option<usize>,
) -> Result<Model, NeighbourError> {
    let total = catalog(model).len();
    let missing = NeighbourError::Part(SelectError::Missing { number, total });
    if number == 0 || number > total {
        return Err(missing);
    }
    let Some(assembly) = &model.assembly else {
        return match stop_at {
            Some(stop_at) => stop_at_feature(model, stop_at).map_err(NeighbourError::Feature),
            None => Ok(model.clone()),
        };
    };
    let mut seen = 0usize;
    let (owner, assembly, at) = holder(model, assembly, number, &mut seen).ok_or(missing)?;
    let wanted = at.saturating_sub(1)..=at.saturating_add(1);
    let mut parts = Vec::new();
    let mut definitions = Vec::new();
    let mut identities = Vec::new();
    let mut moved = vec![None; assembly.parts.len()];
    for (index, part) in assembly.parts.iter().enumerate() {
        if !wanted.contains(&index) {
            continue;
        }
        if let Some(slot) = moved.get_mut(index) {
            *slot = Some(parts.len());
        }
        let mut part = part.clone();
        if index == at {
            if let Some(stop_at) = stop_at {
                cut_history(&mut part, stop_at).map_err(NeighbourError::Feature)?;
            }
        }
        parts.push(part);
        definitions.push(assembly.definitions.get(index).cloned().flatten());
        // A part cut short is no longer the same as another with its identity.
        let cut = index == at && stop_at.is_some();
        identities.push(
            assembly
                .identities
                .get(index)
                .cloned()
                .flatten()
                .filter(|_| !cut),
        );
    }
    let placements = assembly
        .placements
        .iter()
        .filter_map(|placement| {
            let part = moved.get(placement.part).copied().flatten()?;
            let mut placement = placement.clone();
            placement.part = part;
            Some(placement)
        })
        .collect();
    let mut view = owner.clone();
    view.assembly = Some(Assembly {
        own_part: assembly
            .own_part
            .and_then(|part| moved.get(part).copied().flatten()),
        parts,
        definitions,
        identities,
        placements,
        joints: Vec::new(),
        left_out: Vec::new(),
        held: Vec::new(),
        joint_notes: Vec::new(),
        stored_placements: Vec::new(),
    });
    Ok(view)
}

/// Why [`with_neighbours`] has no view to give.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NeighbourError {
    /// The design has no such part.
    Part(SelectError),
    /// Its history has no such feature.
    Feature(PastTheEnd),
}

impl std::fmt::Display for NeighbourError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Part(why) => why.fmt(f),
            Self::Feature(why) => why.fmt(f),
        }
    }
}

fn walk<'a>(assembly: &'a Assembly, number: usize, seen: &mut usize) -> Option<&'a Model> {
    for part in &assembly.parts {
        *seen += 1;
        if *seen == number {
            return Some(part);
        }
        if let Some(inner) = &part.assembly {
            if let Some(found) = walk(inner, number, seen) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::{Format, Report};

    /// The synthetic unit cube with history rows at these ordinals, each `(ordinal, suppressed)`.
    fn with_history(rows: &[(u64, bool)]) -> Model {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/generated_unit_cube.f3d");
        let mut model = crate::read(Format::FusionArchive, &path, &mut Report::default()).unwrap();
        let mut ir = serde_json::to_value(&model.ir).unwrap();
        ir["model"]["features"] = Value::Array(
            rows.iter()
                .map(|(ordinal, suppressed)| {
                    json!({"id": format!("f3d:model:feature#{}", ordinal + 100),
                        "ordinal": ordinal, "name": format!("f{ordinal}"),
                        "suppressed": suppressed,
                        "definition": {"definition": "native", "kind": "X", "parameters": {}}})
                })
                .collect(),
        );
        model.ir = serde_json::from_value(ir).unwrap();
        model
    }

    fn ordinals(model: &Model) -> Vec<u64> {
        let mut kept: Vec<u64> = model.ir.model.features.iter().map(|f| f.ordinal).collect();
        kept.sort_unstable();
        kept
    }

    #[test]
    fn a_history_is_cut_in_timeline_order_counting_only_active_features() {
        // Arena order differs from timeline order, and ordinal 2 is suppressed.
        let model = with_history(&[(3, false), (1, false), (2, true), (4, false)]);
        assert_eq!(history_length(&model), 3);
        assert_eq!(ordinals(&stop_at_feature(&model, 0).unwrap()), [0u64; 0]);
        assert_eq!(ordinals(&stop_at_feature(&model, 1).unwrap()), [1]);
        assert_eq!(ordinals(&stop_at_feature(&model, 2).unwrap()), [1, 2, 3]);
        assert_eq!(ordinals(&stop_at_feature(&model, 3).unwrap()), [1, 2, 3, 4]);
        let past = stop_at_feature(&model, 4).unwrap_err();
        assert_eq!(
            past.to_string(),
            "there is no feature 4: this history has 3 features"
        );
        assert_eq!(
            model.ir.model.features[feature_position(&model, 2).unwrap()].ordinal,
            3
        );
        assert_eq!(feature_position(&model, 0), None);
        assert_eq!(feature_position(&model, 4), None);
    }

    #[test]
    fn missing_part_messages_name_the_range() {
        let text = SelectError::Missing {
            number: 9,
            total: 3,
        }
        .to_string();
        assert!(text.contains("there is no part 9"), "{text}");
        assert!(text.contains("3 parts (numbered 1 to 3"), "{text}");
        assert!(SelectError::Missing {
            number: 2,
            total: 1
        }
        .to_string()
        .contains("1 part ("));
    }
}
