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
mod tests {
    use super::*;

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
