//! Evidence for final history comparisons. Counts describe the evaluated representation;
//! they do not prove surface equivalence or manifoldness.
use super::*;
use cadmpeg_ir::features::ParameterValue;
use dutchman_core::{Model, Report};
use serde_json::json;

const MAX_ROWS: usize = 100_000;

fn skipped(reason: &str) -> Value {
    json!({"status": "not_checked", "reason": reason})
}

fn unavailable(reason: &str) -> Value {
    let mut checks = serde_json::Map::new();
    for check in [
        "body_count",
        "volume",
        "surface_area",
        "bounding_box",
        "faces",
        "edges",
        "shells",
        "closed_shell",
        "manifold",
        "centroid",
    ] {
        checks.insert(check.to_owned(), skipped(reason));
    }
    json!({"checks": checks, "bodies": []})
}

pub(crate) fn record_unavailable(report: &mut Report, part: &str, reason: &str) {
    report.record_verification_checks(json!({"part": part,
        "comparison":"final_history_against_source_geometry", "geometry":unavailable(reason),
        "parameters":[], "status":"not_checked", "reason":reason}));
}

fn measured(source: Value, written: Value, difference: f64, tolerance: f64) -> Value {
    if !difference.is_finite() || !tolerance.is_finite() || tolerance < 0.0 {
        return skipped("Measurements are not finite or ordered");
    }
    json!({"status": if within(difference, tolerance) { "pass" } else { "fail" },
        "source": source, "written": written, "difference": difference, "tolerance": tolerance})
}

fn numeric(source: Option<f64>, written: Option<f64>, tolerance: f64) -> Value {
    match source
        .zip(written)
        .filter(|(a, b)| a.is_finite() && b.is_finite())
    {
        Some((a, b)) => measured(json!(a), json!(b), (a - b).abs(), tolerance),
        None => skipped("Source or replay measurement is unavailable"),
    }
}

fn vector(value: Option<&Value>) -> Option<[f64; 3]> {
    let array = value?.as_array()?;
    if array.len() != 3 {
        return None;
    }
    Some([
        array.first()?.as_f64()?,
        array.get(1)?.as_f64()?,
        array.get(2)?.as_f64()?,
    ])
    .filter(|v| v.iter().all(|x| x.is_finite()))
}

fn body_check(want: &Measured, got: &Measured, source: &Value, written: &Value) -> Value {
    let allowed = want.size.zip(got.size).map(|(a, b)| {
        POSITION_BOX_ALLOWANCE
            .0
            .max(POSITION_BOX_ALLOWANCE.1 * a.max(b))
    });
    let volume_tolerance =
        (VOLUME_TOLERANCE * want.volume.abs().max(got.volume.abs())).max(mesh_slack(want, got));
    let mut checks = json!({
        "volume": numeric(source.get("volume_mm3").and_then(Value::as_f64), written.get("volume_mm3").and_then(Value::as_f64), volume_tolerance),
        "surface_area": numeric(want.area.filter(|v| *v >= 0.0), got.area.filter(|v| *v >= 0.0),
            AREA_TOLERANCE * want.area.unwrap_or(0.0).abs().max(got.area.unwrap_or(0.0).abs())),
        "bounding_box": skipped("Source or replay bounds or box centre is unavailable"),
        "centroid": skipped("Source or replay centre of mass or box size is unavailable"),
        "closed_shell": skipped("Source or replay closure is unavailable"),
        "manifold": skipped("The evaluator does not expose a manifold check")
    }).as_object().cloned().unwrap_or_default();
    if let (Some((a, b)), Some((c, d)), Some(at), Some(to), Some(tolerance)) =
        (want.bounds, got.bounds, want.center, got.center, allowed)
    {
        if a.iter()
            .zip(b)
            .chain(c.iter().zip(d))
            .all(|(lo, hi)| *lo <= hi)
        {
            let difference = a
                .iter()
                .chain(&b)
                .zip(c.iter().chain(&d))
                .map(|(a, b)| (a - b).abs())
                .fold(0.0, f64::max)
                .max(center_distance(&at, &to));
            checks.insert(
                "bounding_box".to_owned(),
                measured(
                    json!({"min":a,"max":b}),
                    json!({"min":c,"max":d}),
                    difference,
                    tolerance,
                ),
            );
        }
    }
    if let (Some(a), Some(b), Some(tolerance)) = (
        vector(source.get("center_of_mass")),
        vector(written.get("center_of_mass")),
        allowed,
    ) {
        checks.insert(
            "centroid".to_owned(),
            measured(json!(a), json!(b), center_distance(&a, &b), tolerance),
        );
    }
    for (name, pointer) in [
        ("faces", "/kernel/faces"),
        ("edges", "/kernel/edges"),
        ("shells", "/kernel/shells"),
    ] {
        let check = match (
            source.pointer(pointer).and_then(Value::as_u64),
            written.pointer(pointer).and_then(Value::as_u64),
        ) {
            (Some(a), Some(b)) => {
                json!({"status": if a == b {"info"} else {"differs"}, "source":a, "written":b})
            }
            _ => skipped("Source or replay kernel count is unavailable"),
        };
        checks.insert(name.to_owned(), check);
    }
    if let (Some(a), Some(b)) = (
        source.get("closed").and_then(Value::as_bool),
        written.get("closed").and_then(Value::as_bool),
    ) {
        checks.insert(
            "closed_shell".to_owned(),
            json!({"status": if a == b {"pass"} else {"fail"}, "source":a,"written":b}),
        );
    }
    Value::Object(checks)
}

