#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;
use std::collections::BTreeMap;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures")
        .join(name)
}

/// Three independent cubes from the public synthetic fixture. Rename identities in
/// memory, then add a periodic NURBS use-curve crossing its stored parameter seam to
/// body #2. No native file or private geometry is needed.
fn seam_model(count: usize) -> Model {
    let mut model = dutchman_core::read(
        Format::FusionArchive,
        &fixture("generated_unit_cube.f3d"),
        &mut Report::default(),
    )
    .unwrap();
    let body_ids: Vec<_> = model
        .ir
        .model
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect();
    model = dutchman_core::with_bodies(&model, &body_ids, "Synthetic".into()).unwrap();
    let source = serde_json::to_value(&model.ir).unwrap();
    let mut document = source.clone();
    let coedges_per_cube = model.ir.model.coedges.len();
    let failing_cubes = count.div_ceil(coedges_per_cube).max(1);
    let mut combined = serde_json::Map::new();
    for body in 0..failing_cubes + 2 {
        let mut wire = source["model"].clone();
        let ids: BTreeMap<String, String> = wire
            .as_object()
            .unwrap()
            .values()
            .filter_map(Value::as_array)
            .flatten()
            .enumerate()
            .filter_map(|(i, row)| {
                Some((
                    row["id"].as_str()?.into(),
                    format!("secret:part{body}:entity#{i:06}"),
                ))
            })
            .collect();
        fn remap(value: &mut Value, ids: &BTreeMap<String, String>) {
            match value {
                Value::String(id) => {
                    if let Some(new) = ids.get(id) {
                        *id = new.clone();
                    }
                }
                Value::Array(values) => {
                    for value in values {
                        remap(value, ids);
                    }
                }
                Value::Object(values) => {
                    for value in values.values_mut() {
                        remap(value, ids);
                    }
                }
                _ => {}
            }
        }
        remap(&mut wire, &ids);
        wire["bodies"][0]["name"] = json!(format!("PrivateBody{body}"));
        for (arena, rows) in wire.as_object().unwrap() {
            let target = combined.entry(arena.clone()).or_insert_with(|| json!([]));
            target
                .as_array_mut()
                .unwrap()
                .extend(rows.as_array().unwrap().clone());
        }
    }
    document["model"] = Value::Object(combined);
    model.ir = serde_json::from_value(document).unwrap();
    // One failing body may contain several disjoint cube regions; this allows the
    // worked example to produce exactly 88 findings without changing the check.
    let extra_regions: Vec<_> = model
        .ir
        .model
        .bodies
        .iter()
        .skip(2)
        .take(failing_cubes - 1)
        .flat_map(|body| body.regions.clone())
        .collect();
    let owner = model.ir.model.bodies[1].id.clone();
    for region in &mut model.ir.model.regions {
        if extra_regions.contains(&region.id) {
            region.body = owner.clone();
        }
    }
    model.ir.model.bodies[1].regions.extend(extra_regions);
    let last = model.ir.model.bodies.pop().unwrap();
    model.ir.model.bodies.truncate(2);
    model.ir.model.bodies.push(last);
    model.ir.model.bodies[2].name = Some("PrivateBody2".into());
    let checked = dutchman_core::repro::check(&model);
    assert!(checked.is_ok(), "{checked:?}");
    let vertices: BTreeMap<_, _> = model
        .ir
        .model
        .vertices
        .iter()
        .map(|vertex| {
            let point = model
                .ir
                .model
                .points
                .iter()
                .find(|point| point.id == vertex.point)
                .unwrap();
            (
                vertex.id.clone(),
                serde_json::to_value(point).unwrap()["position"].clone(),
            )
        })
        .collect();
    let endpoints: BTreeMap<_, _> = model
        .ir
        .model
        .edges
        .iter()
        .map(|edge| {
            (
                edge.id.clone(),
                (vertices[&edge.start].clone(), vertices[&edge.end].clone()),
            )
        })
        .collect();
    let prototype = model.ir.model.curves[0].clone();
    for (i, coedge) in model
        .ir
        .model
        .coedges
        .iter_mut()
        .skip(coedges_per_cube)
        .take(count)
        .enumerate()
    {
        let (mut start, mut end) = endpoints[&coedge.edge].clone();
        if serde_json::to_value(coedge.sense).unwrap() == "reversed" {
            std::mem::swap(&mut start, &mut end);
        }
        // A closed periodic degree-one NURBS loop used over more than one period: at .75
        // it reaches the traversal start, and at 1.80 (wrapped to .80) the traversal end.
        // Crossing the seam alone is valid; a range wider than the period is not. Matching
        // endpoints avoid introducing a second geometric_consistency failure class.
        let mut curve = prototype.clone();
        curve.id = serde_json::from_value(json!(format!("secret:part1:curve#seam{i:06}"))).unwrap();
        curve.geometry = serde_json::from_value(json!({
            "kind": "nurbs", "degree": 1, "knots": [0.0, 0.0, 0.25, 0.75, 0.8, 1.0, 1.0],
            "poles": {"form": "polynomial", "points": [{"x": 100.0, "y": 100.0, "z": 100.0}, {"x": 100.0, "y": 100.0, "z": 100.0}, start, end, {"x": 100.0, "y": 100.0, "z": 100.0}]},
            "periodic": true
        })).unwrap();
        coedge.use_curve = Some(
            serde_json::from_value(json!({"curve": curve.id, "parameter_range": [0.75, 1.80]}))
                .unwrap(),
        );
        model.ir.model.curves.push(curve);
    }
    model.ir.model.curves.sort_by(|a, b| a.id.cmp(&b.id));
    model.name = "PrivateDesign 123.456".into();
    model.source_file = Some("C:\\PrivateFolder\\PrivateDesign.f3d".into());
    model
}

