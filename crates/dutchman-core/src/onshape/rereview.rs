//! Code-built bundles: no account, stored design or private identifiers.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use super::review::{decode, kind, loss, parameter, validate, wire};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read, Write},
};

fn sketch(native: &str) -> Value {
    json!({"btType":"BTMSketch-151","featureType":"newSketch","featureId":native,"parameters":[
        {"parameterId":"sketchPlane","queries":[{"btType":"BTMIndividualCreatedByQuery-137","featureId":"Top"}]}],
        "entities":[{"btType":"BTMSketchCurveSegment-155","entityId":"line","startPointId":"a","endPointId":"b","startParam":0.0,"endParam":0.02,
        "geometry":{"btType":"BTCurveGeometryLine-117","dirX":1.0}},
        {"btType":"BTMSketchCurveSegment-155","entityId":"right","startParam":0.0,"endParam":0.01,"geometry":{"btType":"BTCurveGeometryLine-117","pntX":0.02,"dirY":1.0}},
        {"btType":"BTMSketchCurveSegment-155","entityId":"top","startParam":0.0,"endParam":0.02,"geometry":{"btType":"BTCurveGeometryLine-117","pntX":0.02,"pntY":0.01,"dirX":-1.0}},
        {"btType":"BTMSketchCurveSegment-155","entityId":"left","startParam":0.0,"endParam":0.01,"geometry":{"btType":"BTCurveGeometryLine-117","pntY":0.01,"dirY":-1.0}},
        {"btType":"BTMSketchPoint-158","entityId":"middle","x":0.01,"y":0.005}],"constraints":[]})
}
fn extrude(native: &str, source: &str) -> Value {
    json!({"featureType":"extrude","featureId":native,"parameters":[
        {"parameterId":"bodyType","value":"SOLID"},{"parameterId":"operationType","value":"NEW"},
        {"parameterId":"entities","queries":[{"btType":"BTMIndividualSketchRegionQuery-140","featureId":source}]},
        {"parameterId":"endBound","value":"BLIND"},{"parameterId":"depth","expression":"2 mm"}]})
}
fn body() -> Value {
    json!({"id":"body","type":"SOLID","edges":[{"id":"edge","curve":{"type":"LINE","origin":{"x":0.0},"direction":{"x":1.0}},
        "geometry":{"startPoint":{"x":0.0},"endPoint":{"x":0.02},"midPoint":{"x":0.01},"startVector":{"x":1.0},"endVector":{"x":1.0}}}],
        "faces":[{"id":"face","surface":{"type":"PLANE","origin":{"z":0.002},"normal":{"z":1.0}},"loops":[]}],"vertices":[]})
}
fn bundle(features: Vec<Value>, states: &[(usize, Value)]) -> Vec<u8> {
    let mut members = BTreeMap::from([
        (
            "manifest.json".to_owned(),
            json!({"schema":"dutchman.onshape-bundle/1","document":{"microversion":"synthetic"},"elements":[{"id":"studio","type":"PARTSTUDIO"}]}),
        ),
        (
            "partstudios/studio/features.json".into(),
            json!({"features":features}),
        ),
        (
            "partstudios/studio/bodydetails/index.json".into(),
            json!(states.iter().map(|(i, _)| i).collect::<Vec<_>>()),
        ),
    ]);
    for (index, bodies) in states {
        members.insert(
            format!("partstudios/studio/bodydetails/{index}.json"),
            json!({"bodies":bodies}),
        );
    }
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (path, value) in members {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&serde_json::to_vec(&value).unwrap()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn selection(key: &str, selected: &str) -> Value {
    json!({"parameterId":key,"queries":[{"deterministicIds":[selected]}]})
}
fn fillet() -> Value {
    json!({"featureType":"fillet","featureId":"round","parameters":[selection("entities","edge"),{"parameterId":"radius","expression":"1 mm"},{"parameterId":"crossSection","value":"CIRCULAR"}]})
}

#[test]
fn dependencies_prove_producers_across_independent_sketches_and_refuse_missing_transitions() {
    for target in ["fillet", "revolve", "face"] {
        for present in [true, false] {
            let mut operation = match target {
                "fillet" => fillet(),
                "revolve" => {
                    json!({"featureType":"revolve","featureId":"turn","parameters":[selection("axis","edge"),
                    {"parameterId":"entities","queries":[{"btType":"BTMIndividualSketchRegionQuery-140","featureId":"independent"}]},
                    {"parameterId":"bodyType","value":"SOLID"},{"parameterId":"operationType","value":"NEW"},{"parameterId":"revolveType","value":"FULL"}]})
                }
                _ => extrude("to-face", "independent"),
            };
            if target == "face" {
                parameter(&mut operation, "endBound")["value"] = json!("UP_TO_SURFACE");
                // Source operand order places the profile before the terminating face.
                operation["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .push(selection("endBoundEntityFace", "face"));
            }
            let states = if present {
                vec![
                    (0, json!([])),
                    (1, json!([])),
                    (2, json!([body()])),
                    (3, json!([body()])),
                ]
            } else {
                vec![(2, json!([])), (3, json!([body()]))]
            };
            let result = decode(&bundle(
                vec![
                    sketch("profile"),
                    extrude("producer", "profile"),
                    sketch("independent"),
                    operation,
                ],
                &states,
            ));
            let output = wire(&result);
            if present {
                assert_eq!(
                    kind(&output, 3),
                    if target == "face" { "extrude" } else { target },
                    "{:?}",
                    result.report().losses()
                );
                let ids: Vec<_> = match target {
                    "fillet" => vec![super::id("feature", "producer")],
                    "revolve" => vec![
                        super::id("feature", "producer"),
                        super::id("feature", "independent"),
                    ],
                    _ => vec![
                        super::id("feature", "independent"),
                        super::id("feature", "producer"),
                    ],
                };
                assert_eq!(output["model"]["features"][3]["dependencies"], json!(ids));
            } else {
                assert_eq!(kind(&output, 3), "native");
                assert!(loss(&result, "onshape/history.not-supported"));
            }
            validate(result);
        }
    }
}

#[test]
fn scope_lists_admit_empty_and_proved_targets_and_refuse_malformed_or_unresolved_operands() {
    for target in ["extrude", "revolve", "hole"] {
        for datum in [
            "empty",
            "present",
            "string",
            "null",
            "absent-list",
            "unknown",
        ] {
            let mut operation = match target {
                "extrude" => extrude("scoped", "profile"),
                "revolve" => json!({"featureType":"revolve","featureId":"scoped","parameters":[
                    {"parameterId":"bodyType","value":"SOLID"},{"parameterId":"operationType","value":"ADD"},
                    {"parameterId":"entities","queries":[{"btType":"BTMIndividualSketchRegionQuery-140","featureId":"profile"}]},
                    selection("axis","edge"),{"parameterId":"revolveType","value":"FULL"}]}),
                _ => json!({"featureType":"hole","featureId":"scoped","parameters":[
                    {"parameterId":"locations","queries":[{"queryString":"query=qCompressed(1.0,\"%M5Sa$entityTypeS6$VERTEXSb$historyTypeS8$CREATIONSb$operationIdA1Se$profile.wireOpS9$queryTypeSd$SKETCH_ENTITYSe$sketchEntityIdS6$middle\",id);"}]},
                    {"parameterId":"style","value":"SIMPLE"},{"parameterId":"endStyle","value":"THROUGH"},{"parameterId":"holeDiameter","expression":"3 mm"}]}),
            };
            let key = if target == "hole" {
                "scope"
            } else {
                "booleanScope"
            };
            let mut scope = selection(
                key,
                if datum == "unknown" {
                    "missing"
                } else {
                    "body"
                },
            );
            match datum {
                "empty" => scope["queries"] = json!([]),
                "string" => scope["queries"] = json!("body"),
                "null" => scope["queries"] = Value::Null,
                "absent-list" => {
                    scope.as_object_mut().unwrap().remove("queries");
                }
                _ => {}
            }
            operation["parameters"].as_array_mut().unwrap().push(scope);
            let result = decode(&bundle(
                vec![sketch("profile"), extrude("producer", "profile"), operation],
                &[(1, json!([])), (2, json!([body()]))],
            ));
            let good = matches!(datum, "empty" | "present");
            assert_eq!(
                kind(&wire(&result), 2),
                if good { target } else { "native" },
                "{target}/{datum}: {:?}",
                result.report().losses()
            );
            if !good {
                assert!(loss(&result, "onshape/history.not-supported"));
            }
            validate(result);
        }
    }
}

#[test]
fn three_midpoints_charge_complete_output_before_projection_and_keep_resource_operation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext};
    let mut feature = sketch("profile");
    feature["entities"]
        .as_array_mut()
        .unwrap()
        .retain(|entity| matches!(entity["entityId"].as_str(), Some("line" | "middle")));
    feature["entities"][1]["y"] = json!(0.0);
    feature["constraints"]=json!((0..3).map(|i|json!({"constraintType":"MIDPOINT","entityId":format!("m{i}"),"parameters":[
        {"parameterId":"localMidpoint","value":"middle"},{"parameterId":"localEntity1","value":"a"},{"parameterId":"localEntity2","value":"b"}]})).collect::<Vec<_>>());
    for maximum in [17, 18, 19] {
        let mut policy = crate::decode::policy();
        policy.limits.max_entities = maximum;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        ctx.charge_entities(2, "synthetic assets").unwrap();
        let mut model = cadmpeg_ir::CadIr::empty().model;
        let result = super::admit_history(&ctx, &feature, &mut model);
        // Two stored entities, three carriers, nine constraints, one sketch and one feature.
        if maximum < 18 {
            let error = result.unwrap_err().to_string();
            assert!(
                error.contains("Entities") && error.contains("Onshape history admission"),
                "{error}"
            );
            assert!(model.features.is_empty() && model.sketch_entities.is_empty());
        } else {
            result.unwrap();
            assert!(model.sketch_entities.capacity() >= 5);
            assert!(model.sketch_constraints.capacity() >= 9);
        }
    }
    let mut policy = crate::decode::policy();
    policy.limits.max_entities = 1;
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(ctx.charge_entities(2, "synthetic asset admission").is_err());
    let mut model = cadmpeg_ir::CadIr::empty().model;
    let error = super::admit_history(&ctx, &feature, &mut model)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("Entities") && error.contains("synthetic asset admission"),
        "{error}"
    );
    assert!(model.features.is_empty() && model.sketch_entities.is_empty());
    let result = decode(&bundle(vec![feature], &[]));
    assert_eq!(kind(&wire(&result), 0), "sketch");
    assert_eq!(result.ir().model.sketch_entities.len(), 5);
    assert_eq!(result.ir().model.sketch_constraints.len(), 9);
    validate(result);
}