pub(super) fn measurements(source: &Eval, written: &Eval) -> Value {
    let mut free: Vec<usize> = (0..written.bodies.len().min(MAX_ROWS)).collect();
    let mut bodies = Vec::new();
    for (index, want) in source.bodies.iter().take(MAX_ROWS).enumerate() {
        let nearest = free
            .iter()
            .enumerate()
            .min_by(|(_, a), (_, b)| {
                let rank = |slot: usize| {
                    written.bodies.get(slot).map(|got| {
                        (
                            want.center
                                .zip(got.center)
                                .map_or(f64::INFINITY, |(a, b)| center_distance(&a, &b)),
                            (want.volume - got.volume).abs(),
                        )
                    })
                };
                match (rank(**a), rank(**b)) {
                    (Some(a), Some(b)) => a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)),
                    _ => std::cmp::Ordering::Equal,
                }
            })
            .map(|(slot, _)| slot);
        if let Some(slot) = nearest {
            let paired = free.remove(slot);
            if let Some(got) = written.bodies.get(paired) {
                let raw = |eval: &Eval, slot| {
                    eval.raw
                        .get("bodies")
                        .and_then(|b| b.get(slot))
                        .cloned()
                        .unwrap_or(Value::Null)
                };
                bodies.push(json!({"source_index": index, "written_index": paired,
                    "checks": body_check(want, got, &raw(source,index), &raw(written,paired))}));
            }
        } else {
            let mut checks = serde_json::Map::new();
            for check in [
                "volume",
                "surface_area",
                "bounding_box",
                "centroid",
                "faces",
                "edges",
                "shells",
                "closed_shell",
                "manifold",
            ] {
                checks.insert(check.to_owned(), skipped("No replay body to pair"));
            }
            bodies.push(
                json!({"source_index": index, "written_index":null, "status":"not_checked",
                "reason":"No replay body to pair", "checks": checks}),
            );
        }
    }
    json!({"body_count": {"status": if source.bodies.len() == written.bodies.len() {"pass"} else {"fail"},
        "source":source.bodies.len(),"written":written.bodies.len()}, "bodies": bodies})
}

fn quantity_kind(unit: &str) -> Option<&'static str> {
    match unit {
        "mm" | "cm" | "m" | "um" | "µm" | "in" | "inch" | "ft" | "yd" | "km" | "mil" | "nm" => {
            Some("length")
        }
        "deg" | "rad" => Some("angle"),
        "" => Some("scalar"),
        "text" => Some("text"),
        _ => None,
    }
}

fn parameter_value(value: Option<&ParameterValue>) -> Option<f64> {
    match value {
        Some(ParameterValue::Length(v)) => Some(v.get()),
        Some(ParameterValue::Angle(v)) => Some(v.get().to_degrees()),
        Some(ParameterValue::Real(v)) => Some(v.get()),
        Some(ParameterValue::Integer(v)) if v.unsigned_abs() <= 9_007_199_254_740_992 => {
            Some(*v as f64)
        }
        _ => None,
    }
}