#[test]
fn repro_seam_check_narrows_to_second_body_and_never_leaks_private_data() {
    let model = seam_model(1);
    let writer = ConvertOptions::default().writer();
    let error = dutchman_core::repro::check(&model).unwrap_err();
    let failure = diagnostics::anonymous_failure(&crate::json::error(
        crate::json::CONVERT,
        Value::Null,
        &error,
    ));
    assert_eq!(failure["findings"], json!({"parameter_domain": 1}));
    assert_eq!(failure["curve_kinds"], json!({"nurbs": 1}));
    let mut narrowed = Narrowed::default();
    narrow(model.clone(), &failure, &writer, &mut narrowed, &mut 0).unwrap();
    assert_eq!(
        narrowed.path,
        vec![json!({"unit": "body", "ordinal": 2, "total": 3})]
    );
    assert!(narrowed.isolated);
    assert_eq!(narrowed.kinds["use_curve_outside_carrier"], 1);
    let anon = anonymous(
        Path::new("/PrivateFolder/PrivateDesign.f3d"),
        &failure,
        &narrowed,
    );
    let text = serde_json::to_string(&anon).unwrap();
    let steps = next_steps(&anon);
    let source = serde_json::to_value(&model.ir).unwrap();
    for id in source["model"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|row| row["id"].as_str())
    {
        assert!(!text.contains(id));
        assert!(!steps.contains(id));
    }
    for name in source["model"]
        .as_object()
        .unwrap()
        .values()
        .filter_map(Value::as_array)
        .flatten()
        .flat_map(|row| [row.get("name"), row.get("label")])
        .flatten()
        .filter_map(Value::as_str)
    {
        if !name.is_empty() {
            assert!(!text.contains(name));
            assert!(!steps.contains(name));
        }
    }
    for private in [
        "PrivateBody",
        "PrivateDesign",
        "PrivateFolder",
        "secret:",
        "123.456",
        "0.75",
        "1.25",
    ] {
        assert!(!text.contains(private), "{text}");
        assert!(!steps.contains(private), "{steps}");
    }
    assert_eq!(narrowed.key[0]["name"], "PrivateBody1");
    assert!(steps.contains("face's seam"));
    assert!(steps.ends_with("Never send local-key.json.\n"));
}

#[test]
fn repro_synthetic_88_findings_worked_example() {
    let model = seam_model(88);
    let writer = ConvertOptions::default().writer();
    let error = dutchman_core::repro::check(&model).unwrap_err();
    let failure = diagnostics::anonymous_failure(&crate::json::error(
        crate::json::CONVERT,
        Value::Null,
        &error,
    ));
    assert_eq!(failure["findings"], json!({"parameter_domain": 88}));
    let mut narrowed = Narrowed::default();
    narrow(model, &failure, &writer, &mut narrowed, &mut 0).unwrap();
    let example = anonymous(Path::new("synthetic.f3d"), &failure, &narrowed);
    assert!(narrowed.isolated);
    println!("{}", serde_json::to_string_pretty(&example).unwrap());
    println!("{}", next_steps(&example));
}