#[test]
fn one_malformed_feature_keeps_the_other_features_and_one_named_loss() {
    let mut broken = extrude("broken", "profile");
    broken["parameters"] = json!("missing operands");
    let bytes = bundle(
        vec![sketch("profile"), broken, extrude("good", "profile")],
        &[],
    );
    // This STEP was generated from elementary geometry by the fixture recipe.
    let mut source = zip::ZipArchive::new(Cursor::new(
        include_bytes!("../../tests/fixtures/onshape-plate-fillet.onshape-bundle").as_slice(),
    ))
    .unwrap();
    let mut step = Vec::new();
    source
        .by_name("partstudios/studio/export.step")
        .unwrap()
        .read_to_end(&mut step)
        .unwrap();
    let bytes = super::review::members(&bytes, |members| {
        members.insert("partstudios/studio/export.step".into(), step);
    });
    let result = decode(&bytes);
    assert_eq!(result.ir().geometry.bodies.len(), 1);
    assert_eq!(kind(&wire(&result), 1), "native");
    assert_eq!(kind(&wire(&result), 2), "extrude");
    assert_eq!(
        result
            .report()
            .losses()
            .iter()
            .filter(|loss| loss
                .code
                .as_deref()
                .is_some_and(|code| code.starts_with("onshape/")))
            .count(),
        1,
        "{:?}",
        result.report().losses()
    );
    assert!(loss(&result, "onshape/history.not-supported"));
    validate(result);
}

