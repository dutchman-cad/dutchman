//! Local-only model inspection for minimum reproductions. No native decoding outside the
//! existing guarded decoder. None of the returned models or labels are safe to share.
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{catch_unwind, AssertUnwindSafe};

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodeDepth};
use serde_json::Value;

use crate::{decode, Error, Model, Report};

/// Re-run only cadmpeg's checks on the retained model, without decoding or recovery.
pub fn check(model: &Model) -> Result<(), Error> {
    catch_unwind(AssertUnwindSafe(|| {
        decode::validate_decoded(
            &mut model.ir.clone(),
            DecodeDepth::Full,
            &decode::policy(),
            0,
            &mut Report::default(),
        )
    }))
    .unwrap_or_else(|_| {
        Err(Error::Decode(
            "the model checker failed unexpectedly".into(),
        ))
    })
}

/// A dependency-closed IR subset, seeded by one arena ordinal (zero based).
/// References stay unchanged in memory; no identities or geometry should be published.
/// A subset that cannot deserialize is refused, never repaired with invented geometry.
pub fn subset(model: &Model, arena: &str, ordinal: usize) -> Result<Model, String> {
    catch_unwind(AssertUnwindSafe(|| subset_guarded(model, arena, ordinal)))
        .unwrap_or_else(|_| Err("the model subset could not be built safely".into()))
}

fn subset_guarded(model: &Model, arena: &str, ordinal: usize) -> Result<Model, String> {
    let mut document = serde_json::to_value(&model.ir).map_err(|e| e.to_string())?;
    let wire = document
        .get_mut("model")
        .and_then(Value::as_object_mut)
        .ok_or("model arenas are unavailable")?;
    let count = wire
        .values()
        .filter_map(Value::as_array)
        .try_fold(0usize, |sum, rows| sum.checked_add(rows.len()))
        .ok_or("subset entity limit reached")?;
    if count > 100_000 {
        return Err("subset entity limit reached".into());
    }
    let rows: BTreeMap<String, Value> = wire
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|row| Some((row.get("id")?.as_str()?.to_owned(), row.clone())))
        .collect();
    let seed = wire
        .get(arena)
        .and_then(Value::as_array)
        .and_then(|rows| rows.get(ordinal))
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .ok_or("subset seed is unavailable")?
        .to_owned();
    let mut keep = BTreeSet::new();
    let mut queue = vec![seed];
    while let Some(id) = queue.pop() {
        if !keep.insert(id.clone()) {
            continue;
        }
        if let Some(row) = rows.get(&id) {
            references(row, &rows, &keep, &mut queue);
        }
    }
    for values in wire.values_mut().filter_map(Value::as_array_mut) {
        values.retain(|row| {
            row.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| keep.contains(id))
        });
    }
    let mut subset = model.clone();
    subset.ir = serde_json::from_value(document).map_err(|e| e.to_string())?;
    subset.fidelity = None;
    subset.assembly = None;
    subset.source_step = None;
    Ok(subset)
}

fn references(
    value: &Value,
    rows: &BTreeMap<String, Value>,
    keep: &BTreeSet<String>,
    queue: &mut Vec<String>,
) {
    match value {
        Value::String(id) if rows.contains_key(id) && !keep.contains(id) => queue.push(id.clone()),
        Value::Array(values) => {
            for value in values {
                references(value, rows, keep, queue);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                if key != "name" && key != "label" {
                    references(value, rows, keep, queue);
                }
            }
        }
        _ => {}
    }
}

/// Local visitor for an archive member's ordinal, total, private name and decoded model.
pub type MemberVisitor<'a> = dyn FnMut(usize, usize, &str, Result<Model, Error>) -> bool + 'a;

pub use crate::decode::repro_members as members;

/// Structural specialization of parameter_domain. Uses the same carrier-domain query as
/// cadmpeg's check; does not inspect finding messages or reveal the range or carrier id.
pub fn use_curve_outside_carrier_count(model: &Model) -> u64 {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let curves: BTreeMap<_, _> = model
        .ir
        .model
        .curves
        .iter()
        .map(|curve| (&curve.id, &curve.geometry))
        .collect();
    catch_unwind(AssertUnwindSafe(|| {
        model
            .ir
            .model
            .coedges
            .iter()
            .filter(|coedge| {
                let Some(use_curve) = &coedge.use_curve else {
                    return false;
                };
                match curves.get(&use_curve.curve).copied() {
                    None => true,
                    Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs))) => {
                        let range = use_curve.parameter_range.endpoints();
                        cadmpeg_ir::eval::nurbs_curve_parameter_domain(nurbs)
                            .map(|domain| {
                                let domain = domain.endpoints();
                                // The pinned carrier check uses this public predicate and
                                // relative tolerance; keep classification consistent.
                                range.into_iter().any(|v| {
                                    !cadmpeg_ir::math::parameter_in_domain(v, domain, 1.0e-12)
                                })
                            })
                            .unwrap_or(true)
                    }
                    _ => false,
                }
            })
            .count() as u64
    }))
    .unwrap_or(0)
}

/// Entity arena kinds cited by structured error findings. The fixed arena names are safe;
/// referenced entity identities and checker messages never leave this function.
pub fn entity_kinds(model: &Model) -> Result<BTreeMap<String, u64>, Error> {
    catch_unwind(AssertUnwindSafe(|| {
        let arena = DecodeArena::new();
        let policy = decode::policy();
        let ctx = DecodeContext::for_loaded_input(&arena, &policy, 0)
            .map_err(|e| Error::Decode(e.to_string()))?;
        let validation =
            cadmpeg_ir::validate::validate_neutral_for_decode(&ctx, &model.ir, Vec::new())
                .map_err(|e| Error::Decode(e.to_string()))?;
        ctx.finish_session()
            .map_err(|e| Error::Decode(e.to_string()))?;
        let mut identities = BTreeMap::new();
        macro_rules! arenas { ($($arena:ident),+ $(,)?) => { $(
            for entity in &model.ir.model.$arena {
                identities.insert(entity.id.as_str(), stringify!($arena));
            }
        )+ }; }
        arenas!(
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
        let mut counts = BTreeMap::<String, u64>::new();
        for finding in validation
            .findings
            .iter()
            .filter(|f| f.severity >= cadmpeg_ir::report::Severity::Error)
        {
            let ids: BTreeSet<_> = finding.entity.iter().chain(&finding.related).collect();
            for id in ids {
                *counts
                    .entry(
                        identities
                            .get(id.as_str())
                            .copied()
                            .unwrap_or("other")
                            .into(),
                    )
                    .or_default() += 1;
            }
        }
        Ok(counts)
    }))
    .unwrap_or_else(|_| {
        Err(Error::Decode(
            "the model checker failed unexpectedly".into(),
        ))
    })
}
