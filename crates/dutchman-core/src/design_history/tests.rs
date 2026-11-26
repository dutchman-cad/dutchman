//! Tests of the design-history projection on synthetic CADIR built in code.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use cadmpeg_ir::CadIr;
use serde_json::{json, Value};

use super::ops::{refuse_stated, EXTRUDE_KEYS, REVOLVE_KEYS};
use super::{
    project, CODE_CONSTRUCTION, CODE_DECLINED, CODE_FEATURE, CODE_PROJECTED, CODE_ROLLED_BACK,
};
use crate::Report;

const SCOPE: &str = "test:design:product-definition#1";
const TIMELINE: &str = "test:design:container#1";

pub(crate) fn feature_id(n: u32) -> String {
    format!("test:design:feature#{n}")
}

pub(crate) fn sketch_id(n: u32) -> String {
    format!("test:design:sketch#{n}")
}

fn lit(value: Value) -> Value {
    json!({"kind": "literal", "value": value})
}

fn frame(origin: [f64; 3], normal: [f64; 3], x: [f64; 3]) -> Value {
    let y = [
        normal[1] * x[2] - normal[2] * x[1],
        normal[2] * x[0] - normal[0] * x[2],
        normal[0] * x[1] - normal[1] * x[0],
    ];
    let xyz = |v: [f64; 3]| json!({"x": v[0], "y": v[1], "z": v[2]});
    json!({"kind": "placed", "inputs": [], "after": [], "resolved_in": {},
        "resolved": {"origin": xyz(origin), "x_axis": xyz(x), "y_axis": xyz(y), "z_axis": xyz(normal)}})
}