fn parameters(model: &Model, replayed: Result<Vec<Value>, String>) -> Value {
    let (_, names) = crate::history::params::map(model, &mut Report::default());
    let rows = replayed
        .as_ref()
        .ok()
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("parameters"))
        .and_then(Value::as_array);
    let oversized = rows.is_some_and(|rows| rows.len() > MAX_ROWS);
    let mut evaluated = std::collections::BTreeMap::new();
    if let Some(rows) = rows.filter(|_| !oversized) {
        for row in rows {
            if let Some(name) = row.get("name").and_then(Value::as_str) {
                evaluated.entry(name).or_insert(row);
            }
        }
    }
    model.ir.model.parameters.iter().take(MAX_ROWS).map(|parameter| {
        let name = names.get(parameter.id.as_str());
        let expected = parameter_value(parameter.value.as_ref());
        let unit = match parameter.value.as_ref() {
            Some(ParameterValue::Length(_)) => Some("mm"),
            Some(ParameterValue::Angle(_)) => Some("deg"),
            Some(ParameterValue::Real(_) | ParameterValue::Integer(_)) => Some(""),
            _ => None,
        };
        let found = name.and_then(|name| evaluated.get(name.as_str()).copied());
        let written_unit = found.and_then(|row| row.get("unit")).and_then(Value::as_str);
        let kinds = unit.and_then(quantity_kind).zip(written_unit.and_then(quantity_kind));
        let check = if name.is_none() {
            skipped("Source parameter was not mapped")
        } else if name.is_some_and(|name| name != &parameter.name) {
            skipped("Source parameter was renamed in the written design")
        } else if expected.is_none() {
            skipped("A comparable numeric source value is unavailable")
        } else if oversized {
            skipped("Replay parameter count exceeds the verification limit")
        } else if rows.is_none() {
            skipped("Replay parameter values are unavailable")
        } else if let Some(row) = found {
            // The values are already evaluated in mm/degrees. Only floating point
            // serialization roundoff is allowed; body meshing allowances do not apply.
            let tolerance = expected.map_or(0.0, |v| v.abs().max(1.0) * crate::tolerances::PARAMETER_RELATIVE);
            if row.get("error").is_some_and(|v| !v.is_null()) {
                json!({"status":"fail", "source":expected, "written":row.get("value"), "reason":"Replay parameter evaluation failed"})
            } else if let Some((source_kind, written_kind)) = kinds {
                if source_kind != written_kind {
                    json!({"status":"fail", "source":expected, "written":row.get("value"),
                        "reason":format!("Replay parameter quantity kind is {written_kind}; source is {source_kind}")})
                } else {
                    numeric(expected, row.get("value").and_then(Value::as_f64), tolerance)
                }
            } else {
                skipped("Replay parameter quantity unit is unavailable or not supported yet")
            }
        } else {
            skipped("Mapped parameter is absent or renamed after replay")
        };
        json!({"source_id":parameter.id, "name":name, "unit":unit, "written_unit":written_unit, "check":check})
    }).collect()
}

fn failed(value: &Value) -> bool {
    match value {
        Value::Object(fields) => {
            fields.get("status").and_then(Value::as_str) == Some("fail")
                || fields.iter().any(|(name, value)| {
                    !matches!(name.as_str(), "faces" | "edges" | "shells") && failed(value)
                })
        }
        Value::Array(rows) => rows.iter().any(failed),
        _ => false,
    }
}

