//! Holes, patterns, sheet-metal walls, fillets and chamfers, and sketch end-point roles.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use serde_json::{json, Value};

use super::{
    blind, design_only, extrude, feature, feature_id, lit, new_body, operation, rows,
    square_sketch, whole_sketch,
};
use crate::design_history::{
    project, CODE_FEATURE, CODE_REFERENCE_FACES, CODE_SHEET_RULE, CODE_THREAD,
};
use crate::Report;

fn xyz(v: [f64; 3]) -> Value {
    json!({"x": v[0], "y": v[1], "z": v[2]})
}

/// A construction that states the value it resolved to.
fn resolved(value: Value) -> Value {
    json!({"inputs": [], "after": [], "resolved_in": {}, "resolved": value})
}

fn point(p: [f64; 3]) -> Value {
    resolved(xyz(p))
}

fn unresolved() -> Value {
    json!({"inputs": [], "after": [], "resolved_in": {}})
}

fn amount(value: f64) -> Value {
    json!({"kind": "amount", "value": lit(json!(value))})
}

/// The design with a block built by feature 1, then `more` features from 2 on.
fn after_block(more: Vec<Value>) -> cadmpeg_ir::CadIr {
    let mut features = vec![feature(
        1,
        "Block",
        extrude(1, Some(blind(10.0)), new_body()),
    )];
    features.extend(more);
    design_only(features, vec![square_sketch(1, 1)], None)
}

fn project_rows(ir: &mut cadmpeg_ir::CadIr) -> (Vec<Value>, Report) {
    let mut report = Report::default();
    project(ir, &mut report).unwrap();
    (rows(ir), report)
}

fn last_operation(rows: &[Value]) -> &Value {
    operation(rows.last().unwrap())
}

fn feature_codes(report: &Report, code: &str) -> Vec<String> {
    report
        .losses()
        .iter()
        .filter(|loss| loss.code.as_deref() == Some(code))
        .map(|loss| loss.detail.clone())
        .collect()
}

// Holes.

fn hole(head: Value, extent: Option<Value>) -> Value {
    let mut value = json!({
        "positions": {"kind": "placed", "value": [point([2.0, 3.0, 10.0]), point([6.0, 7.0, 10.0])]},
        "direction": point([0.0, 0.0, -1.0]),
        "shape": {"kind": "bore", "diameter": lit(json!(4.0)), "head": head},
        "series": [], "native_options": {}, "source_settings": {}});
    if let Some(extent) = extent {
        value["extent"] = extent;
    }
    json!({"kind": "hole", "value": value})
}

fn hole_rows(hole: Value) -> (Vec<Value>, Report) {
    project_rows(&mut after_block(vec![feature(2, "Hole", hole)]))
}

#[test]
fn holes_state_their_positions_form_and_depth() {
    let cases = [
        (
            json!({"kind": "plain"}),
            json!({"kind": "through_all"}),
            json!({"kind": "simple"}),
            json!({"kind": "through_all"}),
        ),
        (
            json!({"kind": "counterbore", "value": {"diameter": lit(json!(8.0)), "depth": lit(json!(3.0))}}),
            amount(5.0),
            json!({"kind": "counterbore", "diameter": 8.0, "depth": 3.0}),
            json!({"kind": "blind", "length": 5.0}),
        ),
        (
            json!({"kind": "countersink", "value": {"diameter": lit(json!(8.0)), "angle": lit(json!(1.5))}}),
            amount(5.0),
            json!({"kind": "countersink", "diameter": 8.0, "angle": 1.5}),
            json!({"kind": "blind", "length": 5.0}),
        ),
    ];
    for (head, limit, form, extent) in cases {
        let (rows, _) = hole_rows(hole(head, Some(limit)));
        let row = last_operation(&rows);
        assert_eq!(row["kind"], "hole");
        let value = &row["value"];
        assert_eq!(
            value["shape"]["construction"],
            json!({"kind": "form", "value": {"kind": form}})
        );
        assert_eq!(value["shape"]["diameter"], 4.0);
        assert_eq!(value["extent"], extent);
        assert_eq!(
            value["placements"],
            json!([
                {"kind": "directed", "position": xyz([2.0, 3.0, 10.0]), "direction": xyz([0.0, 0.0, -1.0])},
                {"kind": "directed", "position": xyz([6.0, 7.0, 10.0]), "direction": xyz([0.0, 0.0, -1.0])},
            ])
        );
        assert!(value.get("profile").is_none());
        assert_eq!(
            rows.last().unwrap()["source_properties"]["projected_from"],
            "design"
        );
    }
}

