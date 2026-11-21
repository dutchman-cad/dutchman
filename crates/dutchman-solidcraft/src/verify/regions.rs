//! STEP imports connected solid regions separately, while a Join can retain them in one body.
use super::{parse, Eval, Expected, Outcome, NO_VOLUME};
use serde_json::{json, Value};

pub(super) fn normalize(
    source: Eval,
    written: Eval,
    expected: Expected,
) -> Result<(Eval, Eval), Outcome> {
    let regions = expected.single_body_regions;
    if regions <= 1 {
        return Ok((source, written));
    }
    // Never hide a dropped region or an empty/inside-out imported piece in a sum.
    if source.bodies.len() != regions {
        return Err(Outcome::Differs(format!(
            "{} original regions imported, {regions} expected",
            source.bodies.len()
        )));
    }
    if written.bodies.len() != 1 {
        // When both evaluations split the body, retain every per-piece check.
        return Ok((source, written));
    }
    let source = aggregate(source)?;
    let written = aggregate(written)?;
    Ok((source, written))
}

fn aggregate(source: Eval) -> Result<Eval, Outcome> {
    let unavailable = || {
        Outcome::Unverified(
            "comparing a multi-region body requires positive volume, area and bounds for every imported region"
                .to_owned(),
        )
    };
    let mut volume = 0.0;
    let mut area = 0.0;
    let mut min = [f64::INFINITY; 3];
    let mut max = [f64::NEG_INFINITY; 3];
    for body in &source.bodies {
        let a = body.area.filter(|a| a.is_finite() && *a > 0.0);
        let bounds = body.bounds.filter(|(lo, hi)| {
            lo.iter()
                .zip(hi)
                .all(|(lo, hi)| lo.is_finite() && hi.is_finite() && lo <= hi)
        });
        let (Some(a), Some((lo, hi))) = (a, bounds) else {
            return Err(unavailable());
        };
        if !body.volume.is_finite() || body.volume <= NO_VOLUME {
            return Err(unavailable());
        }
        volume += body.volume;
        area += a;
        for ((min, max), (lo, hi)) in min.iter_mut().zip(&mut max).zip(lo.iter().zip(hi)) {
            *min = min.min(*lo);
            *max = max.max(hi);
        }
    }
    if !volume.is_finite() || !area.is_finite() {
        return Err(unavailable());
    }
    let mut body = serde_json::Map::from_iter([
        ("volume_mm3".to_owned(), json!(volume)),
        ("area_mm2".to_owned(), json!(area)),
        ("bbox".to_owned(), json!({"min": min, "max": max})),
    ]);
    // Preserve available evidence, without inventing closure or centroid measurements.
    let raw = source.raw.get("bodies").and_then(Value::as_array);
    if let Some(raw) = raw.filter(|raw| raw.len() == source.bodies.len()) {
        let closed: Option<Vec<bool>> = raw.iter().map(|v| v.get("closed")?.as_bool()).collect();
        if let Some(closed) = closed {
            body.insert("closed".to_owned(), json!(closed.iter().all(|v| *v)));
        }
        for name in ["faces", "edges"] {
            let count = raw
                .iter()
                .try_fold(0u64, |sum, v| sum.checked_add(v.get(name)?.as_u64()?));
            if let Some(count) = count {
                body.insert(name.to_owned(), json!(count));
            }
        }
        // A missing kind in a reported map means zero, as in the per-body check.
        // A missing map, malformed count or overflow makes the whole map unavailable.
        let kinds = raw.iter().try_fold(
            serde_json::Map::new(),
            |mut counts, v| -> Option<serde_json::Map<String, Value>> {
                for (kind, count) in v.get("face_types")?.as_object()? {
                    let previous = counts.get(kind).and_then(Value::as_u64).unwrap_or(0);
                    let sum = previous.checked_add(count.as_u64()?)?;
                    counts.insert(kind.clone(), json!(sum));
                }
                Some(counts)
            },
        );
        if let Some(kinds) = kinds {
            body.insert("face_types".to_owned(), Value::Object(kinds));
        }
        let mut kernel = serde_json::Map::new();
        for name in ["faces", "edges", "shells"] {
            let count = raw.iter().try_fold(0u64, |sum, v| {
                sum.checked_add(v.get("kernel")?.get(name)?.as_u64()?)
            });
            if let Some(count) = count {
                kernel.insert(name.to_owned(), json!(count));
            }
        }
        if !kernel.is_empty() {
            body.insert("kernel".to_owned(), Value::Object(kernel));
        }
        let centroid = raw.iter().try_fold([0.0; 3], |mut sum, v| {
            let at = super::evidence::vector(v.get("center_of_mass"))?;
            let weight = v.get("volume_mm3")?.as_f64()? / volume;
            for (sum, at) in sum.iter_mut().zip(at) {
                *sum += at * weight;
            }
            Some(sum)
        });
        if let Some(at) = centroid.filter(|at| at.iter().all(|v| v.is_finite())) {
            body.insert("center_of_mass".to_owned(), json!(at));
        }
    }
    let mut combined = parse(&json!({"bodies": [body]})).map_err(Outcome::Unverified)?;
    combined.errors = source.errors;
    combined.warnings = source.warnings;
    Ok(combined)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::verify::{evidence, match_bodies};

    fn piece(x: f64) -> Value {
        json!({"volume_mm3":1000.0,"area_mm2":600.0,
            "bbox":{"min":[x,0,0],"max":[x+10.0,10,10]},
            "center_of_mass":[x+5.0,5,5],"closed":true,
            "faces":6,"edges":12,"face_types":{"plane":6},
            "kernel":{"faces":6,"edges":12,"shells":1}})
    }

    fn joined() -> Value {
        json!({"volume_mm3":2000.0,"area_mm2":1200.0,
            "bbox":{"min":[0,0,0],"max":[40,10,10]},
            "center_of_mass":[20,5,5],"closed":true,
            "faces":12,"edges":24,"face_types":{"plane":12},
            "kernel":{"faces":12,"edges":24,"shells":2}})
    }

    fn evaluations(
        source: Vec<Value>,
        written: Vec<Value>,
        regions: usize,
    ) -> Result<(Eval, Eval), Outcome> {
        normalize(
            parse(&json!({"bodies": source})).unwrap(),
            parse(&json!({"bodies": written})).unwrap(),
            Expected {
                solids: 1,
                single_body_regions: regions,
                ..Expected::default()
            },
        )
    }

    #[test]
    fn a_multi_region_body_compares_sums_bounds_and_evidence() {
        let (source, written) =
            evaluations(vec![piece(0.0), piece(30.0)], vec![joined()], 2).unwrap();
        assert_eq!(
            match_bodies(&written.bodies, &source.bodies),
            Outcome::Matches
        );
        let checks = evidence::measurements(&source, &written);
        assert_eq!(checks["body_count"]["status"], "pass");
        for name in [
            "volume",
            "surface_area",
            "bounding_box",
            "centroid",
            "closed_shell",
            "merged_faces",
            "analytic_faces",
        ] {
            assert_eq!(
                checks["bodies"][0]["checks"][name]["status"], "pass",
                "{checks}"
            );
        }
        assert_eq!(checks["bodies"][0]["checks"]["shells"]["source"], 2);
    }

    #[test]
    fn incomplete_or_overflowing_topology_stays_unavailable() {
        for field in ["faces", "edges", "face_types"] {
            for value in [
                None,
                Some(json!("unavailable")),
                Some(json!({"plane":"unavailable"})),
                Some(json!(u64::MAX)),
            ] {
                let mut region = piece(30.0);
                if field == "face_types" && value == Some(json!(u64::MAX)) {
                    region[field] = json!({"plane":u64::MAX});
                } else if let Some(value) = value {
                    region[field] = value;
                } else {
                    region.as_object_mut().unwrap().remove(field);
                }
                let (source, written) =
                    evaluations(vec![piece(0.0), region], vec![joined()], 2).unwrap();
                let checks = evidence::measurements(&source, &written);
                let check = match field {
                    "faces" => "merged_faces",
                    "edges" => "merged_edges",
                    _ => "analytic_faces",
                };
                assert_eq!(
                    checks["bodies"][0]["checks"][check]["status"], "not_checked",
                    "{checks}"
                );
                assert!(source.raw["bodies"][0].get(field).is_none());
            }
        }
    }

    #[test]
    fn freeform_and_mesh_regions_keep_their_topology_exclusions() {
        for kind in ["other", "mesh"] {
            let mut region = piece(30.0);
            region["face_types"][kind] = json!(1);
            let (source, written) =
                evaluations(vec![piece(0.0), region], vec![joined()], 2).unwrap();
            let checks = evidence::measurements(&source, &written);
            assert_eq!(
                checks["bodies"][0]["checks"]["merged_faces"]["status"],
                "not_checked"
            );
            assert_eq!(
                checks["bodies"][0]["checks"]["analytic_faces"]["status"],
                if kind == "mesh" {
                    "not_checked"
                } else {
                    "pass"
                }
            );
        }
    }

    #[test]
    fn a_multi_region_body_still_rejects_loss_area_changes_and_movement() {
        for (field, value) in [
            ("volume_mm3", json!(1900)),
            ("area_mm2", json!(1100)),
            ("bbox", json!({"min":[1,0,0],"max":[40,10,10]})),
        ] {
            let mut body = joined();
            body[field] = value;
            let (source, written) =
                evaluations(vec![piece(0.0), piece(30.0)], vec![body], 2).unwrap();
            assert!(matches!(
                match_bodies(&written.bodies, &source.bodies),
                Outcome::Differs(_)
            ));
        }
        assert!(matches!(
            evaluations(vec![piece(0.0)], vec![joined()], 2),
            Err(Outcome::Differs(_))
        ));
        for field in ["volume_mm3", "area_mm2", "bbox"] {
            let mut missing = piece(30.0);
            missing.as_object_mut().unwrap().remove(field);
            assert!(evaluations(vec![piece(0.0), missing], vec![joined()], 2).is_err());
        }
    }

    #[test]
    fn single_region_and_split_replays_keep_the_existing_per_piece_checks() {
        let (source, written) =
            evaluations(vec![piece(0.0), piece(30.0)], vec![joined()], 1).unwrap();
        assert!(matches!(
            match_bodies(&written.bodies, &source.bodies),
            Outcome::Differs(_)
        ));
        let mut changed = piece(30.0);
        changed["volume_mm3"] = json!(900);
        let (source, written) =
            evaluations(vec![piece(0.0), piece(30.0)], vec![piece(0.0), changed], 2).unwrap();
        assert!(matches!(
            match_bodies(&written.bodies, &source.bodies),
            Outcome::Differs(_)
        ));
    }
}
