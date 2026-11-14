//! Synthetic REST bundles only; fixture provenance and generator live in fixtures/.
#![cfg(feature = "onshape")]
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use dutchman_core::{read_bytes, Format, Model, Report};
use serde_json::{json, Value};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/onshape-{name}.onshape-bundle")),
    )
    .unwrap()
}

fn read(bytes: &[u8]) -> Result<(Model, Report), dutchman_core::Error> {
    let mut report = Report::default();
    let model = read_bytes(
        Format::OnshapeBundle,
        Path::new("test.onshape-bundle"),
        bytes,
        &mut report,
    )?;
    Ok((model, report))
}

fn changed(bytes: &[u8], mut edit: impl FnMut(&str, &mut Value)) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let mut data = Vec::new();
        member.read_to_end(&mut data).unwrap();
        if member.name().unwrap().ends_with(".json") {
            let mut value = serde_json::from_slice(&data).unwrap();
            edit(&member.name().unwrap(), &mut value);
            data = serde_json::to_vec(&value).unwrap();
        }
        zip.start_file(
            member.name().unwrap().as_ref(),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn timeline(model: &Model) -> Value {
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|f| f["ordinal"].as_u64());
    wire
}

fn kinds(model: &Model) -> Vec<String> {
    timeline(model)["model"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["definition"]["definition"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn synthetic_bundles_retain_the_expected_cadir_rows_and_final_step() {
    for (name, expected) in [
        ("plate-fillet", vec!["sketch", "extrude", "fillet"]),
        ("revolve", vec!["sketch", "revolve"]),
        ("hole", vec!["sketch", "extrude", "sketch", "hole"]),
        ("unsupported", vec!["sketch", "extrude", "native"]),
    ] {
        let (model, report) = read(&fixture(name)).unwrap();
        assert_eq!(kinds(&model), expected, "{name}: {:?}", report.losses());
        assert_eq!(model.ir.model.bodies.len(), 1);
        let wire = timeline(&model);
        assert_eq!(
            wire["model"]["features"][1]["outputs"],
            json!([model.ir.model.bodies[0].id])
        );
        assert!(model
            .source_step
            .as_ref()
            .unwrap()
            .starts_with("ISO-10303-21;"));
        assert_eq!(model.source_file.as_deref(), Some("test.onshape-bundle"));
        let losses: Vec<_> = report
            .losses()
            .iter()
            .filter(|loss| {
                loss.code
                    .as_deref()
                    .is_some_and(|s| s.starts_with("onshape/"))
            })
            .collect();
        if name == "unsupported" {
            assert_eq!(losses.len(), 1);
            assert_eq!(
                losses[0].code.as_deref(),
                Some("onshape/history.not-supported")
            );
            assert!(losses[0].detail.contains("custom FeatureScript"));
        } else {
            assert!(losses.is_empty(), "{name}: {losses:?}");
        }
    }
}

#[test]
fn built_in_plane_queries_preserve_quoted_identity() {
    for (plane, expected) in [
        ("Top", vec!["sketch", "revolve"]),
        ("T op", vec!["native", "native"]),
    ] {
        let bytes = changed(&fixture("revolve"), |path, value| {
            if path.ends_with("features.json") {
                let query = &mut value["features"][0]["parameters"][0]["queries"][0];
                *query = json!({
                    "btType": "BTMIndividualQuery-138",
                    "queryString": format!("qCreatedBy ( makeId ( \"{plane}\" ), EntityType.FACE ) ;")
                });
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model), expected, "{plane}: {:?}", report.losses());
        if plane == "T op" {
            assert!(report
                .losses()
                .iter()
                .any(|loss| loss.detail.contains("sketch plane query")));
        }
    }
}

#[test]
fn exact_input_edges_are_converted_from_metres_at_the_feature_rollback() {
    let (model, _) = read(&fixture("plate-fillet")).unwrap();
    let wire = timeline(&model);
    let state = &wire["model"]["feature_input_topologies"][0];
    assert_eq!(state["input_of"], "onshape:model:feature#Corner%20round");
    assert_eq!(
        state["edge_geometry"][0]["interior"],
        json!({"x": 0.0, "y": 0.0, "z": 1.0})
    );
    assert_eq!(state["edge_geometry"][0]["end"]["z"], 2.0);
    assert_eq!(
        wire["model"]["sketch_entities"][0]["geometry"]["end"]["u"],
        20.0
    );
    assert_eq!(
        wire["model"]["sketch_constraints"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(wire["model"]["parameters"][0]["value"]["value"], 20.0);
}

#[test]
fn unresolved_edges_never_fall_back_to_final_topology_or_another_rollback() {
    for failure in ["id", "index", "microversion", "curve", "duplicate"] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("bodydetails/2.json") {
                match failure {
                    "id" => value["bodies"][0]["edges"][0]["id"] = json!("other"),
                    "microversion" => value["microversionId"]["theId"] = json!("other"),
                    "curve" => value["bodies"][0]["edges"][0]["curve"]["type"] = json!("BCURVE"),
                    "duplicate" => {
                        let row = value["bodies"][0]["edges"][0].clone();
                        value["bodies"][0]["edges"]
                            .as_array_mut()
                            .unwrap()
                            .push(row);
                    }
                    _ => {}
                }
            }
            if path.ends_with("bodydetails/index.json") && failure == "index" {
                *value = json!([1]);
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
        assert!(model.ir.model.feature_input_topologies.is_empty());
        assert!(
            report
                .losses()
                .iter()
                .any(|loss| loss.code.as_deref() == Some("onshape/history.not-supported")),
            "{failure}: {:?}",
            report.losses()
        );
    }
}

#[test]
fn wrong_magic_schema_and_microversion_are_refused() {
    assert!(read(b"not a zip").is_err());
    for failure in [
        "schema",
        "microversion",
        "assembly",
        "multiple",
        "path",
        "duplicate",
    ] {
        let bytes = changed(&fixture("revolve"), |path, value| {
            if path == "manifest.json" {
                match failure {
                    "schema" => value["schema"] = json!("dutchman.onshape-bundle/2"),
                    "assembly" => value["elements"][0]["type"] = json!("ASSEMBLY"),
                    "multiple" => {
                        let row = value["elements"][0].clone();
                        value["elements"].as_array_mut().unwrap().push(row);
                    }
                    "path" => value["elements"][0]["id"] = json!("../studio"),
                    _ => {}
                }
            }
            if path.ends_with("features.json") {
                if failure == "microversion" {
                    value["sourceMicroversion"] = json!("other");
                }
                if failure == "duplicate" {
                    value["features"][1]["featureId"] = value["features"][0]["featureId"].clone();
                }
            }
        });
        assert!(read(&bytes).is_err(), "{failure}");
    }
    assert_eq!(
        Format::from_path(&PathBuf::from("Plate.ONSHAPE-BUNDLE")),
        Some(Format::OnshapeBundle)
    );
    assert!(read_bytes(
        Format::InventorPart,
        Path::new("renamed.ipt"),
        &fixture("hole"),
        &mut Report::default()
    )
    .is_err());
}

#[test]
fn consistency_flag_refuses_mixed_snapshots_and_preserves_legacy_v1() {
    // The checked-in v1 fixtures predate the optional consistency/provenance fields.
    let legacy = fixture("revolve");
    let (original, _) = read(&legacy).unwrap();
    let consistent = changed(&legacy, |path, value| {
        if path == "manifest.json" {
            value["consistent"] = json!(true);
        }
    });
    let (current, _) = read(&consistent).unwrap();
    assert_eq!(current.ir, original.ir);
    assert_eq!(current.source_step, original.source_step);

    let inconsistent = changed(&legacy, |path, value| {
        if path == "manifest.json" {
            value["consistent"] = json!(false);
        }
        // This must be refused before trying to decode construction history.
        if path.ends_with("features.json") {
            value["features"] = Value::Null;
        }
    });
    let reason = read(&inconsistent).unwrap_err().to_string();
    assert!(reason.contains("marked inconsistent"), "{reason}");
    assert!(
        reason.contains("fetch it again, or use a version link"),
        "{reason}"
    );
    for flag in [Value::Null, json!("false"), json!(0)] {
        let malformed = changed(&legacy, |path, value| {
            if path == "manifest.json" {
                value["consistent"] = flag.clone();
            }
        });
        assert!(read(&malformed)
            .unwrap_err()
            .to_string()
            .contains("consistency flag must be true or false"));
    }
}

#[test]
fn unsupported_sketch_constraints_keep_exact_curves_with_a_named_loss() {
    let bytes = changed(&fixture("revolve"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][0]["constraints"][0]["constraintType"] = json!("FREEZE");
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(kinds(&model), ["sketch", "revolve"]);
    assert_eq!(model.ir.model.sketch_entities.len(), 4);
    assert_eq!(model.ir.model.sketch_constraints.len(), 1);
    assert!(report.losses().iter().any(|loss| loss.code.as_deref()
        == Some("onshape/sketch.constraint-not-supported")
        && loss.detail.contains("FREEZE")));
}

#[test]
fn extrude_operations_through_all_and_equal_chamfers_map() {
    for operation in ["NEW", "ADD", "REMOVE", "INTERSECT"] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("features.json") {
                value["features"][1]["parameters"][1]["value"] = json!(operation);
                value["features"][1]["parameters"][3]["value"] = json!("THROUGH_ALL");
                value["features"][2]["featureType"] = json!("chamfer");
                value["features"][2]["parameters"][1]["parameterId"] = json!("width");
                value["features"][2]["parameters"].as_array_mut().unwrap().push(json!({"btType": "BTMParameterEnum-145", "parameterId": "chamferType", "value": "EQUAL_OFFSETS"}));
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            ["sketch", "extrude", "chamfer"],
            "{:?}",
            report.losses()
        );
        let wire = timeline(&model);
        assert_eq!(
            wire["model"]["features"][1]["definition"]["extent"]["side"]["termination"]["kind"],
            "through_all"
        );
    }
}

#[test]
fn up_to_face_uses_analytic_boundaries_of_that_feature_input() {
    let bytes = changed(&fixture("plate-fillet"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][2] = json!({"btType": "BTMFeature-134", "featureType": "extrude", "featureId": "toFace", "name": "To face", "parameters": [
                {"parameterId": "bodyType", "value": "SOLID"}, {"parameterId": "operationType", "value": "NEW"},
                {"parameterId": "entities", "queries": [{"btType": "BTMIndividualSketchRegionQuery-140", "featureId": "Base sketch"}]},
                {"parameterId": "endBound", "value": "UP_TO_SURFACE"},
                {"parameterId": "endBoundEntityFace", "queries": [{"btType": "BTMIndividualQuery-138", "deterministicIds": ["top"]}]}
            ]});
        }
        if path.ends_with("bodydetails/2.json") {
            let points = [
                [0.0, 0.0, 0.002],
                [0.02, 0.0, 0.002],
                [0.02, 0.01, 0.002],
                [0.0, 0.01, 0.002],
            ];
            let point = |p: [f64; 3]| json!({"x": p[0], "y": p[1], "z": p[2]});
            let edges: Vec<_> = (0..4).map(|i| {
                let a = points[i];
                let b = points[(i+1)%4];
                let delta = std::array::from_fn::<_,3,_>(|j| b[j]-a[j]);
                let length = delta.iter().map(|v| v*v).sum::<f64>().sqrt();
                json!({"id": format!("edge{i}"), "curve": {"type": "LINE", "origin": point(a), "direction": point(delta.map(|v| v/length))},
                    "geometry": {"startPoint": point(a), "endPoint": point(b), "midPoint": point(std::array::from_fn(|j| (a[j]+b[j])/2.0))}})
            }).collect();
            value["bodies"][0]["edges"] = json!(edges);
            value["bodies"][0]["faces"] = json!([{"id": "top", "surface": {"type": "PLANE", "origin": point(points[0]), "normal": point([0.0,0.0,1.0])},
                "loops": [{"isOuter": true, "coedges": (0..4).map(|i| json!({"edgeId": format!("edge{i}"), "orientation": true})).collect::<Vec<_>>()}]}]);
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "extrude", "extrude"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    let termination = &wire["model"]["features"][2]["definition"]["extent"]["side"]["termination"];
    assert_eq!(termination["kind"], "to_face");
    assert_eq!(
        termination["face"]["value"]["state"],
        "onshape:model:input#toFace"
    );
    assert_eq!(
        wire["model"]["feature_input_topologies"][0]["edge_geometry"][0]["faces"],
        json!(["top"])
    );

    let curved = changed(&bytes, |path, value| {
        if path.ends_with("bodydetails/2.json") {
            value["bodies"][0]["faces"][0]["surface"]["type"] = json!("CYLINDER");
        }
    });
    let (model, report) = read(&curved).unwrap();
    assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("non-planar terminating")));
}

#[test]
fn circles_arcs_and_simple_splines_remain_exact_sketch_entities() {
    let bytes = changed(&fixture("revolve"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][0]["constraints"] = json!([]);
            value["features"][0]["entities"] = json!([
                {"btType": "BTMSketchCurve-4", "entityId": "circle", "geometry": {"btType": "BTCurveGeometryCircle-115", "xCenter": 0.0, "yCenter": 0.0, "radius": 0.005}},
                {"btType": "BTMSketchCurveSegment-155", "entityId": "arc", "startParam": 0.0, "endParam": std::f64::consts::FRAC_PI_2,
                 "geometry": {"btType": "BTCurveGeometryCircle-115", "xCenter": 0.01, "yCenter": 0.0, "radius": 0.002, "xDir": 1.0, "yDir": 0.0}},
                {"btType": "BTMSketchCurve-4", "entityId": "spline", "isConstruction": true,
                 "geometry": {"btType": "BTCurveGeometrySpline-118", "degree": 3, "dimension": 2, "controlPointCount": 4,
                 "controlPoints": [0.0,0.0, 0.001,0.002, 0.003,0.002, 0.004,0.0], "knots": [0.0,0.0,0.0,0.0,1.0,1.0,1.0,1.0]}}
            ]);
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "revolve"],
        "{:?}",
        report.losses()
    );
    let wire = timeline(&model);
    let entities = wire["model"]["sketch_entities"].as_array().unwrap();
    assert_eq!(
        entities
            .iter()
            .map(|e| e["geometry"]["kind"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["arc", "circle", "nurbs"]
    );
    assert_eq!(entities[1]["geometry"]["radius"], 5.0);
    assert_eq!(
        entities[2]["geometry"]["curve"]["poles"]["points"][1],
        json!({"u": 1.0, "v": 2.0})
    );
}

#[test]
fn archive_allocation_and_expansion_are_bounded_before_decode() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        "manifest.json",
        zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated),
    )
    .unwrap();
    zip.write_all(&vec![b' '; (32 << 20) + 1]).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert!(read(&bytes)
        .unwrap_err()
        .to_string()
        .contains("expanded data"));

    let mut bytes = fixture("revolve");
    let end = bytes.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
    bytes[end + 8..end + 12].copy_from_slice(&[0xff; 4]);
    assert!(read(&bytes)
        .unwrap_err()
        .to_string()
        .contains("ZIP directory"));

    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file("../manifest.json", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"{}").unwrap();
    assert!(read(&zip.finish().unwrap().into_inner()).is_err());
}

#[test]
fn rest_zero_defaults_and_empty_constraints_can_be_omitted() {
    let bytes = changed(&fixture("plate-fillet"), |path, value| {
        if path.ends_with("features.json") {
            value["features"][0]
                .as_object_mut()
                .unwrap()
                .remove("constraints");
            for entity in value["features"][0]["entities"].as_array_mut().unwrap() {
                if let Some(geometry) = entity.get_mut("geometry").and_then(Value::as_object_mut) {
                    geometry.retain(|_, value| value.as_f64() != Some(0.0));
                }
                entity.as_object_mut().unwrap().remove("startParam");
            }
        }
        if path.ends_with("bodydetails/2.json") {
            let edge = &mut value["bodies"][0]["edges"][0];
            for key in ["origin", "direction"] {
                edge["curve"][key]
                    .as_object_mut()
                    .unwrap()
                    .retain(|_, v| v.as_f64() != Some(0.0));
            }
        }
    });
    let (model, report) = read(&bytes).unwrap();
    assert_eq!(
        kinds(&model),
        ["sketch", "extrude", "fillet"],
        "{:?}",
        report.losses()
    );
    assert!(model.ir.model.sketch_constraints.is_empty());
}

#[test]
fn unsupported_variants_and_unresolved_quantities_get_named_losses() {
    for (field, expected) in [
        ("radius", "radius"),
        ("tangentPropagation", "tangent-chain"),
        ("isVariable", "isVariable"),
        ("deterministicIds", "deterministic"),
        ("crossSection", "circular constant-radius"),
    ] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("features.json") {
                let parameters = value["features"][2]["parameters"].as_array_mut().unwrap();
                match field {
                    "radius" => {
                        parameters[1] = json!({"parameterId": "radius", "expression": "#unknown"})
                    }
                    "deterministicIds" => {
                        parameters[0]["queries"][0]["deterministicIds"] = json!([])
                    }
                    "crossSection" => parameters[2]["value"] = json!("CONIC"),
                    _ => parameters.push(json!({"parameterId": field, "value": true})),
                }
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
        assert!(
            report.losses().iter().any(|loss| loss.code.as_deref()
                == Some("onshape/history.not-supported")
                && loss.detail.contains(expected)),
            "{field}: {:?}",
            report.losses()
        );
    }
}

#[test]
fn boolean_scopes_must_resolve_to_the_sole_rollback_body() {
    for operation in ["ADD", "REMOVE", "INTERSECT"] {
        let bytes = changed(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("features.json") {
                value["features"][2] = json!({"btType": "BTMFeature-134", "featureType": "extrude", "featureId": "scoped", "name": "Scoped extrude", "parameters": [
                    {"parameterId": "bodyType", "value": "SOLID"}, {"parameterId": "operationType", "value": operation},
                    {"parameterId": "entities", "queries": [{"btType": "BTMIndividualSketchRegionQuery-140", "featureId": "Base sketch"}]},
                    {"parameterId": "endBound", "value": "THROUGH_ALL"},
                    {"parameterId": "booleanScope", "queries": [{"btType": "BTMIndividualQuery-138", "deterministicIds": ["body"]}]}
                ]});
            }
        });
        let (model, report) = read(&bytes).unwrap();
        assert_eq!(
            kinds(&model),
            ["sketch", "extrude", "extrude"],
            "{:?}",
            report.losses()
        );
        let wire = timeline(&model);
        assert_eq!(
            wire["model"]["features"][1]["outputs"],
            json!([model.ir.model.bodies[0].id])
        );
        let bad_scope = changed(&bytes, |path, value| {
            if path.ends_with("features.json") {
                value["features"][2]["parameters"][4]["queries"][0]["deterministicIds"] =
                    json!(["other"]);
            }
        });
        let (model, report) = read(&bad_scope).unwrap();
        assert_eq!(kinds(&model), ["sketch", "extrude", "native"]);
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("Boolean scope does not select")));
    }
}