/// A planar sketch on the XY plane owned by `owner`, holding a square of side 10 mm.
pub(crate) fn square_sketch(n: u32, owner: u32) -> (Value, Vec<Value>) {
    let sketch = json!({"id": sketch_id(n), "owner": feature_id(owner), "name": format!("Sketch {n}"),
        "plane": frame([0.0; 3], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        "solve": {"status": "current"}, "native_options": {}, "source_settings": {}});
    let corners = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
    let entities = (0..4)
        .map(|k| {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            json!({"id": format!("test:design:sketch-entity#{n}{k}"), "sketch": sketch_id(n),
                "definition": {"kind": "free", "value": {"kind": "line", "value": {
                    "start": {"u": a[0], "v": a[1]}, "end": {"u": b[0], "v": b[1]}}}},
                "roles": [], "native_options": {}, "source_settings": {}})
        })
        .collect();
    (sketch, entities)
}

pub(crate) fn whole_sketch(n: u32) -> Value {
    json!({"at": {"kind": "input"},
        "picks": {"kind": "named", "value": [{"target": {"kind": "sketch", "value": sketch_id(n)}}]}})
}

pub(crate) fn output(boolean: Value) -> Value {
    json!({"body": {"kind": "solid"}, "boolean": boolean})
}

pub(crate) fn new_body() -> Value {
    json!({"kind": "new_body"})
}

pub(crate) fn cut() -> Value {
    json!({"kind": "apply", "op": {"kind": "cut"}})
}

/// A blind span of `length` along the direction.
pub(crate) fn blind(length: f64) -> Value {
    json!({"kind": "between", "start": {"kind": "at_origin"},
        "end": {"kind": "amount", "value": lit(json!(length))}})
}

pub(crate) fn extrude(sketch: u32, span: Option<Value>, boolean: Value) -> Value {
    let mut value = json!({"profile": whole_sketch(sketch), "direction": {"kind": "profile_normal"},
        "origin": {"kind": "profile_plane"},
        "output": output(boolean), "native_options": {}, "source_settings": {}});
    if let Some(span) = span {
        value["span"] = span;
    }
    json!({"kind": "extrude", "value": value})
}

pub(crate) fn revolve(sketch: u32, span: Value) -> Value {
    let xyz = |v: [f64; 3]| json!({"x": v[0], "y": v[1], "z": v[2]});
    json!({"kind": "revolve", "value": {"profile": whole_sketch(sketch),
        "axis": {"inputs": [], "after": [], "resolved_in": {},
            "resolved": {"origin": xyz([0.0; 3]), "direction": xyz([0.0, 1.0, 0.0])}},
        "span": span, "output": output(new_body()),
        "native_options": {}, "source_settings": {}}})
}

pub(crate) fn native(kind: &str) -> Value {
    json!({"kind": "native", "value": {"kind": kind, "native_options": {}, "source_settings": {}}})
}

pub(crate) fn feature(n: u32, name: &str, operation: Value) -> Value {
    json!({"id": feature_id(n), "name": name, "definition": {"operation": operation}})
}

/// The synthetic cube with a design-only history: `features` in timeline order, `sketches`
/// as `(sketch, entities)`, and `rollback` as the feature the history is rolled back before.
pub(crate) fn design_only(
    features: Vec<Value>,
    sketches: Vec<(Value, Vec<Value>)>,
    rollback: Option<u32>,
) -> CadIr {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut wire = serde_json::to_value(&ir).unwrap();
    let members: Vec<Value> = features
        .iter()
        .map(|f| json!({"kind": "feature", "value": f["id"]}))
        .collect();
    let rollback = rollback.map_or_else(
        || json!({"kind": "end"}),
        |n| json!({"kind": "before", "value": {"kind": "feature", "value": feature_id(n)}}),
    );
    let design = &mut wire["design"];
    design["scopes"] = json!([{"definition": SCOPE, "roots": [TIMELINE], "active_arrangements": [],
        "occurrences": {}, "current": [], "unproduced": []}]);
    design["containers"] = json!([{"kind": "ordered", "value": {"id": TIMELINE, "kind": "timeline",
        "members": members, "folders": [], "rollback": rollback}}]);
    design["features"] = Value::Array(features);
    design["sketches"] = Value::Array(sketches.iter().map(|(s, _)| s.clone()).collect());
    design["sketch_entities"] = Value::Array(
        sketches
            .into_iter()
            .flat_map(|(_, entities)| entities)
            .collect(),
    );
    CadIr::from_json(&wire.to_string()).unwrap()
}

/// A sketch, then an extrude of it, as the Solid Edge codec states them: the sketch is owned by
/// the extrude and is no timeline member.
pub(crate) fn block(span: Option<Value>) -> CadIr {
    design_only(
        vec![feature(1, "Block", extrude(1, span, new_body()))],
        vec![square_sketch(1, 1)],
        None,
    )
}

/// The feature rows in timeline order. The model writes its arenas in id order, so the
/// timeline is read back from each row's ordinal.
fn rows(ir: &CadIr) -> Vec<Value> {
    let mut rows = serde_json::to_value(&ir.model).unwrap()["features"]
        .as_array()
        .unwrap()
        .clone();
    rows.sort_by_key(|row| row["ordinal"].as_u64().unwrap());
    rows
}

fn operation(row: &Value) -> &Value {
    &row["definition"]["value"]
}

fn codes(report: &Report) -> Vec<String> {
    report
        .losses()
        .iter()
        .filter_map(|loss| loss.code.clone())
        .collect()
}

#[test]
fn a_sketch_and_a_blind_extrude_become_two_rows_in_order() {
    let mut ir = block(Some(blind(5.0)));
    let mut report = Report::default();
    let written = project(&mut ir, &mut report).unwrap();
    assert_eq!(
        (written.features, written.sketches, written.unprojected),
        (2, 1, 0)
    );
    let rows = rows(&ir);
    assert_eq!(rows[0]["ordinal"], 0);
    assert_eq!(rows[1]["ordinal"], 1);
    assert_eq!(rows[0]["id"], format!("{}/sketch/1", feature_id(1)));
    assert_eq!(operation(&rows[0])["kind"], "sketch");
    assert_eq!(
        operation(&rows[0])["value"]["sketch"]["sketch"],
        sketch_id(1)
    );
    assert_eq!(rows[1]["id"], feature_id(1));
    assert_eq!(rows[1]["name"], "Block");
    assert_eq!(rows[1]["dependencies"], json!([rows[0]["id"]]));
    let extrude = &operation(&rows[1])["value"];
    assert_eq!(
        extrude["profile"]["value"],
        json!({"kind": "sketch", "value": sketch_id(1)})
    );
    assert_eq!(extrude["direction"], json!({"kind": "profile_normal"}));
    assert_eq!(extrude["start"], json!({"kind": "profile_plane"}));
    assert_eq!(
        extrude["extent"],
        json!({"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 5.0}}})
    );
    assert_eq!(extrude["op"], "new_body");
    assert_eq!(extrude["solid"], true);
    for row in &rows {
        assert_eq!(row["source_properties"]["projected_from"], "design");
    }
    let wire = serde_json::to_value(&ir.model).unwrap();
    assert_eq!(wire["sketches"].as_array().unwrap().len(), 1);
    assert_eq!(wire["sketch_entities"].as_array().unwrap().len(), 4);
    assert_eq!(
        wire["sketches"][0]["placement"]["frame"]["normal"]["z"],
        1.0
    );
    assert_eq!(
        wire["sketches"][0]["placement"]["frame"]["u_axis"]["x"],
        1.0
    );
    assert_eq!(codes(&report), [CODE_CONSTRUCTION, CODE_PROJECTED]);
    assert!(report.losses()[1].detail.contains("design layer"));
}

#[test]
fn the_extents_the_design_states_become_the_extents_the_mapper_reads() {
    let along = |a: Value, b: Value| json!({"kind": "between", "start": a, "end": b});
    let amount = |v: f64| json!({"kind": "amount", "value": lit(json!(v))});
    let blind_side = |v: f64| json!({"termination": {"kind": "blind", "length": v}});
    let cases = [
        (
            json!({"kind": "symmetric", "amount": lit(json!(8.0)), "measure": "total"}),
            json!({"kind": "symmetric", "side": blind_side(8.0)}),
            "profile_normal",
        ),
        (
            json!({"kind": "symmetric", "amount": lit(json!(4.0)), "measure": "per_side"}),
            json!({"kind": "symmetric", "side": blind_side(8.0)}),
            "profile_normal",
        ),
        (
            along(amount(2.0), amount(3.0)),
            json!({"kind": "two_sided", "first": blind_side(3.0), "second": blind_side(2.0)}),
            "profile_normal",
        ),
        (
            along(json!({"kind": "at_origin"}), json!({"kind": "through_all"})),
            json!({"kind": "one_sided", "side": {"termination": {"kind": "through_all"}}}),
            "profile_normal",
        ),
        (
            along(amount(6.0), json!({"kind": "at_origin"})),
            json!({"kind": "one_sided", "side": blind_side(6.0)}),
            "reversed_profile_normal",
        ),
        (
            along(json!({"kind": "at_origin"}), amount(-7.0)),
            json!({"kind": "one_sided", "side": blind_side(7.0)}),
            "reversed_profile_normal",
        ),
    ];
    for (span, extent, direction) in cases {
        let mut ir = block(Some(span.clone()));
        let mut report = Report::default();
        project(&mut ir, &mut report).unwrap();
        let rows = rows(&ir);
        let extrude = &operation(&rows[1])["value"];
        assert_eq!(extrude["extent"], extent, "{span}");
        assert_eq!(extrude["direction"]["kind"], direction, "{span}");
        assert_eq!(operation(&rows[1])["kind"], "extrude");
    }
}

#[test]
fn a_cut_states_its_boolean() {
    let mut ir = design_only(
        vec![
            feature(1, "Block", extrude(1, Some(blind(5.0)), new_body())),
            feature(2, "Pocket", extrude(2, Some(blind(2.0)), cut())),
        ],
        vec![square_sketch(1, 1), square_sketch(2, 2)],
        None,
    );
    project(&mut ir, &mut Report::default()).unwrap();
    let rows = rows(&ir);
    assert_eq!(rows.len(), 4);
    assert_eq!(operation(&rows[3])["value"]["op"], "cut");
    assert_eq!(rows[3]["dependencies"], json!([rows[2]["id"]]));
}

#[test]
fn revolves_state_a_full_and_a_partial_turn() {
    let amount = |v: f64| {
        json!({"kind": "between", "start": {"kind": "at_origin"},
            "end": {"kind": "amount", "value": lit(json!(v))}})
    };
    for turn in [std::f64::consts::TAU, std::f64::consts::FRAC_PI_3] {
        let mut ir = design_only(
            vec![feature(1, "Ring", revolve(1, amount(turn)))],
            vec![square_sketch(1, 1)],
            None,
        );
        project(&mut ir, &mut Report::default()).unwrap();
        let rows = rows(&ir);
        let construction = &operation(&rows[1])["value"]["construction"];
        assert_eq!(construction["kind"], "resolved");
        let value = &construction["value"];
        assert_eq!(
            value["extent"],
            json!({"kind": "one_sided", "termination": {"kind": "angle", "angle": turn}})
        );
        assert_eq!(
            value["axis"]["direction"],
            json!({"x": 0.0, "y": 1.0, "z": 0.0})
        );
        assert_eq!(
            value["profile"],
            json!({"kind": "sketch", "value": sketch_id(1)})
        );
        assert_eq!(operation(&rows[1])["value"]["op"], "new_body");
    }
}

#[test]
fn a_suppressed_feature_stays_suppressed() {
    let mut suppressed = feature(1, "Block", extrude(1, Some(blind(5.0)), new_body()));
    suppressed["suppressed"] = lit(json!(true));
    let mut ir = design_only(vec![suppressed], vec![square_sketch(1, 1)], None);
    project(&mut ir, &mut Report::default()).unwrap();
    let rows = rows(&ir);
    assert_eq!(rows[1]["suppressed"], true);
    // The sketch the feature owns follows it.
    assert_eq!(rows[0]["suppressed"], true);
}

#[test]
fn the_timeline_orders_the_rows_not_the_arena() {
    let ir = design_only(
        vec![
            feature(2, "Second", extrude(2, Some(blind(2.0)), cut())),
            feature(1, "First", extrude(1, Some(blind(5.0)), new_body())),
        ],
        vec![square_sketch(1, 1), square_sketch(2, 2)],
        None,
    );
    // The arena holds First before Second, the timeline the reverse.
    let mut wire = serde_json::to_value(&ir).unwrap();
    wire["design"]["features"].as_array_mut().unwrap().reverse();
    let mut ir = CadIr::from_json(&wire.to_string()).unwrap();
    project(&mut ir, &mut Report::default()).unwrap();
    let names: Vec<String> = rows(&ir)
        .iter()
        .map(|row| row["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["Sketch 2", "Second", "Sketch 1", "First"]);
    let ordinals: Vec<u64> = rows(&ir)
        .iter()
        .map(|r| r["ordinal"].as_u64().unwrap())
        .collect();
    assert_eq!(ordinals, [0, 1, 2, 3]);
}

#[test]
fn a_rollback_is_not_a_suppression() {
    let mut ir = design_only(
        vec![
            feature(1, "Block", extrude(1, Some(blind(5.0)), new_body())),
            feature(2, "Pocket", extrude(2, Some(blind(2.0)), cut())),
        ],
        vec![square_sketch(1, 1), square_sketch(2, 2)],
        Some(2),
    );
    let mut report = Report::default();
    project(&mut ir, &mut report).unwrap();
    let rows = rows(&ir);
    let block = rows.iter().find(|row| row["name"] == "Block").unwrap();
    let pocket = rows.iter().find(|row| row["name"] == "Pocket").unwrap();
    assert!(block["source_properties"].get("rolled_back").is_none());
    assert_eq!(pocket["source_properties"]["rolled_back"], "true");
    assert!(rows
        .iter()
        .all(|row| row.get("suppressed").is_none_or(|s| s != &json!(true))));
    assert!(codes(&report).contains(&CODE_ROLLED_BACK.to_owned()));
}

#[test]
fn a_feature_out_of_scope_stops_the_history_with_a_reason() {
    let mut ir = design_only(
        vec![
            feature(1, "Vendor step", native("vendor")),
            feature(2, "Block", extrude(1, Some(blind(5.0)), new_body())),
        ],
        vec![square_sketch(1, 2)],
        None,
    );
    let mut report = Report::default();
    let written = project(&mut ir, &mut report).unwrap();
    assert_eq!(written.unprojected, 1);
    let rows = rows(&ir);
    assert_eq!(rows[0]["name"], "Vendor step");
    assert_eq!(operation(&rows[0])["kind"], "native");
    assert_eq!(rows[0]["source_properties"]["projected_from"], "design");
    let loss = report
        .losses()
        .iter()
        .find(|loss| loss.code.as_deref() == Some(CODE_FEATURE))
        .unwrap();
    assert_eq!(loss.subject, "Vendor step");
    assert!(
        loss.detail.contains("native feature is not projected"),
        "{}",
        loss.detail
    );
}

#[test]
fn what_the_design_leaves_silent_is_written_as_unresolved_not_as_a_value() {
    let ir = block(Some(blind(5.0)));
    let mut wire = serde_json::to_value(&ir).unwrap();
    let value = &mut wire["design"]["features"][0]["definition"]["operation"]["value"];
    value.as_object_mut().unwrap().remove("direction");
    value.as_object_mut().unwrap().remove("origin");
    value["output"] = json!({});
    let mut ir = CadIr::from_json(&wire.to_string()).unwrap();
    project(&mut ir, &mut Report::default()).unwrap();
    let rows = rows(&ir);
    assert_eq!(operation(&rows[1])["kind"], "extrude");
    let extrude = &operation(&rows[1])["value"];
    assert_eq!(extrude["direction"], json!({"kind": "unresolved"}));
    assert_eq!(extrude["start"], json!({"kind": "unresolved"}));
    assert_eq!(extrude["op"], "unresolved");
    assert!(extrude.get("solid").is_none(), "a form was invented");
}

#[test]
fn an_extent_the_design_leaves_silent_is_refused_with_its_reason() {
    let ir = block(Some(blind(5.0)));
    let mut wire = serde_json::to_value(&ir).unwrap();
    let value = &mut wire["design"]["features"][0]["definition"]["operation"]["value"];
    value.as_object_mut().unwrap().remove("span");
    let mut ir = CadIr::from_json(&wire.to_string()).unwrap();
    let mut report = Report::default();
    project(&mut ir, &mut report).unwrap();
    assert_eq!(operation(&rows(&ir)[1])["kind"], "native");
    assert!(
        report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some(CODE_FEATURE)
                && loss.detail.contains("no extent")),
        "{:?}",
        report.losses()
    );
}

#[test]
fn an_operand_not_projected_yet_is_refused_not_guessed() {
    let bound = json!({"kind": "between", "start": {"kind": "at_origin"},
        "end": {"kind": "amount", "value": {"kind": "parameter", "value": "test:design:parameter#1"}}});
    let mut ir = block(Some(bound));
    let mut report = Report::default();
    let written = project(&mut ir, &mut report).unwrap();
    assert_eq!(written.unprojected, 1);
    assert_eq!(operation(&rows(&ir)[1])["kind"], "native");
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("parameter")));

    let ir = block(Some(blind(5.0)));
    let mut wire = serde_json::to_value(&ir).unwrap();
    wire["design"]["sketches"][0]["plane"]
        .as_object_mut()
        .unwrap()
        .remove("resolved");
    let mut ir = CadIr::from_json(&wire.to_string()).unwrap();
    project(&mut ir, &mut Report::default()).unwrap();
    let wire = serde_json::to_value(&ir.model).unwrap();
    assert_eq!(
        wire["sketches"][0]["placement"],
        json!({"kind": "unresolved"})
    );
}

/// Projects a one-feature history whose operation carries one extra operand, and returns the
/// detail of the refusal.
fn refusal_with(mut operation: Value, key: &str, value: Value) -> String {
    operation["value"][key] = value;
    let mut ir = design_only(
        vec![feature(1, "Feature", operation)],
        vec![square_sketch(1, 1)],
        None,
    );
    let mut report = Report::default();
    let written = project(&mut ir, &mut report).unwrap();
    assert_eq!(written.unprojected, 1);
    assert_eq!(operation_kind(&ir), "native");
    report
        .losses()
        .iter()
        .find(|loss| loss.code.as_deref() == Some(CODE_FEATURE))
        .map(|loss| loss.detail.clone())
        .expect("the refusal is reported")
}

fn operation_kind(ir: &CadIr) -> String {
    operation(&rows(ir)[1])["kind"].as_str().unwrap().to_owned()
}

#[test]
fn stated_loops_refuse_the_feature_by_name() {
    let nesting = json!("one_level");
    let detail = refusal_with(
        extrude(1, Some(blind(5.0)), new_body()),
        "loops",
        nesting.clone(),
    );
    assert!(detail.contains("loop"), "{detail}");
    let detail = refusal_with(revolve(1, blind(1.0)), "loops", nesting);
    assert!(detail.contains("loop"), "{detail}");
}

#[test]
fn an_operand_outside_the_allowlist_is_refused_naming_the_key() {
    // The typed CADIR rejects an unknown field, so the allowlist is exercised on the wire value.
    let operation = json!({"profile": {}, "span": {}, "native_options": {}, "source_settings": {},
        "taper_angle": {"kind": "literal", "value": 1.0}});
    let refusal = refuse_stated(&operation, EXTRUDE_KEYS, &[]).unwrap_err();
    assert!(refusal.contains("taper_angle"), "{refusal}");
    let operation = json!({"profile": {}, "twist": true});
    let refusal = refuse_stated(&operation, REVOLVE_KEYS, &[]).unwrap_err();
    assert!(refusal.contains("twist"), "{refusal}");
    // Unstated extras (null or empty) and the known keys do not refuse.
    let operation = json!({"profile": {}, "axis": {}, "twist": null, "tags": [], "note": ""});
    assert!(refuse_stated(&operation, REVOLVE_KEYS, &[]).is_ok());
    // A named operand keeps its own description.
    let operation = json!({"profile": {}, "draft": {"angle": 1.0}});
    let refusal = refuse_stated(&operation, EXTRUDE_KEYS, &[("draft", "a draft")]).unwrap_err();
    assert!(refusal.contains("a draft"), "{refusal}");
}

#[test]
fn it_does_nothing_when_model_history_exists_or_design_has_none() {
    let mut ir = block(Some(blind(5.0)));
    let mut report = Report::default();
    assert!(project(&mut ir, &mut report).is_some());
    let before = serde_json::to_value(&ir.model).unwrap();
    let mut again = Report::default();
    assert!(project(&mut ir, &mut again).is_none());
    assert_eq!(serde_json::to_value(&ir.model).unwrap(), before);
    assert!(again.losses().is_empty());

    let mut empty = cadmpeg_ir::examples::unit_cube().unwrap();
    assert!(project(&mut empty, &mut Report::default()).is_none());
}

#[test]
fn several_scopes_are_declined_by_name() {
    let ir = block(Some(blind(5.0)));
    let mut wire = serde_json::to_value(&ir).unwrap();
    let scope = wire["design"]["scopes"][0].clone();
    let mut other = scope;
    other["definition"] = json!("test:design:product-definition#2");
    other["roots"] = json!([]);
    wire["design"]["scopes"].as_array_mut().unwrap().push(other);
    let mut ir = CadIr::from_json(&wire.to_string()).unwrap();
    let mut report = Report::default();
    assert!(project(&mut ir, &mut report).is_none());
    assert!(ir.model.features.is_empty());
    assert_eq!(codes(&report), [CODE_DECLINED]);
}

#[test]
fn a_sketch_whose_construction_is_silent_is_reported_once_and_marked() {
    let mut ir = block(Some(blind(5.0)));
    let mut report = Report::default();
    project(&mut ir, &mut report).unwrap();
    let rows = rows(&ir);
    assert_eq!(rows[0]["source_properties"]["construction_stated"], "false");
    assert!(rows[1]["source_properties"]
        .get("construction_stated")
        .is_none());
    let notes: Vec<_> = report
        .losses()
        .iter()
        .filter(|loss| loss.code.as_deref() == Some(CODE_CONSTRUCTION))
        .collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].subject, "Sketch 1");
    assert_eq!(notes[0].severity, crate::Severity::Info);
    assert!(notes[0].detail.contains("construction geometry"));
}

#[test]
fn a_sketch_that_states_construction_for_every_entity_is_not_reported() {
    let (sketch, mut entities) = square_sketch(1, 1);
    for entity in &mut entities {
        entity["construction"] = json!(false);
    }
    let mut ir = design_only(
        vec![feature(
            1,
            "Block",
            extrude(1, Some(blind(5.0)), new_body()),
        )],
        vec![(sketch, entities)],
        None,
    );
    let mut report = Report::default();
    project(&mut ir, &mut report).unwrap();
    assert_eq!(codes(&report), [CODE_PROJECTED]);
    assert!(rows(&ir)[0]["source_properties"]
        .get("construction_stated")
        .is_none());
}