#[test]
fn repro_writer_trials_are_candidates_without_structured_codes() {
    struct FailsOnBody;
    impl Writer for FailsOnBody {
        fn extension(&self) -> &'static str {
            "sc"
        }
        fn write(
            &self,
            model: &Model,
            _: &mut dyn std::io::Write,
            _: &mut Report,
        ) -> Result<(), Error> {
            if model
                .ir
                .model
                .bodies
                .iter()
                .any(|body| body.name.as_deref() == Some("PrivateBody1"))
            {
                Err(Error::Encode(
                    "PrivateDesign cannot map PrivateBody1 123.456".into(),
                ))
            } else {
                Ok(())
            }
        }
    }
    let model = seam_model(0);
    let failure = test_model(&model, &json!({"kind": "encode"}), &FailsOnBody);
    let mut narrowed = Narrowed::default();
    narrow(model, &failure, &FailsOnBody, &mut narrowed, &mut 0).unwrap();
    assert_eq!(
        narrowed.path,
        vec![json!({"unit": "body", "ordinal": 2, "total": 3})]
    );
    assert!(!narrowed.isolated);
    assert!(narrowed
        .limits
        .iter()
        .any(|limit| limit.contains("candidates")));
    let text = anonymous(Path::new("PrivateDesign.f3d"), &failure, &narrowed).to_string();
    assert!(!text.contains("PrivateDesign") && !text.contains("123.456"));
}

#[test]
fn repro_member_api_checks_each_independent_synthetic_document() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("PrivateArchive.f3z");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&input).unwrap());
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for name in ["PrivateAlpha.f3d", "PrivateBeta.f3d"] {
        zip.start_file(name, stored).unwrap();
        zip.write_all(&std::fs::read(fixture("generated_unit_cube.f3d")).unwrap())
            .unwrap();
    }
    zip.start_file("Manifest.json", stored).unwrap();
    zip.write_all(br#"{"root":"PrivateAlpha.f3d"}"#).unwrap();
    zip.start_file("DesignDescription.json", stored).unwrap();
    zip.write_all(br#"{"name":"Autodesk Design Description","version":"0.1","designDescription":{"id":"0","designGraphs":[]}}"#).unwrap();
    zip.finish().unwrap();
    let mut visited = Vec::new();
    dutchman_core::repro::members(&input, &mut |ordinal, total, name, model| {
        assert!(ordinal > 0 && ordinal <= total);
        assert!(dutchman_core::repro::check(&model.unwrap()).is_ok());
        visited.push(name.to_owned());
        false
    })
    .unwrap();
    assert_eq!(visited, ["PrivateAlpha.f3d", "PrivateBeta.f3d"]);
}

#[test]
fn repro_signature_requires_the_same_stage_and_complete_code_set_not_counts() {
    let expected =
        json!({"stage": "project", "kind": "cadmpeg_checks", "findings": {"parameter_domain": 88}});
    let smaller =
        json!({"stage": "project", "kind": "cadmpeg_checks", "findings": {"parameter_domain": 1}});
    assert!(same(&expected, &smaller));
    let mut different = smaller.clone();
    different["stage"] = json!("decode");
    assert!(!same(&expected, &different));
    different = smaller;
    different["findings"]["referential_integrity"] = json!(1);
    assert!(!same(&expected, &different));
    assert!(!same(
        &json!({"stage": "decode", "kind": "decode", "findings": {}}),
        &json!({"stage": "decode", "kind": "decode", "findings": {}})
    ));
}

#[test]
fn repro_success_cc0_fixtures_and_local_key_opt_in() {
    for name in [
        "generated_unit_cube.f3d",
        "freecad-1.1.4-A01/P01a.FCStd",
        "freecad-1.1.4-A01/P02a.FCStd",
    ] {
        if !cfg!(feature = "freecad") && name.ends_with("FCStd") {
            continue;
        }
        let dir = tempfile::tempdir().unwrap();
        let result = run(&fixture(name), dir.path(), false).unwrap();
        assert_eq!(
            result["anonymous"]["status"], "no_failure_to_reproduce",
            "{result}"
        );
        assert!(result.get("local_key").is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
        assert!(run(&fixture(name), dir.path(), false).is_err());
    }
}

#[test]
fn repro_decode_failure_reports_limits_and_cleans_temporary_files() {
    let dir = tempfile::tempdir().unwrap();
    let result = run(&fixture("container_metadata_only.f3d"), dir.path(), true).unwrap();
    assert_eq!(result["anonymous"]["status"], "failure");
    assert_eq!(result["anonymous"]["failure"]["stage"], "decode");
    assert_eq!(
        result["anonymous"]["narrowing"]["smallest_failing_unit_isolated"],
        false
    );
    assert_eq!(result["local_key"]["warning"], "Do not share");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