#[test]
fn what_a_hole_leaves_silent_is_unresolved_not_a_value() {
    // No depth: the row would otherwise read as a hole through everything.
    let (rows, _) = hole_rows(hole(json!({"kind": "plain"}), None));
    assert_eq!(
        last_operation(&rows)["value"]["extent"],
        json!({"kind": "unresolved"})
    );
    // No direction: the placements are left out, so the mapper refuses them by name.
    let mut silent = hole(
        json!({"kind": "plain"}),
        Some(json!({"kind": "through_all"})),
    );
    silent["value"].as_object_mut().unwrap().remove("direction");
    let (rows, _) = hole_rows(silent);
    assert!(last_operation(&rows)["value"].get("placements").is_none());
    // A position left unresolved does the same.
    let mut silent = hole(
        json!({"kind": "plain"}),
        Some(json!({"kind": "through_all"})),
    );
    silent["value"]["positions"]["value"][1] = unresolved();
    let (rows, _) = hole_rows(silent);
    assert!(last_operation(&rows)["value"].get("placements").is_none());
    // An entry the design states in part is the row's unresolved form.
    let (rows, _) = hole_rows(hole(
        json!({"kind": "counterbore", "value": {"diameter": lit(json!(8.0))}}),
        Some(json!({"kind": "through_all"})),
    ));
    assert_eq!(
        last_operation(&rows)["value"]["shape"]["construction"]["value"]["kind"],
        json!({"kind": "unresolved"})
    );
}

#[test]
fn a_hole_the_projection_cannot_state_is_refused_with_its_reason() {
    let through = Some(json!({"kind": "through_all"}));
    let mut slot = hole(json!({"kind": "plain"}), through.clone());
    slot["value"]["shape"] = json!({"kind": "revolved", "at": {"kind": "input"},
        "picks": {"kind": "unread"}});
    let mut sketched = hole(json!({"kind": "plain"}), through.clone());
    sketched["value"]["positions"] = json!({"kind": "points", "value": {
        "points": {"at": {"kind": "input"}, "picks": {"kind": "unread"}}}});
    let mut shallow = hole(
        json!({"kind": "counterbore", "value": {"diameter": lit(json!(3.0)), "depth": lit(json!(1.0))}}),
        through.clone(),
    );
    shallow["value"]["shape"]["diameter"] = lit(json!(4.0));
    let mut tapered = hole(json!({"kind": "plain"}), through);
    tapered["value"]["shape"]["taper"] = lit(json!(0.1));
    for (operation, why) in [
        (slot, "revolved hole"),
        (sketched, "picked from a sketch"),
        (shallow, "does not exceed"),
        (tapered, "a taper"),
    ] {
        let (rows, report) = hole_rows(operation);
        assert_eq!(last_operation(&rows)["kind"], "native", "{why}");
        assert!(
            feature_codes(&report, CODE_FEATURE)
                .iter()
                .any(|detail| detail.contains(why)),
            "{why}: {:?}",
            report.losses()
        );
    }
}

fn threaded(spec: Value) -> Value {
    let mut operation = hole(
        json!({"kind": "plain"}),
        Some(json!({"kind": "through_all"})),
    );
    operation["value"]["thread"] = spec;
    operation
}

#[test]
fn a_thread_with_a_pitch_becomes_a_specification_and_one_without_a_loss() {
    let full = json!({"standard": {"standard": "ISO", "size": lit(json!("M5")), "class": "6H"},
        "pitch": lit(json!(0.8)), "hand": "right", "modeled": lit(json!(false)),
        "major_diameter": lit(json!(5.0)), "length": {"kind": "blind", "value": lit(json!(6.0))}});
    let (rows, report) = hole_rows(threaded(full.clone()));
    let specification =
        &last_operation(&rows)["value"]["shape"]["construction"]["value"]["specification"];
    assert_eq!(
        *specification,
        json!({"kind": "threaded", "standard": "ISO", "designation": "M5", "class": "6H",
            "modeled": false, "cosmetic": true, "pitch": 0.8, "major_diameter": 5.0,
            "hand": "right", "depth": {"kind": "blind", "depth": 6.0}})
    );
    assert!(feature_codes(&report, CODE_THREAD).is_empty());

    // The codec states the designation as the standard and no pitch.
    for (spec, why) in [
        (
            json!({"standard": {"standard": "M5x0.8"}, "major_diameter": lit(json!(5.0))}),
            "states no pitch",
        ),
        (
            json!({"standard": {"standard": "M5"}, "pitch": lit(json!(0.8)), "modeled": lit(json!(false))}),
            "states no hand",
        ),
        (
            json!({"pitch": lit(json!(0.8)), "hand": "right", "modeled": lit(json!(false))}),
            "states no standard",
        ),
    ] {
        let (rows, report) = hole_rows(threaded(spec));
        let row = last_operation(&rows);
        assert_eq!(row["kind"], "hole", "{why}");
        assert!(
            row["value"]["shape"]["construction"]["value"]
                .get("specification")
                .is_none(),
            "{why}"
        );
        let notes = feature_codes(&report, CODE_THREAD);
        assert_eq!(notes.len(), 1, "{why}");
        assert!(notes[0].contains(why), "{why}: {}", notes[0]);
    }
}