#[test]
fn through_bore_keeps_thread_depth_from_its_own_controls() {
    for datum in ["finite", "through", "missing-depth", "missing-flag"] {
        let mut hole = json!({"featureType":"hole","featureId":"thread","parameters":[
            {"parameterId":"locations","queries":[{"queryString":"query=qCompressed(1.0,\"%M5Sa$entityTypeS6$VERTEXSb$historyTypeS8$CREATIONSb$operationIdA1Se$profile.wireOpS9$queryTypeSd$SKETCH_ENTITYSe$sketchEntityIdS6$middle\",id);"}]},
            {"parameterId":"style","value":"SIMPLE"},{"parameterId":"endStyle","value":"THROUGH"},{"parameterId":"holeDiameter","expression":"3 mm"},
            {"parameterId":"isTappedThrough","value":datum=="through"},{"parameterId":"tappedDepth","expression":"5 mm"},
            {"parameterId":"standardTappedOrClearance","value":{"type":"Tapped","standard":"ISO","size":"M4","pitch":"0.7 mm","hand":"right","modeled":false,"cosmetic":true}}]});
        if datum.starts_with("missing") {
            hole["parameters"].as_array_mut().unwrap().retain(|p| {
                p["parameterId"]
                    != if datum == "missing-depth" {
                        "tappedDepth"
                    } else {
                        "isTappedThrough"
                    }
            });
        }
        let result = decode(&bundle(vec![sketch("profile"), hole], &[]));
        let output = wire(&result);
        assert_eq!(
            kind(&output, 1),
            if datum.starts_with("missing") {
                "native"
            } else {
                "hole"
            },
            "{:?}",
            result.report().losses()
        );
        if datum.starts_with("missing") {
            assert!(loss(&result, "onshape/history.not-supported"));
        } else {
            let construction = &output["model"]["features"][1]["definition"]["value"]["value"]
                ["shape"]["construction"]["value"]["specification"];
            assert_eq!(
                construction["depth"]["kind"],
                if datum == "finite" {
                    "blind"
                } else {
                    "hole_depth"
                },
                "{construction}"
            );
            if datum == "finite" {
                assert_eq!(construction["depth"]["depth"], json!(5.0));
            }
        }
        validate(result);
    }
}

