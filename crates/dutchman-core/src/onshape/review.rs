//! Synthetic regressions ported from the reviewed cadmpeg reader.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
fn fixture(name: &str) -> Vec<u8> {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/fixtures/onshape-{name}.onshape-bundle")),
    )
    .unwrap();
    if name != "revolve" {
        return bytes;
    }
    // Quantity controls use an explicit source-sketch axis, so they test quantity
    // admission independently of missing historical-producer data in the old bundle.
    edit(&bytes, |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        let sketch = response["features"][0]["featureId"]
            .as_str()
            .unwrap()
            .to_owned();
        response["features"][0]["entities"].as_array_mut().unwrap().push(json!({"btType":"BTMSketchCurveSegment-155","entityId":"quantity-axis","isConstruction":true,"geometry":{"btType":"BTCurveGeometryLine-117","dirY":1.0},"startParam":0.0,"endParam":0.015}));
        let query = json!({"entityType":"EDGE","historyType":"CREATION","queryType":"SKETCH_ENTITY","operationId":[format!("{sketch}.wireOp")],"sketchEntityId":"quantity-axis"});
        let encoded =
            serde_json::to_string(&format!("%{}", super::tests::compact(&query))).unwrap();
        parameter(&mut response["features"][1], "axis")["queries"] =
            json!([{"queryString":format!("query=qCompressed(1.0,{encoded},id);")}]);
    })
}
pub(super) fn loss(result: &ResultModel, code: &str) -> bool {
    result
        .report()
        .losses()
        .iter()
        .any(|l| l.code.as_deref() == Some(code))
}
pub(super) fn kind(wire: &Value, ordinal: usize) -> &Value {
    &wire["model"]["features"][ordinal]["definition"]["value"]["kind"]
}
pub(crate) fn members(bytes: &[u8], edit: impl FnOnce(&mut BTreeMap<String, Vec<u8>>)) -> Vec<u8> {
    let mut source = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut members = BTreeMap::new();
    for index in 0..source.len() {
        let mut member = source.by_index(index).unwrap();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        members.insert(member.name().unwrap().into_owned(), bytes);
    }
    edit(&mut members);
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in members {
        output
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}
pub(crate) fn edit(bytes: &[u8], mut edit: impl FnMut(&str, &mut Value)) -> Vec<u8> {
    members(bytes, |members| {
        for (path, bytes) in members.iter_mut().filter(|(path, _)| {
            path.rsplit_once('.')
                .is_some_and(|(_, extension)| extension == "json")
        }) {
            let mut value = serde_json::from_slice(bytes).unwrap();
            edit(path, &mut value);
            *bytes = serde_json::to_vec(&value).unwrap();
        }
    })
}
pub(super) struct ResultModel {
    model: crate::Model,
    report: crate::Report,
}
impl ResultModel {
    pub(super) fn ir(&self) -> &cadmpeg_ir::CadIr {
        &self.model.ir
    }
    pub(super) fn report(&self) -> &crate::Report {
        &self.report
    }
}
pub(super) fn decode(bytes: &[u8]) -> ResultModel {
    let mut report = crate::Report::default();
    let model = crate::read_bytes(
        crate::Format::OnshapeBundle,
        std::path::Path::new("synthetic.onshape-bundle"),
        bytes,
        &mut report,
    )
    .unwrap();
    ResultModel { model, report }
}
pub(super) fn wire(result: &ResultModel) -> Value {
    let mut wire = serde_json::to_value(result.ir()).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|f| f["ordinal"].as_u64());
    let order: BTreeMap<String, u64> = wire["model"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|feature| {
            (
                feature["id"].as_str().unwrap().to_owned(),
                feature["ordinal"].as_u64().unwrap(),
            )
        })
        .collect();
    wire["design"]["features"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|feature| order.get(feature["id"].as_str().unwrap()).copied());
    wire
}
pub(super) fn validate(result: ResultModel) {
    let validation = cadmpeg_ir::validate_neutral(&result.model.ir, Vec::new()).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
    if let Some(fidelity) = result.model.fidelity {
        let validation = cadmpeg_ir::validate_neutral_with_source_fidelity(
            &result.model.ir,
            &fidelity,
            Vec::new(),
        )
        .unwrap();
        assert!(validation.is_ok(), "{validation:#?}");
    }
}
pub(crate) fn parameter<'a>(feature: &'a mut Value, name: &str) -> &'a mut Value {
    feature["parameters"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["parameterId"] == name)
        .unwrap()
}
pub(crate) fn variable(name: &str, id: &str, kind: &str, expression: &str) -> Value {
    let key = if kind == "ANGLE" {
        "angleValue"
    } else {
        "lengthValue"
    };
    json!({"featureId":id,"featureType":"assignVariable","parameters":[
        {"parameterId":"name","value":name},{"parameterId":"variableType","value":kind},
        {"parameterId":key,"expression":expression}]})
}