// Patterns.

fn seed_features(ids: &[u32]) -> Value {
    let picks: Vec<Value> = ids
        .iter()
        .map(|n| json!({"target": {"kind": "feature_result", "value": feature_id(*n)}}))
        .collect();
    json!({"kind": "features", "features": {"at": {"kind": "input"},
        "picks": {"kind": "named", "value": picks}}})
}

fn run(count: u32, pitch: f64) -> Value {
    json!({"spacing": {"kind": "pitch", "value": {"count": lit(json!(count)), "pitch": lit(json!(pitch))}},
        "sides": {"kind": "one"}, "increments": []})
}

fn linear_run(direction: [f64; 3], count: u32, pitch: f64) -> Value {
    json!({"direction": point(direction), "run": run(count, pitch)})
}

fn pattern(seeds: Value, layout: Value) -> Value {
    json!({"kind": "pattern", "value": {"seeds": seeds, "layout": layout,
        "instances": {"kind": "implicit", "value": []},
        "native_options": {}, "source_settings": {}}})
}

fn pattern_rows(operation: Value) -> (Vec<Value>, Report) {
    project_rows(&mut after_block(vec![
        feature(
            2,
            "Hole",
            hole(
                json!({"kind": "plain"}),
                Some(json!({"kind": "through_all"})),
            ),
        ),
        feature(3, "Copies", operation),
    ]))
}

#[test]
fn linear_patterns_state_their_directions_counts_and_spacing() {
    let layout = json!({"kind": "linear", "value": {
        "first": linear_run([1.0, 0.0, 0.0], 3, 20.0),
        "second": linear_run([0.0, 1.0, 0.0], 2, 15.0),
        "seed_only": lit(json!(false))}});
    let (rows, _) = pattern_rows(pattern(seed_features(&[2]), layout));
    let row = last_operation(&rows);
    assert_eq!(row["kind"], "pattern");
    assert_eq!(
        row["value"]["seeds"],
        json!([{"kind": "feature", "value": feature_id(2)}])
    );
    assert_eq!(
        row["value"]["pattern"],
        json!({"kind": "linear", "direction": xyz([1.0, 0.0, 0.0]), "spacing": 20.0, "count": 3,
            "omitted_instances": [], "symmetric": false,
            "second": {"direction": xyz([0.0, 1.0, 0.0]), "spacing": 15.0, "count": 2, "symmetric": false}})
    );
    assert_eq!(rows.last().unwrap()["dependencies"], json!([feature_id(2)]));
}

#[test]
fn a_run_against_its_direction_turns_the_direction_around() {
    let mut against = linear_run([1.0, 0.0, 0.0], 3, 20.0);
    against["run"]["sides"] = json!({"kind": "against"});
    let layout = json!({"kind": "linear", "value": {"first": against}});
    let (rows, _) = pattern_rows(pattern(seed_features(&[2]), layout));
    assert_eq!(
        last_operation(&rows)["value"]["pattern"]["direction"],
        xyz([-1.0, 0.0, 0.0])
    );
    // A negative pitch is a run against the direction.
    let layout =
        json!({"kind": "linear", "value": {"first": linear_run([0.0, 1.0, 0.0], 3, -5.0)}});
    let (rows, _) = pattern_rows(pattern(seed_features(&[2]), layout));
    let pattern_row = &last_operation(&rows)["value"]["pattern"];
    assert_eq!(pattern_row["direction"], xyz([0.0, -1.0, 0.0]));
    assert_eq!(pattern_row["spacing"], 5.0);
}