#[test]
fn malformed_members_and_oversized_tables_keep_independent_history() {
    for datum in ["json", "table", "step", "index", "body", "sketch"] {
        let bytes = bundle(
            vec![sketch("profile"), extrude("good", "profile")],
            &[(1, json!([]))],
        );
        let bytes = super::review::members(&bytes, |members| {
            let (path, bytes) = match datum {
                "json" => ("configuration.json", b"{".to_vec()),
                "table" => (
                    "bodydetails/1.json",
                    serde_json::to_vec(
                        &json!({"bodies":[{"id":"body","edges":vec![json!({});super::ROWS+1]}]}),
                    )
                    .unwrap(),
                ),
                "step" => ("export.step", b"not a STEP export".to_vec()),
                "index" => ("bodydetails/index.json", b"null".to_vec()),
                "sketch" => (
                    "sketches.json",
                    serde_json::to_vec(&json!({"sketches":[{"no-id":true}]})).unwrap(),
                ),
                _ => (
                    "bodydetails/1.json",
                    serde_json::to_vec(&json!({"bodies":[{"id":"body","edges":"unreadable"}]}))
                        .unwrap(),
                ),
            };
            members.insert(format!("partstudios/studio/{path}"), bytes);
        });
        let result = decode(&bytes);
        assert_eq!(
            kind(&wire(&result), 0),
            "sketch",
            "{datum}: {:?}",
            result.report().losses()
        );
        assert_eq!(kind(&wire(&result), 1), "extrude");
        assert!(
            result
                .report()
                .losses()
                .iter()
                .any(|l| l.code.as_deref() == Some("onshape/member.not-supported")),
            "{datum}: {:?}",
            result.report().losses()
        );
        validate(result);
    }
}

#[test]
fn ambiguous_topology_producers_remain_native_instead_of_selecting_the_previous_feature() {
    let result = decode(&bundle(
        vec![
            sketch("profile"),
            extrude("first", "profile"),
            extrude("second", "profile"),
            fillet(),
        ],
        &[(0, json!([])), (3, json!([body()]))],
    ));
    assert_eq!(kind(&wire(&result), 3), "native");
    assert!(result
        .report()
        .losses()
        .iter()
        .any(
            |loss| loss.code.as_deref() == Some("onshape/history.not-supported")
                && loss.detail.contains("ambiguous")
        ));
    validate(result);
}