#[test]
fn latest_refused_variable_binding_blocks_later_consumers() {
    for replacement in ["2 mm", "unsupported(2 mm)", "repeated-name", "missing-name"] {
        let bytes = edit(&fixture("plate-fillet"), |path, value| {
            if !path.ends_with("features.json") {
                return;
            }
            let features = value["features"].as_array_mut().unwrap();
            parameter(&mut features[1], "depth")["expression"] = json!("#span");
            let mut replaced = variable(
                "span",
                "replacement",
                "LENGTH",
                if replacement == "repeated-name" {
                    "2 mm"
                } else {
                    replacement
                },
            );
            if replacement == "missing-name" {
                replaced["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|p| p["parameterId"] != "name");
            }
            if replacement == "repeated-name" {
                replaced["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":"name","value":"span"}));
            }
            features.insert(1, replaced);
            features.insert(1, variable("span", "original", "LENGTH", "1 mm"));
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        assert_eq!(kind(&wire, 2), "native");
        assert_eq!(kind(&wire, 3), "native");
        assert!(result
            .report()
            .losses()
            .iter()
            .any(|l| l.detail.contains("unresolved latest binding")));
        assert!(loss(&result, "onshape/history.not-supported"));
        validate(result);
    }
}

#[test]
fn boolean_operands_refuse_invalid_and_repeated_values_and_keep_valid_controls() {
    for key in [
        "oppositeDirection",
        "hasDraft",
        "symmetric",
        "hasSecondDirection",
    ] {
        for values in [
            vec![json!(false)],
            vec![json!(true)],
            vec![json!("true")],
            vec![json!(1)],
            vec![json!(false), json!(true)],
        ] {
            let bytes = edit(&fixture("plate-fillet"), |path, response| {
                if !path.ends_with("features.json") {
                    return;
                }
                let parameters = response["features"][1]["parameters"]
                    .as_array_mut()
                    .unwrap();
                for value in &values {
                    parameters.push(json!({"parameterId":key,"value":value}));
                }
            });
            let result = decode(&bytes);
            let wire = wire(&result);
            let valid = values.len() == 1 && values[0].is_boolean();
            let gated = matches!(key, "hasDraft" | "hasSecondDirection") && values[0] == true;
            assert_eq!(
                kind(&wire, 1),
                if valid && !gated { "extrude" } else { "native" },
                "{key} {values:?}"
            );
            if valid && !gated && key == "oppositeDirection" {
                assert_eq!(
                    wire["model"]["features"][1]["definition"]["value"]["value"]["direction"]
                        ["kind"],
                    if values[0] == true {
                        "reversed_profile_normal"
                    } else {
                        "profile_normal"
                    }
                );
            }
            if !valid || gated {
                assert!(loss(&result, "onshape/history.not-supported"));
            }
            validate(result);
        }
    }
    for key in ["suppressed", "parentSuppressed"] {
        for value in [json!("false"), json!(0), json!(null)] {
            let bytes = edit(&fixture("plate-fillet"), |path, response| {
                if path.ends_with("features.json") {
                    response["features"][1][key] = value.clone();
                }
            });
            let result = decode(&bytes);
            let wire = wire(&result);
            assert_eq!(kind(&wire, 1), "native");
            assert!(wire["model"]["features"][1].get("suppressed").is_none());
            assert!(loss(&result, "onshape/history.not-supported"));
        }
    }
}

#[test]
fn quantities_require_dimensioned_expressions_or_explicit_units() {
    for (source, expected) in [
        (json!({"value":1.0}), None),
        (json!({"value":1.0,"units":1}), None),
        (json!({"value":1.0,"units":""}), None),
        (json!({"value":1.0,"units":"mm"}), Some(1.0)),
        (json!({"value":1.0,"units":"m"}), Some(1000.0)),
        (json!({"expression":"1 mm + 2 mm","value":0.0}), Some(3.0)),
        (
            json!({"expression":"(1 cm + 2 mm) / 2","value":0.0}),
            Some(6.0),
        ),
        (
            json!({"expression":"2 * (1 mm + 2 mm)","value":0.0}),
            Some(6.0),
        ),
    ] {
        let bytes = edit(&fixture("plate-fillet"), |path, response| {
            if path.ends_with("features.json") {
                let target = parameter(&mut response["features"][1], "depth");
                *target = source.clone();
                target["parameterId"] = json!("depth");
            }
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        if let Some(expected) = expected {
            assert_eq!(kind(&wire, 1), "extrude");
            assert_eq!(
                wire["model"]["features"][1]["definition"]["value"]["value"]["extent"]["side"]
                    ["termination"]["length"],
                expected
            );
        } else {
            assert_eq!(kind(&wire, 1), "native");
            assert!(loss(&result, "onshape/history.not-supported"));
        }
        validate(result);
    }
    for (units, value, expected) in [
        (json!(null), 1.0, None),
        (json!(17), 1.0, None),
        (json!("deg"), 90.0, Some(std::f64::consts::FRAC_PI_2)),
        (json!("rad"), 1.0, Some(1.0)),
    ] {
        let bytes = edit(&fixture("revolve"), |path, response| {
            if path.ends_with("features.json") {
                let feature = &mut response["features"][1];
                parameter(feature, "revolveType")["value"] = json!("ONE_DIRECTION");
                feature["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":"angle","value":value,"units":units}));
            }
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        if let Some(expected) = expected {
            assert_eq!(kind(&wire, 1), "revolve");
            let angle = wire["model"]["features"][1]["definition"]["value"]["value"]
                ["construction"]["value"]["extent"]["termination"]["angle"]
                .as_f64()
                .unwrap();
            assert!((angle - expected).abs() <= f64::EPSILON);
        } else {
            assert_eq!(kind(&wire, 1), "native");
            assert!(loss(&result, "onshape/history.not-supported"));
        }
        validate(result);
    }
}

#[test]
fn thin_open_curves_refuse_unresolved_sides_for_both_directions_and_flips() {
    for reversed in [false, true] {
        for flip in [false, true] {
            let bytes = edit(&fixture("plate-fillet"), |path, response| {
                if !path.ends_with("features.json") {
                    return;
                }
                response["features"][0]["entities"]
                    .as_array_mut()
                    .unwrap()
                    .truncate(1);
                response["features"][0]["constraints"] = json!([]);
                if reversed {
                    response["features"][0]["entities"][0]["geometry"]["pntX"] = json!(0.02);
                    response["features"][0]["entities"][0]["geometry"]["dirX"] = json!(-1.0);
                }
                let feature = &mut response["features"][1];
                parameter(feature, "bodyType")["value"] = json!("THIN");
                parameter(feature, "entities")["parameterId"] = json!("wallShape");
                feature["parameters"].as_array_mut().unwrap().extend([
                    json!({"parameterId":"midplane","value":false}),
                    json!({"parameterId":"flipWall","value":flip}),
                    json!({"parameterId":"thickness1","expression":"1 mm"}),
                    json!({"parameterId":"thickness2","expression":"0 mm"}),
                ]);
            });
            let result = decode(&bytes);
            assert_eq!(kind(&wire(&result), 1), "native");
            assert!(result.ir().design.features.is_empty());
            assert!(loss(&result, "onshape/extrude.thin-not-supported"));
            validate(result);
        }
    }
}

#[test]
fn blind_holes_require_drill_point_and_depth_reference_and_read_thread_facts() {
    for case in [
        "missing-angle",
        "repeated-angle",
        "missing-depth-reference",
        "invalid-depth-reference",
        "left",
        "tip",
        "missing-thread-hand",
        "missing-thread-mode",
    ] {
        let bytes = edit(&fixture("hole-forms"), |path, response| {
            if !path.ends_with("features.json") {
                return;
            }
            let feature = &mut response["features"][5];
            match case {
                "missing-angle" => feature["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|p| p["parameterId"] != "tipAngle"),
                "repeated-angle" => feature["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"parameterId":"tipAngle","expression":"90 deg"})),
                "missing-depth-reference" => feature["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|p| p["parameterId"] != "depthToTip"),
                "invalid-depth-reference" => {
                    parameter(feature, "depthToTip")["value"] = json!("false");
                }
                "left" => {
                    let thread = &mut parameter(feature, "standardTappedOrClearance")["value"];
                    thread["hand"] = json!("left");
                    thread["modeled"] = json!(true);
                    thread["cosmetic"] = json!(false);
                }
                "tip" => parameter(feature, "depthToTip")["value"] = json!(true),
                "missing-thread-hand" => {
                    parameter(feature, "standardTappedOrClearance")["value"]
                        .as_object_mut()
                        .unwrap()
                        .remove("hand");
                }
                "missing-thread-mode" => {
                    parameter(feature, "standardTappedOrClearance")["value"]
                        .as_object_mut()
                        .unwrap()
                        .remove("modeled");
                }
                _ => panic!("unknown synthetic case"),
            }
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        if matches!(case, "left" | "tip") {
            assert_eq!(
                kind(&wire, 5),
                "hole",
                "{case}: {:?}",
                result.report().losses()
            );
            let hole = &wire["model"]["features"][5]["definition"]["value"]["value"];
            if case == "left" {
                let thread = &hole["shape"]["construction"]["value"]["specification"];
                assert_eq!(thread["hand"], "left");
                assert_eq!(thread["modeled"], true);
                assert_eq!(thread["cosmetic"], false);
            } else {
                assert_eq!(hole["bottom"]["depth_to_tip"], true);
            }
        } else {
            assert_eq!(kind(&wire, 5), "native", "{case}");
            assert!(loss(&result, "onshape/history.not-supported"));
        }
        validate(result);
    }
}

#[test]
fn negative_variable_angles_keep_their_signed_parameter_value() {
    let bytes = edit(&fixture("revolve"), |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        let features = response["features"].as_array_mut().unwrap();
        parameter(&mut features[1], "revolveType")["value"] = json!("ONE_DIRECTION");
        features[1]["parameters"]
            .as_array_mut()
            .unwrap()
            .push(json!({"parameterId":"angle","expression":"#sweep"}));
        features.insert(1, variable("sweep", "sweep-var", "ANGLE", "-90 deg"));
    });
    let bytes = members(&bytes, |members| {
        let state = members
            .remove("partstudios/studio/bodydetails/1.json")
            .unwrap();
        members.insert("partstudios/studio/bodydetails/2.json".into(), state);
        members.insert(
            "partstudios/studio/bodydetails/index.json".into(),
            b"[2]".to_vec(),
        );
    });
    let result = decode(&bytes);
    let wire = wire(&result);
    assert_eq!(kind(&wire, 2), "native");
    assert!(result.report().losses().iter().any(|l| l.code.as_deref()
        == Some("onshape/history.not-supported")
        && l.detail.contains("negative revolve angle")));
    let parameters = wire["model"]["parameters"].as_array().unwrap();
    let variable = parameters
        .iter()
        .find(|p| p["name"] == "sweep" && p["owner"].is_null())
        .unwrap();
    assert!(
        (variable["value"]["value"].as_f64().unwrap() + std::f64::consts::FRAC_PI_2).abs()
            < f64::EPSILON
    );
    assert!(result.ir().model.parameters.iter().all(|p| {
        p.owner
            .as_ref()
            .is_none_or(|owner| owner.as_str() != super::id("feature", "Ring"))
    }));
    validate(result);
}

#[test]
fn a_resolved_binding_after_a_refused_initial_assignment_can_drive_a_consumer() {
    let bytes = edit(&fixture("plate-fillet"), |path, response| {
        if !path.ends_with("features.json") {
            return;
        }
        let features = response["features"].as_array_mut().unwrap();
        parameter(&mut features[1], "depth")["expression"] = json!("#span");
        features.insert(1, variable("span", "resolved-binding", "LENGTH", "2 mm"));
        features.insert(
            1,
            variable("span", "refused-binding", "LENGTH", "unsupported(1 mm)"),
        );
    });
    let result = decode(&bytes);
    let wire = wire(&result);
    assert_eq!(kind(&wire, 1), "native");
    assert_eq!(kind(&wire, 2), "tree_node");
    assert_eq!(kind(&wire, 3), "extrude");
    assert_eq!(
        wire["model"]["features"][3]["definition"]["value"]["value"]["extent"]["side"]
            ["termination"]["length"],
        2.0
    );
    validate(result);
}

#[test]
fn unresolved_variables_refuse_sketch_dimension_values_and_leave_inactive_values_unused() {
    for constraint_consumer in [false, true] {
        let bytes = edit(&fixture("plate-fillet"), |path, response| {
            if !path.ends_with("features.json") {
                return;
            }
            let features = response["features"].as_array_mut().unwrap();
            if constraint_consumer {
                parameter(&mut features[0]["constraints"][1], "length")["expression"] =
                    json!("#span");
                features.insert(0, variable("span", "replacement", "LENGTH", "2 mm"));
                features.insert(0, variable("span", "original", "LENGTH", "1 mm"));
            } else {
                parameter(&mut features[1], "depth")["expression"] = json!("#span");
                parameter(&mut features[1], "endBound")["value"] = json!("THROUGH_ALL");
                features.insert(1, variable("span", "replacement", "LENGTH", "2 mm"));
                features.insert(1, variable("span", "original", "LENGTH", "1 mm"));
            }
        });
        let result = decode(&bytes);
        let wire = wire(&result);
        assert_eq!(kind(&wire, 3), "extrude");
        let consumer = super::id(
            "feature",
            if constraint_consumer {
                "Base sketch"
            } else {
                "Plate"
            },
        );
        assert!(result.ir().model.parameters.iter().all(|p| {
            p.owner
                .as_ref()
                .is_none_or(|owner| owner.as_str() != consumer)
        }));
        if constraint_consumer {
            assert!(result.report().losses().iter().any(|l| l.code.as_deref()
                == Some("onshape/sketch.constraint-not-supported")
                && l.detail.contains("unresolved latest binding")));
        } else {
            assert_eq!(
                wire["model"]["features"][3]["definition"]["value"]["value"]["extent"]["side"]
                    ["termination"]["kind"],
                "through_all"
            );
        }
        validate(result);
    }
}

#[test]
fn caller_context_bounds_json_and_history_before_step() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};
    for resource in ["depth", "storage", "history", "topology"] {
        let bytes = edit(&fixture("plate-fillet"), |path, value| {
            if path.ends_with("features.json") {
                if resource == "depth" {
                    value["extra"] = json!([[[[[[[[0]]]]]]]]);
                }
                if resource == "storage" {
                    value["extra"] = json!(vec![0; 4096]);
                }
            }
            if resource == "topology" && path.ends_with("bodydetails/2.json") {
                value["bodies"][0]["faces"] = json!([{"id":"face","loops":[{"coedges":vec![json!({"edgeId":"edge","orientation":true});64]}]}]);
            }
        });
        let mut policy = crate::decode::policy();
        match resource {
            "depth" => policy.limits.max_recursion_depth = 7,
            "storage" => policy.limits.max_decompressed_bytes_per_expand = 32_768,
            "history" => policy.limits.max_entities = 3,
            "topology" => policy.limits.max_entities = 32,
            _ => panic!("unknown synthetic resource"),
        }
        // No body-building STEP: entity exhaustion must happen before projection.
        let bytes = if resource == "history" {
            members(&bytes, |m| {
                m.remove("partstudios/studio/export.step");
                let p = m.get_mut("partstudios/studio/features.json").unwrap();
                let mut j: Value = serde_json::from_slice(p).unwrap();
                j["features"].as_array_mut().unwrap().truncate(1);
                *p = serde_json::to_vec(&j).unwrap();
            })
        } else {
            bytes
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut report = crate::Report::default();
        let result = super::read(
            &ctx,
            std::path::Path::new("synthetic.onshape-bundle"),
            &bytes,
            &mut report,
            None,
        );
        if resource == "storage" {
            assert!(result.is_ok());
            assert!(report
                .losses()
                .iter()
                .any(|loss| loss.code.as_deref() == Some("onshape/member.not-supported")));
            continue;
        }
        let error = result.unwrap_err();
        let message = error.to_string();
        assert!(
            message.contains(match resource {
                "storage" => "JSON",
                "history" | "topology" => "Entities",
                _ => "RecursionDepth",
            }),
            "{resource}: {message}"
        );
    }
}

#[test]
fn next_face_extents_refuse_recesses_and_uncovered_interiors() {
    for obstruction in ["recess", "interior-gap", "none"] {
        let bytes = edit(&fixture("plate-fillet"), |path, response| {
            if path.ends_with("features.json") {
                response["features"][2] = response["features"][1].clone();
                response["features"][2]["featureId"] = json!("next");
                parameter(&mut response["features"][2], "endBound")["value"] = json!("UP_TO_NEXT");
            }
            if path.ends_with("bodydetails/2.json") {
                response["bodies"][0]["faces"].as_array_mut().unwrap().push(json!({
                    "id":obstruction,"surface":{"type":"PLANE","origin":{"x":0.01,"y":0.005,"z":0.001},"normal":{"z":1.0}},
                    "loops":[{"coedges":[]}]}));
            }
        });
        let result = decode(&bytes);
        assert_eq!(kind(&wire(&result), 2), "native");
        assert!(result
            .report()
            .losses()
            .iter()
            .any(|l| l.detail.contains("complete profile")));
        validate(result);
    }
}

#[test]
fn thin_variants_keep_source_fidelity_valid_and_do_not_create_design_rows() {
    for midplane in [false, true] {
        for flip in [false, true] {
            for second in ["0 mm", "0.2 mm"] {
                let bytes = edit(&fixture("plate-fillet"), |path, response| {
                    if path.ends_with("features.json") {
                        let feature = &mut response["features"][1];
                        parameter(feature, "bodyType")["value"] = json!("THIN");
                        feature["parameters"].as_array_mut().unwrap().extend([
                            json!({"parameterId":"midplane","value":midplane}),
                            json!({"parameterId":"flipWall","value":flip}),
                            json!({"parameterId":"thickness1","expression":"0.1 mm"}),
                            json!({"parameterId":"thickness2","expression":second}),
                        ]);
                    }
                });
                let result = decode(&bytes);
                assert_eq!(kind(&wire(&result), 1), "native");
                assert!(result.ir().design.features.is_empty());
                assert!(loss(&result, "onshape/extrude.thin-not-supported"));
                validate(result);
            }
        }
    }
}

#[test]
fn embedded_step_consumes_the_parent_entity_allowance() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};
    let bytes = members(&fixture("plate-fillet"), |members| {
        members.retain(|path, _| !path.contains("/bodydetails/"));
        members.insert(
            "partstudios/studio/bodydetails/index.json".into(),
            b"[]".to_vec(),
        );
        members.insert(
            "partstudios/studio/features.json".into(),
            br#"{"features":[]}"#.to_vec(),
        );
    });
    let mut policy = crate::decode::policy();
    policy.limits.max_entities = 3;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let error = super::read(
        &ctx,
        std::path::Path::new("synthetic.onshape-bundle"),
        &bytes,
        &mut crate::Report::default(),
        None,
    )
    .unwrap_err();
    assert!(error.to_string().contains("Entities"), "{error}");
}

#[test]
fn admitted_json_trees_are_reused_without_reparsing() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};
    let bytes = fixture("plate-fillet");
    let arena = DecodeArena::new();
    let policy = crate::decode::policy();
    let (ctx, _) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let bundle = super::archive::Bundle::open(&ctx, &bytes).unwrap();
    for path in [
        "manifest.json",
        "partstudios/studio/features.json",
        "partstudios/studio/bodydetails/2.json",
    ] {
        assert!(std::ptr::eq(
            bundle.json(path).unwrap(),
            bundle.json(path).unwrap()
        ));
    }
}

#[test]
fn opaque_json_attachments_are_not_admitted_as_studio_response_trees() {
    let bytes = members(&fixture("plate-fillet"), |members| {
        members.insert(
            "attachments/opaque.json".into(),
            b"opaque attachment".to_vec(),
        );
        members.insert(
            "partstudios/other/configuration.json".into(),
            b"opaque unrelated payload".to_vec(),
        );
    });
    let result = decode(&bytes);
    assert_eq!(kind(&wire(&result), 1), "extrude");
    validate(result);
}