#[test]
fn circular_patterns_state_the_angle_of_every_instance() {
    let axis = resolved(json!({"origin": xyz([5.0, 5.0, 0.0]), "direction": xyz([0.0, 0.0, 1.0])}));
    let layout = json!({"kind": "circular", "value": {"axis": axis, "angular": run(4, std::f64::consts::FRAC_PI_2)}});
    let (rows, _) = pattern_rows(pattern(seed_features(&[2]), layout));
    let pattern_row = &last_operation(&rows)["value"]["pattern"];
    assert_eq!(pattern_row["kind"], "circular_angles");
    assert_eq!(pattern_row["axis_origin"], xyz([5.0, 5.0, 0.0]));
    assert_eq!(pattern_row["axis_dir"], xyz([0.0, 0.0, 1.0]));
    let angles: Vec<f64> = pattern_row["angles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_f64().unwrap())
        .collect();
    let want = [0.0, 0.5, 1.0, 1.5].map(|k| k * std::f64::consts::PI);
    assert_eq!(angles.len(), 4);
    for (got, want) in angles.iter().zip(want) {
        assert!((got - want).abs() < 1e-12, "{angles:?}");
    }
}

#[test]
fn mirrors_state_their_plane() {
    let plane = resolved(
        json!({"origin": xyz([20.0, 0.0, 0.0]), "normal": xyz([1.0, 0.0, 0.0]),
        "x_axis": xyz([0.0, 1.0, 0.0])}),
    );
    let layout = json!({"kind": "mirror", "value": {"plane": plane}});
    let (rows, _) = pattern_rows(pattern(seed_features(&[2]), layout));
    assert_eq!(
        last_operation(&rows)["value"]["pattern"],
        json!({"kind": "mirror", "plane_origin": xyz([20.0, 0.0, 0.0]),
            "plane_normal": xyz([1.0, 0.0, 0.0])})
    );
}

#[test]
fn body_seeds_name_the_bodies_the_picks_resolve_to() {
    let body = |n: u32| json!({"kind": "body", "value": format!("test:design:body#{n}")});
    let seeds = json!({"kind": "bodies", "bodies": {"at": {"kind": "input"}, "picks": {
        "kind": "named", "value": [
            {"target": {"kind": "feature_result", "value": feature_id(1)}, "resolved": [body(1), body(2)]},
            {"target": {"kind": "feature_result", "value": feature_id(2)}, "resolved": [body(2)]}]}}});
    let plane = resolved(
        json!({"origin": xyz([0.0; 3]), "normal": xyz([1.0, 0.0, 0.0]),
        "x_axis": xyz([0.0, 1.0, 0.0])}),
    );
    let (rows, _) = pattern_rows(pattern(
        seeds,
        json!({"kind": "mirror", "value": {"plane": plane}}),
    ));
    assert_eq!(
        last_operation(&rows)["value"]["seeds"],
        json!([{"kind": "bodies", "value": {"kind": "bodies",
            "value": ["test:design:body#1", "test:design:body#2"]}}])
    );
    // A pick that resolves to no body names none.
    let seeds = json!({"kind": "bodies", "bodies": {"at": {"kind": "input"}, "picks": {
        "kind": "named", "value": [
            {"target": {"kind": "feature_result", "value": feature_id(1)}}]}}});
    let (rows, report) = pattern_rows(pattern(
        seeds,
        json!({"kind": "mirror", "value": {"plane": unresolved()}}),
    ));
    assert_eq!(last_operation(&rows)["kind"], "native");
    assert!(feature_codes(&report, CODE_FEATURE)
        .iter()
        .any(|detail| detail.contains("not resolved to a body")));
}

#[test]
fn a_pattern_the_projection_cannot_state_is_refused_with_its_reason() {
    let linear = |first: Value| json!({"kind": "linear", "value": {"first": first}});
    let mut spanned = linear_run([1.0, 0.0, 0.0], 3, 20.0);
    spanned["run"]["spacing"] = json!({"kind": "span", "value": {
        "count": lit(json!(3)), "span": lit(json!(40.0))}});
    let mut seeded = linear_run([1.0, 0.0, 0.0], 3, 20.0);
    seeded["run"]["seed"] = lit(json!(1));
    let mut both = linear_run([1.0, 0.0, 0.0], 3, 20.0);
    both["run"]["sides"] = json!({"kind": "symmetric", "value": {}});
    let cases = [
        (
            pattern(
                seed_features(&[9]),
                linear(linear_run([1.0, 0.0, 0.0], 3, 20.0)),
            ),
            "is not part of the history",
        ),
        (
            pattern(seed_features(&[2]), linear(spanned)),
            "count and a pitch",
        ),
        (
            pattern(seed_features(&[2]), linear(seeded)),
            "seed instance",
        ),
        (pattern(seed_features(&[2]), linear(both)), "symmetric run"),
        (
            pattern(
                seed_features(&[2]),
                json!({"kind": "linear", "value": {"first": linear_run([1.0, 0.0, 0.0], 3, 20.0),
                    "seed_only": lit(json!(true))}}),
            ),
            "only the seed",
        ),
        (
            pattern(
                seed_features(&[2]),
                json!({"kind": "linear", "value": {"first": linear_run([1.0, 0.0, 0.0], 3, 20.0),
                    "second": {"direction": unresolved(), "run": run(2, 5.0)}}}),
            ),
            "second run has no resolved direction",
        ),
    ];
    for (operation, why) in cases {
        let (rows, report) = pattern_rows(operation);
        assert_eq!(last_operation(&rows)["kind"], "native", "{why}");
        assert!(
            feature_codes(&report, CODE_FEATURE)
                .iter()
                .any(|detail| detail.contains(why)),
            "{why}: {:?}",
            report.losses()
        );
    }
}

// Sheet metal.

fn wall(side: Option<&str>, rule: bool) -> Value {
    let mut value = json!({"profile": whole_sketch(1), "thickness": lit(json!(2.0))});
    if let Some(side) = side {
        value["side"] = json!(side);
    }
    if rule {
        value["rule"] = json!("test:design:sheet-metal-rule#1");
    }
    json!({"kind": "sheet_metal", "value": {"kind": {"kind": "wall", "value": value},
        "native_options": {}, "source_settings": {}}})
}

fn wall_rows(wall: Value) -> (Vec<Value>, Report) {
    project_rows(&mut design_only(
        vec![feature(1, "Base tab", wall)],
        vec![square_sketch(1, 1)],
        None,
    ))
}

#[test]
fn a_wall_becomes_a_base_flange_on_the_side_it_states() {
    for (side, want) in [
        ("along", "forward"),
        ("against", "reverse"),
        ("symmetric", "symmetric"),
    ] {
        let (rows, report) = wall_rows(wall(Some(side), false));
        let row = last_operation(&rows);
        assert_eq!(row["kind"], "sheet_metal_base_flange", "{side}");
        assert_eq!(row["value"]["side"], want);
        assert_eq!(row["value"]["thickness"], 2.0);
        assert_eq!(
            row["value"]["profile"],
            json!({"kind": "sketch", "value": super::sketch_id(1)})
        );
        assert_eq!(rows.last().unwrap()["dependencies"], json!([rows[0]["id"]]));
        assert!(feature_codes(&report, CODE_SHEET_RULE).is_empty());
    }
    let (_, report) = wall_rows(wall(Some("along"), true));
    assert_eq!(feature_codes(&report, CODE_SHEET_RULE).len(), 1);
}

#[test]
fn a_wall_without_a_side_is_refused_and_a_missing_thickness_left_unstated() {
    let (rows, report) = wall_rows(wall(None, false));
    assert_eq!(last_operation(&rows)["kind"], "native");
    assert!(feature_codes(&report, CODE_FEATURE)
        .iter()
        .any(|detail| detail.contains("states no side")));
    let mut silent = wall(Some("along"), false);
    silent["value"]["kind"]["value"]
        .as_object_mut()
        .unwrap()
        .remove("thickness");
    let (rows, _) = wall_rows(silent);
    assert!(last_operation(&rows)["value"].get("thickness").is_none());
}

// Fillets and chamfers.

fn witness_pick(a: [f64; 3], b: [f64; 3], direction: Option<[f64; 3]>) -> Value {
    let mut witness = json!({"endpoints": [xyz(a), xyz(b)], "curves": []});
    if let Some(direction) = direction {
        witness["direction"] = xyz(direction);
    }
    json!({"target": {"kind": "native", "value": {}}, "witness": witness})
}

fn edges(picks: Vec<Value>) -> Value {
    json!({"at": {"kind": "input"}, "picks": {"kind": "named", "value": picks}})
}

fn controls() -> Value {
    json!({"corners": [], "limits": [], "transitions": []})
}

fn fillet(sets: Vec<Value>) -> Value {
    json!({"kind": "fillet", "value": {"sets": sets, "controls": controls(),
        "native_options": {}, "source_settings": {}}})
}

fn round_set(edges: Value, radius: f64) -> Value {
    json!({"kind": "edges", "edges": edges, "section": {"size": {"kind": "radius", "value": lit(json!(radius))}},
        "hold": [null, null]})
}

fn chamfer(sets: Vec<Value>) -> Value {
    json!({"kind": "chamfer", "value": {"sets": sets, "controls": controls(),
        "native_options": {}, "source_settings": {}}})
}

fn model_topologies(ir: &cadmpeg_ir::CadIr) -> Vec<Value> {
    serde_json::to_value(&ir.model).unwrap()["feature_input_topologies"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// The edge of the block along X at y = 0, z = 10.
fn top_edge() -> Value {
    witness_pick([0.0, 0.0, 10.0], [10.0, 0.0, 10.0], Some([1.0, 0.0, 0.0]))
}

#[test]
fn a_witnessed_edge_becomes_a_one_edge_input_topology_and_a_historical_selection() {
    let mut ir = after_block(vec![feature(
        2,
        "Round",
        fillet(vec![round_set(edges(vec![top_edge()]), 1.5)]),
    )]);
    let (rows, _) = project_rows(&mut ir);
    let id = format!("{}/edge/0", feature_id(2));
    let row = last_operation(&rows);
    assert_eq!(row["kind"], "fillet");
    assert_eq!(
        row["value"]["groups"],
        json!([{"edges": {"kind": "historical", "value": {
            "state": id, "edges": [id], "native": id}},
            "radius": {"kind": "constant", "radius": 1.5}}])
    );
    let topologies = model_topologies(&ir);
    assert_eq!(topologies.len(), 1);
    let topology = &topologies[0];
    assert_eq!(topology["id"], id);
    assert_eq!(topology["input_of"], feature_id(2));
    assert_eq!(topology["edges"], json!([id]));
    assert_eq!(topology["faces"], json!([]));
    let geometry = &topology["edge_geometry"][0];
    assert_eq!(geometry["edge"], id);
    assert_eq!(geometry["start"], xyz([0.0, 0.0, 10.0]));
    assert_eq!(geometry["end"], xyz([10.0, 0.0, 10.0]));
    assert_eq!(geometry["interior"], xyz([5.0, 0.0, 10.0]));
    assert_eq!(geometry["tangent"], xyz([1.0, 0.0, 0.0]));
    assert_eq!(geometry["faces"], json!([]));
}

#[test]
fn every_edge_gets_its_own_state_numbered_through_the_feature() {
    let second = witness_pick([0.0, 10.0, 10.0], [10.0, 10.0, 10.0], Some([1.0, 0.0, 0.0]));
    let third = witness_pick([0.0, 0.0, 0.0], [0.0, 10.0, 0.0], Some([0.0, 1.0, 0.0]));
    let mut ir = after_block(vec![feature(
        2,
        "Round",
        fillet(vec![
            round_set(edges(vec![top_edge()]), 1.0),
            round_set(edges(vec![second]), 1.0),
            round_set(edges(vec![third]), 2.0),
        ]),
    )]);
    let (rows, _) = project_rows(&mut ir);
    let groups = &last_operation(&rows)["value"]["groups"];
    let id = |k: u32| format!("{}/edge/{k}", feature_id(2));
    for k in 0..3_u32 {
        let group = &groups[k as usize];
        assert_eq!(group["edges"]["value"]["state"], id(k));
        assert_eq!(group["edges"]["value"]["edges"], json!([id(k)]));
    }
    assert_eq!(groups[2]["radius"]["radius"], 2.0);
    let topologies = model_topologies(&ir);
    assert_eq!(
        topologies
            .iter()
            .map(|t| t["id"].clone())
            .collect::<Vec<_>>(),
        [json!(id(0)), json!(id(1)), json!(id(2))]
    );
    assert_eq!(
        topologies[2]["edge_geometry"][0]["end"],
        xyz([0.0, 10.0, 0.0])
    );
}

#[test]
fn chamfers_state_their_setbacks_and_leave_the_reference_faces_to_the_mapper() {
    let set = |size: Value, reference: bool| {
        let mut set = json!({"edges": edges(vec![top_edge()]), "size": size});
        if reference {
            set["reference"] = json!({"at": {"kind": "input"}, "picks": {"kind": "unread"}});
        }
        set
    };
    let cases = [
        (
            json!({"kind": "distance", "value": lit(json!(1.0))}),
            false,
            json!({"kind": "distance", "distance": 1.0}),
        ),
        (
            json!({"kind": "distances", "value": {"first": lit(json!(1.0)), "second": lit(json!(2.0))}}),
            true,
            json!({"kind": "two_distances", "first": 1.0, "second": 2.0}),
        ),
        (
            json!({"kind": "distance_angle", "value": {"distance": lit(json!(1.0)), "angle": lit(json!(0.5))}}),
            true,
            json!({"kind": "distance_angle", "distance": 1.0, "angle": 0.5}),
        ),
    ];
    for (size, reference, spec) in cases {
        let (rows, report) = project_rows(&mut after_block(vec![feature(
            2,
            "Chamfer",
            chamfer(vec![set(size, reference)]),
        )]));
        let row = last_operation(&rows);
        assert_eq!(row["kind"], "chamfer");
        assert_eq!(row["value"]["groups"][0]["spec"], spec);
        assert_eq!(row["value"]["flip_direction"], false);
        assert_eq!(
            feature_codes(&report, CODE_REFERENCE_FACES).len(),
            usize::from(reference)
        );
    }
}

#[test]
fn an_edge_the_witness_does_not_pin_down_refuses_the_feature() {
    let no_ends = json!({"target": {"kind": "native", "value": {}},
        "witness": {"direction": xyz([1.0, 0.0, 0.0]), "curves": []}});
    let one_point = json!({"target": {"kind": "native", "value": {}},
        "witness": {"point": xyz([1.0, 0.0, 0.0]), "curves": []}});
    let no_witness = json!({"target": {"kind": "native", "value": {}}});
    let named = json!({"target": {"kind": "feature_result", "value": feature_id(1)},
        "witness": {"endpoints": [xyz([0.0; 3]), xyz([1.0, 0.0, 0.0])], "curves": []}});
    for (pick, why) in [
        (no_ends, "both edge end points"),
        (one_point, "both edge end points"),
        (no_witness, "states no witness"),
        (named, "some way other than a witness"),
    ] {
        let mut ir = after_block(vec![feature(
            2,
            "Round",
            fillet(vec![round_set(edges(vec![pick]), 1.0)]),
        )]);
        let (rows, report) = project_rows(&mut ir);
        assert_eq!(last_operation(&rows)["kind"], "native", "{why}");
        assert!(
            feature_codes(&report, CODE_FEATURE)
                .iter()
                .any(|detail| detail.contains(why)),
            "{why}: {:?}",
            report.losses()
        );
        assert!(model_topologies(&ir).is_empty(), "{why}");
    }
}

#[test]
fn a_witness_without_a_direction_states_no_tangent_and_an_unread_selection_is_unresolved() {
    let pick = witness_pick([0.0, 0.0, 10.0], [10.0, 0.0, 10.0], None);
    let mut ir = after_block(vec![feature(
        2,
        "Round",
        fillet(vec![round_set(edges(vec![pick]), 1.0)]),
    )]);
    project_rows(&mut ir);
    assert!(model_topologies(&ir)[0]["edge_geometry"][0]
        .get("tangent")
        .is_none());

    let unread = json!({"at": {"kind": "input"}, "picks": {"kind": "unread"}});
    let mut ir = after_block(vec![feature(
        2,
        "Round",
        fillet(vec![round_set(unread, 1.0)]),
    )]);
    let (rows, _) = project_rows(&mut ir);
    assert_eq!(
        last_operation(&rows)["value"]["groups"][0]["edges"],
        json!({"kind": "unresolved"})
    );
    assert!(model_topologies(&ir).is_empty());
}

#[test]
fn blends_the_projection_cannot_state_are_refused_with_their_reason() {
    let one = || edges(vec![top_edge()]);
    let mut weighted = round_set(one(), 1.0);
    weighted["section"]["weight"] = lit(json!(0.5));
    let mut held = round_set(one(), 1.0);
    held["hold"] = json!([one(), null]);
    let mut chord = round_set(one(), 1.0);
    chord["section"]["size"] = json!({"kind": "chord", "value": lit(json!(1.0))});
    let mut controlled = fillet(vec![round_set(one(), 1.0)]);
    controlled["value"]["controls"]["keep_features"] = lit(json!(true));
    let mut referenced =
        json!({"edges": one(), "size": {"kind": "distance", "value": lit(json!(1.0))}});
    referenced["reference"] = edges(vec![top_edge()]);
    let cases = [
        (fillet(vec![chord]), "chord fillet"),
        (fillet(vec![held]), "holds a face"),
        (controlled, "the control keep_features"),
        (chamfer(vec![referenced]), "the face its first setback"),
    ];
    for (operation, why) in cases {
        let (rows, report) = project_rows(&mut after_block(vec![feature(2, "Blend", operation)]));
        assert_eq!(last_operation(&rows)["kind"], "native", "{why}");
        assert!(
            feature_codes(&report, CODE_FEATURE)
                .iter()
                .any(|detail| detail.contains(why)),
            "{why}: {:?}",
            report.losses()
        );
    }
    // A tangency weight is carried for the mapper to judge.
    let (rows, _) = project_rows(&mut after_block(vec![feature(
        2,
        "Round",
        fillet(vec![weighted]),
    )]));
    assert_eq!(
        last_operation(&rows)["value"]["groups"][0]["tangency_weight"],
        0.5
    );
}

// Sketch roles.

#[test]
fn the_end_points_a_line_names_become_its_endpoint_refs() {
    let (sketch, mut entities) = square_sketch(1, 1);
    let point_id = |k: u32| format!("test:design:sketch-entity#p{k}");
    let role = |of: String, name: &str| json!({"of": of, "role": {"kind": name}});
    entities[0]["roles"] = json!([role(point_id(0), "start"), role(point_id(1), "end")]);
    entities[1]["roles"] = json!([role(point_id(1), "start")]);
    entities[2]["roles"] = json!([role(point_id(3), "end")]);
    let mut ir = design_only(
        vec![feature(
            1,
            "Block",
            extrude(1, Some(blind(5.0)), new_body()),
        )],
        vec![(sketch, entities)],
        None,
    );
    project(&mut ir, &mut Report::default()).unwrap();
    let wire = serde_json::to_value(&ir.model).unwrap();
    let refs: Vec<&Value> = wire["sketch_entities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entity| &entity["endpoint_refs"])
        .collect();
    assert_eq!(refs[0], &json!([point_id(0), point_id(1)]));
    assert_eq!(refs[1], &json!([point_id(1)]));
    // An end without a start has no place in the ordered list.
    assert_eq!(refs[2], &json!([]));
    assert_eq!(refs[3], &json!([]));
}

#[test]
fn every_projected_row_of_every_kind_says_where_it_came_from() {
    let mut ir = after_block(vec![
        feature(
            2,
            "Hole",
            hole(
                json!({"kind": "plain"}),
                Some(json!({"kind": "through_all"})),
            ),
        ),
        feature(
            3,
            "Round",
            fillet(vec![round_set(edges(vec![top_edge()]), 1.0)]),
        ),
        feature(
            4,
            "Copies",
            pattern(
                seed_features(&[2]),
                json!({"kind": "linear", "value": {"first": linear_run([1.0, 0.0, 0.0], 2, 5.0)}}),
            ),
        ),
    ]);
    let (rows, _) = project_rows(&mut ir);
    assert!(rows.len() >= 5);
    for row in &rows {
        assert_eq!(row["source_properties"]["projected_from"], "design");
    }
}

fn empty_plan() -> crate::design_history::Plan {
    crate::design_history::Plan {
        order: crate::design_history::timeline::Order {
            members: Vec::new(),
            rolled_back: std::collections::BTreeSet::new(),
        },
        features: std::collections::BTreeMap::new(),
        sketch_features: std::collections::BTreeMap::new(),
        owned: std::collections::BTreeMap::new(),
    }
}

/// The typed CADIR rejects an unknown field, so the allowlists are exercised on wire values:
/// an operand outside the known set refuses the feature by name, whichever operation reads it.
#[test]
fn an_operand_outside_an_operations_allowlist_is_refused_naming_the_key() {
    let extra = |mut value: Value, key: &str| {
        value[key] = json!({"kind": "literal", "value": 1});
        value
    };
    let plan = empty_plan();
    let sketches_design = json!({});
    let sketches = crate::design_history::sketch::Sketches::new(&sketches_design);
    let refusals = [
        (
            "hole_twist",
            crate::design_history::hole::project(&extra(
                hole(json!({"kind": "plain"}), Some(amount(5.0)))["value"].clone(),
                "hole_twist",
            ))
            .err(),
        ),
        ("bore_twist", {
            let mut value = hole(json!({"kind": "plain"}), Some(amount(5.0)))["value"].clone();
            value["shape"] = extra(value["shape"].clone(), "bore_twist");
            crate::design_history::hole::project(&value).err()
        }),
        (
            "pattern_twist",
            crate::design_history::pattern::project(&extra(json!({}), "pattern_twist"), &plan)
                .err(),
        ),
        (
            "sheet_twist",
            crate::design_history::sheet::project(
                &extra(json!({}), "sheet_twist"),
                &plan,
                &sketches,
            )
            .err(),
        ),
        (
            "fillet_twist",
            crate::design_history::blend::fillet("f", &extra(json!({}), "fillet_twist")).err(),
        ),
        (
            "chamfer_twist",
            crate::design_history::blend::chamfer("f", &extra(json!({}), "chamfer_twist")).err(),
        ),
    ];
    for (key, refusal) in refusals {
        let refusal = refusal.unwrap_or_else(|| panic!("{key} was not refused"));
        assert!(refusal.contains(key), "{refusal}");
    }
}