#[test]
fn repeated_feature_ids_refuse_ambiguous_consumers_and_keep_independent_records() {
    let bytes = bundle(
        vec![
            sketch("profile"),
            extrude("producer", "profile"),
            extrude("producer", "profile"),
            fillet(),
            extrude("independent", "profile"),
        ],
        &[
            (0, json!([])),
            (1, json!([])),
            (2, json!([body()])),
            (3, json!([body()])),
        ],
    );
    let result = decode(&bytes);
    let output = wire(&result);
    let features = output["model"]["features"].as_array().unwrap();
    assert_eq!(
        features
            .iter()
            .find(|feature| feature["ordinal"] == 3)
            .unwrap()["definition"]["value"]["kind"],
        "native"
    );
    assert_eq!(
        features
            .iter()
            .find(|feature| feature["ordinal"] == 4)
            .unwrap()["definition"]["value"]["kind"],
        "extrude"
    );
    assert_eq!(
        result
            .report()
            .losses()
            .iter()
            .filter(|loss| loss
                .code
                .as_deref()
                .is_some_and(|code| code.starts_with("onshape/")))
            .count(),
        2,
        "{:?}",
        result.report().losses()
    );
    assert!(result
        .report()
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("ambiguous")));
    validate(result);
}

#[test]
fn created_body_scope_survives_modification_but_refuses_contradictions_and_id_reuse() {
    let hole = |native: &str| {
        json!({"featureType":"hole","featureId":native,"parameters":[
            {"parameterId":"locations","queries":[{"queryString":"query=qCompressed(1.0,\"%M5Sa$entityTypeS6$VERTEXSb$historyTypeS8$CREATIONSb$operationIdA1Se$profile.wireOpS9$queryTypeSd$SKETCH_ENTITYSe$sketchEntityIdS6$middle\",id);"}]},
            {"parameterId":"style","value":"SIMPLE"},{"parameterId":"endStyle","value":"THROUGH"},
            {"parameterId":"holeDiameter","expression":"3 mm"}]})
    };
    for case in [
        "creator",
        "latest",
        "unrelated",
        "reused",
        "ambiguous",
        "ambiguous-creation",
    ] {
        let mut changed = body();
        changed["faces"][0]["surface"]["origin"]["z"] = json!(0.001);
        let mut consumer = hole("consumer");
        let mut scope = selection("scope", "body");
        let query = json!({"entityType":"BODY", "historyType":"CREATION",
        "queryType":"SWEPT_BODY", "operationId":[format!(
            "{}.opExtrude",
            match case {
                "latest" => "first-hole",
                "unrelated" => "profile",
                _ => "producer",
            }
        )]});
        let encoded =
            serde_json::to_string(&format!("%{}", super::tests::compact(&query))).unwrap();
        scope["queries"][0]["queryString"] = json!(format!("query=qCompressed(1.0,{encoded},id);"));
        consumer["parameters"].as_array_mut().unwrap().push(scope);
        let mut states = vec![
            (1, json!([])),
            (2, json!([body()])),
            (3, json!([changed.clone()])),
            (4, json!([changed])),
        ];
        if case == "reused" {
            states[2].1 = json!([]);
        }
        if case == "ambiguous-creation" {
            // The latest revision is unique even though the creator cannot be
            // separated from another body feature in the earlier snapshot gap.
            states[3].1[0]["faces"][0]["surface"]["origin"]["z"] = json!(0.0005);
        }
        if matches!(case, "ambiguous" | "ambiguous-creation") {
            states.remove(1);
        }
        let result = decode(&bundle(
            vec![
                sketch("profile"),
                extrude("producer", "profile"),
                hole("first-hole"),
                hole("modifier"),
                consumer,
            ],
            &states,
        ));
        let output = wire(&result);
        if matches!(case, "creator" | "latest") {
            assert_eq!(
                kind(&output, 4),
                "hole",
                "{case}: {:?}",
                result.report().losses()
            );
            assert_eq!(
                output["model"]["features"][4]["dependencies"],
                json!([
                    super::id("feature", "profile"),
                    super::id("feature", "first-hole")
                ])
            );
        } else {
            assert_eq!(kind(&output, 4), "native", "{case}");
            let reason = if case == "ambiguous" {
                "topology producer is ambiguous"
            } else {
                "explicit producer contradicts stored topology"
            };
            assert!(
                result
                    .report()
                    .losses()
                    .iter()
                    .any(|l| l.detail.contains(reason)),
                "{case}: {:?}",
                result.report().losses()
            );
        }
        validate(result);
    }
}