fn failure_reason(evidence: &Value) -> String {
    let is_failure = |check: &Value| check.get("status").and_then(Value::as_str) == Some("fail");
    let values = |check: &Value| {
        let display = |key| {
            check
                .get(key)
                .filter(|value| !value.is_null())
                .map_or_else(|| "unavailable".to_owned(), Value::to_string)
        };
        let numbers = format!(
            "source {}, replay {}",
            display("source"),
            display("written")
        );
        match check.get("reason").and_then(Value::as_str) {
            Some(reason) => format!("{numbers}; {reason}"),
            None => numbers,
        }
    };
    if let Some(bodies) = evidence
        .pointer("/geometry/bodies")
        .and_then(Value::as_array)
    {
        for body in bodies {
            if let Some(checks) = body.get("checks").and_then(Value::as_object) {
                for (name, check) in checks {
                    if !matches!(name.as_str(), "faces" | "edges" | "shells") && is_failure(check) {
                        let index = body
                            .get("source_index")
                            .and_then(Value::as_u64)
                            .map(|v| v.saturating_add(1));
                        let label =
                            index.map_or_else(|| "a body".to_owned(), |v| format!("body {v}"));
                        return format!(
                            "{label} {} differs: {}",
                            name.replace('_', " "),
                            values(check)
                        );
                    }
                }
            }
        }
    }
    if let Some(parameters) = evidence.get("parameters").and_then(Value::as_array) {
        for parameter in parameters {
            if let Some(check) = parameter.get("check").filter(|check| is_failure(check)) {
                let name = parameter
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed");
                return format!("parameter {name} differs: {}", values(check));
            }
        }
    }
    "source comparison failed; see verification_checks for measurements".to_owned()
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn compare_report(
    cli: &(impl SolidCraftEvaluator + ?Sized),
    timeout: Duration,
    history: &[u8],
    original: &[u8],
    bodies: Expected,
    progress: Option<&dutchman_core::Progress<'_>>,
    model: &Model,
    consequence: &str,
    report: &mut Report,
) -> Outcome {
    let mut checks = unavailable("Source and replay measurements could not both be evaluated");
    let mut outcome = compare_measurements(
        cli,
        timeout,
        history,
        original,
        bodies,
        progress,
        &model.name,
        consequence,
        false,
        Some(&mut checks),
    );
    if checks.get("body_count").is_none() {
        if let Outcome::Unverified(reason) | Outcome::Differs(reason) = &outcome {
            checks = unavailable(reason);
        }
    }
    if matches!(outcome, Outcome::Matches)
        && checks.pointer("/body_count/source").and_then(Value::as_u64) == Some(0)
    {
        outcome = Outcome::Unverified("No source bodies were measured".to_owned());
    }
    let (_, names) = crate::history::params::map(model, &mut Report::default());
    let has_parameter_checks = model
        .ir
        .model
        .parameters
        .iter()
        .take(MAX_ROWS)
        .any(|parameter| {
            names.get(parameter.id.as_str()) == Some(&parameter.name)
                && parameter_value(parameter.value.as_ref()).is_some()
        });
    let params = if !has_parameter_checks {
        Ok(Vec::new())
    } else {
        cli.run(
            history,
            &[json!({"command":"parameters.list","params":{}})],
            timeout,
            "verification-parameters",
        )
    };
    let evidence = json!({"part": model.name, "comparison":"final_history_against_source_geometry",
        "geometry":checks, "parameters": parameters(model, params)});
    let failure = failed(&evidence).then(|| failure_reason(&evidence));
    // Open-body parsing uses an internal zero to keep the existing comparator usable.
    // That placeholder is never measured evidence: retain a real failed check, or
    // leave the comparison unverified. Feature errors and measured differences stay put.
    let unavailable_volume = evidence
        .pointer("/geometry/bodies")
        .and_then(Value::as_array)
        .is_some_and(|bodies| {
            bodies.iter().any(|body| {
                body.pointer("/checks/volume/status")
                    .and_then(Value::as_str)
                    == Some("not_checked")
            })
        });
    if unavailable_volume
        && matches!(&outcome, Outcome::Differs(reason) if reason.starts_with("a body's volume is "))
    {
        outcome = match &failure {
            Some(reason) => Outcome::Differs(reason.clone()),
            None => {
                Outcome::Unverified("No comparable volume was reported for an open body".to_owned())
            }
        };
    }
    report.record_verification_checks(evidence);
    match (outcome, failure) {
        (Outcome::Matches, Some(reason)) => Outcome::Differs(reason),
        (outcome, _) => outcome,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    #[cfg(not(target_family = "wasm"))]
    use dutchman_core::Writer;

    // Synthetic 10 mm cube measurements, confirmed with the evaluator. Mutations
    // model damaged written outputs; no third-party model bytes are used.
    fn cube() -> Value {
        json!({"bodies":[{"volume_mm3":1000.0,"area_mm2":600.0,
            "bbox":{"min":[0,0,0],"max":[10,10,10]},
            "center_of_mass":[5,5,5],"closed":true,
            "kernel":{"faces":6,"edges":12,"shells":1}}],"timeline":[]})
    }

    fn compare(written: Value) -> Value {
        measurements(&parse(&cube()).unwrap(), &parse(&written).unwrap())
    }

    fn status(report: &Value, check: &str) -> String {
        report["bodies"][0]["checks"][check]["status"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn cube_passes_geometric_checks_and_records_informational_counts() {
        let report = compare(cube());
        for check in [
            "volume",
            "surface_area",
            "bounding_box",
            "centroid",
            "closed_shell",
        ] {
            assert_eq!(status(&report, check), "pass", "{check}");
        }
        for check in ["faces", "edges", "shells"] {
            assert_eq!(status(&report, check), "info");
        }
        assert_eq!(status(&report, "manifold"), "not_checked");
        assert!(!failed(&report));
    }

    #[test]
    fn scaled_output_fails_volume_area_bounds_and_centroid() {
        let mut written = cube();
        written["bodies"][0]["volume_mm3"] = json!(8000);
        written["bodies"][0]["area_mm2"] = json!(2400);
        written["bodies"][0]["bbox"]["max"] = json!([20, 20, 20]);
        written["bodies"][0]["center_of_mass"] = json!([10, 10, 10]);
        let report = compare(written);
        for check in ["volume", "surface_area", "bounding_box", "centroid"] {
            assert_eq!(status(&report, check), "fail", "{check}");
        }
        assert_eq!(report["bodies"][0]["checks"]["volume"]["source"], 1000.0);
        assert_eq!(report["bodies"][0]["checks"]["volume"]["written"], 8000.0);
    }

    #[test]
    fn open_shell_and_missing_area_fail_while_counts_are_informational() {
        let mut written = cube();
        written["bodies"][0]["kernel"]["faces"] = json!(5);
        written["bodies"][0]["area_mm2"] = json!(500);
        written["bodies"][0]["closed"] = json!(false);
        written["bodies"][0]
            .as_object_mut()
            .unwrap()
            .remove("volume_mm3");
        let report = compare(written);
        for check in ["surface_area", "closed_shell"] {
            assert_eq!(status(&report, check), "fail", "{check}");
        }
        assert_eq!(status(&report, "faces"), "differs");
        assert!(failed(&report));
        assert_eq!(status(&report, "volume"), "not_checked");
        let mut written = cube();
        written["bodies"][0]["kernel"]["edges"] = json!(11);
        written["bodies"][0]["kernel"]["shells"] = json!(2);
        let report = compare(written);
        assert_eq!(status(&report, "edges"), "differs");
        assert_eq!(status(&report, "shells"), "differs");
        assert!(!failed(&report));
    }

    #[test]
    fn absent_measurements_never_become_passes() {
        let report = compare(json!({"bodies":[{"volume_mm3":1000}],"timeline":[]}));
        assert_eq!(status(&report, "volume"), "pass");
        for check in [
            "surface_area",
            "bounding_box",
            "centroid",
            "faces",
            "edges",
            "shells",
            "closed_shell",
            "manifold",
        ] {
            assert_eq!(status(&report, check), "not_checked", "{check}");
        }
        let empty = compare(json!({"bodies":[]}));
        assert_eq!(empty["body_count"]["status"], "fail");
        assert_eq!(empty["bodies"][0]["status"], "not_checked");
    }

    #[test]
    fn tolerance_boundaries_are_inclusive_and_nonfinite_values_do_not_pass() {
        assert_eq!(numeric(Some(1.0), Some(1.125), 0.125)["status"], "pass");
        assert_eq!(
            numeric(Some(1.0), Some(1.125 + f64::EPSILON), 0.125)["status"],
            "fail"
        );
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                numeric(Some(value), Some(value), 1.0)["status"],
                "not_checked"
            );
        }
        let mut written = cube();
        written["bodies"][0]["center_of_mass"] = json!([5.009, 5, 5]);
        assert_eq!(status(&compare(written.clone()), "centroid"), "pass");
        written["bodies"][0]["center_of_mass"] = json!([5.011, 5, 5]);
        assert_eq!(status(&compare(written), "centroid"), "fail");
        let mut written = cube();
        written["bodies"][0]["area_mm2"] = json!(600.5);
        assert_eq!(status(&compare(written.clone()), "surface_area"), "pass");
        written["bodies"][0]["area_mm2"] = json!(600.7);
        assert_eq!(status(&compare(written), "surface_area"), "fail");
    }

    #[test]
    fn measured_volume_and_box_allowances_reject_values_outside_the_boundary() {
        let mut written = cube();
        written["bodies"][0]["volume_mm3"] = json!(1001.0);
        assert_eq!(status(&compare(written.clone()), "volume"), "pass");
        written["bodies"][0]["volume_mm3"] = json!(1001.1);
        assert_eq!(status(&compare(written), "volume"), "fail");
        let mut written = cube();
        written["bodies"][0]["bbox"]["min"] = json!([0.009, 0, 0]);
        written["bodies"][0]["bbox"]["max"] = json!([10.009, 10, 10]);
        assert_eq!(status(&compare(written.clone()), "bounding_box"), "pass");
        written["bodies"][0]["bbox"]["min"] = json!([0.011, 0, 0]);
        written["bodies"][0]["bbox"]["max"] = json!([10.011, 10, 10]);
        assert_eq!(status(&compare(written), "bounding_box"), "fail");
    }

    #[test]
    fn diagonal_box_shifts_keep_the_existing_position_allowance() {
        let mut written = cube();
        written["bodies"][0]["bbox"] =
            json!({"min":[0.009,0.009,0.009],"max":[10.009,10.009,10.009]});
        written["bodies"][0]["center_of_mass"] = json!([5.009, 5.009, 5.009]);
        let report = compare(written);
        assert_eq!(status(&report, "bounding_box"), "fail");
        assert_eq!(status(&report, "centroid"), "fail");
    }

    #[test]
    fn box_centres_do_not_stand_in_for_missing_centroids() {
        let mut written = cube();
        written["bodies"][0]
            .as_object_mut()
            .unwrap()
            .remove("center_of_mass");
        let report = compare(written);
        assert_eq!(status(&report, "bounding_box"), "pass");
        assert_eq!(status(&report, "centroid"), "not_checked");
    }

    #[test]
    fn evidence_pairs_reordered_bodies_by_position() {
        let mut source = cube();
        let mut shifted = source["bodies"][0].clone();
        shifted["bbox"] = json!({"min":[20,0,0],"max":[30,10,10]});
        shifted["center_of_mass"] = json!([25, 5, 5]);
        source["bodies"]
            .as_array_mut()
            .unwrap()
            .push(shifted.clone());
        let written = json!({"bodies":[shifted, source["bodies"][0]]});
        let report = measurements(&parse(&source).unwrap(), &parse(&written).unwrap());
        assert!(!failed(&report));
        assert_eq!(report["bodies"][0]["written_index"], 1);
        assert_eq!(report["bodies"][1]["written_index"], 0);
    }

    fn model() -> Model {
        let mut wire = serde_json::to_value(cadmpeg_ir::examples::unit_cube().unwrap()).unwrap();
        wire["model"]["parameters"] = json!([{
            "id":"f3d:model:parameter#1", "name":"depth", "expression":"10 mm",
            "value":{"kind":"length","value":10}, "ordinal":0,
            "dependencies":[],"properties":{}
        }]);
        Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            fidelity: None,
            name: "Cube".to_owned(),
            source_file: None,
            source_step: None,
            assembly: None,
        }
    }

    #[test]
    fn source_parameter_values_are_checked_after_replay() {
        let model = model();
        let params = |value| {
            Ok(vec![
                json!({"parameters":[{"name":"depth","unit":"mm","value":value,"error":null}]}),
            ])
        };
        assert_eq!(
            parameters(&model, params(10.0))[0]["check"]["status"],
            "pass"
        );
        let wrong = parameters(&model, params(11.0));
        assert_eq!(wrong[0]["check"]["status"], "fail");
        assert_eq!(wrong[0]["check"]["source"], 10.0);
        assert_eq!(wrong[0]["check"]["written"], 11.0);
        let wrong_unit = parameters(
            &model,
            Ok(vec![
                json!({"parameters":[{"name":"depth","unit":"deg","value":10}]}),
            ]),
        );
        assert_eq!(wrong_unit[0]["check"]["status"], "fail");
        let no_unit = parameters(
            &model,
            Ok(vec![json!({"parameters":[{"name":"depth","value":10}]})]),
        );
        assert_eq!(no_unit[0]["check"]["status"], "not_checked");
        assert_eq!(
            parameters(&model, params(10.0 + 4e-11))[0]["check"]["status"],
            "pass"
        );
        assert_eq!(
            parameters(&model, params(10.0 + 6e-11))[0]["check"]["status"],
            "fail"
        );
        assert_eq!(
            parameters(&model, Err("unavailable".to_owned()))[0]["check"]["status"],
            "not_checked"
        );
        assert_eq!(
            parameters(&model, Ok(vec![json!({"parameters":[]})]))[0]["check"]["status"],
            "not_checked"
        );
    }

    #[test]
    fn omitted_or_unmatched_parameters_are_not_checked() {
        for rows in [
            json!([]),
            json!([{"name":"renamed_depth","unit":"mm","value":11}]),
        ] {
            let report = parameters(&model(), Ok(vec![json!({"parameters":rows})]));
            assert_eq!(report[0]["check"]["status"], "not_checked");
            assert!(!failed(&report));
        }
        let mut model = model();
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["parameters"][0]["expression"] = json!("unsupported()");
        wire["model"]["parameters"][0]
            .as_object_mut()
            .unwrap()
            .remove("value");
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let report = parameters(&model, Ok(vec![json!({"parameters":[]})]));
        assert_eq!(report[0]["name"], Value::Null);
        assert_eq!(report[0]["check"]["status"], "not_checked");
        assert!(!failed(&report));
    }

    #[test]
    fn oversized_parameter_lists_are_unchecked_instead_of_reported_missing() {
        let report = parameters(
            &model(),
            Ok(vec![json!({"parameters":vec![Value::Null;MAX_ROWS + 1]})]),
        );
        assert_eq!(report[0]["check"]["status"], "not_checked");
        assert!(report[0]["check"]["reason"]
            .as_str()
            .unwrap()
            .contains("limit"));
    }

    #[test]
    fn renamed_parameter_identity_is_retained_without_checking_its_value() {
        let mut model = model();
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["parameters"][0]["name"] = json!("depth value");
        let mut angle = wire["model"]["parameters"][0].clone();
        angle["id"] = json!("f3d:model:parameter#2");
        angle["name"] = json!("angle");
        angle["ordinal"] = json!(1);
        angle["expression"] = json!("90 deg");
        angle["value"] = json!({"kind":"angle","value":std::f64::consts::FRAC_PI_2});
        wire["model"]["parameters"]
            .as_array_mut()
            .unwrap()
            .push(angle);
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let report = parameters(
            &model,
            Ok(vec![json!({"parameters":[
            {"name":"depth_value","unit":"mm","value":11}, {"name":"angle","unit":"rad","value":90} ]})]),
        );
        assert_eq!(report[0]["source_id"], "f3d:model:parameter#1");
        assert_eq!(report[0]["name"], "depth_value");
        assert_eq!(report[0]["check"]["status"], "not_checked");
        assert_eq!(report[1]["check"]["status"], "pass");
        assert_eq!(report[1]["check"]["source"], 90.0);
        assert!(!failed(&report));
    }

    #[derive(Debug)]
    struct Host {
        written: Value,
        parameter: f64,
    }
    impl SolidCraftEvaluator for Host {
        fn evaluate(&self, _: &[u8], _: Duration, label: &str) -> Result<Value, String> {
            if label == "original" {
                Ok(cube())
            } else {
                Ok(self.written.clone())
            }
        }
        fn run(&self, _: &[u8], _: &[Value], _: Duration, _: &str) -> Result<Vec<Value>, String> {
            Ok(vec![
                json!({"parameters":[{"name":"depth","unit":"mm","value":self.parameter}]}),
            ])
        }
    }

    #[test]
    fn parameters_without_comparable_mapped_values_do_not_request_replay() {
        #[derive(Debug)]
        struct NoParameterReplay;
        impl SolidCraftEvaluator for NoParameterReplay {
            fn evaluate(&self, _: &[u8], _: Duration, _: &str) -> Result<Value, String> {
                Ok(cube())
            }
            fn run(
                &self,
                _: &[u8],
                _: &[Value],
                _: Duration,
                label: &str,
            ) -> Result<Vec<Value>, String> {
                panic!("Unexpected replay: {label}");
            }
        }
        for (name, expression) in [
            ("depth value", "10 mm"),
            ("depth", "unsupported()"),
            ("depth", "2 + 2"),
        ] {
            let mut model = model();
            let mut wire = serde_json::to_value(&model.ir).unwrap();
            wire["model"]["parameters"][0]["name"] = json!(name);
            wire["model"]["parameters"][0]["expression"] = json!(expression);
            if name == "depth" {
                wire["model"]["parameters"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("value");
            }
            model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
            let mut report = Report::default();
            let outcome = compare_report(
                &NoParameterReplay,
                DEFAULT_TIMEOUT,
                b"written",
                b"source",
                Expected {
                    solids: 1,
                    sheets: 0,
                },
                None,
                &model,
                "check",
                &mut report,
            );
            assert_eq!(outcome, Outcome::Matches);
            assert_eq!(
                report.verification_checks()[0]["parameters"][0]["check"]["status"],
                "not_checked"
            );
        }
    }

    #[test]
    fn split_faces_and_other_kernel_counts_do_not_reject_matching_geometry() {
        for (count, value) in [("faces", 7), ("edges", 13), ("shells", 2)] {
            let mut written = cube();
            written["bodies"][0]["kernel"][count] = json!(value);
            let host = Host {
                written,
                parameter: 10.0,
            };
            let mut report = Report::default();
            let outcome = compare_report(
                &host,
                DEFAULT_TIMEOUT,
                b"written",
                b"source",
                Expected {
                    solids: 1,
                    sheets: 0,
                },
                None,
                &model(),
                "check",
                &mut report,
            );
            assert_eq!(outcome, Outcome::Matches);
            let geometry = &report.verification_checks()[0]["geometry"];
            assert_eq!(status(geometry, count), "differs");
            assert_eq!(geometry["bodies"][0]["checks"][count]["written"], value);
            assert!(!failed(geometry));
        }
        // Topology counts cannot reject even if a producer supplies a fail status.
        assert!(!failed(&json!({"checks":{"faces":{"status":"fail"},
            "edges":{"status":"fail"},"shells":{"status":"fail"}}})));
    }

    #[test]
    fn final_comparison_records_evidence_and_downgrades_failed_checks() {
        for (parameter, closure, volume, differs) in [
            (10.0, true, 1000.0, false),
            (11.0, true, 1000.0, true),
            (10.0, false, 1000.0, true),
            (10.0, true, 8000.0, true),
        ] {
            let mut written = cube();
            written["bodies"][0]["closed"] = json!(closure);
            written["bodies"][0]["volume_mm3"] = json!(volume);
            let host = Host { written, parameter };
            let mut report = Report::default();
            let outcome = compare_report(
                &host,
                DEFAULT_TIMEOUT,
                b"written",
                b"source",
                Expected {
                    solids: 1,
                    sheets: 0,
                },
                None,
                &model(),
                "check",
                &mut report,
            );
            assert_eq!(matches!(outcome, Outcome::Differs(_)), differs);
            assert_eq!(report.verification_checks().len(), 1);
        }
    }
    #[test]
    fn missing_open_body_volume_preserves_replay_and_checks_closure() {
        let mut written = cube();
        written["bodies"][0]["closed"] = json!(false);
        written["bodies"][0]
            .as_object_mut()
            .unwrap()
            .remove("volume_mm3");
        let host = Host {
            written,
            parameter: 10.0,
        };
        assert!(matches!(
            replay(&host, DEFAULT_TIMEOUT, b"prefix").0,
            Outcome::Matches
        ));
        let mut report = Report::default();
        let outcome = compare_report(
            &host,
            DEFAULT_TIMEOUT,
            b"written",
            b"source",
            Expected {
                solids: 1,
                sheets: 0,
            },
            None,
            &model(),
            "check",
            &mut report,
        );
        assert!(
            matches!(&outcome, Outcome::Differs(reason) if reason.contains("closed shell") && !reason.contains("volume"))
        );
        assert_eq!(
            status(&report.verification_checks()[0]["geometry"], "closed_shell"),
            "fail"
        );
    }

    #[test]
    fn successful_parameter_checks_do_not_upgrade_an_unavailable_geometry_check() {
        #[derive(Debug)]
        struct Unavailable;
        impl SolidCraftEvaluator for Unavailable {
            fn evaluate(&self, _: &[u8], _: Duration, _: &str) -> Result<Value, String> {
                Err("Evaluation timed out".to_owned())
            }
            fn run(
                &self,
                _: &[u8],
                _: &[Value],
                _: Duration,
                _: &str,
            ) -> Result<Vec<Value>, String> {
                Ok(vec![
                    json!({"parameters":[{"name":"depth","unit":"mm","value":10}]}),
                ])
            }
        }
        let mut report = Report::default();
        let outcome = compare_report(
            &Unavailable,
            DEFAULT_TIMEOUT,
            b"written",
            b"source",
            Expected {
                solids: 1,
                sheets: 0,
            },
            None,
            &model(),
            "check",
            &mut report,
        );
        assert!(matches!(outcome, Outcome::Unverified(_)));
        assert_eq!(
            report.verification_checks()[0]["geometry"]["checks"]["volume"]["status"],
            "not_checked"
        );
        assert_eq!(
            report.verification_checks()[0]["parameters"][0]["check"]["status"],
            "pass"
        );
        assert!(report.verification_checks()[0]["geometry"]["checks"]
            .get("parameters")
            .is_none());
    }

    #[cfg(not(target_family = "wasm"))]
    #[test]
    fn native_replay_checks_written_designs_and_wrong_parameters() {
        let Ok(path) = find_cli(&Verification::Auto) else {
            eprintln!("No evaluator available for native comparison");
            return;
        };
        let evaluator = crate::CliEvaluator::new(path);
        let script = |scale: f64, depth: f64| {
            serde_json::to_vec(&json!([
            {"command":"parameters.add","params":{"name":"depth","expression":format!("{depth} mm")}},
            {"command":"solid.box","params":{"length":10.0*scale,"width":10.0*scale,"height":10.0*scale}}
        ])).unwrap()
        };
        let mut source = Vec::new();
        crate::SolidCraftWriter::new()
            .verification(Verification::Off)
            .write(&model(), &mut source, &mut Report::default())
            .unwrap();
        let mut open: Vec<Value> = serde_json::from_slice(&script(1.0, 10.0)).unwrap();
        open.push(json!({"command":"face.delete","params":{"body":"Body1","faces":[[5,5,10]],"heal":false}}));
        let mut report = Report::default();
        let outcome = compare_report(
            &evaluator,
            DEFAULT_TIMEOUT,
            &serde_json::to_vec(&open).unwrap(),
            &source,
            Expected {
                solids: 1,
                sheets: 0,
            },
            None,
            &model(),
            "check",
            &mut report,
        );
        assert!(matches!(outcome, Outcome::Differs(_)), "{outcome:?}");
        assert_eq!(
            status(&report.verification_checks()[0]["geometry"], "faces"),
            "differs"
        );
        for check in ["closed_shell", "surface_area"] {
            assert_eq!(
                status(&report.verification_checks()[0]["geometry"], check),
                "fail"
            );
        }
        for (scale, depth, differs, check) in [
            (1.0, 10.0, false, "volume"),
            (2.0, 10.0, true, "volume"),
            (1.0, 11.0, true, "parameters"),
        ] {
            let mut report = Report::default();
            let outcome = compare_report(
                &evaluator,
                DEFAULT_TIMEOUT,
                &script(scale, depth),
                &source,
                Expected {
                    solids: 1,
                    sheets: 0,
                },
                None,
                &model(),
                "check",
                &mut report,
            );
            assert_eq!(
                matches!(outcome, Outcome::Differs(_)),
                differs,
                "{outcome:?}"
            );
            let evidence = &report.verification_checks()[0];
            let status = if check == "parameters" {
                &evidence["parameters"][0]["check"]["status"]
            } else {
                &evidence["geometry"]["bodies"][0]["checks"][check]["status"]
            };
            assert_eq!(status, if differs { "fail" } else { "pass" });
        }
    }
}
