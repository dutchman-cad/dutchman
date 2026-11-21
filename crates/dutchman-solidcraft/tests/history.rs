//! Feature history written end to end, on cadmpeg's example 10 mm cube with a synthetic
//! sketch-and-extrude history added as CADIR rows.
//!
//! The tests that replay the history in SolidCraft run when `SOLIDCRAFT_CLI` names a
//! SolidCraft CLI (CI's round-trip job sets it) and are skipped otherwise. Set
//! `UPDATE_GOLDEN=1` to rewrite the golden file after an intended change.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use dutchman_core::{Check, Mode, Model, Report, Severity, Writer};
use dutchman_solidcraft::{SolidCraftWriter, Verification, CLI_ENV};
use serde_json::{json, Value};
use zip::ZipArchive;

const SKETCH: &str = "f3d:model:sketch#1";

fn entity(key: u32, geometry: Value) -> Value {
    json!({"id": format!("f3d:model:sketch-entity#{key}"), "sketch": SKETCH, "construction": false, "geometry": geometry, "endpoint_refs": []})
}

fn line(key: u32, a: [f64; 2], b: [f64; 2]) -> Value {
    entity(
        key,
        json!({"kind": "line", "start": {"u": a[0], "v": a[1]}, "end": {"u": b[0], "v": b[1]}}),
    )
}

/// The example cube with a history: a 10 mm square on XY, extruded by `length` with
/// `extent` (`None`: one-sided blind) from `profile`.
fn cube_with_history(length: f64, profile: Value, extent: Option<Value>) -> Model {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut wire = serde_json::to_value(&ir).unwrap();
    wire["geometry"]["bodies"][0]["color"] = json!({"r": 0.8, "g": 0.2, "b": 0.1, "a": 1.0});
    let model = &mut wire["model"];
    model["sketches"] = json!([{
        "id": SKETCH,
        "name": "Base",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [[
            {"entity": "f3d:model:sketch-entity#1", "reversed": false},
            {"entity": "f3d:model:sketch-entity#2", "reversed": false},
            {"entity": "f3d:model:sketch-entity#3", "reversed": false},
            {"entity": "f3d:model:sketch-entity#4", "reversed": false},
        ]],
    }]);
    model["sketch_entities"] = json!([
        line(1, [0.0, 0.0], [10.0, 0.0]),
        line(2, [10.0, 0.0], [10.0, 10.0]),
        line(3, [10.0, 10.0], [0.0, 10.0]),
        line(4, [0.0, 10.0], [0.0, 0.0]),
    ]);
    let constraint = |key: u32, definition: Value| {
        json!({"id": format!("f3d:model:sketch-constraint#{key}"), "sketch": SKETCH,
               "definition": definition})
    };
    model["sketch_constraints"] = json!([
        constraint(
            1,
            json!({"kind": "horizontal", "entity": "f3d:model:sketch-entity#1"})
        ),
        constraint(
            2,
            json!({"kind": "vertical", "entity": "f3d:model:sketch-entity#2"})
        ),
        constraint(
            3,
            json!({"kind": "perpendicular", "first": "f3d:model:sketch-entity#1",
                              "second": "f3d:model:sketch-entity#4"})
        ),
        constraint(
            4,
            json!({"kind": "equal", "first": "f3d:model:sketch-entity#1",
                              "second": "f3d:model:sketch-entity#2"})
        ),
        constraint(
            5,
            json!({"kind": "distance", "entities": ["f3d:model:sketch-entity#1"],
                              "parameter": "f3d:model:parameter#2"})
        ),
        constraint(
            7,
            json!({"kind": "horizontal", "entity": "f3d:model:sketch-entity#3"})
        ),
        constraint(
            8,
            json!({"kind": "vertical", "entity": "f3d:model:sketch-entity#4"})
        ),
        constraint(
            6,
            json!({"kind": "native", "native_kind": "Fusion only", "entities": ["f3d:model:sketch-entity#3"], "native_properties": {}, "operands": []})
        ),
    ]);
    let extent = extent.unwrap_or_else(|| {
        json!({"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": length}}})
    });
    model["features"] = json!([
        {"id": "f3d:model:feature#1", "ordinal": 0, "name": "Base sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": SKETCH}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []},
        {"id": "f3d:model:feature#2", "ordinal": 1, "name": "Block", "suppressed": false, "dependencies": ["f3d:model:feature#1"], "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": {"kind": "extrude", "value": {"profile": {"kind":"planar", "value": profile}, "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"}, "extent": extent, "op": "new_body"}}}, "source_properties": {}, "source_content": []},
    ]);
    model["parameters"] = json!([
        {"id": "f3d:model:parameter#1", "owner": "f3d:model:feature#2", "name": "d1", "expression": format!("{length} mm"), "value": {"kind": "length", "value": length}, "ordinal": 0, "dependencies": [], "properties": {}},
        {"id": "f3d:model:parameter#2", "owner": "f3d:model:feature#1", "ordinal": 1, "name": "side", "expression": "10 mm", "value": {"kind": "length", "value": 10.0}, "dependencies": [], "properties": {}},
    ]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "cube".to_owned(),
        source_file: Some("cube.f3d".to_owned()),
        source_step: None,
        assembly: None,
    }
}

const TOP: &str = "f3d:model:sketch#2";

/// The example cube built instead by a loft from its 10 mm base square to a square of side
/// `top` centred above it, 10 mm up; `change` edits the loft's definition.
fn loft_section(section: Value) -> Value {
    let kind = match section["kind"].as_str() {
        Some("point" | "native_point" | "vertex") => "point",
        _ => "profile",
    };
    let section = if kind == "profile" {
        json!({"kind": "planar", "value": section})
    } else {
        section
    };
    json!({"kind": kind, "value": section})
}

fn loft_history(top: f64, change: impl FnOnce(&mut Value)) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    let mut sketches = model["sketches"].as_array().unwrap().clone();
    let (a, b) = (5.0 - top / 2.0, 5.0 + top / 2.0);
    let top_loop: Vec<Value> = [11, 12, 13, 14]
        .map(|k| json!({"entity": format!("f3d:model:sketch-entity#{k}"), "reversed": false}))
        .to_vec();
    sketches.push(json!({
        "id": TOP,
        "name": "Top",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 10.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [top_loop],
    }));
    model["sketches"] = Value::Array(sketches);
    let top_line = |key: u32, from: [f64; 2], to: [f64; 2]| {
        let mut row = line(key, from, to);
        row["sketch"] = json!(TOP);
        row
    };
    let entities = model["sketch_entities"].as_array_mut().unwrap();
    entities.extend([
        top_line(11, [a, a], [b, a]),
        top_line(12, [b, a], [b, b]),
        top_line(13, [b, b], [a, b]),
        top_line(14, [a, b], [a, a]),
    ]);
    let mut definition = json!({"kind": "loft", "value": {"sections": [
            first_profile(),
            {"kind": "sketch_profiles", "value": {"sketch": TOP, "profiles": [0]}},
        ], "guidance": {"kind": "guides", "value": []}, "op": "new_body", "closed": false, "solid": true, "ruled": false, "linearize": false}});
    change(&mut definition["value"]);
    if definition["value"].get("kind").is_some() {
        definition = definition["value"].take();
    } else {
        let sections = definition["value"]["sections"].as_array_mut().unwrap();
        for section in sections {
            *section = loft_section(section.take());
        }
    }

    model["features"] = json!([
        model["features"][0].clone(),
        {"id": "f3d:model:feature#3", "ordinal": 1, "name": "Top sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": TOP}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []},
        {"id": "f3d:model:feature#2", "ordinal": 2, "name": "Block", "suppressed": false, "dependencies": ["f3d:model:feature#1", "f3d:model:feature#3"], "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": definition}, "source_properties": {}, "source_content": []},
    ]);
    // The extrude's length parameter has no feature to own it now.
    model["parameters"] = json!([model["parameters"][1].clone()]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

const PATH: &str = "f3d:model:sketch#3";

/// The example cube built instead by sweeping its 10 mm base square along a 10 mm line up Z,
/// drawn in a sketch on XZ; `change` edits the sweep's definition.
fn sweep_history(change: impl FnOnce(&mut Value)) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    let mut sketches = model["sketches"].as_array().unwrap().clone();
    sketches.push(json!({
        "id": PATH,
        "name": "Path",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": -1.0, "z": 0.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [],
    }));
    model["sketches"] = Value::Array(sketches);
    let mut path = line(21, [0.0, 0.0], [0.0, 10.0]);
    path["sketch"] = json!(PATH);
    model["sketch_entities"].as_array_mut().unwrap().push(path);
    let mut definition = json!({"kind": "sweep", "value": {"shape": {"kind": "solid", "op": "new_body", "section": {"kind": "profile", "value": first_profile()}, "sections": []}, "path": {"kind": "sketch_curves",
                 "value": {"sketch": PATH, "curves": ["f3d:model:sketch-entity#21"]}}, "path_tangent": false, "linearize": false, "path_extent": {"along_fraction": 1.0, "against_fraction": 0.0}}});
    change(&mut definition["value"]);
    model["features"] = json!([
        model["features"][0].clone(),
        {"id": "f3d:model:feature#3", "ordinal": 1, "name": "Path sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": PATH}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []},
        {"id": "f3d:model:feature#2", "ordinal": 2, "name": "Block", "suppressed": false, "dependencies": ["f3d:model:feature#1", "f3d:model:feature#3"], "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": definition}, "source_properties": {}, "source_content": []},
    ]);
    model["parameters"] = json!([model["parameters"][1].clone()]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

#[test]
fn a_sweep_along_sketch_curves_becomes_a_solidcraft_sweep() {
    let (document, report) = convert(
        &sweep_history(|_| {}),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = document["features"].as_array().unwrap();
    let id_of = |name: &str| features.iter().find(|f| f["name"] == name).unwrap()["id"].clone();
    let sweep = features.iter().find(|f| f["type"] == "sweep").unwrap();
    assert_eq!(sweep["name"], "Block");
    assert_eq!(sweep["sketch"], id_of("Base sketch"));
    assert_eq!(sweep["path_sketch"], id_of("Path sketch"));
    assert_eq!(sweep["path"].as_array().unwrap().len(), 1);
    assert_eq!(sweep["operation"], "new_body");
    assert_eq!(sweep["body_names"], json!(["unit cube"]));
    assert!(sweep.get("twist").is_none());
    assert!(sweep.get("scale").is_none());
    assert!(sweep.get("orientation").is_none());
    assert!(
        history_line(&report).contains("3 of 3 features rebuilt"),
        "{}",
        history_line(&report)
    );

    let (document, _) = convert(
        &sweep_history(|d| d["twist"] = json!(std::f64::consts::FRAC_PI_2)),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let sweep = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "sweep")
        .unwrap()
        .clone();
    assert_eq!(sweep["twist"], "90 deg");
}

const SPATIAL: &str = "f3d:model:spatial-sketch#1";

fn xyz(p: [f64; 3]) -> Value {
    json!({"x": p[0], "y": p[1], "z": p[2]})
}

fn spatial_entity(key: u32, geometry: Value) -> Value {
    json!({"id": format!("f3d:model:spatial-sketch-entity#{key}"), "sketch": SPATIAL, "construction": false, "geometry": geometry, "endpoint_refs": []})
}

fn spatial_id(key: u32) -> String {
    format!("f3d:model:spatial-sketch-entity#{key}")
}

/// The unit square swept along a 3D sketch: a 30 mm line up Z, a quarter arc of radius 20 mm
/// turning toward +X, then a cubic Bezier spline; `change` edits the spatial sketch's entities
/// and the sweep's definition.
fn spatial_sweep_history(change: impl FnOnce(&mut Vec<Value>, &mut Value)) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    model["spatial_sketches"] = json!([{"id": SPATIAL, "name": "Rail", "profiles": []}]);
    let mut entities = vec![
        spatial_entity(
            1,
            json!({"kind": "line", "start": xyz([0.0, 0.0, 0.0]), "end": xyz([0.0, 0.0, 30.0])}),
        ),
        spatial_entity(
            2,
            json!({"kind": "arc", "center": xyz([20.0, 0.0, 30.0]), "normal": xyz([0.0, 1.0, 0.0]),
                   "reference_direction": xyz([-1.0, 0.0, 0.0]), "radius": 20.0,
                   "start_angle": 0.0, "end_angle": std::f64::consts::FRAC_PI_2}),
        ),
        spatial_entity(
            3,
            json!({"kind": "nurbs", "curve": {
                "degree": 3,
                "knots": [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                "poles": {"kind": "polynomial", "points": [
                    xyz([20.0, 0.0, 50.0]), xyz([30.0, 0.0, 50.0]),
                    xyz([40.0, 5.0, 52.0]), xyz([50.0, 10.0, 60.0])]},
                "periodic": false}}),
        ),
    ];
    let mut definition = json!({"kind": "sweep", "value": {"shape": {"kind": "solid", "op": "new_body", "section": {"kind": "profile", "value": first_profile()}, "sections": []}, "path": {"kind": "spatial_sketch_curves",
                 "value": {"sketch": SPATIAL, "curves": [spatial_id(1), spatial_id(2), spatial_id(3)]}}, "path_tangent": false, "linearize": false, "path_extent": {"along_fraction": 1.0, "against_fraction": 0.0}}});
    change(&mut entities, &mut definition["value"]);
    model["spatial_sketch_entities"] = Value::Array(entities);
    model["features"] = json!([
        model["features"][0].clone(),
        {"id": "f3d:model:feature#3", "ordinal": 1, "name": "Rail sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "spatial_sketch", "value": {"sketch": SPATIAL}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []},
        {"id": "f3d:model:feature#2", "ordinal": 2, "name": "Block", "suppressed": false, "dependencies": ["f3d:model:feature#1", "f3d:model:feature#3"], "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": definition}, "source_properties": {}, "source_content": []},
    ]);
    model["parameters"] = json!([model["parameters"][1].clone()]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

#[test]
fn a_spatial_sketch_becomes_a_3d_sketch_and_a_sweep_follows_its_curves() {
    // Wire constraints are written from SolidCraft 0.44, so the target is named rather than
    // taken from whichever SolidCraft the test run finds.
    let (document, report) = convert(
        &spatial_sweep_history(|_, _| {}),
        &SolidCraftWriter::new()
            .verification(Verification::Off)
            .target_version(Some("0.44.0".parse().unwrap())),
    );
    let features = document["features"].as_array().unwrap();
    let rail = features
        .iter()
        .find(|f| f["name"] == "Rail sketch")
        .unwrap();
    assert_eq!(rail["type"], "sketch");
    assert_eq!(rail["sketch"]["view"]["three_d"], true);
    let wires = rail["sketch"]["wires"].as_array().unwrap();
    assert_eq!(wires.len(), 3);
    assert_eq!(wires[0]["fit"], json!([[0.0, 0.0, 0.0], [0.0, 0.0, 30.0]]));
    assert_eq!(wires[1]["arc"]["radius"], 20.0);
    assert_eq!(wires[1]["arc"]["end"][2], 50.0);
    assert!(wires[2]["pts"].as_array().unwrap().len() > 20);
    assert_eq!(wires[2]["pts"][0], json!([20.0, 0.0, 50.0]));
    assert_eq!(
        wires[2]["pts"].as_array().unwrap().last().unwrap(),
        &json!([50.0, 10.0, 60.0])
    );
    assert_eq!(rail["sketch"]["counters"], json!({"q": 0, "w": 3}));
    assert_eq!(rail["sketch"]["wire_constraints"], json!([]));
    let sweep = features.iter().find(|f| f["type"] == "sweep").unwrap();
    assert_eq!(sweep["path_sketch"], rail["id"]);
    assert_eq!(sweep["path"], json!(["w1", "w2", "w3"]));
    assert!(
        history_line(&report).contains("3 of 3 features rebuilt"),
        "{}",
        history_line(&report)
    );
    assert!(
        report
            .losses()
            .iter()
            .any(|loss| loss.subject == "3D sketch \"Rail sketch\""
                && loss.detail.contains("1 spline")),
        "{report:?}"
    );
}

#[test]
fn circles_points_and_unmappable_geometry_in_a_spatial_sketch_are_reported() {
    let model = spatial_sweep_history(|entities, definition| {
        entities.push(spatial_entity(
            4,
            json!({"kind": "circle", "center": xyz([0.0, 0.0, 0.0]), "normal": xyz([0.0, 0.0, 1.0]),
                   "reference_direction": xyz([1.0, 0.0, 0.0]), "radius": 3.0}),
        ));
        entities.push(spatial_entity(
            5,
            json!({"kind": "point", "position": xyz([1.0, 2.0, 3.0])}),
        ));
        entities.push(spatial_entity(
            6,
            json!({"kind": "native", "native_kind": "Helix"}),
        ));
        definition["path"]["value"]["curves"] =
            json!([spatial_id(1), spatial_id(2), spatial_id(3), spatial_id(4)]);
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new()
            .target_version(Some("0.43.0".parse().unwrap()))
            .verification(Verification::Off),
    );
    let rail = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Rail sketch")
        .unwrap();
    let wires = rail["sketch"]["wires"].as_array().unwrap();
    // line, arc, spline, two half arcs of the circle, and the point
    assert_eq!(wires.len(), 6, "{wires:?}");
    assert_eq!(wires[3]["arc"]["radius"], 3.0);
    assert_eq!(wires[5]["id"], "q1");
    assert_eq!(rail["sketch"]["counters"], json!({"q": 1, "w": 5}));
    let sweep = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "sweep")
        .unwrap();
    assert_eq!(sweep["path"], json!(["w1", "w2", "w3", "w4", "w5"]));
    let losses = report
        .losses()
        .iter()
        .map(|loss| format!("{}: {}", loss.subject, loss.detail))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        losses.contains("1 curve left out: source geometry \"Helix\" has no neutral form"),
        "{losses}"
    );
}

#[test]
fn a_sweep_along_a_curve_a_spatial_sketch_could_not_carry_stops_the_history_there() {
    let model = spatial_sweep_history(|entities, _| {
        entities[2] = spatial_entity(3, json!({"kind": "native", "native_kind": "Helix"}));
    });
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let line = history_line(&report);
    assert!(
        line.contains("its path uses a curve the 3D sketch does not carry"),
        "{line}"
    );
}

#[test]
fn a_sweep_selecting_spatial_curves_by_native_records_is_a_precise_reason() {
    let model = spatial_sweep_history(|_, definition| {
        definition["path"] = json!({"kind": "spatial_sketch_selection",
            "value": {"sketch": SPATIAL, "selections": ["native"]}});
    });
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("native selection records"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn a_sweep_along_a_spatial_sketch_replays_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let version =
        dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()));
    if version < "0.26.1".parse().unwrap() {
        eprintln!("skipped: 3D sketches were checked from SolidCraft 0.26.1");
        return;
    }
    // The line and arc entities are stored against the path direction in the second run.
    for reversed in [false, true] {
        let model = spatial_sweep_history(|entities, _| {
            if reversed {
                entities[0] = spatial_entity(
                    1,
                    json!({"kind": "line", "start": xyz([0.0, 0.0, 30.0]), "end": xyz([0.0, 0.0, 0.0])}),
                );
            }
        });
        let mut report = Report::default();
        let mut bytes = Vec::new();
        SolidCraftWriter::new()
            .verification(Verification::Off)
            .write(&model, &mut bytes, &mut report)
            .unwrap();
        let body = bounds(&cli, &bytes);
        assert_eq!(body.len(), 1);
        let (min, max) = body[0];
        assert!(min[2].abs() < 1e-6, "{min:?}");
        assert!(max[2] > 59.0 && max[0] > 49.0, "{max:?}");
        let volume = volumes(&cli, &bytes)[0];
        // A 10 mm square swept some 80 mm along its path.
        assert!(volume > 5000.0 && volume < 12000.0, "{volume}");
    }
}

#[test]
fn sweep_options_keep_their_units_and_result_mode() {
    for surface in [false, true] {
        for (change, key, expected) in [
            (json!({"scale": 2.0}), "scale", json!("2")),
            (json!({"scale": 0.01}), "scale", json!("0.01")),
            (json!({"scale": 100.0}), "scale", json!("100")),
            (
                json!({"orientation": {"kind": "binormal",
                    "direction": {"x": 0.0, "y": 1.0, "z": 0.0}}}),
                "orientation",
                json!({"direction": [0.0, 1.0, 0.0]}),
            ),
            (
                json!({"orientation": {"kind": "fixed"}}),
                "orientation",
                json!("parallel"),
            ),
            (
                json!({"twist": -std::f64::consts::FRAC_PI_2}),
                "twist",
                json!("-90 deg"),
            ),
        ] {
            let model = sweep_history(|d| {
                if surface {
                    d["shape"]["kind"] = json!("surface");
                    d["shape"].as_object_mut().unwrap().remove("op");
                }
                for (key, value) in change.as_object().unwrap() {
                    d[key] = value.clone();
                }
            });
            let (document, report) = convert(
                &model,
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            let kind = if surface { "surface_sweep" } else { "sweep" };
            let sweep = document["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["type"] == kind)
                .unwrap();
            assert_eq!(sweep[key], expected);
            assert!(sweep["profiles"].is_object());
            assert_eq!(sweep["body_names"], json!(["unit cube"]));
            assert_eq!(sweep.get("operation").is_none(), surface);
            assert!(history_line(&report).contains("3 of 3 features rebuilt"));
        }
    }
}

#[test]
fn a_single_curve_whole_sketch_maps_as_a_sweep_path() {
    let model = sweep_history(|d| d["path"] = json!({"kind": "sketch", "value": PATH}));
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = document["features"].as_array().unwrap();
    let sweep = features.iter().find(|f| f["type"] == "sweep").unwrap();
    let path = features
        .iter()
        .find(|f| f["name"] == "Path sketch")
        .unwrap();
    assert_eq!(sweep["path_sketch"], path["id"]);
    assert_eq!(sweep["path"].as_array().unwrap().len(), 1);
    assert_eq!(sweep["path"][0], path["sketch"]["curves"][0]["id"]);
    assert!(history_line(&report).contains("3 of 3 features rebuilt"));
}

#[test]
fn whole_sketch_paths_require_one_non_construction_curve() {
    for (empty, construction, reason) in [
        (true, false, Some("its path sketch has no path curves")),
        (
            false,
            false,
            Some("a whole-sketch path with several curves has no resolved traversal order"),
        ),
        (false, true, None),
    ] {
        let base = sweep_history(|d| d["path"] = json!({"kind": "sketch", "value": PATH}));
        let mut wire = serde_json::to_value(&base.ir).unwrap();
        let entities = wire["model"]["sketch_entities"].as_array_mut().unwrap();
        if empty {
            entities.retain(|entity| entity["sketch"] != PATH);
        } else {
            let mut extra = line(22, [0.0, 10.0], [0.0, 20.0]);
            extra["sketch"] = json!(PATH);
            extra["construction"] = json!(construction);
            entities.push(extra);
        }
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..base
        };
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        if let Some(reason) = reason {
            assert!(
                history_line(&report).contains(reason),
                "{}",
                history_line(&report)
            );
        } else {
            let sweep = document["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["type"] == "sweep")
                .unwrap();
            assert_eq!(sweep["path"].as_array().unwrap().len(), 1);
        }
    }
}

fn railed_sweep_history(whole_sketch: bool) -> Model {
    const RAIL: &str = "f3d:model:sketch#4";
    let base = sweep_history(|d| {
        d["guide_rail"] = json!({
            "path": {"kind": "sketch_curves", "value": {
                "sketch": RAIL, "curves": ["f3d:model:sketch-entity#22"]}},
            "extent": {"along_fraction": 1.0, "against_fraction": 0.0},
        });
        if whole_sketch {
            d["guide_rail"]["path"] = json!({"kind": "sketch", "value": RAIL});
        }
    });
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    let mut rail_sketch = model["sketches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sketch| sketch["id"] == PATH)
        .unwrap()
        .clone();
    rail_sketch["id"] = json!(RAIL);
    rail_sketch["name"] = json!("Rail");
    model["sketches"].as_array_mut().unwrap().push(rail_sketch);
    let mut rail = line(22, [10.0, 0.0], [10.0, 10.0]);
    rail["sketch"] = json!(RAIL);
    model["sketch_entities"].as_array_mut().unwrap().push(rail);
    let features = model["features"].as_array_mut().unwrap();
    let sweep = features.last_mut().unwrap();
    sweep["ordinal"] = json!(3);
    sweep["dependencies"]
        .as_array_mut()
        .unwrap()
        .push(json!("f3d:model:feature#4"));
    features.insert(
        2,
        json!({"id": "f3d:model:feature#4", "ordinal": 2, "name": "Rail sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": RAIL}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
    );
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

#[test]
fn a_sweep_guide_rail_keeps_its_sketch_and_curve_selection() {
    for whole_sketch in [false, true] {
        let (document, report) = convert(
            &railed_sweep_history(whole_sketch),
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let features = document["features"].as_array().unwrap();
        let sweep = features.iter().find(|f| f["type"] == "sweep").unwrap();
        let rail = features
            .iter()
            .find(|f| f["name"] == "Rail sketch")
            .unwrap();
        assert_eq!(sweep["rails"][0]["sketch"], rail["id"]);
        assert_eq!(sweep["rails"][0]["curves"].as_array().unwrap().len(), 1);
        assert_ne!(sweep["rails"][0]["sketch"], sweep["path_sketch"]);
        assert_eq!(
            sweep["rails"][0]["curves"][0],
            rail["sketch"]["curves"][0]["id"]
        );
        for key in ["orientation", "twist", "scale"] {
            assert!(sweep.get(key).is_none());
        }
        assert!(history_line(&report).contains("4 of 4 features rebuilt"));
    }
}

#[test]
fn solidcraft_replays_whole_sketch_paths_and_guide_rails() {
    let Some(cli) = solidcraft() else { return };
    for model in [
        sweep_history(|d| d["path"] = json!({"kind": "sketch", "value": PATH})),
        railed_sweep_history(false),
        railed_sweep_history(true),
    ] {
        let volume = volumes(&cli, &written(&model).0);
        assert_eq!(volume.len(), 1);
        assert!((volume[0] - 1000.0).abs() / 1000.0 < dutchman_solidcraft::VOLUME_TOLERANCE);
    }
}

#[test]
fn guide_rails_cannot_be_combined_with_sweep_options() {
    for change in [
        json!({"scale": 2.0}),
        json!({"twist": 0.5}),
        json!({"orientation": {"kind": "fixed"}}),
    ] {
        let model = sweep_history(|d| {
            d["guide_rail"] = json!({"path": d["path"].clone(),
                "extent": {"along_fraction": 1.0, "against_fraction": 0.0}});
            for (key, value) in change.as_object().unwrap() {
                d[key] = value.clone();
            }
        });
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).contains(
            "sweeps combining a guide rail with orientation, twist or scale are not mapped yet"
        ));
    }
}

#[test]
fn solidcraft_replays_scaled_and_binormal_sweeps_and_surface_sweeps() {
    let Some(cli) = solidcraft() else { return };
    // Linear scaling from 1 to 2 gives area 100 * (1 + t)^2 over 10 mm.
    let scaled = sweep_history(|d| d["scale"] = json!(2.0));
    let volume = volumes(&cli, &written(&scaled).0);
    let expected = 1000.0 * 7.0 / 3.0;
    assert_eq!(volume.len(), 1);
    assert!((volume[0] - expected).abs() / expected < dutchman_solidcraft::VOLUME_TOLERANCE);

    let directed = sweep_history(|d| {
        d["orientation"] = json!({"kind": "binormal",
            "direction": {"x": 0.0, "y": 1.0, "z": 0.0}});
    });
    let volume = volumes(&cli, &written(&directed).0);
    assert_eq!(volume.len(), 1);
    assert!((volume[0] - 1000.0).abs() / 1000.0 < dutchman_solidcraft::VOLUME_TOLERANCE);

    let surface = sweep_history(|d| {
        d["shape"]["kind"] = json!("surface");
        d["shape"].as_object_mut().unwrap().remove("op");
    });
    let boxes = bounds(&cli, &written(&surface).0);
    assert_eq!(boxes.len(), 1);
    for (actual, expected) in [
        (boxes[0].0, [0.0, 0.0, 0.0]),
        (boxes[0].1, [10.0, 10.0, 10.0]),
    ] {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6, "{boxes:?}");
        }
    }
}

#[test]
fn sweeps_solidcraft_would_build_differently_flatten_the_history() {
    for (change, reason) in [
        (
            json!({"shape": {"kind": "surface", "section": {"kind": "unresolved"}, "sections": []}}),
            "its sweep section is not resolved",
        ),
        (
            json!({"path_extent": {"along_fraction": 0.5, "against_fraction": 0.0}}),
            "sweeps along part of their path are not mapped yet",
        ),
        (json!({"taper": 0.1}), "tapered sweeps are not mapped yet"),
        (
            json!({"scale": 0.001}),
            "sweep end scales outside 0.01 to 100 are not mapped yet",
        ),
        (
            json!({"scale": 101.0}),
            "sweep end scales outside 0.01 to 100 are not mapped yet",
        ),
        (
            json!({"twist": 5.0 * std::f64::consts::TAU}),
            "sweep twists beyond four turns are not mapped yet",
        ),
        (
            json!({"orientation": {"kind": "frenet"}}),
            "sweeps oriented by frenet are not mapped yet",
        ),
        (
            json!({"orientation": {"kind": "auxiliary", "tangent": false,
                "curvilinear": true, "path": {"kind": "sketch", "value": PATH}}}),
            "sweeps oriented by an auxiliary path are not mapped yet",
        ),
        (
            json!({"orientation": {"kind": "guide_surface", "faces": {"kind": "unresolved"}}}),
            "sweeps oriented by guide faces are not mapped yet",
        ),
        (
            json!({"transition": "right_corner"}),
            "sweeps with right corner transitions are not mapped yet",
        ),
        (
            json!({"transformation": "linear"}),
            "sweeps with linear section interpolation are not mapped yet",
        ),
        (
            json!({"shape": {"kind": "solid", "op": "new_body", "section": {"kind": "profile", "value": first_profile()}, "sections": [{"kind": "profile", "value": first_profile()}]}}),
            "sweeps through several sections are not mapped yet",
        ),
        (
            json!({"transition": "round_corner"}),
            "sweeps with round corner transitions are not mapped yet",
        ),
        (
            json!({"path": {"kind": "native", "value": "n"}}),
            "a path given by a native reference is not mapped yet",
        ),
        (
            json!({"path": {"kind": "unresolved", "value": "n"}}),
            "its path is not resolved",
        ),
        (
            json!({"guide_rail": {"path": {"kind": "sketch", "value": PATH},
                "extent": {"along_fraction": 0.5, "against_fraction": 0.0}}}),
            "sweeps along part of their guide rail are not mapped yet",
        ),
        (
            json!({"path_tangent": true}),
            "sweeps that extend the path through tangent-connected edges are not mapped yet",
        ),
        (
            json!({"linearize": true}),
            "sweeps that simplify linear edges and planar faces are not mapped yet",
        ),
        (
            json!({"transformation": "multi_section"}),
            "sweeps with multiple section interpolation are not mapped yet",
        ),
        (
            json!({"transformation": "s_shape"}),
            "sweeps with S-shaped section interpolation are not mapped yet",
        ),
        (
            json!({"transformation": "interpolation"}),
            "sweeps with native smooth section interpolation are not mapped yet",
        ),
        (
            json!({"shape": {"kind": "solid", "op": "new_body", "section": {"kind": "generated", "value": {
                    "kind": "circular_region", "region": {"outer_radius": 2.0}}}, "sections": []}}),
            "sweeps with generated circular sections are not mapped yet",
        ),
    ] {
        let model = sweep_history(|definition| {
            for (key, value) in change.as_object().unwrap() {
                definition[key] = value.clone();
            }
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            document["features"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["type"] != "sweep" && f["type"] != "surface_sweep"),
            "{reason}"
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn a_sweep_inside_an_assembly_keeps_its_own_sketches() {
    let mut model = assembly_of_histories();
    for part in &mut model.assembly.as_mut().unwrap().parts {
        part.ir = sweep_history(|_| {}).ir;
    }
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = document["features"].as_array().unwrap();
    let sweeps: Vec<&Value> = features.iter().filter(|f| f["type"] == "sweep").collect();
    assert_eq!(sweeps.len(), 2);
    for sweep in sweeps {
        for key in ["sketch", "path_sketch"] {
            let sketch = features.iter().find(|f| f["id"] == sweep[key]).unwrap();
            assert_eq!(sketch["type"], "sketch", "{key}");
            assert_eq!(sketch["component"], sweep["component"], "{key}");
        }
    }
}

#[test]
fn solidcraft_sweeps_the_profile_along_the_path() {
    let Some(cli) = solidcraft() else { return };
    // The cube, and twisted a quarter turn about the path through the square's corner: the
    // same square at every height, so the same volume.
    for twist in [None, Some(std::f64::consts::FRAC_PI_2)] {
        let model = sweep_history(|d| {
            if let Some(twist) = twist {
                d["twist"] = json!(twist);
            }
        });
        let volume = volumes(&cli, &written(&model).0);
        assert_eq!(volume.len(), 1, "{twist:?}");
        // A twisted sweep's faces are fitted splines: within the history check's tolerance.
        assert!(
            (volume[0] - 1000.0).abs() / 1000.0 < dutchman_solidcraft::VOLUME_TOLERANCE,
            "{twist:?}: {volume:?}"
        );
    }
    // Checked against the original cube by the writer itself.
    let (_, report) = convert(
        &sweep_history(|_| {}),
        &SolidCraftWriter::new().verification(Verification::With(cli)),
    );
    assert!(
        history_line(&report).contains("verified against the original in SolidCraft"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn a_loft_between_sketch_profiles_becomes_a_solidcraft_loft() {
    let (document, report) = convert(
        &loft_history(10.0, |_| {}),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = document["features"].as_array().unwrap();
    let id_of = |name: &str| features.iter().find(|f| f["name"] == name).unwrap()["id"].clone();
    let loft = features.iter().find(|f| f["type"] == "loft").unwrap();
    assert_eq!(loft["name"], "Block");
    assert_eq!(loft["operation"], "new_body");
    assert_eq!(loft["body_names"], json!(["unit cube"]));
    let sections = loft["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0]["sketch"], id_of("Base sketch"));
    assert_eq!(sections[1]["sketch"], id_of("Top sketch"));
    assert_eq!(sections[1]["profiles"]["type"], "curves");
    assert!(
        history_line(&report).contains("3 of 3 features rebuilt"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn lofts_solidcraft_would_build_differently_flatten_the_history() {
    for (change, reason) in [
        (
            json!({"ruled": true, "sections": [first_profile(),
                {"kind": "sketch_profiles", "value": {"sketch": TOP, "profiles": [0]}},
                {"kind": "point", "value": {"x": 5.0, "y": 5.0, "z": 20.0}}]}),
            "ruled lofts through more than two sections are not mapped yet",
        ),
        (json!({"closed": true}), "closed lofts are not mapped yet"),
        (
            json!({"max_degree": 3}),
            "lofts with a degree limit are not mapped yet",
        ),
        (
            json!({"guidance": {"kind": "centerline", "value": {"kind": "native", "value": "n"}}}),
            "lofts along a centreline are not mapped yet",
        ),
        (
            json!({"sections": [first_profile(), {"kind": "native_point", "value": "native:apex"}]}),
            "lofts that close to an unresolved point are not mapped yet",
        ),
        (
            json!({"sections": [first_profile(),
                {"kind": "point", "value": {"x": 5.0, "y": 5.0, "z": 10.0}},
                {"kind": "sketch", "value": TOP}]}),
            "lofts with an interior point section are not mapped yet",
        ),
        (
            json!({"sections": vec![first_profile(); 51]}),
            "a loft needs between two and fifty sections",
        ),
        (
            json!({"solid": false, "sections": [first_profile(),
                {"kind": "point", "value": {"x": 5.0, "y": 5.0, "z": 10.0}}]}),
            "surface lofts to a point are not mapped yet",
        ),
    ] {
        let model = loft_history(10.0, |definition| {
            for (key, value) in change.as_object().unwrap() {
                definition[key] = value.clone();
            }
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            document["features"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["type"] != "loft"),
            "{reason}"
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

/// A square pyramid, with a solved position or a B-rep vertex as its apex.
fn pointed_loft(vertex: bool, first: bool) -> Model {
    let model = loft_history(10.0, |definition| {
        let apex = json!({"kind": "point", "value": {"x": 5.0, "y": 5.0, "z": 10.0}});
        definition["sections"] = if first {
            json!([apex, first_profile()])
        } else {
            json!([first_profile(), apex])
        };
    });
    if !vertex {
        return model;
    }
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["geometry"]["points"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": "synthetic:apex:point#0",
            "position": {"x": 5.0, "y": 5.0, "z": 10.0},
        }));
    wire["geometry"]["vertices"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": "synthetic:apex:vertex#0", "point": "synthetic:apex:point#0",
        }));
    let index = if first { 0 } else { 1 };
    wire["model"]["features"][2]["definition"]["value"]["value"]["sections"][index] =
        loft_section(json!({"kind": "vertex", "value": "synthetic:apex:vertex#0"}));
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn resolved_loft_point_ends_become_sketch_points() {
    for vertex in [false, true] {
        for first in [false, true] {
            let (document, report) = convert(
                &pointed_loft(vertex, first),
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            let features = document["features"].as_array().unwrap();
            let loft = features.iter().find(|f| f["type"] == "loft").unwrap();
            let index = if first { 0 } else { 1 };
            let section = &loft["sections"][index];
            assert_eq!(section["point"], "origin");
            assert_eq!(section["profiles"], json!({"type": "all"}));
            let sketch = features
                .iter()
                .find(|f| f["id"] == section["sketch"])
                .unwrap();
            assert_eq!(sketch["plane"]["plane"]["origin"], json!([5.0, 5.0, 10.0]));
            assert_eq!(sketch["sketch"]["points"][0]["id"], "origin");
            assert!(sketch["id"].as_u64().unwrap() < loft["id"].as_u64().unwrap());
            assert!(history_line(&report).contains("3 of 3 features rebuilt"));
        }
    }
}

#[test]
fn generated_loft_apex_names_do_not_shadow_source_features() {
    let model = pointed_loft(false, false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["name"] = json!("Loft apex 3");
    wire["model"]["features"][2]["name"] = json!("Loft apex 3 (2)");
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = document["features"].as_array().unwrap();
    let names: Vec<&str> = features.iter().filter_map(|f| f["name"].as_str()).collect();
    let unique: std::collections::BTreeSet<&str> = names.iter().copied().collect();
    assert_eq!(names.len(), unique.len());
    assert!(names.contains(&"Loft apex 3 (2)"));
}

/// Two separate square regions in one sketch form the feet of an arch; a third
/// square, halfway across and 10 mm above them, forms its middle section.
fn shared_sketch_loft() -> Model {
    let model = loft_history(10.0, |definition| {
        definition["sections"].as_array_mut().unwrap().push(json!({
            "kind": "sketch_regions", "value": {"sketch": SKETCH,
            "regions": [{"kind": "loops", "outer": 1, "holes": []}]},
        }));
    });
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketches"][0]["profiles"]
        .as_array_mut()
        .unwrap()
        .push(json!([
            {"entity": "f3d:model:sketch-entity#41", "reversed": false},
            {"entity": "f3d:model:sketch-entity#42", "reversed": false},
            {"entity": "f3d:model:sketch-entity#43", "reversed": false},
            {"entity": "f3d:model:sketch-entity#44", "reversed": false},
        ]));
    m["sketch_entities"].as_array_mut().unwrap().extend([
        line(41, [20.0, 0.0], [30.0, 0.0]),
        line(42, [30.0, 0.0], [30.0, 10.0]),
        line(43, [30.0, 10.0], [20.0, 10.0]),
        line(44, [20.0, 10.0], [20.0, 0.0]),
    ]);
    m["sketches"][1]["placement"]["frame"] = json!({
        "origin": {"x": 15.0, "y": 0.0, "z": 5.0},
        "normal": {"x": 1.0, "y": 0.0, "z": 0.0},
        "u_axis": {"x": 0.0, "y": 1.0, "z": 0.0},
    });
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn a_loft_section_taking_several_loops_of_its_sketch_is_refused() {
    for section in [
        json!({"kind": "sketch", "value": SKETCH}),
        json!({"kind": "sketch_profiles", "value": {"sketch": SKETCH, "profiles": [0, 1]}}),
    ] {
        let model = shared_sketch_loft();
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let loft = wire["model"]["features"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|f| f["definition"]["value"]["kind"] == "loft")
            .unwrap();
        loft["definition"]["value"]["value"]["sections"][0] = loft_section(section);
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model
        };
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(!document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["type"] == "loft"));
        assert!(
            history_line(&report).contains("takes several loops of its sketch"),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn loft_sections_in_one_sketch_keep_their_selected_regions() {
    let (document, report) = convert(
        &shared_sketch_loft(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let loft = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "loft")
        .unwrap();
    assert_eq!(loft["sections"][0]["sketch"], loft["sections"][2]["sketch"]);
    assert_eq!(loft["sections"][0]["profiles"]["type"], "curves");
    assert_eq!(loft["sections"][2]["profiles"]["type"], "whole_boundaries");
    let first = &loft["sections"][0]["profiles"]["loops"][0];
    let last = &loft["sections"][2]["profiles"]["profiles"][0]["outer"];
    assert_eq!(first.as_array().unwrap().len(), 4);
    assert_eq!(last.as_array().unwrap().len(), 4);
    assert!(first
        .as_array()
        .unwrap()
        .iter()
        .all(|id| !last.as_array().unwrap().contains(id)));
    assert!(history_line(&report).contains("3 of 3 features rebuilt"));
}

#[test]
fn solidcraft_lofts_to_resolved_point_ends() {
    let Some(cli) = solidcraft() else { return };
    for vertex in [false, true] {
        for first in [false, true] {
            let volume = volumes(&cli, &written(&pointed_loft(vertex, first)).0);
            assert_eq!(volume.len(), 1);
            let expected = 1000.0 / 3.0;
            assert!(
                (volume[0] - expected).abs() / expected < dutchman_solidcraft::VOLUME_TOLERANCE,
                "{volume:?}"
            );
        }
    }
}

#[test]
fn solidcraft_lofts_through_regions_of_a_shared_sketch() {
    let Some(cli) = solidcraft() else { return };
    // Known SolidCraft 0.40 failure: this loft builds with no mass there
    // (https://github.com/tangency-labs/solidcraft/issues/15263); it passes on 0.30.
    let version = std::process::Command::new(&cli)
        .arg("--version")
        .output()
        .ok()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .and_then(|line| dutchman_solidcraft::solidcraft_minor(&line));
    if version.as_deref() == Some("0.40") {
        eprintln!("skipped on SolidCraft 0.40: tangency-labs/solidcraft#15263");
        return;
    }
    let volume = volumes(&cli, &written(&shared_sketch_loft()).0);
    assert_eq!(volume.len(), 1);
    assert!(volume[0].is_finite() && volume[0] > 0.0, "{volume:?}");
}

#[test]
fn a_loft_inside_an_assembly_keeps_its_own_sketches() {
    for pointed in [false, true] {
        let mut model = assembly_of_histories();
        let assembly = model.assembly.as_mut().unwrap();
        for part in &mut assembly.parts {
            part.ir = if pointed {
                pointed_loft(false, false).ir
            } else {
                loft_history(10.0, |_| {}).ir
            };
        }
        let (document, _) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let features = document["features"].as_array().unwrap();
        let lofts: Vec<&Value> = features.iter().filter(|f| f["type"] == "loft").collect();
        assert_eq!(lofts.len(), 2);
        for loft in lofts {
            for section in loft["sections"].as_array().unwrap() {
                let sketch = features
                    .iter()
                    .find(|f| f["id"] == section["sketch"])
                    .unwrap();
                assert_eq!(sketch["type"], "sketch");
                assert_eq!(sketch["component"], loft["component"]);
            }
        }
    }
}

const RAIL: &str = "f3d:model:sketch#4";

/// [`loft_history`] guided by a rail: a line up Z along the cube's edge at the origin, drawn in
/// its own sketch on XZ before the loft.
fn railed_loft(top: f64) -> Model {
    let model = loft_history(top, |definition| {
        definition["guidance"] = json!({"kind": "guides", "value": [
            {"kind": "sketch_curves", "value": {"sketch": RAIL, "curves": ["f3d:model:sketch-entity#31"]}}
        ]});
    });
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketches"].as_array_mut().unwrap().push(json!({
        "id": RAIL,
        "name": "Rail",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": -1.0, "z": 0.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [],
    }));
    let mut rail = line(31, [0.0, 0.0], [0.0, 10.0]);
    rail["sketch"] = json!(RAIL);
    m["sketch_entities"].as_array_mut().unwrap().push(rail);
    let features = m["features"].as_array_mut().unwrap();
    features.insert(
        2,
        json!({"id": "f3d:model:feature#4", "ordinal": 2, "name": "Rail sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": RAIL}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
    );
    features[3]["ordinal"] = json!(3);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

/// A whole-sketch rail: the rail line, or with `split` that line in two spans, beside a
/// construction line and a standalone point.
fn whole_sketch_railed_loft(split: bool) -> Model {
    let model = railed_loft(10.0);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["features"][3]["definition"]["value"]["value"]["guidance"]["value"] =
        json!([{"kind": "sketch", "value": RAIL}]);
    let entities = m["sketch_entities"].as_array_mut().unwrap();
    if split {
        let first = entities
            .iter_mut()
            .find(|e| e["id"] == "f3d:model:sketch-entity#31")
            .unwrap();
        first["geometry"]["end"]["v"] = json!(5.0);
        let mut second = line(32, [0.0, 5.0], [0.0, 10.0]);
        second["sketch"] = json!(RAIL);
        entities.push(second);
    }
    let mut construction = line(33, [1.0, 0.0], [1.0, 10.0]);
    construction["sketch"] = json!(RAIL);
    construction["construction"] = json!(true);
    let mut point = entity(
        34,
        json!({"kind": "point", "position": {"u": 2.0, "v": 2.0}}),
    );
    point["sketch"] = json!(RAIL);
    entities.extend([construction, point]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn a_whole_sketch_loft_rail_is_its_one_curve() {
    let (document, report) = convert(
        &whole_sketch_railed_loft(false),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = document["features"].as_array().unwrap();
    let sketch = features
        .iter()
        .find(|f| f["name"] == "Rail sketch")
        .unwrap();
    let loft = features.iter().find(|f| f["type"] == "loft").unwrap();
    let curve = sketch["sketch"]["curves"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["construction"] != json!(true))
        .unwrap();
    assert_eq!(loft["rails"][0]["sketch"], sketch["id"]);
    assert_eq!(loft["rails"][0]["curves"], json!([curve["id"]]));
    assert!(history_line(&report).contains("4 of 4 features rebuilt"));

    let (_, report) = convert(
        &whole_sketch_railed_loft(true),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains(
            "a whole-sketch guide rail with several curves has no resolved traversal order"
        ),
        "{}",
        history_line(&report)
    );
}

#[test]
fn a_loft_rail_of_edges_is_refused() {
    let model = loft_history(10.0, |definition| {
        definition["guidance"] = json!({"kind": "guides", "value": [
            {"kind": "edges", "value": []}
        ]});
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(document["features"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["type"] != "loft"));
    assert!(
        history_line(&report).contains("a guide rail of topological edges is not mapped yet"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn solidcraft_lofts_along_a_whole_sketch_rail() {
    let Some(cli) = solidcraft() else { return };
    let volume = volumes(&cli, &written(&whole_sketch_railed_loft(false)).0);
    assert_eq!(volume.len(), 1);
    assert!(
        (volume[0] - 1000.0).abs() / 1000.0 < dutchman_solidcraft::VOLUME_TOLERANCE,
        "{volume:?}"
    );
}

#[test]
fn a_loft_along_a_guide_rail_keeps_its_rail() {
    let (document, report) = convert(
        &railed_loft(10.0),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = document["features"].as_array().unwrap();
    let rail_sketch = features
        .iter()
        .find(|f| f["name"] == "Rail sketch")
        .unwrap()["id"]
        .clone();
    let loft = features.iter().find(|f| f["type"] == "loft").unwrap();
    let rails = loft["rails"].as_array().unwrap();
    assert_eq!(rails.len(), 1);
    assert_eq!(rails[0]["sketch"], rail_sketch);
    assert_eq!(rails[0]["curves"].as_array().unwrap().len(), 1);
    assert!(
        history_line(&report).contains("4 of 4 features rebuilt"),
        "{}",
        history_line(&report)
    );

    // Three rails are more than SolidCraft takes.
    let three = loft_history(10.0, |definition| {
        let rail = json!({"kind": "sketch_curves", "value": {"sketch": TOP, "curves": ["f3d:model:sketch-entity#11"]}});
        definition["guidance"] =
            json!({"kind": "guides", "value": [rail.clone(), rail.clone(), rail]});
    });
    let (_, report) = convert(
        &three,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("lofts along more than two guide rails are not mapped yet"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn solidcraft_lofts_along_the_rail() {
    let Some(cli) = solidcraft() else { return };
    let volume = volumes(&cli, &written(&railed_loft(10.0)).0);
    assert_eq!(volume.len(), 1);
    assert!(
        (volume[0] - 1000.0).abs() / 1000.0 < dutchman_solidcraft::VOLUME_TOLERANCE,
        "{volume:?}"
    );
    // Checked against the original cube by the writer itself.
    let (_, report) = convert(
        &railed_loft(10.0),
        &SolidCraftWriter::new().verification(Verification::With(cli)),
    );
    assert!(
        history_line(&report).contains("verified against the original in SolidCraft"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn ruled_and_surface_lofts_map_to_what_solidcraft_builds() {
    for (change, kind) in [
        (json!({"ruled": true}), "loft"),
        (json!({"solid": false}), "surface_loft"),
        (json!({"linearize": true}), "loft"),
    ] {
        let model = loft_history(4.0, |definition| {
            for (key, value) in change.as_object().unwrap() {
                definition[key] = value.clone();
            }
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let features = document["features"].as_array().unwrap();
        let loft = features.iter().find(|f| f["name"] == "Block").unwrap();
        assert_eq!(loft["type"], kind, "{change}");
        assert_eq!(loft["sections"].as_array().unwrap().len(), 2);
        assert_eq!(loft.get("operation").is_none(), kind == "surface_loft");
        assert!(
            history_line(&report).contains("3 of 3 features rebuilt"),
            "{}",
            history_line(&report)
        );
    }
    let railed = railed_loft(10.0);
    let mut wire = serde_json::to_value(&railed.ir).unwrap();
    for feature in wire["model"]["features"].as_array_mut().unwrap() {
        if feature["definition"]["value"]["kind"] == "loft" {
            feature["definition"]["value"]["value"]["ruled"] = json!(true);
        }
    }
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..railed
    };
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("ruled lofts along guide rails are not mapped yet"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn solidcraft_replays_ruled_and_surface_lofts() {
    let Some(cli) = solidcraft() else { return };
    // A ruled frustum from the 10 mm base square to a 4 mm top: 520 mm³.
    let ruled = loft_history(4.0, |d| d["ruled"] = json!(true));
    let volume = volumes(&cli, &written(&ruled).0);
    assert_eq!(volume.len(), 1);
    assert!((volume[0] - 520.0).abs() < 1e-6, "{volume:?}");

    // The surface loft is the cube's four walls: one sheet with the cube's bounds.
    let surface = loft_history(10.0, |d| {
        d["solid"] = json!(false);
    });
    let boxes = bounds(&cli, &written(&surface).0);
    assert_eq!(boxes.len(), 1);
    for (actual, expected) in [
        (boxes[0].0, [0.0, 0.0, 0.0]),
        (boxes[0].1, [10.0, 10.0, 10.0]),
    ] {
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-6, "{boxes:?}");
        }
    }
}

#[test]
fn solidcraft_lofts_between_the_sections() {
    let Some(cli) = solidcraft() else { return };
    // The cube again, then a frustum: 10/3 × (100 + 16 + 40) mm³.
    for (top, expected) in [(10.0, 1000.0), (4.0, 520.0)] {
        let volume = volumes(&cli, &written(&loft_history(top, |_| {})).0);
        assert_eq!(volume.len(), 1);
        assert!((volume[0] - expected).abs() < 1e-6, "{volume:?}");
    }
}

fn whole_sketch() -> Value {
    json!({"kind": "sketch", "value": SKETCH})
}

fn first_profile() -> Value {
    json!({"kind": "sketch_profiles", "value": {"sketch": SKETCH, "profiles": [0]}})
}

fn convert(model: &Model, writer: &SolidCraftWriter) -> (Value, Report) {
    let mut report = Report::default();
    let mut bytes = Vec::new();
    writer.write(model, &mut bytes, &mut report).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document = serde_json::from_str(&text).unwrap();
    assert_output_links(&document, &report);
    (document, report)
}

fn history_line(report: &Report) -> String {
    report
        .losses()
        .iter()
        .filter(|loss| loss.subject == "history")
        .map(|loss| loss.detail.clone())
        .collect::<Vec<_>>()
        .join("; ")
}

fn types(document: &Value) -> Vec<(String, bool)> {
    document["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["type"].as_str().unwrap().to_owned(),
                f["suppressed"].as_bool().unwrap(),
            )
        })
        .collect()
}

fn transition_geometry_history(property: Option<&str>) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let features = wire["model"]["features"].as_array_mut().unwrap();
    let mut cut = features[1].clone();
    cut["id"] = json!("f3d:model:feature#3");
    cut["ordinal"] = json!(2);
    cut["name"] = json!("Missed cut");
    cut["definition"]["value"]["value"]["op"] = json!("cut");
    cut["definition"]["value"]["value"]["profile"]["value"] = json!({
        "kind": "sketch_profiles", "value": {"sketch": SKETCH, "profiles": [99]}
    });
    if let Some(property) = property {
        cut["source_properties"] = json!({"transition_geometry": property});
    }
    let mut later = features[1].clone();
    later["id"] = json!("f3d:model:feature#4");
    later["ordinal"] = json!(3);
    later["name"] = json!("Later cut");
    later["definition"]["value"]["value"]["op"] = json!("cut");
    later["definition"]["value"]["value"]["extent"]["side"]["termination"]["length"] = json!(1.0);
    features.extend([cut, later]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn transition_geometry_unchanged_leaves_out_an_unmappable_cut_and_continues() {
    use dutchman_core::FeatureStatus::{LeftOut, Mapped};

    let model = transition_geometry_history(Some("unchanged"));
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let outcome = report.outcome().unwrap();
    assert_eq!(
        outcome
            .features
            .iter()
            .map(|f| f.status)
            .collect::<Vec<_>>(),
        [Mapped, Mapped, LeftOut, Mapped]
    );
    assert_eq!(
        outcome.features[2].reason.as_deref(),
        Some("changes no geometry in the source")
    );
    assert_eq!(
        document["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["type"] == "extrude")
            .count(),
        2
    );
    assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
}

#[test]
fn transition_geometry_absent_still_blocks_an_unmappable_cut() {
    assert_transition_geometry_blocks(None);
}

#[test]
fn transition_geometry_changed_still_blocks_an_unmappable_cut() {
    assert_transition_geometry_blocks(Some("changed"));
}

fn assert_transition_geometry_blocks(property: Option<&str>) {
    use dutchman_core::FeatureStatus::{Blocked, Mapped};

    let model = transition_geometry_history(property);
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let outcome = report.outcome().unwrap();
    assert_eq!(
        outcome
            .features
            .iter()
            .map(|f| f.status)
            .collect::<Vec<_>>(),
        [Mapped, Mapped, Blocked, Blocked]
    );
    assert_eq!(
        outcome.features[2].reason.as_deref(),
        Some("it selects a sketch profile that does not exist")
    );
}

#[test]
fn a_sketch_and_extrude_history_is_written() {
    let model = cube_with_history(10.0, first_profile(), None);
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);

    assert_eq!(
        history_line(&report),
        "2 of 2 features rebuilt; unverified: verification turned off"
    );
    assert_eq!(
        types(&document),
        [
            ("sketch".to_owned(), false),
            ("extrude".to_owned(), false),
            ("import".to_owned(), true)
        ]
    );

    let mut pretty = serde_json::to_string_pretty(&document).unwrap() + "\n";
    // The STEP blob's name depends on cadmpeg's STEP writer, not on the history.
    let blob = document["features"][2]["step_blob"].as_str().unwrap();
    pretty = pretty.replace(blob, "blobs/<sha256>.step");
    let golden =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cube_history.document.json");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &pretty).unwrap();
    }
    assert_eq!(pretty, std::fs::read_to_string(&golden).unwrap());
}

#[test]
fn target_045_keeps_the_previous_compatible_history_document() {
    let model = cube_with_history(10.0, first_profile(), None);
    let write = |target: &str| {
        convert(
            &model,
            &SolidCraftWriter::new()
                .verification(Verification::Off)
                .target_version(Some(target.parse().unwrap())),
        )
        .0
    };
    let previous = write("0.44.1");
    let newest = write("0.45.0");
    assert_eq!(newest["format"], "solidcraft/23");
    assert_eq!(newest, previous);
}

#[test]
fn as_built_joints_between_features_do_not_stop_history() {
    for source_tag in ["As-built", "As-built Joint"] {
        let model = cube_with_history(10.0, first_profile(), None);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["features"][1]["ordinal"] = json!(2);
        wire["model"]["features"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "id": "f3d:model:feature#3", "ordinal": 1,
                // Recognition must use the source kind, not a user-editable display name.
                "name": "Renamed joint", "source_tag": source_tag, "suppressed": false,
                "definition": {"kind": "operation", "value": {"kind": "native", "value": {"kind": source_tag, "parameters": {}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": [],
            }));
        let changed = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model
        };
        let (document, report) = convert(
            &changed,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let features = &report.outcome().unwrap().features;
        assert_eq!(features.len(), 3);
        assert_eq!(features[1].status, dutchman_core::FeatureStatus::NotNeeded);
        assert_eq!(features[2].status, dutchman_core::FeatureStatus::Mapped);
        assert_eq!(
            types(&document),
            [
                ("sketch".to_owned(), false),
                ("extrude".to_owned(), false),
                ("import".to_owned(), true),
            ]
        );
        assert_eq!(
            history_line(&report),
            "2 of 2 features rebuilt; unverified: verification turned off"
        );
    }
}

#[test]
fn a_native_feature_named_as_built_still_stops_history() {
    let model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["ordinal"] = json!(3);
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#3", "ordinal": 1, "name": "As-built Joint 1", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "native", "value": {"kind": "Other operation", "parameters": {}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#4", "ordinal": 2, "name": "Renamed joint", "source_tag": "As-built", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "native", "value": {"kind": "As-built", "parameters": {}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    let changed = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (_, report) = convert(
        &changed,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = &report.outcome().unwrap().features;
    assert_eq!(features[1].status, dutchman_core::FeatureStatus::Blocked);
    assert_eq!(
        features[1].reason.as_deref(),
        Some("native is not mapped yet")
    );
    assert_eq!(features[2].status, dutchman_core::FeatureStatus::NotNeeded);
    assert_eq!(features[3].status, dutchman_core::FeatureStatus::Blocked);
}

#[test]
fn an_unverified_history_can_be_kept_hidden_beside_the_visible_original() {
    let model = cube_with_history(10.0, first_profile(), None);
    let writer = SolidCraftWriter::new()
        .verification(Verification::Off)
        .hide_unverified_history(true);
    let (document, report) = convert(&model, &writer);

    assert!(history_line(&report).contains("unverified: verification turned off; kept hidden"));
    // The original is the visible model; the history is all there, in a hidden part.
    assert_eq!(
        types(&document),
        [
            ("sketch".to_owned(), false),
            ("extrude".to_owned(), false),
            ("import".to_owned(), false)
        ]
    );
    assert_eq!(
        document["components"][0]["name"],
        "Editable history (unverified)"
    );
    let occurrence = document["occurrences"][0]["id"].clone();
    assert_eq!(
        document["visibility"]["hidden_occurrences"],
        json!([[occurrence]])
    );
}

#[test]
fn a_symmetric_through_all_extent_is_mapped() {
    let extent = json!({"kind": "symmetric", "side": {"termination": {"kind": "through_all"}}});
    let model = cube_with_history(10.0, whole_sketch(), Some(extent));
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, _) = convert(&model, &writer);
    let extrude = &document["features"][1];
    assert_eq!(extrude["profiles"], json!({"type": "all"}));
    assert_eq!(
        extrude["extent"],
        json!({"direction": "symmetric", "convention": "total_distance",
               "distance": "0 mm", "through_all": true})
    );
}

/// The final cube with a blocked feature between two otherwise mapped sketch features.
fn reference_sketch_history() -> Model {
    let mut model = loft_history(10.0, |definition| {
        *definition =
            json!({"kind": "native", "value": {"kind": "Reference blocker", "parameters": {}}});
    });
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["ordinal"] = json!(2);
    wire["model"]["features"][2]["ordinal"] = json!(1);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn assert_reference_sketches(document: &Value, count: usize) {
    let mut expected = vec![("import".to_owned(), false)];
    expected.extend((0..count).map(|_| ("sketch".to_owned(), false)));
    assert_eq!(types(document), expected);
    let features = document["features"].as_array().unwrap();
    let ids: Vec<_> = features.iter().skip(1).map(|f| f["id"].clone()).collect();
    assert_eq!(document["visibility"]["hidden_sketches"], json!(ids));
    for feature in features.iter().skip(1) {
        assert_eq!(feature["component"], document["features"][0]["component"]);
        assert!(feature["id"].as_u64().unwrap() > 3);
        assert!(feature["id"].as_u64().unwrap() < document["next_id"].as_u64().unwrap());
    }
    let unique: std::collections::BTreeSet<_> =
        features.iter().map(|f| f["id"].as_u64().unwrap()).collect();
    assert_eq!(unique.len(), features.len());
}

#[test]
fn flattened_history_keeps_mapped_sketches_hidden_in_source_order() {
    let model = reference_sketch_history();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_reference_sketches(&document, 2);
    assert_eq!(document["features"][1]["name"], "Base sketch");
    assert_eq!(document["features"][2]["name"], "Top sketch");
    assert_eq!(document["features"][2]["plane"]["type"], "custom");
    // The first sketch's dimension still has its parameter.
    assert!(document["params"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "side"));
    let losses: Vec<_> = report
        .losses()
        .iter()
        .filter(|loss| loss.subject == "sketches")
        .collect();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, Severity::Info);
    assert_eq!(losses[0].detail, "kept 2 sketches for reference");
    assert!(matches!(
        report.outcome().unwrap().check,
        Check::Flattened(_)
    ));
}

#[test]
fn flattened_history_leaves_out_a_sketch_with_an_unresolved_plane() {
    let mut model = reference_sketch_history();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketches"][0]["placement"] = json!({"kind": "unresolved"});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_reference_sketches(&document, 1);
    assert_eq!(document["features"][1]["name"], "Top sketch");
    let losses: Vec<_> = report
        .losses()
        .iter()
        .filter(|loss| loss.subject == "sketches")
        .collect();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].severity, Severity::Info);
    assert_eq!(losses[0].detail, "kept 1 sketch for reference; 1 left out");
}

#[test]
fn flattened_history_leaves_out_a_sketch_that_does_not_map() {
    let mut model = reference_sketch_history();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketch_entities"][0]["geometry"] =
        json!({"kind": "native", "native_kind": "Reference curve"});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_reference_sketches(&document, 1);
    assert_eq!(document["features"][1]["name"], "Top sketch");
    assert!(report.losses().iter().any(|loss| {
        loss.subject == "sketches"
            && loss.severity == Severity::Info
            && loss.detail == "kept 1 sketch for reference; 1 left out"
    }));
}

#[test]
fn reference_sketches_preserve_duplicate_source_names() {
    let mut model = reference_sketch_history();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["name"] = json!("Base sketch");
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_reference_sketches(&document, 2);
    assert_eq!(document["features"][1]["name"], "Base sketch");
    assert_eq!(document["features"][2]["name"], "Base sketch");
}

#[test]
fn hidden_reference_sketches_leave_the_imported_body_unchanged_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let model = reference_sketch_history();
    let (bytes, _) = written(&model);
    let mut original = Vec::new();
    SolidCraftWriter::new()
        .import_only(true)
        .verification(Verification::Off)
        .write(&model, &mut original, &mut Report::default())
        .unwrap();
    let actual = volumes(&cli, &bytes);
    assert_eq!(actual, volumes(&cli, &original));
    assert_eq!(actual.len(), 1);
    assert!((actual[0] - 1000.0).abs() < 1e-6);
}

#[test]
fn import_only_never_writes_history() {
    let model = cube_with_history(10.0, first_profile(), None);
    let (document, report) = convert(&model, &SolidCraftWriter::new().import_only(true));
    assert_eq!(history_line(&report), "not written (--import-only)");
    assert_eq!(
        types(&document),
        [("import".to_owned(), false), ("sketch".to_owned(), false)]
    );
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().import_only(true).sketches(false),
    );
    assert_eq!(types(&document), [("import".to_owned(), false)]);
}

/// The SolidCraft CLI the replay tests use, if one is configured.
fn solidcraft() -> Option<PathBuf> {
    let cli = std::env::var_os(CLI_ENV).map(PathBuf::from);
    if cli.is_none() {
        eprintln!("skipped: set {CLI_ENV} to replay histories in SolidCraft");
    }
    cli
}

#[test]
fn a_faithful_history_is_verified_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let model = cube_with_history(10.0, first_profile(), None);
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::With(cli)),
    );
    assert_eq!(
        history_line(&report),
        "2 of 2 features rebuilt; verified against the original in SolidCraft"
    );
    assert_eq!(types(&document).len(), 3);
}

#[test]
fn a_wrong_sized_replay_uses_only_the_imported_final_body() {
    let Some(cli) = solidcraft() else { return };
    let model = cube_with_history(4.0, first_profile(), None);
    let writer = SolidCraftWriter::new().verification(Verification::With(cli.clone()));
    let (document, report) = convert(&model, &writer);
    assert_reference_sketches(&document, 1);
    assert_eq!(report.outcome().unwrap().mode, Mode::Import);
    assert!(history_line(&report).contains("replay differs"));
    assert!(document["features"]
        .as_array()
        .unwrap()
        .iter()
        .all(|feature| { matches!(feature["type"].as_str(), Some("sketch" | "import")) }));
    let (bytes, _) = written_with(&model, &writer);
    assert_eq!(volumes(&cli, &bytes), [1000.0]);
}

#[test]
fn a_displaced_replay_uses_only_the_imported_final_body() {
    let Some(cli) = solidcraft() else { return };
    for offset in [-4.0, 7.0] {
        let mut model = cube_with_history(10.0, first_profile(), None);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["sketches"][0]["placement"]["frame"]["origin"]["z"] = json!(offset);
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let writer = SolidCraftWriter::new().verification(Verification::With(cli.clone()));
        let (document, report) = convert(&model, &writer);
        assert_reference_sketches(&document, 1);
        assert_eq!(report.outcome().unwrap().mode, Mode::Import);
        assert!(history_line(&report).contains("position"), "{report:?}");
        assert!(!document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| { feature["type"] == "extrude" }));
        let (bytes, _) = written_with(&model, &writer);
        let measured = evaluated(&cli, &bytes);
        let bodies = measured["bodies"].as_array().unwrap();
        assert_eq!(bodies.len(), 1, "{measured}");
        assert_eq!(bodies[0]["bbox"]["min"], json!([0.0, 0.0, 0.0]));
        assert_eq!(bodies[0]["bbox"]["max"], json!([10.0, 10.0, 10.0]));
        assert!((bodies[0]["volume_mm3"].as_f64().unwrap() - 1000.0).abs() < 1e-6);
    }
}

#[derive(Debug)]
struct MeasuredReplayDifference(&'static str);

impl dutchman_solidcraft::SolidCraftEvaluator for MeasuredReplayDifference {
    fn evaluate(
        &self,
        design: &[u8],
        timeout: std::time::Duration,
        label: &str,
    ) -> Result<Value, String> {
        let mut result = dutchman_solidcraft::SolidCraftEvaluator::evaluate(
            &RecordedEvaluator::default(),
            design,
            timeout,
            label,
        )?;
        if label == "history" {
            match self.0 {
                "position" => {
                    result["bodies"][0]["bbox"] = json!({"min": [0, 0, 4], "max": [10, 10, 14]});
                }
                "box" => {
                    result["bodies"][0]["bbox"] = json!({"min": [0, 0, -2], "max": [10, 10, 12]});
                }
                "volume" => result["bodies"][0]["volume_mm3"] = json!(900.0),
                "area" => result["bodies"][0]["area_mm2"] = json!(700.0),
                _ => return Err("unknown synthetic measurement".to_owned()),
            }
        }
        Ok(result)
    }

    fn run(
        &self,
        design: &[u8],
        commands: &[Value],
        timeout: std::time::Duration,
        label: &str,
    ) -> Result<Vec<Value>, String> {
        dutchman_solidcraft::SolidCraftEvaluator::run(
            &RecordedEvaluator::default(),
            design,
            commands,
            timeout,
            label,
        )
    }
}

#[test]
fn every_measured_replay_difference_removes_the_edited_body_and_reports_the_amount() {
    for (check, amount) in [
        ("position", "4.000 mm"),
        ("box", "2.000 mm"),
        ("volume", "difference 100.000000 mm³"),
        ("area", "difference 100.000000 mm²"),
    ] {
        let identity = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        for model in [
            cube_with_history(10.0, first_profile(), None),
            primitive_history(block([10.0; 3], identity)),
        ] {
            let writer = SolidCraftWriter::new()
                .evaluator(std::sync::Arc::new(MeasuredReplayDifference(check)));
            let (document, report) = convert(&model, &writer);
            assert_reference_sketches(&document, 1);
            let outcome = report.outcome().unwrap();
            assert_eq!(outcome.mode, Mode::Import, "{check}: {report:?}");
            let Check::Flattened(reason) = &outcome.check else {
                panic!("{report:?}")
            };
            assert!(
                reason.contains(check) && reason.contains(amount),
                "{check}: {reason}"
            );
            assert_eq!(
                outcome
                    .features
                    .iter()
                    .find(|feature| feature.kind != "sketch")
                    .unwrap()
                    .status,
                dutchman_core::FeatureStatus::Blocked
            );
            assert!(document["features"]
                .as_array()
                .unwrap()
                .iter()
                .all(|feature| { matches!(feature["type"].as_str(), Some("sketch" | "import")) }));
        }
    }
}

#[test]
fn a_measured_difference_keeps_the_source_base_before_the_failed_edit() {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    let features = wire["model"]["features"].as_array_mut().unwrap();
    for feature in features.iter_mut() {
        feature["ordinal"] = json!(feature["ordinal"].as_u64().unwrap() + 1);
    }
    features[1]["definition"]["value"]["value"]["op"] = json!("join");
    features.insert(
        0,
        json!({
            "id": "f3d:model:feature#0", "ordinal": 0, "name": "Stored base",
            "suppressed": false, "dependencies": [], "outputs": [body],
            "source_properties": {}, "source_content": [],
            "definition": {"kind": "operation", "value": {
                "kind": "base_feature", "value": {"bodies": {"kind": "resolved", "value": {
                    "bodies": [body], "native": "synthetic:base#0"
                }}}
            }}
        }),
    );
    let nests = serde_json::to_value(&based_cube_changed_later(true).ir).unwrap();
    wire["model"]["history_bodies"] = nests["model"]["history_bodies"].clone();
    wire["model"]["history_bodies"][0]["produced_by"] = json!("f3d:model:feature#0");
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let writer = SolidCraftWriter::new()
        .evaluator(std::sync::Arc::new(MeasuredReplayDifference("position")));
    let (document, report) = convert(&model, &writer);
    assert_partial(&document, &report, 1);
    let features = document["features"].as_array().unwrap();
    assert!(features
        .iter()
        .any(|feature| feature["name"] == "Stored base"));
    assert!(!features.iter().any(|feature| feature["type"] == "extrude"));
    assert!(history_line(&report).contains("4.000 mm"), "{report:?}");
    let outcomes = &report.outcome().unwrap().features;
    assert_eq!(outcomes[0].status, dutchman_core::FeatureStatus::Mapped);
    assert_eq!(outcomes[2].status, dutchman_core::FeatureStatus::Blocked);
}

#[test]
fn a_retained_prefix_with_a_measured_source_state_difference_uses_the_final_import() {
    let mut model = partial_history_model();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|feature| feature["id"] == "f3d:model:feature#4")
        .unwrap()["source_properties"] = json!({
        "body_centers": "1:5,5,12", "body_centers_exact": "1"
    });
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let writer =
        SolidCraftWriter::new().evaluator(std::sync::Arc::new(RecordedEvaluator::default()));
    let (document, report) = convert(&model, &writer);
    assert_eq!(report.outcome().unwrap().mode, Mode::Import, "{report:?}");
    assert!(document["features"]
        .as_array()
        .unwrap()
        .iter()
        .all(|feature| { matches!(feature["type"].as_str(), Some("sketch" | "import")) }));
    assert!(
        report.losses().iter().any(|loss| {
            loss.code.as_deref() == Some("history/body-mismatch") && loss.detail.contains("7.0 mm")
        }),
        "{report:?}"
    );
    assert_eq!(
        report.outcome().unwrap().features[1].status,
        dutchman_core::FeatureStatus::Blocked
    );
}

/// The design's container with parameter `name`'s expression changed, every other entry kept.
fn with_param(bytes: &[u8], name: &str, expr: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let path = entry.name().unwrap().into_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if path == "document.json" {
            let mut document: Value = serde_json::from_slice(&data).unwrap();
            for param in document["params"].as_array_mut().unwrap() {
                if param["name"] == name {
                    param["expr"] = json!(expr);
                }
            }
            data = serde_json::to_vec(&document).unwrap();
        }
        out.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut out, &data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// SolidCraft's evaluation of a design, requiring every timeline feature to succeed.
fn evaluated(cli: &Path, design: &[u8]) -> Value {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("design.solidcraft");
    let output = dir.path().join("eval.json");
    std::fs::write(&input, design).unwrap();
    let status = std::process::Command::new(cli)
        .arg("eval")
        .arg(&input)
        .arg("--json-out")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let result: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    for entry in result["timeline"].as_array().unwrap() {
        assert!(entry["error"].is_null(), "{entry}");
    }
    result
}

/// SolidCraft's body volumes for a design.
fn volumes(cli: &Path, design: &[u8]) -> Vec<f64> {
    evaluated(cli, design)["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|body| {
            let volume = body["volume_mm3"].as_f64();
            assert!(
                volume.is_some(),
                "SolidCraft returned no numeric body volume: {body}"
            );
            volume.unwrap()
        })
        .collect()
}

#[test]
fn a_converted_dimension_drives_the_geometry_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let model = cube_with_history(10.0, first_profile(), None);
    let mut report = Report::default();
    let mut bytes = Vec::new();
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .write(&model, &mut bytes, &mut report)
        .unwrap();

    assert_eq!(volumes(&cli, &bytes), [1000.0]);
    assert_eq!(
        volumes(&cli, &with_param(&bytes, "side", "20 mm")),
        [4000.0]
    );
}

/// The example cube's geometry with a revolve history: a 5 mm × 10 mm rectangle on XZ, 5 mm
/// to 10 mm out from Z, turned about the Z axis by `extent`.
fn ring_history(extent: Value) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketches"][0]["placement"]["frame"]["normal"] = json!({"x": 0.0, "y": 1.0, "z": 0.0});
    // On XZ, sketch v runs along -Z.
    m["sketch_entities"] = json!([
        line(1, [5.0, 0.0], [10.0, 0.0]),
        line(2, [10.0, 0.0], [10.0, -10.0]),
        line(3, [10.0, -10.0], [5.0, -10.0]),
        line(4, [5.0, -10.0], [5.0, 0.0]),
    ]);
    m["sketch_constraints"] = json!([]);
    m["features"][1] = json!({"id": "f3d:model:feature#2", "ordinal": 1, "name": "Ring", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "revolve", "value": {"construction": {"kind": "resolved", "value": {"profile": whole_sketch(), "axis": {
                    "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
                }, "extent": extent}}, "op": "new_body"}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn changed_ring(change: impl FnOnce(&mut Value)) -> Model {
    let mut model = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": std::f64::consts::TAU}}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    change(&mut wire["model"]["features"][1]["definition"]["value"]["value"]["construction"]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn written(model: &Model) -> (Vec<u8>, Report) {
    written_with(
        model,
        &SolidCraftWriter::new()
            .verification(Verification::Off)
            .target_version(Some(dutchman_solidcraft::SolidCraftVersion::detected(
                &Verification::Auto,
            ))),
    )
}

fn helical_history(change: impl FnOnce(&mut Value)) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketch_constraints"] = json!([]);
    m["sketch_entities"] = json!([entity(
        1,
        json!({
            "kind": "circle", "center": {"u": 10.0, "v": 0.0}, "radius": 1.0
        })
    )]);
    m["sketches"][0]["profiles"] =
        json!([[{"entity": "f3d:model:sketch-entity#1", "reversed": false}]]);
    m["sketches"][0]["placement"]["frame"] = json!({
        "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "normal": {"x": 0.0, "y": -0.9968486721503291, "z": -0.0793266968436579},
        "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
    });
    m["parameters"] = json!([
        {"id": "f3d:model:parameter#1", "owner": "f3d:model:feature#2", "name": "Pitch", "expression": "5 mm", "value": {"kind": "length", "value": 5.0}, "ordinal": 0, "dependencies": [], "properties": {}},
        {"id": "f3d:model:parameter#2", "owner": "f3d:model:feature#2", "name": "Height", "expression": "20 mm", "value": {"kind": "length", "value": 20.0}, "ordinal": 0, "dependencies": [], "properties": {}},
        {"id": "f3d:model:parameter#3", "owner": "f3d:model:feature#2", "name": "Turns", "expression": "4", "value": {"kind": "real", "value": 4.0}, "ordinal": 0, "dependencies": [], "properties": {}}
    ]);
    m["features"][1]["definition"] = json!({"kind": "post_process", "value": {"refine": true, "fuzzy_tolerance": {"kind": "kernel_default"}, "operation": {"kind": "helical_sweep", "value": {"op": "join", "construction": {
        "profile": {"kind": "sketch", "value": SKETCH},
        "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis_direction": {"x": 1e-16, "y": -1e-16, "z": 1.0},
        "law": "pitch_height_angle", "pitch": 5.0,
        "travel": {"kind": "axial", "height": 20.0}, "turns": 4.0,
        "cone_angle": 0.0, "left_handed": false, "reversed": false,
        "tolerance": 0.0001, "allow_multi_profile_faces": false
    }}}}});
    change(&mut m["features"][1]["definition"]["value"]["operation"]["value"]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn helix_cli() -> Option<PathBuf> {
    let cli = solidcraft()?;
    let version =
        dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()));
    if version < dutchman_solidcraft::SolidCraftVersion::V0_25_1 {
        eprintln!("skipped: sketch-profile coils need SolidCraft 0.25 patch 1 or newer");
        return None;
    }
    Some(cli)
}

#[test]
fn helical_sweeps_block_history_for_the_compatibility_floor() {
    let (document, report) = convert(
        &helical_history(|_| {}),
        &SolidCraftWriter::new()
            .verification(Verification::Off)
            .target_version(Some("0.18.2".parse().unwrap())),
    );
    assert_eq!(
        types(&document).first(),
        Some(&("import".to_owned(), false))
    );
    let reason = report
        .outcome()
        .unwrap()
        .features
        .iter()
        .find(|feature| feature.kind == "helical_sweep")
        .unwrap()
        .reason
        .as_deref()
        .unwrap();
    assert_eq!(
        reason,
        "needs SolidCraft 0.25 patch 1 or newer (target 0.18)"
    );
    assert!(history_line(&report).contains(reason));
}

#[test]
fn helical_sweeps_keep_the_profile_axis_law_hand_and_operation() {
    for (law, pitch, turns) in [
        ("pitch_height_angle", "Pitch", "(Height) / (Pitch)"),
        ("pitch_turns_angle", "Pitch", "Turns"),
        ("height_turns_angle", "(Height) / (Turns)", "Turns"),
        ("height_turns_growth", "(Height) / (Turns)", "Turns"),
    ] {
        for op in ["join", "cut", "intersect"] {
            for reversed in [false, true] {
                for left in [false, true] {
                    let mut model = helical_history(|d| {
                        d["op"] = json!(op);
                        d["construction"]["law"] = json!(law);
                        d["construction"]["reversed"] = json!(reversed);
                        d["construction"]["left_handed"] = json!(left);
                    });
                    // A cut or intersection works on a body an earlier feature built.
                    let first = usize::from(op != "join");
                    if first == 1 {
                        let base = serde_json::to_value(
                            &cube_with_history(10.0, first_profile(), None).ir,
                        )
                        .unwrap();
                        let mut wire = serde_json::to_value(&model.ir).unwrap();
                        let features = wire["model"]["features"].as_array_mut().unwrap();
                        features[1]["ordinal"] = json!(2);
                        let mut block = base["model"]["features"][1].clone();
                        block["id"] = json!("f3d:model:feature#block");
                        features.insert(1, block);
                        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
                    }
                    let (document, report) = convert(
                        &model,
                        &SolidCraftWriter::new()
                            .verification(Verification::Off)
                            .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_25_1)),
                    );
                    let coil = &document["features"][1 + first];
                    assert!(history_line(&report).contains("target SolidCraft 0.25"));
                    assert_eq!(coil["type"], "coil");
                    assert_eq!(coil["sketch"], document["features"][0]["id"]);
                    assert_eq!(coil["profiles"], json!({"type": "all"}));
                    assert_eq!(
                        coil["axis"],
                        json!([0.0, 0.0, if reversed { -1.0 } else { 1.0 }])
                    );
                    assert_eq!(coil["pitch"], pitch);
                    assert_eq!(coil["turns"], turns);
                    assert_eq!(coil["clockwise"], left);
                    assert_eq!(coil["operation"], op);
                }
            }
        }
    }
}

#[test]
fn conical_helical_sweeps_name_the_missing_solidcraft_option() {
    let model = helical_history(|d| {
        d["construction"]["cone_angle"] = json!(0.1);
        d["construction"]["travel"] =
            json!({"kind": "conical", "height": 20.0, "radial_growth": 0.5});
    });
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new()
            .verification(Verification::Off)
            .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_25_1)),
    );
    assert!(history_line(&report).contains("no radial growth or cone angle"));
}

#[test]
fn helical_turn_parameters_drive_the_replayed_coil() {
    let Some(cli) = helix_cli() else { return };
    let model = helical_history(|d| d["construction"]["law"] = json!("pitch_turns_angle"));
    let bytes = written(&model).0;
    let full = volumes(&cli, &bytes);
    let half = volumes(&cli, &with_param(&bytes, "Turns", "2"));
    assert_eq!(full.len(), 1);
    assert_eq!(half.len(), 1);
    assert!(
        (full[0] / half[0] - 2.0).abs() < 1e-3,
        "{full:?} / {half:?}"
    );
}

#[test]
fn helical_handedness_and_reversal_replay_on_the_correct_side_of_the_axis() {
    let Some(cli) = helix_cli() else { return };
    for reversed in [false, true] {
        for left in [false, true] {
            let model = helical_history(|d| {
                d["construction"]["law"] = json!("pitch_turns_angle");
                d["construction"]["left_handed"] = json!(left);
                d["construction"]["reversed"] = json!(reversed);
            });
            let bytes = with_param(&written(&model).0, "Turns", "0.25");
            let measurement = evaluated(&cli, &bytes);
            let center = &measurement["bodies"][0]["center_of_mass"];
            let y = center[1].as_f64().unwrap();
            let z = center[2].as_f64().unwrap();
            assert_eq!(y.is_sign_negative(), left != reversed, "{measurement}");
            assert_eq!(z.is_sign_negative(), reversed, "{measurement}");
        }
    }
}

#[test]
fn freecad_f13a_keeps_its_helix_and_replays_at_the_recipe_volume() {
    let input =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/F13a-freecad-1.1.4.FCStd");
    let model = dutchman_core::read(
        dutchman_core::Format::FreeCadDocument,
        &input,
        &mut Report::default(),
    )
    .unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new()
            .verification(Verification::Off)
            .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_25_1)),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let coil = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "coil")
        .unwrap();
    assert_eq!(coil["axis"], json!([0.0, 0.0, 1.0]));
    let Some(cli) = helix_cli() else { return };
    let (bytes, report) = written_with(
        &model,
        &SolidCraftWriter::new().verification(Verification::With(cli.clone())),
    );
    assert_eq!(report.outcome().unwrap().mode, Mode::History);
    assert_eq!(report.outcome().unwrap().check, Check::Verified);
    let volume = volumes(&cli, &bytes);
    assert_eq!(volume.len(), 1);
    // The existing replay bound is area times mesh chord (5e-5 of the body's size).
    let allowed = 1590.4 * 5e-5 * 22.0;
    assert!((volume[0] - 792.06459406).abs() <= allowed, "{volume:?}");
}

fn written_with(model: &Model, writer: &SolidCraftWriter) -> (Vec<u8>, Report) {
    let mut report = Report::default();
    let mut bytes = Vec::new();
    writer.write(model, &mut bytes, &mut report).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let document: Value =
        serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
    assert_output_links(&document, &report);
    (bytes, report)
}

#[test]
fn full_and_symmetric_revolves_are_written() {
    let full = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": std::f64::consts::TAU}}));
    let (document, report) = convert(
        &full,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    let revolve = &document["features"][1];
    assert_eq!(revolve["type"], "revolve");
    assert_eq!(revolve["angle"], "360 deg");
    assert_eq!(
        revolve["axis"],
        json!({"type": "line", "origin": [0.0, 0.0, 0.0], "dir": [0.0, 0.0, 1.0]})
    );
    assert_eq!(revolve["convention"], "sketch_normal");
}

#[test]
fn one_sided_revolves_follow_the_oriented_axis() {
    let cli = solidcraft();
    for axis in [1.0, -1.0] {
        for normal in [1.0, -1.0] {
            for radial in [1.0, -1.0] {
                for placed in [false, true] {
                    let mut model = ring_history(json!({"kind": "one_sided",
                    "termination": {"kind": "angle", "angle": std::f64::consts::PI / 3.0}}));
                    let mut wire = serde_json::to_value(&model.ir).unwrap();
                    wire["model"]["sketches"][0]["placement"]["frame"]["normal"]["y"] =
                        json!(normal);
                    wire["model"]["features"][1]["definition"]["value"]["value"]["construction"]
                        ["value"]["axis"]["direction"]["z"] = json!(axis);
                    for entity in wire["model"]["sketch_entities"].as_array_mut().unwrap() {
                        for end in ["start", "end"] {
                            let point = &mut entity["geometry"][end];
                            point["u"] = json!(point["u"].as_f64().unwrap() * radial);
                            point["v"] = json!(point["v"].as_f64().unwrap() * normal);
                        }
                    }
                    if placed {
                        // Cycle world axes and translate: sketch u is +Y, normal is +/-Z,
                        // and the revolution axis is +/-X through the moved sketch origin.
                        wire["model"]["sketches"][0]["placement"]["frame"] = json!({
                            "origin": {"x": 11.0, "y": 22.0, "z": 33.0},
                            "normal": {"x": 0.0, "y": 0.0, "z": normal},
                            "u_axis": {"x": 0.0, "y": 1.0, "z": 0.0}
                        });
                        wire["model"]["features"][1]["definition"]["value"]["value"]
                            ["construction"]["value"]["axis"] = json!({
                            "origin": {"x": 11.0, "y": 22.0, "z": 33.0},
                            "direction": {"x": axis, "y": 0.0, "z": 0.0}
                        });
                    }
                    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
                    let (design, report) = written(&model);
                    assert!(
                        history_line(&report).starts_with("2 of 2 features rebuilt"),
                        "{report:?}"
                    );
                    let (document, _) = convert(
                        &model,
                        &SolidCraftWriter::new().verification(Verification::Off),
                    );
                    let revolve = &document["features"][1];
                    let degrees = revolve["angle"]
                        .as_str()
                        .unwrap()
                        .trim_end_matches(" deg")
                        .parse::<f64>()
                        .unwrap();
                    assert!((degrees - 60.0 * axis * radial * normal).abs() < 1e-10);
                    assert_eq!(revolve["convention"], "sketch_normal");
                    if let Some(cli) = &cli {
                        let side = axis * radial;
                        let expected_min = [
                            if radial > 0.0 { 2.5 } else { -10.0 },
                            if side > 0.0 { 0.0 } else { -75.0_f64.sqrt() },
                            0.0,
                        ];
                        let expected_max = [
                            if radial > 0.0 { 10.0 } else { -2.5 },
                            if side > 0.0 { 75.0_f64.sqrt() } else { 0.0 },
                            10.0,
                        ];
                        let place = |corner: [f64; 3]| {
                            if placed {
                                [corner[2] + 11.0, corner[0] + 22.0, corner[1] + 33.0]
                            } else {
                                corner
                            }
                        };
                        let expected_min = place(expected_min);
                        let expected_max = place(expected_max);
                        let boxes = bounds(cli, &design);
                        assert_eq!(boxes.len(), 1);
                        for coordinate in 0..3 {
                            assert!(
                                (boxes[0].0[coordinate] - expected_min[coordinate]).abs() < 1e-6,
                                "{boxes:?}"
                            );
                            assert!(
                                (boxes[0].1[coordinate] - expected_max[coordinate]).abs() < 1e-6,
                                "{boxes:?}"
                            );
                        }
                        let actual = volumes(cli, &design);
                        let expected = 125.0 * std::f64::consts::PI;
                        assert_eq!(actual.len(), 1);
                        assert!((actual[0] - expected).abs() / expected < 1e-3, "{actual:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn one_sided_revolves_keep_selected_curves_and_a_sketch_axis() {
    for axis in [1.0, -1.0] {
        let mut model = ring_history(json!({"kind": "one_sided",
            "termination": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2}}));
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        // Move the axis to the inner profile edge. Its sketch line runs opposite +Z.
        let construction = &mut wire["model"]["features"][1]["definition"]["value"]["value"]
            ["construction"]["value"];
        construction["axis"]["origin"]["x"] = json!(5.0);
        construction["axis"]["direction"]["z"] = json!(axis);
        construction["axis"]["reference"] = json!({"kind": "sketch_curves", "value": {
            "sketch": SKETCH, "curves": ["f3d:model:sketch-entity#4"]}});
        construction["profile"] = json!({"kind": "sketch_entities", "value": {
            "sketch": SKETCH, "entities": (1..=4).map(|id| format!("f3d:model:sketch-entity#{id}")).collect::<Vec<_>>()}});
        // An unselected curve on the other side must not change the proof.
        wire["model"]["sketch_entities"]
            .as_array_mut()
            .unwrap()
            .push(line(5, [-10.0, 0.0], [-10.0, -10.0]));
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("2 of 2 features rebuilt"),
            "{report:?}"
        );
        assert_eq!(document["features"][1]["axis"]["type"], "sketch_line");
        if let Some(cli) = solidcraft() {
            let design = written(&model).0;
            let boxes = bounds(&cli, &design);
            assert_eq!(boxes.len(), 1);
            let expected = if axis > 0.0 { (0.0, 5.0) } else { (-5.0, 0.0) };
            assert!((boxes[0].0[0] - 5.0).abs() < 1e-6, "{boxes:?}");
            assert!((boxes[0].1[0] - 10.0).abs() < 1e-6, "{boxes:?}");
            assert!((boxes[0].0[1] - expected.0).abs() < 1e-6, "{boxes:?}");
            assert!((boxes[0].1[1] - expected.1).abs() < 1e-6, "{boxes:?}");
            let actual = volumes(&cli, &design);
            assert_eq!(actual.len(), 1);
            let expected = 62.5 * std::f64::consts::PI;
            assert!((actual[0] - expected).abs() / expected < 1e-3, "{actual:?}");
        }
    }
}

#[test]
fn one_sided_revolves_bound_circular_profiles() {
    for radial in [7.0, -7.0, 0.0] {
        let mut model = ring_history(json!({"kind": "one_sided",
            "termination": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2}}));
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["sketch_entities"] = json!([entity(
            1,
            json!({"kind": "circle",
            "center": {"u": radial, "v": -5.0}, "radius": 2.0})
        )]);
        wire["model"]["features"][1]["definition"]["value"]["value"]["construction"]["value"]
            ["profile"] = whole_sketch();
        wire["model"]["sketches"][0]["profiles"] =
            json!([[{"entity": "f3d:model:sketch-entity#1", "reversed": false}]]);
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (design, report) = written(&model);
        if radial == 0.0 {
            assert!(history_line(&report).contains("sweep side is not proven"));
        } else {
            assert!(
                history_line(&report).starts_with("2 of 2 features rebuilt"),
                "{report:?}"
            );
            if let Some(cli) = solidcraft() {
                let boxes = bounds(&cli, &design);
                assert_eq!(boxes.len(), 1);
                let expected = if radial > 0.0 {
                    (0.0, 9.0)
                } else {
                    (-9.0, 0.0)
                };
                // SolidCraft measures a circular profile's extrema from its mesh.
                assert!((boxes[0].0[1] - expected.0).abs() < 5e-4, "{boxes:?}");
                assert!((boxes[0].1[1] - expected.1).abs() < 5e-4, "{boxes:?}");
                let actual = volumes(&cli, &design);
                assert_eq!(actual.len(), 1);
                let expected = 14.0 * std::f64::consts::PI.powi(2);
                assert!((actual[0] - expected).abs() / expected < 1e-3, "{actual:?}");
            }
        }
    }
}

#[test]
fn a_one_sided_revolve_with_profiles_on_both_sides_names_the_missing_proof() {
    let mut model = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2}}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for entity in wire["model"]["sketch_entities"].as_array_mut().unwrap() {
        for end in ["start", "end"] {
            let point = &mut entity["geometry"][end];
            point["u"] = json!(point["u"].as_f64().unwrap() - 7.5);
        }
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = written(&model);
    assert!(history_line(&report)
        .contains("selected profile curves must lie on one side of the oriented axis"));
}

#[test]
fn revolves_rebuild_their_volume_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let ring = std::f64::consts::PI * (10.0_f64.powi(2) - 5.0_f64.powi(2)) * 10.0;
    let close = |got: &[f64], want: f64| {
        assert_eq!(got.len(), 1, "{got:?}");
        assert!((got[0] - want).abs() / want < 1e-3, "{got:?} vs {want}");
    };
    let full = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": std::f64::consts::TAU}}));
    close(&volumes(&cli, &written(&full).0), ring);
    let quarter = ring_history(json!({"kind": "symmetric",
        "termination": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2}}));
    close(&volumes(&cli, &written(&quarter).0), ring / 4.0);
}

#[test]
fn unequal_two_sided_full_turns_map_without_an_angular_start() {
    for (first, second) in [(90.0_f64, 270.0_f64), (270.0, 90.0)] {
        let model = ring_history(json!({"kind": "two_sided",
            "first": {"kind": "angle", "angle": first.to_radians()},
            "second": {"kind": "angle", "angle": second.to_radians()}}));
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
        let revolve = &document["features"][1];
        assert_eq!(revolve["angle"], "360 deg");
        assert_eq!(revolve["convention"], "sketch_normal");
        assert!(revolve.get("symmetric").is_none());
        assert!(revolve.get("angle2").is_none());
        if let Some(cli) = solidcraft() {
            let actual = volumes(&cli, &written(&model).0);
            assert_eq!(actual.len(), 1);
            let expected = 750.0 * std::f64::consts::PI;
            assert!((actual[0] - expected).abs() / expected < 1e-3, "{actual:?}");
        }
    }
}

#[test]
fn unequal_partial_and_overlapping_revolve_sides_are_refused() {
    for (first, second, reason) in [
        (30.0_f64, 60.0_f64, "which way each side turns"),
        (180.0, 270.0, "beyond a full turn"),
    ] {
        let model = ring_history(json!({"kind": "two_sided",
            "first": {"kind": "angle", "angle": first.to_radians()},
            "second": {"kind": "angle", "angle": second.to_radians()}}));
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn every_non_angle_revolve_end_has_a_specific_refusal() {
    let face = json!({"kind": "faces", "value": ["synthetic:cube:face#top"]});
    for (termination, reason) in [
        (json!({"kind": "unresolved"}), "termination is not resolved"),
        (json!({"kind": "through_all"}), "through_all revolve ends"),
        (json!({"kind": "through_next"}), "through_next revolve ends"),
        (json!({"kind": "to_first"}), "to_first revolve ends"),
        (json!({"kind": "to_last"}), "to_last revolve ends"),
        (json!({"kind": "to_face", "face": face}), "to-face revolves"),
        (
            json!({"kind": "to_face", "face": face, "offset": 1.0}),
            "to-face revolves with an offset",
        ),
        (
            json!({"kind": "to_vertex", "vertex": {"kind": "unresolved"}}),
            "to-vertex revolves",
        ),
        (
            json!({"kind": "offset_from_face", "face": face, "offset": 1.0}),
            "offset-from-face revolves",
        ),
        (
            json!({"kind": "to_shape", "target": face}),
            "to-shape revolves",
        ),
    ] {
        for extent in [
            json!({"kind": "one_sided", "termination": termination}),
            json!({"kind": "symmetric", "termination": termination}),
            json!({"kind": "two_sided", "first": termination,
                "second": {"kind": "angle", "angle": 1.0}}),
            json!({"kind": "two_sided", "first": {"kind": "angle", "angle": 1.0},
                "second": termination}),
        ] {
            let (_, report) = convert(
                &ring_history(extent),
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            assert!(
                history_line(&report).contains(reason),
                "{}",
                history_line(&report)
            );
        }
    }
}

#[test]
fn revolve_axis_sources_use_the_resolved_line_placement() {
    for reference in [
        Value::Null,
        json!({"kind": "native", "value": "synthetic datum axis"}),
        json!({"kind": "sketch_curves", "value": {
            "sketch": "f3d:model:sketch#9", "curves": ["f3d:model:sketch-entity#1"]}}),
        json!({"kind": "edges", "value": ["synthetic:cube:edge#0"]}),
        json!({"kind": "curves", "value": ["synthetic:cube:curve#0"]}),
    ] {
        let model = changed_ring(|construction| {
            if !reference.is_null() {
                construction["value"]["axis"]["reference"] = reference;
            }
            construction["value"]["axis"]["origin"]["z"] = json!(3.0);
            construction["value"]["axis"]["direction"]["z"] = json!(-2.0);
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
        assert_eq!(
            document["features"][1]["axis"],
            json!({
                "type": "line", "origin": [0.0, 0.0, 3.0], "dir": [0.0, 0.0, -1.0]
            })
        );
    }
}

#[test]
fn a_revolve_about_a_line_of_its_sketch_keeps_that_line() {
    // The rectangle's inner side, 5 mm out from Z, turns the ring into a cylinder.
    let model = changed_ring(|construction| {
        construction["value"]["axis"]["origin"]["x"] = json!(5.0);
        construction["value"]["axis"]["reference"] = json!({"kind": "sketch_curves", "value": {
            "sketch": SKETCH, "curves": ["f3d:model:sketch-entity#4"]}});
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    let axis = &document["features"][1]["axis"];
    assert_eq!(axis["type"], "sketch_line");
    let curve = axis["curve"].as_str().unwrap();
    let sketch = &document["features"][0];
    assert!(
        sketch["sketch"]["curves"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == curve && c["type"] == "line"),
        "{sketch}"
    );
    if let Some(cli) = solidcraft() {
        let actual = volumes(&cli, &written(&model).0);
        assert_eq!(actual.len(), 1);
        let expected = 250.0 * std::f64::consts::PI;
        assert!((actual[0] - expected).abs() / expected < 1e-3, "{actual:?}");
    }
}

#[test]
fn surface_revolves_of_open_curve_chains_take_them_as_curves() {
    let quarter = json!({"kind": "symmetric",
        "termination": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2}});
    let picked = |entities: Value| {
        let quarter = quarter.clone();
        changed_ring(move |construction| {
            construction["value"]["solid"] = json!(false);
            construction["value"]["extent"] = quarter;
            construction["value"]["profile"] = json!({"kind": "sketch_entities",
                "value": {"sketch": SKETCH, "entities": entities}});
        })
    };
    // One open line: the outer wall of the ring, x = 10 on the sketch.
    let model = picked(json!(["f3d:model:sketch-entity#2"]));
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let revolve = &document["features"][1];
    assert_eq!(revolve["type"], "surface_revolve");
    assert!(revolve.get("profiles").is_none(), "{revolve}");
    assert_eq!(
        revolve["curves"].as_array().map(Vec::len),
        Some(1),
        "{revolve}"
    );
    if let Some(cli) = solidcraft() {
        let boxes = bounds(&cli, &written(&model).0);
        assert_eq!(boxes.len(), 1, "{boxes:?}");
        let (min, max) = boxes[0];
        // A quarter of a radius-10 cylinder wall, symmetric about the sketch, z 0..10.
        assert!((max[0] - 10.0).abs() < 1e-6, "{boxes:?}");
        assert!(
            min[2].abs() < 1e-6 && (max[2] - 10.0).abs() < 1e-6,
            "{boxes:?}"
        );
        assert!((min[1] + max[1]).abs() < 1e-6 && max[1] > 0.0, "{boxes:?}");
    }

    // The four lines of the rectangle close a profile: it stays a profile.
    let model = picked(json!([
        "f3d:model:sketch-entity#1",
        "f3d:model:sketch-entity#2",
        "f3d:model:sketch-entity#3",
        "f3d:model:sketch-entity#4"
    ]));
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let revolve = &document["features"][1];
    assert!(revolve.get("curves").is_none(), "{revolve}");
    assert_eq!(revolve["profiles"]["type"], "curves", "{revolve}");
}

#[test]
fn surface_revolves_short_of_a_full_turn_map() {
    let quarter = json!({"kind": "symmetric",
        "termination": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2}});
    let model = changed_ring(|construction| {
        construction["value"]["solid"] = json!(false);
        construction["value"]["face_maker_class"] = json!("Part::FaceMakerBullseye");
        construction["value"]["extent"] = quarter;
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    let revolve = &document["features"][1];
    assert_eq!(revolve["type"], "surface_revolve");
    assert_eq!(revolve["angle"], "90 deg");
    assert_eq!(revolve["symmetric"], true);
    assert!(revolve.get("operation").is_none());
    assert!(revolve.get("convention").is_none());
    assert!(revolve["profiles"].is_object());
    if let Some(cli) = solidcraft() {
        let boxes = bounds(&cli, &written(&model).0);
        assert_eq!(boxes.len(), 1, "{boxes:?}");
        let (min, max) = boxes[0];
        assert!(
            min[2].abs() < 1e-6 && (max[2] - 10.0).abs() < 1e-6,
            "{boxes:?}"
        );
        assert!((min[1] + max[1]).abs() < 1e-6 && max[1] > 0.0, "{boxes:?}");
    }

    let model = changed_ring(|construction| construction["value"]["solid"] = json!(false));
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("full-turn surface revolves are not supported yet"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn revolve_construction_options_map_or_name_the_face_maker() {
    for maker in [
        "Part::FaceMakerSimple",
        "Part::FaceMakerCheese",
        "Part::FaceMakerExtrusion",
        "Part::FaceMakerUnified",
        "SyntheticFaceMaker",
    ] {
        let model =
            changed_ring(|construction| construction["value"]["face_maker_class"] = json!(maker));
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(&format!("revolves whose faces are built by {maker}")),
            "{}",
            history_line(&report)
        );
    }
    for change in [
        json!({"face_maker_class": "Part::FaceMakerBullseye", "solid": true}),
        json!({"fuse_order": "base_first", "allow_multi_profile_faces": false}),
        json!({"fuse_order": "feature_first", "allow_multi_profile_faces": true}),
    ] {
        let model = changed_ring(|construction| {
            for (key, value) in change.as_object().unwrap() {
                construction["value"][key] = value.clone();
            }
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("2 of 2 features rebuilt"),
            "{change}: {}",
            history_line(&report)
        );
        assert_eq!(document["features"][1]["type"], "revolve");
    }
}

#[test]
fn unresolved_revolve_inputs_name_the_missing_operand() {
    for missing in ["profile", "axis", "extent"] {
        let model = changed_ring(|construction| {
            construction["kind"] = json!("unresolved");
            construction["value"]["kind"] = json!(missing);
            construction["value"]
                .as_object_mut()
                .unwrap()
                .remove(missing);
        });
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(&format!("revolve {missing} is not resolved")),
            "{}",
            history_line(&report)
        );
    }
}

const HOLES: &str = "f3d:model:sketch#3";

/// The example cube's history, with a sketch on its top face holding a circle of radius 2 about
/// (5, 5) and a simple hole of `diameter` drilled down from it; `change` edits the hole.
fn holed_cube(diameter: f64, change: impl FnOnce(&mut Value)) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketches"].as_array_mut().unwrap().push(
        json!({"id": HOLES, "name": "Holes", "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 10.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }}, "profiles": []}),
    );
    m["sketch_entities"].as_array_mut().unwrap().push(json!({"id": "f3d:model:sketch-entity#9", "sketch": HOLES, "construction": false, "geometry": {"kind": "circle", "center": {"u": 5.0, "v": 5.0}, "radius": 2.0}, "endpoint_refs": []}));
    let mut hole = json!({"kind": "hole", "value": {"profile": {"kind": "sketch", "value": HOLES}, "profile_filter": "circles", "direction": {"x": 0.0, "y": 0.0, "z": -1.0}, "shape": {"construction": {"kind": "form", "value": {"kind": {"kind": "simple"}}},
                  "diameter": diameter}, "extent": {"kind": "through_all"}}});
    change(&mut hole["value"]);
    let features = m["features"].as_array_mut().unwrap();
    features.push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Hole sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": HOLES}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    features.push(json!({"id": "f3d:model:feature#4", "ordinal": 3, "name": "Hole", "suppressed": false, "dependencies": ["f3d:model:feature#3", "f3d:model:feature#2"], "definition": {"kind": "operation", "value": hole}, "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_simple_hole_at_a_sketch_circle_becomes_a_solidcraft_hole() {
    let (document, report) = convert(
        &holed_cube(4.0, |_| {}),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("4 of 4 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let hole = &document["features"][3];
    assert_eq!(hole["type"], "hole");
    assert_eq!(hole["diameter"], "4 mm");
    assert_eq!(hole["direction"], json!([0.0, 0.0, -1.0]));
    assert_eq!(hole["position"], json!([5.0, 5.0, 10.0]));
    assert_eq!(hole["points"]["sketch"], document["features"][2]["id"]);
}

/// `historical_hole` (a hole placed on a face of the earlier body, so the face must be proven)
/// with its extruded block replaced by a box feature of `height` standing on the origin with a
/// 10 x 10 footprint.
fn holed_box(height: f64) -> Model {
    let mut model = historical_hole(false, directed([5.0, 5.0, 10.0], [0.0, 0.0, -1.0]));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    wire["model"]["features"][1]["definition"]["value"] = block([10.0, 10.0, height], identity);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_hole_on_the_face_of_a_box_primitive_maps() {
    let hole = mapped_hole(&holed_box(10.0)).unwrap();
    assert_eq!(hole["position"], json!([5.0, 5.0, 10.0]));
    assert_eq!(hole["direction"], json!([0.0, 0.0, -1.0]));
}

#[test]
fn a_hole_on_a_face_the_box_primitive_lacks_still_refuses() {
    // The box stops at z = 8, so the face at z = 10 is not one of its faces.
    let why = mapped_hole(&holed_box(8.0)).unwrap_err();
    assert!(why.contains("not proven to exist before the hole"), "{why}");
}

#[test]
fn counterbores_and_countersinks_keep_their_entry_dimensions() {
    for version in ["0.18.2", "0.25.1"] {
        for (form, expected) in [
            (
                json!({"kind": "counterbore", "diameter": 8.0, "depth": 2.0}),
                json!({"type": "counterbore", "cb_diameter": "8 mm", "cb_depth": "2 mm"}),
            ),
            (
                json!({"kind": "countersink", "diameter": 8.0,
                "angle": std::f64::consts::FRAC_PI_2}),
                json!({"type": "countersink", "cs_diameter": "8 mm", "cs_angle": "90"}),
            ),
            (
                json!({"kind": "chamfer", "diameter": 8.0,
                "angle": std::f64::consts::FRAC_PI_2}),
                json!({"type": "countersink", "cs_diameter": "8 mm", "cs_angle": "90"}),
            ),
        ] {
            for blind in [false, true] {
                let mut expected = expected.clone();
                if version
                    .parse::<dutchman_solidcraft::SolidCraftVersion>()
                    .unwrap()
                    >= dutchman_solidcraft::SolidCraftVersion::V0_22_1
                {
                    expected["tip_angle"] = json!("180");
                }
                let model = holed_cube(4.0, |hole| {
                    hole["shape"]["construction"]["value"]["kind"] = form.clone();
                    if blind {
                        hole["extent"] = json!({"kind": "blind", "length": 6.0});
                        hole["bottom"] = json!({"kind": "flat"});
                    }
                });
                let (document, report) = convert(
                    &model,
                    &SolidCraftWriter::new()
                        .verification(Verification::Off)
                        .target_version(Some(version.parse().unwrap())),
                );
                assert!(
                    history_line(&report).starts_with("4 of 4 features rebuilt"),
                    "{}",
                    history_line(&report)
                );
                let hole = &document["features"][3];
                assert_eq!(hole["hole"], expected);
                assert_eq!(hole["diameter"], "4 mm");
                assert_eq!(hole["position"], json!([5.0, 5.0, 10.0]));
                assert_eq!(hole["direction"], json!([0.0, 0.0, -1.0]));
                assert_eq!(hole["points"]["sketch"], document["features"][2]["id"]);
                if blind {
                    assert_eq!(hole["depth"], "6 mm");
                } else {
                    assert!(hole.get("depth").is_none());
                }
            }
        }
    }
}

fn pointed_hole_forms() -> Vec<(Value, &'static str, f64)> {
    let pi = std::f64::consts::PI;
    vec![
        (json!({"kind": "simple"}), "simple", 0.0),
        (
            json!({"kind": "simple_drilled", "drill_point_angle": 118_f64.to_radians()}),
            "simple",
            0.0,
        ),
        (
            json!({"kind": "counterbore", "diameter": 8.0, "depth": 2.0}),
            "counterbore",
            24.0 * pi,
        ),
        (
            json!({"kind": "counterbore_drilled", "diameter": 8.0, "depth": 2.0,
            "drill_point_angle": 118_f64.to_radians()}),
            "counterbore",
            24.0 * pi,
        ),
        // A 90 degree entry is a 2 mm deep frustum, less the bore it overlaps.
        (
            json!({"kind": "countersink", "diameter": 8.0, "angle": pi / 2.0}),
            "countersink",
            32.0 * pi / 3.0,
        ),
        (
            json!({"kind": "chamfer", "diameter": 8.0, "angle": pi / 2.0}),
            "countersink",
            32.0 * pi / 3.0,
        ),
    ]
}

#[test]
fn through_all_holes_drop_drill_points_on_every_target() {
    for version in [
        "0.18.2".parse().unwrap(),
        "0.22.0".parse().unwrap(),
        "0.22.1".parse().unwrap(),
        dutchman_solidcraft::SolidCraftVersion::default(),
    ] {
        for (form, kind, _) in pointed_hole_forms() {
            // Readers may carry the point on the form, on the bottom, or both.
            for separate_bottom in [false, true] {
                if !separate_bottom
                    && !matches!(
                        form["kind"].as_str(),
                        Some("simple_drilled" | "counterbore_drilled")
                    )
                {
                    continue;
                }
                let model = holed_cube(4.0, |hole| {
                    hole["shape"]["construction"]["value"]["kind"] = form.clone();
                    if separate_bottom {
                        hole["bottom"] = json!({"kind": "angled", "included_angle": 118_f64.to_radians(), "depth_to_tip": true});
                    }
                });
                let (document, report) = convert(
                    &model,
                    &SolidCraftWriter::new()
                        .verification(Verification::Off)
                        .target_version(Some(version)),
                );
                assert!(
                    history_line(&report).starts_with("4 of 4 features rebuilt"),
                    "{report:?}"
                );
                let hole = &document["features"][3];
                assert_eq!(hole["hole"]["type"], kind);
                assert!(hole["hole"].get("tip_angle").is_none(), "{hole}");
                assert!(hole.get("depth").is_none());
                assert!(hole.get("to").is_none());
            }
        }
    }
}

#[test]
fn bounded_entry_recess_points_follow_the_existing_version_gate() {
    for version in ["0.18.2", "0.22.0", "0.22.1"] {
        for (form, kind, _) in pointed_hole_forms().into_iter().skip(2) {
            for to_face in [false, true] {
                let model = holed_cube(4.0, |hole| {
                    hole["shape"]["construction"]["value"]["kind"] = form.clone();
                    if to_face {
                        to_face_hole(hole);
                    } else {
                        hole["extent"] = json!({"kind": "blind", "length": 6.0});
                    }
                    hole["bottom"] = json!({"kind": "angled", "included_angle": 118_f64.to_radians(), "depth_to_tip": false});
                });
                let (document, report) = convert(
                    &model,
                    &SolidCraftWriter::new()
                        .verification(Verification::Off)
                        .target_version(Some(version.parse().unwrap())),
                );
                if version == "0.22.1" {
                    assert!(
                        history_line(&report).starts_with("4 of 4 features rebuilt"),
                        "{report:?}"
                    );
                    assert_eq!(document["features"][3]["hole"]["type"], kind);
                    assert_eq!(
                        document["features"][3]["hole"]["tip_angle"],
                        "118.00000000000001"
                    );
                    assert_eq!(
                        document["features"][3]["depth"],
                        if to_face { "10 mm" } else { "6 mm" }
                    );
                } else {
                    assert!(
                        history_line(&report).contains(
                            "SolidCraft holes take a drill point only on a plain drilled hole"
                        ),
                        "{report:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn solidcraft_replays_through_all_holes_without_drill_points() {
    let Some(cli) = solidcraft() else { return };
    let detected =
        dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()));
    for version in [
        "0.18.2".parse().unwrap(),
        dutchman_solidcraft::SolidCraftVersion::default(),
    ] {
        if detected < version {
            continue;
        }
        for (form, _, extra) in pointed_hole_forms() {
            let model = holed_cube(4.0, |hole| {
                hole["shape"]["construction"]["value"]["kind"] = form;
                hole["bottom"] = json!({"kind": "angled", "included_angle": 118_f64.to_radians(), "depth_to_tip": false});
            });
            let writer = SolidCraftWriter::new()
                .verification(Verification::Off)
                .target_version(Some(version));
            let (bytes, report) = written_with(&model, &writer);
            assert!(
                history_line(&report).starts_with("4 of 4 features rebuilt"),
                "{report:?}"
            );
            let got = volumes(&cli, &bytes);
            let want = 1000.0 - 40.0 * std::f64::consts::PI - extra;
            assert_eq!(got.len(), 1, "{got:?}");
            assert!((got[0] - want).abs() / want < 1e-3, "{got:?} vs {want}");
        }
    }
}

#[test]
fn solidcraft_drills_counterbores_and_countersinks() {
    let Some(cli) = solidcraft() else { return };
    let bore_area = std::f64::consts::PI * 2.0_f64.powi(2);
    let entry_area = std::f64::consts::PI * 4.0_f64.powi(2);
    let angle = 82_f64.to_radians();
    let sink_depth = (4.0 - 2.0) / (angle / 2.0).tan();
    // The countersink is a conical frustum; subtract the bore already counted in it.
    let sink_extra = std::f64::consts::PI * sink_depth / 3.0
        * (4.0_f64.powi(2) + 4.0 * 2.0 + 2.0_f64.powi(2))
        - bore_area * sink_depth;
    for (form, extra) in [
        (
            json!({"kind": "counterbore", "diameter": 8.0, "depth": 2.0}),
            (entry_area - bore_area) * 2.0,
        ),
        (
            json!({"kind": "countersink", "diameter": 8.0, "angle": angle}),
            sink_extra,
        ),
        (
            json!({"kind": "chamfer", "diameter": 8.0, "angle": angle}),
            sink_extra,
        ),
    ] {
        for depth in [None, Some(6.0)] {
            let model = holed_cube(4.0, |hole| {
                hole["shape"]["construction"]["value"]["kind"] = form.clone();
                if let Some(depth) = depth {
                    hole["extent"] = json!({"kind": "blind", "length": depth});
                    hole["bottom"] = json!({"kind": "flat"});
                }
            });
            let got = volumes(&cli, &written(&model).0);
            let want = 1000.0 - bore_area * depth.unwrap_or(10.0) - extra;
            assert_eq!(got.len(), 1, "{got:?}");
            assert!(
                (got[0] - want).abs() / want < 1e-3,
                "{form}: {got:?} vs {want}"
            );
        }
    }
}

#[test]
fn a_plain_drilled_form_keeps_its_angle_and_shoulder_depth() {
    let model = holed_cube(4.0, |hole| {
        hole["shape"]["construction"]["value"]["kind"] = json!({"kind": "simple_drilled",
            "drill_point_angle": std::f64::consts::FRAC_PI_2});
        hole["extent"] = json!({"kind": "blind", "length": 6.0});
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("4 of 4 features rebuilt"));
    let hole = &document["features"][3];
    assert_eq!(hole["hole"], json!({"type": "drilled", "tip_angle": "90"}));
    assert_eq!(hole["diameter"], "4 mm");
    assert_eq!(hole["depth"], "6 mm");
    assert_eq!(hole["position"], json!([5.0, 5.0, 10.0]));
    assert_eq!(hole["direction"], json!([0.0, 0.0, -1.0]));
    assert_eq!(hole["points"]["sketch"], document["features"][2]["id"]);
}

#[test]
fn entry_recesses_write_flat_tips_only_for_new_targets() {
    for version in ["0.18.2", "0.22.0", "0.22.1", "0.25.1"] {
        for form in [
            json!({"kind": "counterbore", "diameter": 8.0, "depth": 2.0}),
            json!({"kind": "countersink", "diameter": 8.0, "angle": std::f64::consts::FRAC_PI_2}),
            json!({"kind": "chamfer", "diameter": 8.0, "angle": std::f64::consts::FRAC_PI_2}),
        ] {
            let model = holed_cube(4.0, |hole| {
                hole["shape"]["construction"]["value"]["kind"] = form;
                hole["extent"] = json!({"kind": "blind", "length": 6.0});
                hole["bottom"] = json!({"kind": "flat"});
            });
            let (document, _) = convert(
                &model,
                &SolidCraftWriter::new()
                    .verification(Verification::Off)
                    .target_version(Some(version.parse().unwrap())),
            );
            let tip = &document["features"][3]["hole"]["tip_angle"];
            if version
                .parse::<dutchman_solidcraft::SolidCraftVersion>()
                .unwrap()
                >= dutchman_solidcraft::SolidCraftVersion::V0_22_1
            {
                assert_eq!(tip, "180");
            } else {
                assert!(tip.is_null());
            }
        }
    }
}

#[test]
fn solidcraft_entry_recess_tip_angles_follow_the_target_release() {
    let Some(cli) = solidcraft() else { return };
    let modern = dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()))
        >= dutchman_solidcraft::SolidCraftVersion::V0_22_1;
    let dir = tempfile::tempdir().unwrap();
    let bore = std::f64::consts::PI * 4.0;
    // Older releases ignore the angle; 0.22.1 and newer cut a cone unless the tip is flat.
    for (kind, extra) in [
        ("counterbore", std::f64::consts::PI * 12.0 * 2.0),
        (
            "countersink",
            std::f64::consts::PI * 2.0 / 3.0 * 28.0 - bore * 2.0,
        ),
    ] {
        let mut previous = None;
        for tip in [None, Some("90"), Some("118"), Some("180")] {
            let mut params = json!({"position": [5, 5, 10], "direction": [0, 0, -1],
                "diameter": "4 mm", "depth": "6 mm", "type": kind,
                "cb_diameter": "8 mm", "cb_depth": "2 mm",
                "cs_diameter": "8 mm", "cs_angle": "90"});
            if let Some(tip) = tip {
                params["tip_angle"] = json!(tip);
            }
            let script = json!([
                {"command": "solid.box", "params": {"length": "10 mm", "width": "10 mm", "height": "10 mm"}},
                {"command": "solid.hole", "params": params},
            ]);
            let input = dir.path().join("hole.json");
            std::fs::write(&input, script.to_string()).unwrap();
            let measured = blend_measurement(&cli, &input, &dir.path().join("eval.json"));
            let volume = measured["volume_mm3"].as_f64().unwrap();
            let flat = 1000.0 - bore * 6.0 - extra;
            let point = if modern {
                tip.map_or(0.0, |angle| {
                    std::f64::consts::PI * 8.0
                        / (3.0 * (angle.parse::<f64>().unwrap().to_radians() / 2.0).tan())
                })
            } else {
                0.0
            };
            let expected = flat - point;
            // Tessellated volumes have a small error, well below either missing cone.
            assert!(
                (volume - expected).abs() < 0.05,
                "{kind} {tip:?}: {volume} vs {expected}"
            );
            if !modern || tip == Some("180") {
                if let Some(previous) = previous {
                    assert_eq!(volume, previous, "{kind} {tip:?}");
                }
            }
            if tip.is_none() {
                previous = Some(volume);
            }
        }
    }
}

#[test]
fn solidcraft_drills_the_holes() {
    let Some(cli) = solidcraft() else { return };
    let close = |got: &[f64], want: f64| {
        assert_eq!(got.len(), 1, "{got:?}");
        assert!((got[0] - want).abs() / want < 1e-3, "{got:?} vs {want}");
    };
    let bore = std::f64::consts::PI * 2.0_f64.powi(2);
    // Through all.
    close(
        &volumes(&cli, &written(&holed_cube(4.0, |_| {})).0),
        1000.0 - bore * 10.0,
    );
    // The same 118° point, with depth measured either to its shoulder or its tip.
    let angle = 118_f64.to_radians();
    let point = 2.0 / (angle / 2.0).tan();
    for to_tip in [false, true] {
        let blind = holed_cube(4.0, |hole| {
            hole["extent"] = json!({"kind": "blind", "length": 6.0});
            hole["bottom"] =
                json!({"kind": "angled", "included_angle": angle, "depth_to_tip": to_tip});
        });
        let shoulder = if to_tip { 6.0 - point } else { 6.0 };
        close(
            &volumes(&cli, &written(&blind).0),
            1000.0 - bore * shoulder - bore * point / 3.0,
        );
    }
}

/// A spatial placement takes its position from the feature, without a sketch.
fn directed_hole(hole: &mut Value) {
    hole.as_object_mut().unwrap().remove("profile");
    hole.as_object_mut().unwrap().remove("profile_filter");
    hole["placements"] = json!([{"kind": "directed",
        "position": {"x": 5.0, "y": 5.0, "z": 10.0},
        "direction": {"x": 0.0, "y": 0.0, "z": -2.0}}]);
}

fn axis_hole(hole: &mut Value, sign: f64) {
    directed_hole(hole);
    hole["face"] = json!({"kind": "faces", "value": ["synthetic:cube:face#top"]});
    hole["placements"] = json!([{"kind": "axis",
        "origin": {"x": 5.0, "y": 5.0, "z": 6.0},
        "axis": {"x": 0.0, "y": 0.0, "z": sign}}]);
}

fn to_face_hole(hole: &mut Value) {
    hole["extent"] = json!({"kind": "to_face", "face": {
        "kind": "resolved", "value": {"faces": ["synthetic:cube:face#bottom"], "native": "bottom"}}});
    hole["bottom"] = json!({"kind": "flat"});
}

#[test]
fn spatial_holes_keep_their_entry_position_and_inward_direction() {
    for model in [
        holed_cube(4.0, directed_hole),
        holed_cube(4.0, |hole| axis_hole(hole, 1.0)),
        holed_cube(4.0, |hole| axis_hole(hole, -1.0)),
        holed_cube(4.0, |hole| {
            directed_hole(hole);
            hole["face"] = json!({"kind": "faces", "value": ["synthetic:cube:face#top"]});
        }),
    ] {
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("4 of 4 features rebuilt"),
            "{}",
            history_line(&report)
        );
        let hole = &document["features"][3];
        assert_eq!(hole["position"], json!([5.0, 5.0, 10.0]));
        let direction: Vec<f64> = hole["direction"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert_eq!(direction, vec![0.0, 0.0, -1.0]);
        assert!(hole.get("points").is_none());
    }
}

#[test]
fn a_hole_to_an_earlier_cap_keeps_a_point_on_that_face() {
    let (document, report) = convert(
        &holed_cube(4.0, to_face_hole),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("4 of 4 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let hole = &document["features"][3];
    let to = hole["to"].as_array().unwrap();
    assert_eq!(to[2], 0.0);
    assert!(to[0].as_f64().unwrap() > 0.0 && to[0].as_f64().unwrap() < 10.0);
    assert!(to[1].as_f64().unwrap() > 0.0 && to[1].as_f64().unwrap() < 10.0);
    assert!(hole.get("depth").is_none());
}

#[test]
fn a_plain_drilled_to_face_hole_keeps_its_shoulder_depth() {
    let model = holed_cube(4.0, |hole| {
        to_face_hole(hole);
        hole.as_object_mut().unwrap().remove("bottom");
        hole["shape"]["construction"]["value"]["kind"] = json!({
            "kind": "simple_drilled", "drill_point_angle": 118_f64.to_radians()
        });
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("4 of 4 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let hole = &document["features"][3];
    assert_eq!(hole["hole"]["type"], "drilled");
    assert_eq!(hole["hole"]["tip_angle"], "118.00000000000001");
    assert_eq!(hole["depth"], "10 mm");
}

/// Entirely synthetic: the cube, a countersink ending at its bottom cap, and a
/// separate box added afterwards. The bottom flag exercises CADIR depth conventions.
fn countersink_to_face_with_later_box(to_tip: bool) -> Model {
    let mut model = holed_cube(4.0, |hole| {
        to_face_hole(hole);
        hole["shape"]["construction"]["value"]["kind"] = json!({
            "kind": "countersink", "diameter": 8.0, "angle": std::f64::consts::FRAC_PI_2
        });
        hole["bottom"] = json!({"kind": "angled",
            "included_angle": 118_f64.to_radians(), "depth_to_tip": to_tip});
    });
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": "f3d:model:feature#5", "ordinal": 4, "name": "Later box", "suppressed": false,
            "definition": {"kind": "operation", "value": block([2.0; 3], [
                [1.0, 0.0, 0.0, 20.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]
            ])}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []
        }));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn countersink_to_face_with_a_drill_point_keeps_later_history() {
    for to_tip in [false, true] {
        let (document, report) = convert(
            &countersink_to_face_with_later_box(to_tip),
            &SolidCraftWriter::new()
                .verification(Verification::Off)
                .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_44_0)),
        );
        assert!(
            history_line(&report).starts_with("5 of 5 features rebuilt"),
            "{}",
            history_line(&report)
        );
        let hole = &document["features"][3];
        assert_eq!(hole["type"], "hole");
        assert_eq!(hole["hole"]["type"], "countersink");
        assert_eq!(hole["hole"]["cs_diameter"], "8 mm");
        assert_eq!(hole["hole"]["cs_angle"], "90");
        assert_eq!(hole["hole"]["tip_angle"], "118.00000000000001");
        let depth: f64 = hole["depth"]
            .as_str()
            .unwrap()
            .trim_end_matches(" mm")
            .parse()
            .unwrap();
        let point = 2.0 / 59_f64.to_radians().tan();
        assert!((depth - if to_tip { 10.0 - point } else { 10.0 }).abs() < 1e-9);
        assert!(hole.get("to").is_none());
        assert_eq!(document["features"][4]["type"], "box");
        assert_eq!(document["features"][4]["name"], "Later box");
    }
}

#[test]
fn pointed_to_face_holes_still_require_a_proven_earlier_cap() {
    for native in [false, true] {
        let mut model = countersink_to_face_with_later_box(false);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        if native {
            wire["model"]["features"][3]["definition"]["value"]["value"]["extent"]["face"] =
                json!({"kind": "native", "value": "unresolved-face"});
        } else {
            // The saved bottom is z=0; the earlier extrusion instead starts at z=2.
            wire["model"]["sketches"][0]["placement"]["frame"]["origin"]["z"] = json!(2.0);
        }
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new()
                .verification(Verification::Off)
                .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_44_0)),
        );
        let reason = if native {
            "its face has no resolved geometry"
        } else {
            "its face is not proven to exist before the hole"
        };
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn solidcraft_replays_countersink_to_face_with_a_drill_point_and_later_box() {
    let Some(cli) = solidcraft() else { return };
    // A drill point on a countersink needs SolidCraft 0.22.1 or newer.
    if dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()))
        < dutchman_solidcraft::SolidCraftVersion::V0_22_1
    {
        eprintln!("skipped: the countersink drill point needs SolidCraft 0.22.1 or newer");
        return;
    }
    let bore = std::f64::consts::PI * 4.0;
    // Extra material removed by the entry frustum beyond the cylindrical bore.
    let entry = std::f64::consts::PI * 2.0 * (16.0 + 8.0 + 4.0) / 3.0 - bore * 2.0;
    for to_tip in [false, true] {
        let mut got = volumes(
            &cli,
            &written(&countersink_to_face_with_later_box(to_tip)).0,
        );
        got.sort_by(f64::total_cmp);
        let point = 2.0 / 59_f64.to_radians().tan();
        let removed =
            bore * if to_tip {
                10.0 - 2.0 * point / 3.0
            } else {
                10.0
            } + entry;
        assert_eq!(got.len(), 2, "{got:?}");
        assert!((got[0] - 8.0).abs() < 1e-6, "{got:?}");
        let want = 1000.0 - removed;
        assert!((got[1] - want).abs() / want < 1e-3, "{got:?} vs {want}");
    }
}

#[test]
fn solidcraft_replays_countersink_to_face_with_a_later_fillet() {
    let Some(cli) = solidcraft() else { return };
    // A drill point on a countersink needs SolidCraft 0.22.1 or newer.
    if dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()))
        < dutchman_solidcraft::SolidCraftVersion::V0_22_1
    {
        eprintln!("skipped: the countersink drill point needs SolidCraft 0.22.1 or newer");
        return;
    }
    let mut model = countersink_to_face_with_later_box(false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // Reuse the synthetic vertical-corner input geometry, after the hole instead
    // of directly after the extrusion. This edge is unchanged by the hole.
    let blend = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    let source = serde_json::to_value(&blend.ir).unwrap();
    let mut fillet = source["model"]["features"][2].clone();
    fillet["id"] = json!("f3d:model:feature#5");
    fillet["ordinal"] = json!(4);
    fillet["name"] = json!("Later fillet");
    fillet["dependencies"] = json!(["f3d:model:feature#4"]);
    let features = wire["model"]["features"].as_array_mut().unwrap();
    features.pop();
    features.push(fillet);
    wire["model"]["feature_input_topologies"] = source["model"]["feature_input_topologies"].clone();
    wire["model"]["feature_input_topologies"][0]["input_of"] = json!("f3d:model:feature#5");
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (bytes, report) = written_with(&model, &replaying(&cli));
    assert!(
        history_line(&report).starts_with("5 of 5 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let got = volumes(&cli, &bytes);
    let pi = std::f64::consts::PI;
    let bore = pi * 4.0;
    let entry = pi * 2.0 * (16.0 + 8.0 + 4.0) / 3.0 - bore * 2.0;
    let corner = 10.0 * (1.0 - std::f64::consts::FRAC_PI_4);
    let want = 1000.0 - bore * 10.0 - entry - corner;
    assert_eq!(got.len(), 1, "{got:?}");
    assert!(
        (got[0] - want).abs() / want < dutchman_solidcraft::VOLUME_TOLERANCE,
        "{got:?} vs {want}"
    );
}

const HOLE_INPUT: &str = "test:hole:feature-input#4";
const HOLE_FACE: &str = "test:hole:historical-face#entry";

/// A historical entry face for the hole of [`holed_cube`]: the top of the cube as it was
/// before the hole (`round` false), or a whole cylinder of radius 2 about the vertical line
/// through (5, 5) (`round` true). Edge geometry is all the input topology carries.
fn historical_entry(round: bool) -> impl FnOnce(&mut Value) {
    move |hole| {
        let point = |x: f64, y: f64, z: f64| json!({"x": x, "y": y, "z": z});
        let edge = |k: usize, start: Value, interior: Value, end: Value, tangent: Value| {
            json!({"edge": format!("test:hole:historical-edge#{k}"),
                "start": start, "interior": interior, "end": end, "tangent": tangent,
                "faces": [HOLE_FACE, format!("test:hole:historical-face#other{k}")]})
        };
        let geometry: Vec<Value> = if round {
            let tangent = point(0.0, 1.0, 0.0);
            vec![
                edge(
                    0,
                    point(7.0, 5.0, 0.0),
                    point(3.0, 5.0, 0.0),
                    point(7.0, 5.0, 0.0),
                    tangent.clone(),
                ),
                edge(
                    1,
                    point(7.0, 5.0, 10.0),
                    point(3.0, 5.0, 10.0),
                    point(7.0, 5.0, 10.0),
                    tangent,
                ),
                edge(
                    2,
                    point(7.0, 5.0, 0.0),
                    point(7.0, 5.0, 5.0),
                    point(7.0, 5.0, 10.0),
                    point(0.0, 0.0, 1.0),
                ),
            ]
        } else {
            let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
            (0..4)
                .map(|k| {
                    let (a, b) = (corners[k], corners[(k + 1) % 4]);
                    edge(
                        k,
                        point(a.0, a.1, 10.0),
                        point((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, 10.0),
                        point(b.0, b.1, 10.0),
                        point((b.0 - a.0) / 10.0, (b.1 - a.1) / 10.0, 0.0),
                    )
                })
                .collect()
        };
        hole["face"] = json!({"kind": "historical", "value": {
            "state": HOLE_INPUT, "faces": [HOLE_FACE], "native": "synthetic selection"}});
        hole["feature_input"] = json!({"edge_geometry": geometry});
    }
}

/// `holed_cube` with the entry face `historical_entry` describes and the hole `placed` by
/// its placement.
fn historical_hole(round: bool, placement: Value) -> Model {
    let mut input = Value::Null;
    let model = holed_cube(4.0, |hole| {
        directed_hole(hole);
        historical_entry(round)(hole);
        hole["placements"] = json!([placement]);
        input = hole
            .as_object_mut()
            .unwrap()
            .remove("feature_input")
            .unwrap();
    });
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["feature_input_topologies"] = json!([{"id": HOLE_INPUT, "input_of": "f3d:model:feature#4", "edges": [], "faces": [HOLE_FACE], "edge_geometry": input["edge_geometry"], "bodies": [], "vertices": []}]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

fn directed(position: [f64; 3], direction: [f64; 3]) -> Value {
    json!({"kind": "directed",
        "position": {"x": position[0], "y": position[1], "z": position[2]},
        "direction": {"x": direction[0], "y": direction[1], "z": direction[2]}})
}

fn axis_through(origin: [f64; 3], axis: [f64; 3]) -> Value {
    json!({"kind": "axis",
        "origin": {"x": origin[0], "y": origin[1], "z": origin[2]},
        "axis": {"x": axis[0], "y": axis[1], "z": axis[2]}})
}

fn mapped_hole(model: &Model) -> Result<Value, String> {
    let (document, report) = convert(
        model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    if history_line(&report).starts_with("4 of 4 features rebuilt") {
        Ok(document["features"][3].clone())
    } else {
        Err(history_line(&report))
    }
}

#[test]
fn a_hole_on_a_historical_plane_is_proven_against_the_cap_it_drills_into() {
    for placement in [
        directed([5.0, 5.0, 10.0], [0.0, 0.0, -1.0]),
        axis_through([5.0, 5.0, 6.0], [0.0, 0.0, 1.0]),
        axis_through([5.0, 5.0, 6.0], [0.0, 0.0, -1.0]),
    ] {
        let hole = mapped_hole(&historical_hole(false, placement)).unwrap();
        assert_eq!(hole["position"], json!([5.0, 5.0, 10.0]));
        assert_eq!(hole["direction"], json!([0.0, 0.0, -1.0]));
    }
    // Drilling out of the cube, or from a point beside the face, is not its entry.
    for (placement, reason) in [
        (
            directed([5.0, 5.0, 10.0], [0.0, 0.0, 1.0]),
            "does not drill into its entry face",
        ),
        (
            directed([15.0, 5.0, 10.0], [0.0, 0.0, -1.0]),
            "does not drill into its entry face",
        ),
    ] {
        let why = mapped_hole(&historical_hole(false, placement)).unwrap_err();
        assert!(why.contains(reason), "{why}");
    }
}

#[test]
fn a_hole_on_a_historical_plane_the_cube_never_had_is_refused() {
    // The bottom of the cube is a cap too, but this plane is neither of the extrude's.
    let model = historical_hole(false, directed([5.0, 5.0, 10.0], [0.0, 0.0, -1.0]));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for row in wire["model"]["feature_input_topologies"][0]["edge_geometry"]
        .as_array_mut()
        .unwrap()
    {
        for key in ["start", "interior", "end"] {
            row[key]["z"] = json!(4.0);
        }
    }
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let why = mapped_hole(&model).unwrap_err();
    assert!(why.contains("not proven to exist before the hole"), "{why}");
}

/// A synthetic 10 mm box with a 2 mm fillet on its vertical origin edge, or
/// a plate with an earlier 2 mm through hole at (3, 5). The second hole's own
/// input boundary describes the resulting top face, never the saved final B-rep.
fn curved_hole(rounded: bool, placement: Value) -> Model {
    let model = historical_hole(false, placement);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let earlier = if rounded {
        let blend = blend_on(
            cube_with_history(10.0, first_profile(), None),
            "fillet",
            json!({"kind": "constant", "radius": 2.0}),
            false,
            0.0,
        );
        let blend = serde_json::to_value(&blend.ir).unwrap();
        wire["model"]["feature_input_topologies"]
            .as_array_mut()
            .unwrap()
            .push(blend["model"]["feature_input_topologies"][0].clone());
        blend["model"]["features"][2].clone()
    } else {
        let mut earlier = wire["model"]["features"][3].clone();
        earlier["id"] = json!("f3d:model:feature#3");
        earlier["ordinal"] = json!(2);
        earlier["name"] = json!("First hole");
        earlier["dependencies"] = json!(["f3d:model:feature#2"]);
        let op = &mut earlier["definition"]["value"]["value"];
        op.as_object_mut().unwrap().remove("face");
        op["placements"] = json!([directed([3.0, 5.0, 10.0], [0.0, 0.0, -1.0])]);
        op["shape"]["diameter"] = json!(2.0);
        earlier
    };
    wire["model"]["features"][2] = earlier;
    wire["model"]["features"][3]["dependencies"] = json!(["f3d:model:feature#3"]);
    wire["model"]["features"][3]["definition"]["value"]["value"]["shape"]["diameter"] = json!(2.0);
    let point = |x: f64, y: f64| json!({"x": x, "y": y, "z": 10.0});
    let sample = |k: usize, a: (f64, f64), m: (f64, f64), b: (f64, f64), t: (f64, f64)| {
        json!({"edge": format!("test:curved-hole:edge#{k}"), "faces": [HOLE_FACE],
            "start": point(a.0, a.1), "interior": point(m.0, m.1),
            "end": point(b.0, b.1), "tangent": {"x": t.0, "y": t.1, "z": 0.0}})
    };
    let geometry = &mut wire["model"]["feature_input_topologies"][0]["edge_geometry"];
    if rounded {
        // The removed corner is outside the quarter circle centred at (2, 2).
        let q = 2.0 - 2.0_f64.sqrt();
        *geometry = json!([
            sample(0, (2.0, 0.0), (6.0, 0.0), (10.0, 0.0), (1.0, 0.0)),
            sample(1, (10.0, 0.0), (10.0, 5.0), (10.0, 10.0), (0.0, 1.0)),
            sample(2, (10.0, 10.0), (5.0, 10.0), (0.0, 10.0), (-1.0, 0.0)),
            sample(3, (0.0, 10.0), (0.0, 6.0), (0.0, 2.0), (0.0, -1.0)),
            sample(
                4,
                (0.0, 2.0),
                (q, q),
                (2.0, 0.0),
                (
                    std::f64::consts::FRAC_1_SQRT_2,
                    -std::f64::consts::FRAC_1_SQRT_2
                )
            ),
        ]);
    } else {
        geometry.as_array_mut().unwrap().push(sample(
            4,
            (4.0, 5.0),
            (2.0, 5.0),
            (4.0, 5.0),
            (0.0, 1.0),
        ));
    }
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn curved_hole_placements_outside_the_face_or_inside_its_existing_hole_are_refused() {
    for (rounded, at) in [(true, [0.1, 0.1, 10.0]), (false, [3.0, 5.0, 10.0])] {
        let (_, report) = convert(
            &curved_hole(rounded, directed(at, [0.0, 0.0, -1.0])),
            &SolidCraftWriter::new()
                .verification(Verification::Off)
                .target_version(Some("0.40.0".parse().unwrap())),
        );
        assert_eq!(
            report.outcome().unwrap().features[3].reason.as_deref(),
            Some("its placement does not drill into its entry face"),
            "{report:?}",
        );
    }
}

#[test]
fn curved_hole_entries_keep_older_targets_open_with_a_named_blocker() {
    for target in ["0.18.2", "0.39.0"] {
        let (_, report) = convert(
            &curved_hole(false, directed([7.0, 5.0, 10.0], [0.0, 0.0, -1.0])),
            &SolidCraftWriter::new()
                .verification(Verification::Off)
                .target_version(Some(target.parse().unwrap())),
        );
        let reason = report.outcome().unwrap().features[3]
            .reason
            .as_deref()
            .unwrap();
        assert!(
            reason.contains("need SolidCraft 0.40.0 or newer")
                && reason.contains(&format!("target SolidCraft {target}")),
            "{reason}"
        );
    }
}

#[test]
fn curved_hole_entries_require_replay_even_when_the_plane_is_known() {
    let (_, report) = convert(
        &curved_hole(false, directed([7.0, 5.0, 10.0], [0.0, 0.0, -1.0])),
        &SolidCraftWriter::new()
            .verification(Verification::Off)
            .replay(Verification::Off)
            .target_version(Some("0.40.0".parse().unwrap())),
    );
    let reason = report.outcome().unwrap().features[3]
        .reason
        .as_deref()
        .unwrap();
    assert!(
        reason.contains("SolidCraft's replay") && reason.contains("verification turned off"),
        "{reason}"
    );
}

/// Circular entry boundaries need replay support from SolidCraft 0.40 or newer.
fn curved_hole_cli() -> Option<PathBuf> {
    let cli = solidcraft()?;
    let version = std::process::Command::new(&cli)
        .arg("--version")
        .output()
        .unwrap();
    let text = String::from_utf8(version.stdout).unwrap();
    let version: dutchman_solidcraft::SolidCraftVersion =
        text.split_whitespace().last().unwrap().parse().unwrap();
    if version < "0.40.0".parse().unwrap() {
        eprintln!("skipped: circular entry boundary replay needs SolidCraft 0.40.0 or newer");
        return None;
    }
    Some(cli)
}

#[test]
fn solidcraft_replays_curved_hole_entries_after_a_vertical_fillet_or_through_hole() {
    let Some(cli) = curved_hole_cli() else { return };
    for rounded in [true, false] {
        for placement in [
            directed([7.0, 5.0, 10.0], [0.0, 0.0, -1.0]),
            axis_through([7.0, 5.0, 12.0], [0.0, 0.0, 1.0]),
            axis_through([7.0, 5.0, 12.0], [0.0, 0.0, -1.0]),
        ] {
            let (bytes, report) = written_with(&curved_hole(rounded, placement), &replaying(&cli));
            assert!(
                history_line(&report).starts_with("4 of 4 features rebuilt"),
                "{rounded}: {}",
                history_line(&report)
            );
            let got = volumes(&cli, &bytes);
            let earlier_cut = if rounded {
                4.0 - std::f64::consts::PI
            } else {
                std::f64::consts::PI
            };
            let want = 1000.0 - (earlier_cut + std::f64::consts::PI) * 10.0;
            assert_eq!(got.len(), 1);
            assert!((got[0] - want).abs() < 1e-3, "{rounded}: {got:?} vs {want}");
        }
    }
}

#[test]
fn solidcraft_replays_a_later_curved_hole_after_signing_a_bottom_entry_axis() {
    let Some(cli) = curved_hole_cli() else { return };
    let model = curved_hole(false, axis_through([7.0, 5.0, -2.0], [0.0, 0.0, 1.0]));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // Same source boundary orientation as the top, but on the bottom plane: replay
    // has to turn the unsigned axis inward before proving the later top face.
    for row in wire["model"]["feature_input_topologies"][0]["edge_geometry"]
        .as_array_mut()
        .unwrap()
    {
        for key in ["start", "interior", "end"] {
            row[key]["z"] = json!(0.0);
        }
    }
    let mut later = wire["model"]["features"][3].clone();
    later["id"] = json!("f3d:model:feature#5");
    later["ordinal"] = json!(4);
    later["name"] = json!("Third hole");
    later["dependencies"] = json!(["f3d:model:feature#4"]);
    let op = &mut later["definition"]["value"]["value"];
    op["placements"] = json!([directed([5.0, 8.0, 10.0], [0.0, 0.0, -1.0])]);
    op["face"]["value"]["state"] = json!("test:hole:feature-input#5");
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(later);
    let mut state = wire["model"]["feature_input_topologies"][0].clone();
    state["id"] = json!("test:hole:feature-input#5");
    state["input_of"] = json!("f3d:model:feature#5");
    for row in state["edge_geometry"].as_array_mut().unwrap() {
        for key in ["start", "interior", "end"] {
            row[key]["z"] = json!(10.0);
        }
    }
    let mut rim = state["edge_geometry"][4].clone();
    rim["edge"] = json!("test:curved-hole:edge#5");
    for key in ["start", "interior", "end"] {
        rim[key]["x"] = json!(rim[key]["x"].as_f64().unwrap() + 4.0);
    }
    state["edge_geometry"].as_array_mut().unwrap().push(rim);
    wire["model"]["feature_input_topologies"]
        .as_array_mut()
        .unwrap()
        .push(state);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (bytes, report) = written_with(&model, &replaying(&cli));
    assert!(
        history_line(&report).starts_with("5 of 5 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let got = volumes(&cli, &bytes);
    assert_eq!(got.len(), 1);
    let want = 1000.0 - 30.0 * std::f64::consts::PI;
    assert!((got[0] - want).abs() < 1e-3, "{got:?} vs {want}");
}

#[test]
fn solidcraft_refuses_curved_hole_entries_with_wrong_side_or_unproven_boundaries() {
    let Some(cli) = curved_hole_cli() else { return };
    let model = curved_hole(false, directed([7.0, 5.0, 10.0], [0.0, 0.0, 1.0]));
    let (_, report) = written_with(&model, &replaying(&cli));
    assert!(
        history_line(&report).contains("does not drill into its entry face"),
        "{}",
        history_line(&report)
    );
    // The circle has another centre/radius, so it cannot prove the earlier hole's rim.
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][3]["definition"]["value"]["value"]["placements"] =
        json!([directed([7.0, 5.0, 10.0], [0.0, 0.0, -1.0])]);
    wire["model"]["feature_input_topologies"][0]["edge_geometry"][4]["interior"]["x"] = json!(1.0);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (_, report) = written_with(&model, &replaying(&cli));
    assert!(
        history_line(&report).contains("not among the faces SolidCraft rebuilds before it"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn a_radial_hole_on_a_historical_cylinder_starts_where_its_placement_says() {
    let hole = mapped_hole(&historical_hole(
        true,
        directed([7.0, 5.0, 5.0], [-1.0, 0.0, 0.0]),
    ))
    .unwrap();
    assert_eq!(hole["position"], json!([7.0, 5.0, 5.0]));
    assert_eq!(hole["direction"], json!([-1.0, 0.0, 0.0]));
    assert_eq!(hole["diameter"], "4 mm");
    // An axis meets the cylinder twice: the end nearer its origin is the entry.
    for (origin, position, direction) in [
        ([9.0, 5.0, 5.0], [7.0, 5.0, 5.0], [-1.0, 0.0, 0.0]),
        ([1.0, 5.0, 5.0], [3.0, 5.0, 5.0], [1.0, 0.0, 0.0]),
    ] {
        let hole = mapped_hole(&historical_hole(
            true,
            axis_through(origin, [1.0, 0.0, 0.0]),
        ))
        .unwrap();
        assert_eq!(hole["position"], json!(position));
        assert_eq!(hole["direction"], json!(direction));
    }
}

#[test]
fn a_hole_that_is_not_along_a_radius_of_its_historical_cylinder_says_so() {
    for (placement, reason) in [
        (
            directed([7.0, 5.0, 5.0], [0.0, 1.0, 0.0]),
            "along a radius into its entry cylinder",
        ),
        (
            directed([7.0, 5.0, 5.0], [1.0, 0.0, 0.0]),
            "along a radius into its entry cylinder",
        ),
        (
            directed([8.0, 5.0, 5.0], [-1.0, 0.0, 0.0]),
            "along a radius into its entry cylinder",
        ),
        (
            directed([7.0, 5.0, 12.0], [-1.0, 0.0, 0.0]),
            "along a radius into its entry cylinder",
        ),
        (
            axis_through([5.0, 5.0, 5.0], [1.0, 0.0, 0.0]),
            "does not choose between them",
        ),
        (
            axis_through([9.0, 5.0, 5.0], [0.0, 1.0, 0.0]),
            "does not cross the axis of its entry cylinder",
        ),
        (
            axis_through([9.0, 5.0, 5.0], [0.0, 0.0, 1.0]),
            "not along a radius of its entry cylinder",
        ),
    ] {
        let why = mapped_hole(&historical_hole(true, placement)).unwrap_err();
        assert!(why.contains(reason), "{why}");
    }
}

#[test]
fn solidcraft_drills_a_radial_hole_into_a_cylinder() {
    let Some(cli) = solidcraft() else { return };
    let dir = tempfile::tempdir().unwrap();
    let script = json!([
        {"command": "solid.cylinder", "params": {"radius": "10 mm", "height": "20 mm"}},
        {"command": "solid.hole", "params": {"position": [10, 0, 10], "direction": [-1, 0, 0],
            "diameter": "4 mm"}},
    ]);
    let input = dir.path().join("radial.json");
    std::fs::write(&input, script.to_string()).unwrap();
    let measured = blend_measurement(&cli, &input, &dir.path().join("eval.json"));
    let volume = measured["volume_mm3"].as_f64().unwrap();
    let whole = std::f64::consts::PI * 100.0 * 20.0;
    // Two bores of radius 2 through the wall, a little under a straight 20 mm bore.
    let bored = whole - volume;
    assert!(
        bored > 200.0 && bored < std::f64::consts::PI * 4.0 * 20.0,
        "{bored}"
    );
}

#[test]
fn a_hole_can_select_sketch_points_or_circle_centres() {
    for point in [false, true] {
        let mut model = holed_cube(4.0, |hole| {
            hole["profile"] = json!({"kind": "sketch_entities", "value": {
                "sketch": HOLES, "entities": ["f3d:model:sketch-entity#9"]}});
            hole["profile_filter"] = json!(if point { "points" } else { "circles" });
        });
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let entities = wire["model"]["sketch_entities"].as_array_mut().unwrap();
        if point {
            entities.last_mut().unwrap()["geometry"] =
                json!({"kind": "point", "position": {"u": 5.0, "v": 5.0}});
        }
        // An unselected entity of the same family must not create a second hole.
        entities.push(json!({"id": "f3d:model:sketch-entity#10", "sketch": HOLES, "construction": false, "geometry": if point {
                json!({"kind": "point", "position": {"u": 3.0, "v": 3.0}})
            } else {
                json!({"kind": "circle", "center": {"u": 3.0, "v": 3.0}, "radius": 1.0})
            }, "endpoint_refs": []}));
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("4 of 4 features rebuilt"),
            "{}",
            history_line(&report)
        );
        let hole = &document["features"][3];
        assert_eq!(hole["position"], json!([5.0, 5.0, 10.0]));
        assert_eq!(hole["points"]["ids"].as_array().unwrap().len(), 1);
    }
}

#[test]
fn a_drilled_form_keeps_its_included_angle_and_shoulder_depth() {
    let (document, report) = convert(
        &holed_cube(4.0, |hole| {
            hole["shape"]["construction"]["value"]["kind"] = json!({"kind": "simple_drilled",
                "drill_point_angle": std::f64::consts::FRAC_PI_2});
            hole["extent"] = json!({"kind": "blind", "length": 6.0});
        }),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("4 of 4 features rebuilt"),
        "{}",
        history_line(&report)
    );
    assert_eq!(
        document["features"][3]["hole"],
        json!({"type": "drilled", "tip_angle": "90"})
    );
    assert_eq!(document["features"][3]["depth"], "6 mm");
}

fn cosmetic_hole_thread() -> Value {
    json!({"kind": "threaded", "standard": "ISO", "designation": "M6",
        "modeled": false, "cosmetic": true, "hand": "right", "depth": {"kind": "hole_depth"}})
}

#[test]
fn cosmetic_hole_threads_keep_the_designation_and_resolved_bore() {
    for blind in [false, true] {
        let model = holed_cube(4.0, |hole| {
            hole["shape"]["construction"]["value"]["specification"] = cosmetic_hole_thread();
            if blind {
                hole["extent"] = json!({"kind": "blind", "length": 6.0});
            }
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("4 of 4 features rebuilt"),
            "{}",
            history_line(&report)
        );
        let hole = &document["features"][3];
        assert_eq!(hole["thread"], "M6x1");
        assert_eq!(hole["diameter"], "4 mm");
        assert_eq!(hole["hole"], json!({"type": "simple"}));
        assert_eq!(hole["points"]["sketch"], document["features"][2]["id"]);
        if blind {
            assert_eq!(hole["depth"], "6 mm");
        } else {
            assert!(hole.get("depth").is_none());
        }
    }
}

#[test]
fn solidcraft_keeps_the_bore_of_a_cosmetic_tapped_hole() {
    let Some(cli) = solidcraft() else { return };
    for depth in [None, Some(6.0)] {
        let model = holed_cube(4.0, |hole| {
            hole["shape"]["construction"]["value"]["specification"] = cosmetic_hole_thread();
            if let Some(depth) = depth {
                hole["extent"] = json!({"kind": "blind", "length": depth});
            }
        });
        let got = volumes(&cli, &written(&model).0);
        let want = 1000.0 - std::f64::consts::PI * 4.0 * depth.unwrap_or(10.0);
        assert_eq!(got.len(), 1, "{got:?}");
        assert!((got[0] - want).abs() / want < 1e-3, "{got:?} vs {want}");
    }
}

#[test]
fn hole_thread_details_solidcraft_lacks_are_reported_not_blocking() {
    let converted = |change: Value, target: &str, blind: Option<f64>| {
        let model = holed_cube(4.0, |hole| {
            let mut specification = cosmetic_hole_thread();
            specification
                .as_object_mut()
                .unwrap()
                .extend(change.as_object().unwrap().clone());
            specification
                .as_object_mut()
                .unwrap()
                .retain(|_, v| !v.is_null());
            hole["shape"]["construction"]["value"]["specification"] = specification;
            if let Some(length) = blind {
                hole["extent"] = json!({"kind": "blind", "length": length});
            }
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new()
                .verification(Verification::Off)
                .target_version(Some(target.parse().unwrap())),
        );
        assert!(
            history_line(&report).starts_with("4 of 4 features rebuilt"),
            "{change}: {}",
            history_line(&report)
        );
        let note = report
            .losses()
            .iter()
            .find(|loss| loss.detail.contains("thread"))
            .map(|loss| loss.detail.clone())
            .unwrap_or_default();
        (document["features"][3].clone(), note)
    };
    let thread_note = |change: Value| {
        let (hole, note) = converted(change, "0.43.0", None);
        (hole["thread"].clone(), note)
    };
    let details = json!({"hand": "left", "class": "6H", "depth": {"kind": "blind", "depth": 3.0}});
    // Older targets keep the label and name everything else.
    for (target, blind) in [("0.30.1", None), ("0.30.1", Some(6.0))] {
        let (hole, note) = converted(details.clone(), target, blind);
        assert_eq!(hole["thread"], "M6x1");
        for key in ["hand", "thread_class", "thread_depth"] {
            assert!(hole.get(key).is_none(), "{target}: {key}");
        }
        assert_eq!(
            note,
            "its thread is kept as a cosmetic M6x1 label over the whole hole, without its \
             left-hand direction, its 6H tolerance class, and its 3 mm thread length"
        );
    }
    // 0.40 stores the hand and, on a blind plain hole, the thread length.
    let (hole, note) = converted(details.clone(), "0.40.0", Some(6.0));
    assert_eq!(hole["hand"], "left");
    assert_eq!(hole["thread_depth"], "3 mm");
    assert!(hole.get("thread_class").is_none());
    assert_eq!(
        note,
        "its thread is kept as a cosmetic M6x1 label, without its 6H tolerance class"
    );
    // 0.43 also stores the class; a through hole's wall is unknown, so its length is named.
    let (hole, note) = converted(details.clone(), "0.43.0", Some(6.0));
    assert_eq!(hole["thread_class"], "6H");
    assert_eq!(hole["thread_depth"], "3 mm");
    assert!(note.is_empty(), "{note}");
    let (hole, note) = converted(details.clone(), "0.43.0", None);
    assert_eq!(hole["hand"], "left");
    assert!(hole.get("thread_depth").is_none());
    assert_eq!(
        note,
        "its thread is kept as a cosmetic M6x1 label over the whole hole, without its 3 mm \
         thread length"
    );
    // A thread as long as the wall is the default full-length thread.
    let (hole, note) = converted(
        json!({"depth": {"kind": "blind", "depth": 6.0}}),
        "0.43.0",
        Some(6.0),
    );
    assert!(hole.get("thread_depth").is_none());
    assert!(note.is_empty(), "{note}");
    // A class SolidCraft would refuse stays a named loss.
    let (hole, note) = converted(json!({"class": "6g"}), "0.43.0", None);
    assert!(hole.get("thread_class").is_none());
    assert!(note.contains("its 6g tolerance class"), "{note}");
    let (label, note) = thread_note(json!({"designation": "M5", "pitch": 0.5}));
    assert_eq!(label, "M5x0.5");
    assert!(note.is_empty(), "{note}");
    let (label, _) = thread_note(json!({"designation": "M6 x 0.75-6H"}));
    assert_eq!(label, "M6x0.75");
    for (change, why) in [
        (
            json!({"designation": "1/4-20 UNC"}),
            "ISO metric threads only",
        ),
        (json!({"designation": "M12"}), "does not fit its 4 mm bore"),
        (json!({"designation": null}), "its thread size is not given"),
    ] {
        let (label, note) = thread_note(change);
        assert!(label.is_null(), "{label}");
        assert!(
            note.contains(why) && note.contains("the bore is kept"),
            "{note}"
        );
    }
    let model = holed_cube(4.0, |hole| {
        let mut specification = cosmetic_hole_thread();
        specification["modeled"] = json!(true);
        hole["shape"]["construction"]["value"]["specification"] = specification;
    });
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).contains("a hole with a modeled thread"));
}

#[test]
fn unsupported_hole_forms_have_specific_reasons() {
    for (form, reason) in [
        (
            json!({"kind": "unresolved", "form": "chamfer"}),
            "its hole entry form or dimensions are not resolved",
        ),
        (
            json!({"kind": "partial_counterbore", "dimension": {"kind": "diameter", "diameter": 8.0}}),
            "its counterbore diameter or depth is not resolved",
        ),
        (
            json!({"kind": "partial_countersink", "dimension": {"kind": "angle", "angle": 1.5}}),
            "its countersink diameter or angle is not resolved",
        ),
        (
            json!({"kind": "counterdrill", "diameter": 8.0, "entry_diameter": 9.0, "depth": 2.0, "angle": 1.5}),
            "a counterdrill with a conical entry and cylindrical recess is not mapped yet",
        ),
    ] {
        let model = holed_cube(4.0, |hole| {
            hole["shape"]["construction"]["value"]["kind"] = form.clone()
        });
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
    for kind in ["counterbore", "countersink", "chamfer"] {
        for blind in [false, true] {
            let model = holed_cube(4.0, |hole| {
                hole["shape"]["construction"]["value"]["kind"] = if kind == "counterbore" {
                    json!({"kind": kind, "diameter": 8.0, "depth": 2.0})
                } else {
                    json!({"kind": kind, "diameter": 8.0, "angle": 1.5})
                };
                if blind {
                    hole["extent"] = json!({"kind": "blind", "length": 6.0});
                }
                hole["bottom"] =
                    json!({"kind": "angled", "included_angle": 1.5, "depth_to_tip": false});
            });
            // SolidCraft before 0.22.1 drills entry-recess holes flat: refused.
            let (_, report) = convert(
                &model,
                &SolidCraftWriter::new()
                    .verification(Verification::Off)
                    .target_version(Some("0.22.0".parse().unwrap())),
            );
            assert_eq!(
                history_line(&report)
                    .contains("SolidCraft holes take a drill point only on a plain drilled hole"),
                blind,
                "{report:?}"
            );
            // SolidCraft 0.22.1 and later keep the entry form and its drill point. Name the
            // version: the default target follows the installed SolidCraft, which can be older.
            let (_, report) = convert(
                &model,
                &SolidCraftWriter::new()
                    .verification(Verification::Off)
                    .target_version(Some("0.22.1".parse().unwrap())),
            );
            assert!(
                !history_line(&report).contains("drill point"),
                "{}",
                history_line(&report)
            );
        }
    }
}

#[test]
fn unsupported_hole_ends_have_specific_reasons() {
    for (extent, reason) in [
        (
            json!({"kind": "unresolved"}),
            "its hole termination is not resolved",
        ),
        (
            json!({"kind": "through_next"}),
            "a hole through the next material region",
        ),
        (
            json!({"kind": "to_first"}),
            "a hole ending at the first encountered face",
        ),
        (
            json!({"kind": "to_last"}),
            "a hole ending at the last encountered face",
        ),
        (
            json!({"kind": "to_vertex", "vertex": {"kind": "unresolved"}}),
            "a hole ending at a source vertex",
        ),
        (
            json!({"kind": "offset_from_face", "offset": 1.0,
            "face": {"kind": "faces", "value": ["synthetic:cube:face#bottom"]}}),
            "a hole that ends offset from a face",
        ),
        (
            json!({"kind": "to_shape", "target": {"kind": "native", "value": "body"}}),
            "a hole ending at a selected shape",
        ),
    ] {
        let model = holed_cube(4.0, |hole| hole["extent"] = extent.clone());
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn native_threads_and_exit_treatments_are_refused() {
    for (change, reason) in [
        (
            json!({"shape": {"diameter": 4.0, "construction": {"kind": "native_thread", "value": {"major_diameter": 6.0, "thread_depth": 6.0, "pitch": 1.0, "drill_point_angle": 1.5}}}}),
            "a hole with native thread geometry is not mapped yet",
        ),
        (
            json!({"shape": {"diameter": 4.0, "construction": {"kind": "form", "value": {"kind": {"kind": "simple"}}}, "exit_kind": {"kind": "chamfer", "diameter": 6.0,
                "angle": 1.5}}}),
            "a hole with an exit treatment is not mapped yet",
        ),
    ] {
        let model = holed_cube(4.0, |hole| {
            hole.as_object_mut()
                .unwrap()
                .extend(change.as_object().unwrap().clone());
        });
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn holes_refuse_unproven_faces_and_unsupported_geometry() {
    for (change, reason) in [
        (
            json!({"taper_angle": 0.1}),
            "a tapered hole is not mapped yet",
        ),
        (
            json!({"shape": {"diameter": 4.0, "construction": {"kind": "form", "value": {"kind": {"kind": "simple"}, "specification": {"kind": "threaded", "standard": "ISO",
                "modeled": true, "cosmetic": false, "hand": "right", "depth": {"kind": "hole_depth"}}}}}}),
            "a hole with a modeled thread is not mapped yet",
        ),
        (
            json!({"placements": [{"kind": "axis", "origin": {"x": 5.0, "y": 5.0, "z": 6.0},
            "axis": {"x": 0.0, "y": 0.0, "z": 1.0}}]}),
            "its face is not resolved",
        ),
        (
            json!({"extent": {"kind": "to_face", "face": {"kind": "native", "value": "face"}}}),
            "its face has no resolved geometry",
        ),
        (
            json!({"extent": {"kind": "to_face", "offset": 1.0,
            "face": {"kind": "faces", "value": ["synthetic:cube:face#bottom"]}}}),
            "a hole that ends offset from a face is not mapped yet",
        ),
    ] {
        let model = holed_cube(4.0, |hole| {
            hole.as_object_mut()
                .unwrap()
                .extend(change.as_object().unwrap().clone());
        });
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
    // The saved top is at 10 mm, while the only earlier solid stops at 8 mm.
    // Neither an entry nor an end can use it merely because it is in the final B-rep.
    for entry in [true, false] {
        let mut model = holed_cube(4.0, |hole| {
            if entry {
                axis_hole(hole, 1.0);
            } else {
                directed_hole(hole);
                hole["placements"][0]["position"]["z"] = json!(0.0);
                hole["placements"][0]["direction"]["z"] = json!(1.0);
                hole["extent"] = json!({"kind": "to_face", "face": {
                    "kind": "faces", "value": ["synthetic:cube:face#top"]}});
            }
        });
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["features"][1]["definition"]["value"]["value"]["extent"]["side"]
            ["termination"]["length"] = json!(8.0);
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains("its face is not proven to exist before the hole"),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn solidcraft_drills_spatial_and_to_face_holes_and_drilled_forms() {
    let Some(cli) = solidcraft() else { return };
    let bore = std::f64::consts::PI * 4.0;
    for (model, removed) in [
        (holed_cube(4.0, directed_hole), bore * 10.0),
        (holed_cube(4.0, |hole| axis_hole(hole, 1.0)), bore * 10.0),
        (holed_cube(4.0, to_face_hole), bore * 10.0),
        (
            holed_cube(4.0, |hole| {
                hole["profile"] = json!({"kind": "sketch_entities", "value": {
                "sketch": HOLES, "entities": ["f3d:model:sketch-entity#9"]}});
            }),
            bore * 10.0,
        ),
        (
            holed_cube(4.0, |hole| {
                hole["shape"]["construction"]["value"]["kind"] = json!({"kind": "simple_drilled",
                "drill_point_angle": std::f64::consts::FRAC_PI_2});
                hole["extent"] = json!({"kind": "blind", "length": 6.0});
            }),
            bore * (6.0 + 2.0 / 3.0),
        ),
    ] {
        let got = volumes(&cli, &written(&model).0);
        let want = 1000.0 - removed;
        assert_eq!(got.len(), 1, "{got:?}");
        assert!((got[0] - want).abs() / want < 1e-3, "{got:?} vs {want}");
    }
}

/// The example cube's geometry with a right-triangle history: a fixed 40 mm horizontal base,
/// a vertical side, and the hypotenuse at `theta` from the base (shown as 36.87°), extruded by
/// 10 mm.
fn triangle_history() -> Model {
    let mut model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketches"][0]["profiles"] = json!([]);
    m["sketch_entities"] = json!([
        line(1, [0.0, 0.0], [40.0, 0.0]),
        line(2, [0.0, 0.0], [40.0, 30.0]),
        line(3, [40.0, 0.0], [40.0, 30.0]),
    ]);
    let constraint = |key: u32, definition: Value| {
        json!({"id": format!("f3d:model:sketch-constraint#{key}"), "sketch": SKETCH,
               "definition": definition})
    };
    let start = json!({"kind": "start", "value": "f3d:model:sketch-entity#1"});
    m["sketch_constraints"] = json!([
        constraint(
            1,
            json!({"kind": "coincident_loci",
            "loci": [start, {"kind": "start", "value": "f3d:model:sketch-entity#2"}]})
        ),
        constraint(
            2,
            json!({"kind": "horizontal", "entity": "f3d:model:sketch-entity#1"})
        ),
        constraint(
            3,
            json!({"kind": "vertical", "entity": "f3d:model:sketch-entity#3"})
        ),
        constraint(
            4,
            json!({"kind": "distance", "entities": ["f3d:model:sketch-entity#1"],
                              "parameter": "f3d:model:parameter#2"})
        ),
        constraint(
            5,
            json!({"kind": "angle", "first": "f3d:model:sketch-entity#1",
                              "second": "f3d:model:sketch-entity#2",
                              "parameter": "f3d:model:parameter#3"})
        ),
    ]);
    m["parameters"][1]["expression"] = json!("40 mm");
    m["parameters"][1]["value"] = json!({"kind": "length", "value": 40.0});
    let theta = 30.0_f64.atan2(40.0);
    m["parameters"].as_array_mut().unwrap().push(json!({"id": "f3d:model:parameter#3", "owner": "f3d:model:feature#1", "ordinal": 2, "name": "theta", "expression": format!("{} deg", theta.to_degrees()), "value": {"kind": "angle", "value": theta}, "dependencies": [], "properties": {}}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_converted_angle_drives_the_geometry_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let (bytes, report) = written(&triangle_history());
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("constraints not carried")),
        "{:?}",
        report.losses()
    );
    let close = |got: Vec<f64>, want: f64| {
        assert_eq!(got.len(), 1, "{got:?}");
        assert!((got[0] - want).abs() < 1e-6, "{got:?} vs {want}");
    };
    close(volumes(&cli, &bytes), 6000.0);
    close(
        volumes(&cli, &with_param(&bytes, "theta", "45 deg")),
        8000.0,
    );
}

/// A square crossed by both diagonals, whose source loop retraces those diagonals,
/// beside a second square which the extrude deliberately leaves out.
fn crossed_profile_history(region: bool) -> Model {
    let profile = if region {
        json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
            "regions": [{"kind": "loops", "outer": 0, "holes": []}]}})
    } else {
        first_profile()
    };
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    let member = |key: u32, reversed: bool| json!({"entity": format!("f3d:model:sketch-entity#{key}"), "reversed": reversed});
    m["sketches"][0]["profiles"] = json!([
        [
            member(1, false),
            member(5, false),
            member(5, true),
            member(2, false),
            member(3, false),
            member(4, false),
            member(6, false),
            member(6, true)
        ],
        [
            member(7, false),
            member(8, false),
            member(9, false),
            member(10, false)
        ],
    ]);
    m["sketch_entities"].as_array_mut().unwrap().extend([
        line(5, [10.0, 0.0], [0.0, 10.0]),
        line(6, [0.0, 0.0], [10.0, 10.0]),
        line(7, [20.0, 0.0], [30.0, 0.0]),
        line(8, [30.0, 0.0], [30.0, 10.0]),
        line(9, [30.0, 10.0], [20.0, 10.0]),
        line(10, [20.0, 10.0], [20.0, 0.0]),
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_selected_crossed_loop_maps_to_its_atomic_regions() {
    for region in [false, true] {
        let model = crossed_profile_history(region);
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
        let selection = &document["features"][1]["profiles"];
        assert_eq!(selection["type"], "curves");
        let mut loops: Vec<Vec<String>> =
            serde_json::from_value(selection["loops"].clone()).unwrap();
        for ring in &mut loops {
            ring.sort();
        }
        loops.sort();
        assert_eq!(
            loops,
            vec![
                vec!["l1", "l5", "l6"],
                vec!["l2", "l5", "l6"],
                vec!["l3", "l5", "l6"],
                vec!["l4", "l5", "l6"],
            ]
        );
        // The source curves and their ids survive: constraints and later references
        // still address the same geometry. SolidCraft splits their pieces on replay.
        let curves = document["features"][0]["sketch"]["curves"]
            .as_array()
            .unwrap();
        assert_eq!(curves.len(), 10);
        // The separate square (curves 7 to 10) bounds nothing the extrude takes, whether it
        // selects a region or a list of loops, so it is construction; the crossed loop's
        // curves stay solid.
        for curve in curves {
            let id = curve["id"].as_str().unwrap();
            let unused = ["l7", "l8", "l9", "l10"].contains(&id);
            assert_eq!(curve["construction"] == true, unused, "{region} {id}");
        }
        assert!(!document["features"][0]["sketch"]["constraints"]
            .as_array()
            .unwrap()
            .is_empty());
    }
}

#[test]
fn solidcraft_replays_only_the_selected_crossed_region() {
    let Some(cli) = solidcraft() else { return };
    for region in [false, true] {
        let volume = volumes(&cli, &written(&crossed_profile_history(region)).0);
        assert_eq!(volume.len(), 1);
        assert!((volume[0] - 1000.0).abs() < 1e-6, "{volume:?}");
    }
}

/// The 10 mm square loop (curves 1 to 4) extruded as region `{outer: 0}`, with curves no loop
/// uses: a line through the square past both sides, and an open segment inside it.
fn stray_curve_history(change: impl FnOnce(&mut Value)) -> Model {
    let profile = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
        "regions": [{"kind": "loops", "outer": 0, "holes": []}]}});
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .extend([
            line(5, [-5.0, 5.0], [15.0, 5.0]),
            line(6, [2.0, 2.0], [4.0, 3.0]),
        ]);
    change(&mut wire["model"]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn construction_flags(document: &Value) -> Vec<(String, bool)> {
    document["features"][0]["sketch"]["curves"]
        .as_array()
        .unwrap()
        .iter()
        .map(|curve| {
            (
                curve["id"].as_str().unwrap().to_owned(),
                curve["construction"] == true,
            )
        })
        .collect()
}

#[test]
fn curves_bounding_no_profile_a_feature_uses_are_written_as_construction() {
    let (document, report) = convert(
        &stray_curve_history(|_| {}),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    let flags = construction_flags(&document);
    assert_eq!(flags.len(), 6);
    assert_eq!(
        flags
            .iter()
            .filter(|(_, construction)| *construction)
            .count(),
        2
    );
    for (id, construction) in &flags {
        assert_eq!(*construction, id == "l5" || id == "l6", "{id}");
    }
    assert!(
        report.losses().iter().any(|loss| loss.detail
            == "2 sketch curves bounding no profile a feature uses written as construction"),
        "{report:?}"
    );
    // With the strays construction, the square's four curves are all the sketch has.
    assert_eq!(document["features"][1]["profiles"]["type"], "all");
}

#[test]
fn solidcraft_replays_a_region_beside_curves_no_loop_uses() {
    let Some(cli) = solidcraft() else { return };
    let volume = volumes(&cli, &written(&stray_curve_history(|_| {})).0);
    assert_eq!(volume.len(), 1);
    assert!((volume[0] - 1000.0).abs() < 1e-6, "{volume:?}");
    // An open segment alone, inside the region.
    let only_open = stray_curve_history(|model| {
        model["sketch_entities"].as_array_mut().unwrap().remove(4);
    });
    let volume = volumes(&cli, &written(&only_open).0);
    assert_eq!(volume.len(), 1);
    assert!((volume[0] - 1000.0).abs() < 1e-6, "{volume:?}");
}

/// One consumer takes the square with its circular hole; a later cut takes the strip
/// left of a divider. Both selections must keep their own complete source region.
fn whole_and_trimmed_history() -> Model {
    let profile = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
        "regions": [{"kind": "loops", "outer": 0, "holes": [1]}]}});
    let mut model = cube_with_history(2.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .extend([line(5, [3.0, 0.0], [3.0, 10.0]), circle(6, [7.0, 5.0], 1.0)]);
    m["sketches"][0]["profiles"]
        .as_array_mut()
        .unwrap()
        .push(json!([
            {"entity": "f3d:model:sketch-entity#6", "reversed": false}
        ]));
    let boundary = |key: u32, range: [f64; 2]| json!({"entity": format!("f3d:model:sketch-entity#{key}"), "parameter_range": range, "reversed": false});
    let mut cut = m["features"][1].clone();
    cut["id"] = json!("synthetic:strip:feature#cut");
    cut["name"] = json!("Strip cut");
    cut["ordinal"] = json!(2);
    cut["outputs"] = json!([]);
    cut["definition"]["value"]["value"]["op"] = json!("cut");
    cut["definition"]["value"]["value"]["extent"]["side"]["termination"]["length"] = json!(1.0);
    cut["definition"]["value"]["value"]["profile"]["value"] = json!({"kind": "sketch_regions", "value": {
        "sketch": SKETCH, "regions": [{"kind": "trimmed", "outer_boundary": [
            boundary(1, [0.0, 0.3]), boundary(5, [0.0, 1.0]),
            boundary(3, [0.7, 1.0]), boundary(4, [0.0, 1.0])], "hole_boundaries": []}]}});
    m["features"].as_array_mut().unwrap().push(cut);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn whole_regions_keep_their_boundaries_beside_another_consumers_trimmed_regions() {
    let (document, report) = convert(
        &whole_and_trimmed_history(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features rebuilt"),
        "{report:?}"
    );
    let features = document["features"].as_array().unwrap();
    let sketches: Vec<_> = features.iter().filter(|f| f["type"] == "sketch").collect();
    assert_eq!(sketches.len(), 2);
    let divider = |sketch: &Value| {
        sketch["sketch"]["curves"]
            .as_array()
            .unwrap()
            .iter()
            .find(|curve| curve["id"] == "l5")
            .unwrap()["construction"]
            .clone()
    };
    assert_eq!(divider(sketches[0]), false);
    assert_eq!(divider(sketches[1]), true);
    assert_eq!(
        sketches[0]["sketch"]["points"],
        sketches[1]["sketch"]["points"]
    );
    assert_eq!(
        sketches[0]["sketch"]["constraints"],
        sketches[1]["sketch"]["constraints"]
    );
    let extrudes: Vec<_> = features.iter().filter(|f| f["type"] == "extrude").collect();
    assert_eq!(extrudes[0]["sketch"], sketches[1]["id"]);
    assert_eq!(extrudes[1]["sketch"], sketches[0]["id"]);
    assert_eq!(extrudes[0]["profiles"]["type"], "whole_boundaries");
}

#[test]
fn solidcraft_replays_whole_and_trimmed_regions_of_the_same_source_sketch() {
    let Some(cli) = solidcraft() else { return };
    let got = volumes(&cli, &written(&whole_and_trimmed_history()).0);
    assert_eq!(got.len(), 1);
    let want = 170.0 - 2.0 * std::f64::consts::PI;
    // Relative, like the other curved replays: older SolidCraft measures arcs less exactly.
    assert!((got[0] - want).abs() / want < 1e-3, "{got:?} vs {want}");
}

/// One consumer takes the square with its circular hole; a later cut takes a notch drawn from
/// the square's top edge, three lines no profile loop holds.
fn whole_and_notch_history() -> Model {
    let profile = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
        "regions": [{"kind": "loops", "outer": 0, "holes": [1]}]}});
    let mut model = cube_with_history(2.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketch_entities"].as_array_mut().unwrap().extend([
        line(5, [3.0, 10.0], [3.0, 8.0]),
        line(7, [3.0, 8.0], [5.0, 8.0]),
        line(8, [5.0, 8.0], [5.0, 10.0]),
        circle(6, [7.0, 5.0], 1.0),
    ]);
    m["sketches"][0]["profiles"]
        .as_array_mut()
        .unwrap()
        .push(json!([
            {"entity": "f3d:model:sketch-entity#6", "reversed": false}
        ]));
    let boundary = |key: u32, range: [f64; 2]| json!({"entity": format!("f3d:model:sketch-entity#{key}"), "parameter_range": range, "reversed": false});
    let mut cut = m["features"][1].clone();
    cut["id"] = json!("synthetic:notch:feature#cut");
    cut["name"] = json!("Notch cut");
    cut["ordinal"] = json!(2);
    cut["outputs"] = json!([]);
    cut["definition"]["value"]["value"]["op"] = json!("cut");
    cut["definition"]["value"]["value"]["extent"]["side"]["termination"]["length"] = json!(1.0);
    cut["definition"]["value"]["value"]["profile"]["value"] = json!({"kind": "sketch_regions", "value": {
        "sketch": SKETCH, "regions": [{"kind": "trimmed", "outer_boundary": [
            boundary(3, [0.5, 0.7]), boundary(5, [0.0, 1.0]),
            boundary(7, [0.0, 1.0]), boundary(8, [0.0, 1.0])], "hole_boundaries": []}]}});
    m["features"].as_array_mut().unwrap().push(cut);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn whole_regions_keep_their_boundaries_beside_a_notch_another_consumer_selects() {
    let (document, report) = convert(
        &whole_and_notch_history(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features rebuilt"),
        "{report:?}"
    );
    let features = document["features"].as_array().unwrap();
    let sketches: Vec<_> = features.iter().filter(|f| f["type"] == "sketch").collect();
    assert_eq!(sketches.len(), 2);
    let construction = |sketch: &Value| {
        sketch["sketch"]["curves"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|curve| curve["construction"] == true)
            .count()
    };
    assert_eq!(construction(sketches[0]), 0);
    assert_eq!(construction(sketches[1]), 3);
    let extrudes: Vec<_> = features.iter().filter(|f| f["type"] == "extrude").collect();
    assert_eq!(extrudes[0]["sketch"], sketches[1]["id"]);
    assert_eq!(extrudes[1]["sketch"], sketches[0]["id"]);
    assert_eq!(extrudes[0]["profiles"]["type"], "whole_boundaries");
}

#[test]
fn solidcraft_replays_a_whole_region_beside_a_notch_of_the_same_source_sketch() {
    let Some(cli) = solidcraft() else { return };
    let got = volumes(&cli, &written(&whole_and_notch_history()).0);
    assert_eq!(got.len(), 1);
    let want = 196.0 - 2.0 * std::f64::consts::PI;
    assert!((got[0] - want).abs() / want < 1e-3, "{got:?} vs {want}");
}

/// Synthetic belt-tooth gap beside a normal region. The normal point must retain its
/// original bytes even when the other region needs the refined search.
fn narrow_gap_history(width: f64) -> Model {
    let boundary = |key: u32| {
        json!({"entity": format!("f3d:model:sketch-entity#{key}"),
        "parameter_range": [0.0, 1.0], "reversed": false})
    };
    let profile = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
    "regions": [
        {"kind": "trimmed", "outer_boundary": [boundary(1), boundary(2), boundary(3), boundary(4)], "hole_boundaries": []},
        {"kind": "trimmed", "outer_boundary": [boundary(5), boundary(6), boundary(7), boundary(8)], "hole_boundaries": []}
    ]}});
    let mut model = cube_with_history(2.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .extend([
            line(5, [20.0, 0.0], [20.0 + width, 0.0]),
            line(6, [20.0 + width, 0.0], [20.0 + width, 10.0]),
            line(7, [20.0 + width, 10.0], [20.0, 10.0]),
            line(8, [20.0, 10.0], [20.0, 0.0]),
        ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn trimmed_belt_tooth_gap_maps_without_changing_existing_point_bytes() {
    let writer = || SolidCraftWriter::new().verification(Verification::Off);
    let (wide, wide_report) = convert(&narrow_gap_history(1.0), &writer());
    let (narrow, narrow_report) = convert(&narrow_gap_history(0.0005), &writer());
    for report in [wide_report, narrow_report] {
        assert!(
            history_line(&report).starts_with("2 of 2 features rebuilt"),
            "{report:?}"
        );
    }
    let points = &narrow["features"][1]["profiles"]["points"];
    assert_eq!(points.as_array().unwrap().len(), 2);
    assert_eq!(
        serde_json::to_vec(&points[0]).unwrap(),
        serde_json::to_vec(&wide["features"][1]["profiles"]["points"][0]).unwrap()
    );
    let point = points[1].as_array().unwrap();
    let (x, y) = (point[0].as_f64().unwrap(), point[1].as_f64().unwrap());
    assert!(x > 20.0 && x < 20.0005 && y > 0.0 && y < 10.0, "{point:?}");
}

fn narrow_annulus_history(width: f64) -> Model {
    let boundary = |key: u32| {
        json!({"entity": format!("f3d:model:sketch-entity#{key}"),
        "parameter_range": [0.0, std::f64::consts::TAU], "reversed": false})
    };
    let profile = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
    "regions": [
        {"kind": "trimmed", "outer_boundary": [boundary(5)], "hole_boundaries": [[boundary(6)]]},
        {"kind": "trimmed", "outer_boundary": [boundary(6)], "hole_boundaries": []}
    ]}});
    let mut model = cube_with_history(2.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .extend([
            circle(5, [20.0, 0.0], 1.0),
            circle(6, [20.0, 0.0], 1.0 - width),
        ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn trimmed_narrow_annulus_and_its_disc_select_exactly_their_own_interiors() {
    let (document, report) = convert(
        &narrow_annulus_history(0.0005),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{report:?}"
    );
    let points = document["features"][1]["profiles"]["points"]
        .as_array()
        .unwrap();
    assert_eq!(points.len(), 2);
    let radius =
        |point: &Value| (point[0].as_f64().unwrap() - 20.0).hypot(point[1].as_f64().unwrap());
    assert!(
        radius(&points[0]) > 0.9995 && radius(&points[0]) < 1.0,
        "{points:?}"
    );
    assert!(radius(&points[1]) < 0.9995, "{points:?}");
}

/// Synthetic construction point precedes both circles, so their written centres merge
/// to it even though the source annulus stays centred at (20, 0).
fn merged_centre_annulus_history(width: f64) -> Model {
    let mut model = narrow_annulus_history(width);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let entities = wire["model"]["sketch_entities"].as_array_mut().unwrap();
    entities.retain(|e| {
        e["id"] != "f3d:model:sketch-entity#5" && e["id"] != "f3d:model:sketch-entity#6"
    });
    let mut point = entity(
        7,
        json!({"kind": "point",
        "position": {"u": 20.0000009, "v": 0.0}}),
    );
    point["construction"] = json!(true);
    entities.extend([
        point,
        circle(5, [20.0, 0.0], 0.01),
        circle(6, [20.0, 0.0], 0.01 - width),
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn trimmed_annulus_narrower_than_merged_boundary_displacement_is_refused() {
    let (document, report) = convert(
        &merged_centre_annulus_history(0.0000005),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        format!("{report:?}").contains("no unambiguous interior point"),
        "{report:?}"
    );
    assert!(!document["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "extrude"));
    assert_merged_circle_centres(&document);
}

fn assert_merged_circle_centres(document: &Value) {
    let sketch = &document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "sketch")
        .unwrap()["sketch"];
    let circles: Vec<_> = sketch["curves"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["type"] == "circle")
        .collect();
    assert_eq!(circles.len(), 2);
    for circle in circles {
        let index = usize::try_from(circle["c"].as_u64().unwrap()).unwrap();
        assert_eq!(sketch["points"][index]["pos"], json!([20.0000009, 0.0]));
    }
}

#[test]
fn wider_trimmed_annulus_with_merged_centres_is_inside_source_and_written_regions() {
    let (document, report) = convert(
        &merged_centre_annulus_history(0.00001),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{report:?}"
    );
    assert_merged_circle_centres(&document);
    let points = document["features"][1]["profiles"]["points"]
        .as_array()
        .unwrap();
    for center in [20.0, 20.0000009] {
        let radius = |p: &Value| (p[0].as_f64().unwrap() - center).hypot(p[1].as_f64().unwrap());
        assert!(
            radius(&points[0]) > 0.00999 && radius(&points[0]) < 0.01,
            "{points:?}"
        );
        assert!(radius(&points[1]) < 0.00999, "{points:?}");
    }
}

fn shared_circle_regions() -> Model {
    let boundary = |key: u32, range: [f64; 2]| json!({"entity": format!("f3d:model:sketch-entity#{key}"), "parameter_range": range, "reversed": false});
    let pi = std::f64::consts::PI;
    let profile = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
    "regions": [
        {"kind": "trimmed", "outer_boundary": [boundary(1, [0.0, pi / 2.0]),
            {"entity": "f3d:model:sketch-entity#3", "parameter_range": [0.0, 1.0], "reversed": true}, boundary(2, [0.5, 1.0])], "hole_boundaries": []},
        {"kind": "trimmed", "outer_boundary": [boundary(1, [pi, 2.0 * pi]),
            {"entity": "f3d:model:sketch-entity#2", "parameter_range": [0.0, 1.0], "reversed": true}], "hole_boundaries": []}
    ]}});
    let mut model = cube_with_history(2.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketches"][0]["profiles"] = json!([
        [{"entity": "f3d:model:sketch-entity#1", "reversed": false}]
    ]);
    wire["model"]["sketch_constraints"] = json!([]);
    wire["model"]["sketch_entities"] = json!([
        circle(1, [0.0, 0.0], 4.0),
        line(2, [-4.0, 0.0], [4.0, 0.0]),
        line(3, [0.0, 0.0], [0.0, 4.0]),
    ]);
    // Keep the example body's end cap from purporting to prove a circular profile.
    wire["model"]["features"][1]["outputs"] = json!([]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn selected_regions_resolve_their_shared_circle_before_choosing_points() {
    let (document, report) = convert(
        &shared_circle_regions(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{report:?}"
    );
    assert_eq!(document["features"][1]["profiles"]["type"], "points");
    assert_eq!(
        document["features"][1]["profiles"]["points"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn solidcraft_replays_regions_resolved_by_a_shared_circle() {
    let Some(cli) = solidcraft() else { return };
    let got = volumes(&cli, &written(&shared_circle_regions()).0);
    let want = 24.0 * std::f64::consts::PI;
    assert!(
        (got.iter().sum::<f64>() - want).abs() / want < 1e-3,
        "{got:?} vs {want}"
    );
}

/// Two half-discs on the same circle and a strict interval of a longer diameter.
/// Opposite directed boundaries must select opposite interiors despite identical curve sets.
fn trimmed_half_disc_revolve(upper: bool, angle: f64) -> Model {
    let mut model = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": angle}}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketch_entities"] = json!([circle(1, [0.0, 0.0], 4.0), line(2, [-8.0, 0.0], [8.0, 0.0])]);
    m["sketches"][0]["profiles"] =
        json!([[{"entity": "f3d:model:sketch-entity#1", "reversed": false}]]);
    let pi = std::f64::consts::PI;
    let regions = [
        json!({"kind": "trimmed", "outer_boundary": [
            {"entity": "f3d:model:sketch-entity#2", "parameter_range": [0.25, 0.75], "reversed": false},
            {"entity": "f3d:model:sketch-entity#1", "parameter_range": [0.0, pi], "reversed": false}], "hole_boundaries": []}),
        json!({"kind": "trimmed", "outer_boundary": [
            {"entity": "f3d:model:sketch-entity#2", "parameter_range": [0.25, 0.75], "reversed": true},
            {"entity": "f3d:model:sketch-entity#1", "parameter_range": [pi, 2.0 * pi], "reversed": false}], "hole_boundaries": []}),
    ];
    let construction =
        &mut m["features"][1]["definition"]["value"]["value"]["construction"]["value"];
    construction["profile"] = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
        "regions": [regions[usize::from(!upper)].clone()]}});
    construction["axis"]["direction"] = json!({"x": 1.0, "y": 0.0, "z": 0.0});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn directed_trimmed_revolves_select_the_interior_of_each_half_disc() {
    for upper in [true, false] {
        for angle in [std::f64::consts::TAU, std::f64::consts::FRAC_PI_2] {
            let (document, report) = convert(
                &trimmed_half_disc_revolve(upper, angle),
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            assert!(
                history_line(&report).starts_with("2 of 2 features rebuilt"),
                "{report:?}"
            );
            let selection = &document["features"][1]["profiles"];
            assert_eq!(selection["type"], "points");
            let p = &selection["points"][0];
            let (x, y) = (p[0].as_f64().unwrap(), p[1].as_f64().unwrap());
            assert!(
                x.hypot(y) < 4.0 && if upper { y > 0.0 } else { y < 0.0 },
                "{selection}"
            );
        }
    }
}

#[test]
fn solidcraft_replays_a_directed_trimmed_half_disc_revolve() {
    let Some(cli) = solidcraft() else { return };
    for upper in [true, false] {
        for angle in [std::f64::consts::TAU, std::f64::consts::FRAC_PI_2] {
            let got = volumes(&cli, &written(&trimmed_half_disc_revolve(upper, angle)).0);
            let want =
                4.0 / 3.0 * std::f64::consts::PI * 4.0_f64.powi(3) * angle / std::f64::consts::TAU;
            assert_eq!(got.len(), 1);
            assert!((got[0] - want).abs() / want < 1e-3, "{got:?} vs {want}");
        }
    }
}

fn circle(key: u32, center: [f64; 2], radius: f64) -> Value {
    entity(
        key,
        json!({"kind": "circle", "center": {"u": center[0], "v": center[1]}, "radius": radius}),
    )
}

/// The 10 mm square loop taken by its extrude, beside loops no feature takes: with `tab`, a
/// 4 by 4 mm tab whose inner edge lies along part of the square's right side; else a 1 mm
/// hole the region keeps (loop 1) inside a 2 mm ring (loop 2).
fn unused_loop_history(tab: bool) -> Model {
    let region = if tab {
        json!({"kind": "loops", "outer": 0, "holes": []})
    } else {
        json!({"kind": "loops", "outer": 0, "holes": [1]})
    };
    let profile =
        json!({"kind": "sketch_regions", "value": {"sketch": SKETCH, "regions": [region]}});
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let id =
        |key: u32| json!({"entity": format!("f3d:model:sketch-entity#{key}"), "reversed": false});
    let (entities, loops) = if tab {
        (
            vec![
                line(7, [10.0, 3.0], [14.0, 3.0]),
                line(8, [14.0, 3.0], [14.0, 7.0]),
                line(9, [14.0, 7.0], [10.0, 7.0]),
                line(10, [10.0, 7.0], [10.0, 3.0]),
            ],
            vec![json!([id(7), id(8), id(9), id(10)])],
        )
    } else {
        (
            vec![circle(11, [5.0, 5.0], 1.0), circle(12, [5.0, 5.0], 2.0)],
            vec![json!([id(11)]), json!([id(12)])],
        )
    };
    let source = &mut wire["model"];
    source["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .extend(entities);
    source["sketches"][0]["profiles"]
        .as_array_mut()
        .unwrap()
        .extend(loops);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_loop_no_feature_takes_is_written_as_construction() {
    for (tab, unused) in [(true, 4), (false, 1)] {
        let (document, report) = convert(
            &unused_loop_history(tab),
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("2 of 2 features rebuilt"),
            "{tab}"
        );
        let flags = construction_flags(&document);
        let construction = flags
            .iter()
            .filter(|(_, construction)| *construction)
            .count();
        assert_eq!(construction, unused, "{tab}: {flags:?}");
        let noun = if unused == 1 { "curve" } else { "curves" };
        let detail = format!(
            "{unused} sketch {noun} bounding no profile a feature uses written as construction"
        );
        assert!(
            report.losses().iter().any(|loss| loss.detail == detail),
            "{tab}: {report:?}"
        );
    }
}

#[test]
fn solidcraft_replays_a_region_beside_a_loop_no_feature_takes() {
    let Some(cli) = solidcraft() else { return };
    // The tab shares part of the square's side: solid, SolidCraft splits that side and finds
    // no profile bounded by the square's four curves.
    let volume = volumes(&cli, &written(&unused_loop_history(true)).0);
    assert_eq!(volume.len(), 1);
    assert!((volume[0] - 1000.0).abs() < 1e-6, "{volume:?}");
    // The ring around the hole: solid, the square's face ends at the ring, not at the hole.
    let volume = volumes(&cli, &written(&unused_loop_history(false)).0);
    assert_eq!(volume.len(), 1);
    // The hole's 1 mm circle, measured on SolidCraft's mesh; the ring's 2 mm would leave 874.
    let expected = 10.0 * (100.0 - std::f64::consts::PI);
    assert!((volume[0] - expected).abs() < 0.05, "{volume:?}");
}

/// The square and the tab beside it, both taken by the extrude.
fn tab_taken_history() -> Model {
    let mut model = unused_loop_history(true);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["definition"]["value"]["value"]["profile"]["value"]["value"]
        ["regions"] = json!([{"kind": "loops", "outer": 0, "holes": []}, {"kind": "loops", "outer": 1, "holes": []}]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn loops_that_share_part_of_an_edge_are_named_by_points() {
    let (document, report) = convert(
        &tab_taken_history(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    // Both loops are taken, so their curves stay solid.
    assert!(construction_flags(&document)
        .iter()
        .all(|(_, construction)| !construction));
    let profiles = &document["features"][1]["profiles"];
    assert_eq!(profiles["type"], "points", "{profiles}");
    assert_eq!(
        profiles["points"].as_array().unwrap().len(),
        2,
        "{profiles}"
    );
}

#[test]
fn solidcraft_replays_loops_that_share_part_of_an_edge() {
    let Some(cli) = solidcraft() else { return };
    // The square's 1000 mm³ and the tab's 160, as one body.
    let volume = volumes(&cli, &written(&tab_taken_history()).0);
    assert_eq!(volume.len(), 1, "{volume:?}");
    assert!((volume[0] - 1160.0).abs() < 1e-6, "{volume:?}");
}

/// The 10 mm square taken with two holes that cross: a 2 mm circle and a 3.4 mm square, both
/// centred in it, so the square's corners stick out of the circle (a nut pocket over a bolt hole).
fn crossing_holes_history() -> Model {
    let profile = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
        "regions": [{"kind": "loops", "outer": 0, "holes": [1, 2]}]}});
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let id =
        |key: u32| json!({"entity": format!("f3d:model:sketch-entity#{key}"), "reversed": false});
    let source = &mut wire["model"];
    source["sketch_entities"].as_array_mut().unwrap().extend([
        circle(11, [5.0, 5.0], 2.0),
        line(12, [3.3, 3.3], [6.7, 3.3]),
        line(13, [6.7, 3.3], [6.7, 6.7]),
        line(14, [6.7, 6.7], [3.3, 6.7]),
        line(15, [3.3, 6.7], [3.3, 3.3]),
    ]);
    source["sketches"][0]["profiles"]
        .as_array_mut()
        .unwrap()
        .extend([json!([id(11)]), json!([id(12), id(13), id(14), id(15)])]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_region_whose_holes_cross_is_named_by_a_point() {
    let (document, report) = convert(
        &crossing_holes_history(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    let profiles = &document["features"][1]["profiles"];
    assert_eq!(profiles["type"], "points", "{profiles}");
    assert_eq!(
        profiles["points"].as_array().unwrap().len(),
        1,
        "{profiles}"
    );
}

#[test]
fn solidcraft_replays_a_region_whose_holes_cross() {
    let Some(cli) = solidcraft() else { return };
    let volume = volumes(&cli, &written(&crossing_holes_history()).0);
    assert_eq!(volume.len(), 1, "{volume:?}");
    // The hole is the union: the circle and the square's corners outside it.
    let (half, radius) = (1.7_f64, 2.0_f64);
    let steps = 200_000;
    let corners: f64 = (0..steps)
        .map(|i| {
            let x = -half + 2.0 * half * (f64::from(i) + 0.5) / f64::from(steps);
            let inside = (radius * radius - x * x).max(0.0).sqrt();
            2.0 * (half - inside).max(0.0) * 2.0 * half / f64::from(steps)
        })
        .sum();
    let hole = std::f64::consts::PI * radius * radius + corners;
    let expected = 10.0 * (100.0 - hole);
    assert!((volume[0] - expected).abs() < 0.05, "{volume:?} {expected}");
}

/// The square taken through a list of profile loops, with two circles about (5, 5), 3 mm and
/// 1.75 mm, that no feature takes, and a line from the corner to their centre in no loop.
fn listed_square_history() -> Model {
    let profile = json!({"kind": "sketch_profiles", "value": {"sketch": SKETCH,
        "profiles": [0]}});
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let id =
        |key: u32| json!({"entity": format!("f3d:model:sketch-entity#{key}"), "reversed": false});
    let source = &mut wire["model"];
    source["sketch_entities"].as_array_mut().unwrap().extend([
        circle(11, [5.0, 5.0], 3.0),
        circle(12, [5.0, 5.0], 1.75),
        line(13, [0.0, 0.0], [5.0, 5.0]),
    ]);
    source["sketches"][0]["profiles"]
        .as_array_mut()
        .unwrap()
        .extend([json!([id(11)]), json!([id(12)])]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_list_of_profile_loops_leaves_unused_curves_construction() {
    let (document, report) = convert(
        &listed_square_history(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{}",
        history_line(&report)
    );
    // The circles and the line to their centre bound nothing the extrude takes; the square's
    // four sides stay solid. (Written curves are numbered by kind: circles c*, lines l*.)
    let flags = construction_flags(&document);
    assert_eq!(flags.len(), 7, "{flags:?}");
    for (id, construction) in &flags {
        let unused = id.starts_with('c') || id == "l5";
        assert_eq!(*construction, unused, "{flags:?}");
    }
}

#[test]
fn solidcraft_replays_a_listed_loop_beside_curves_no_feature_takes() {
    let Some(cli) = solidcraft() else { return };
    let volume = volumes(&cli, &written(&listed_square_history()).0);
    assert_eq!(volume.len(), 1, "{volume:?}");
    // Solid, the circles and the line split the square, and no profile has its boundary.
    assert!((volume[0] - 1000.0).abs() < 1e-6, "{volume:?}");
}

#[test]
fn a_curve_another_feature_names_stays_real_geometry() {
    // A sweep along curve 5 of the same sketch needs it as geometry, so nothing in the
    // sketch is demoted.
    let model = stray_curve_history(|model| {
        let section = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
            "regions": [{"kind": "loops", "outer": 0, "holes": []}]}});
        model["features"][1]["definition"] = json!({"kind":"operation", "value": {"kind": "sweep", "value": {"shape": {"kind": "solid", "op": "new_body", "section": {"kind": "profile", "value": section}, "sections": []}, "path": {"kind": "sketch_curves", "value": {
                "sketch": SKETCH, "curves": ["f3d:model:sketch-entity#5"]}}, "path_tangent": false, "linearize": false, "path_extent": {"along_fraction": 1.0, "against_fraction": 0.0}}}});
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(construction_flags(&document)
        .iter()
        .all(|(_, construction)| !construction));
    assert!(!report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("bounding no profile")));
}

#[test]
fn overlapping_crossing_curves_keep_the_refusal() {
    let mut model = crossed_profile_history(false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // A second line along a diagonal, in the crossed loop too: a line in no loop would bound
    // nothing selected and be construction.
    wire["model"]["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .push(line(11, [0.0, 0.0], [10.0, 10.0]));
    let member =
        |reversed: bool| json!({"entity": "f3d:model:sketch-entity#11", "reversed": reversed});
    let crossed = wire["model"]["sketches"][0]["profiles"][0]
        .as_array_mut()
        .unwrap();
    crossed.extend([member(false), member(true)]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_reference_sketches(&document, 1);
    assert!(
        history_line(&report).contains("a profile crossed by its own sketch's curves"),
        "{}",
        history_line(&report)
    );
}

/// A bow tie uses each curve once but intersects itself between curve endpoints.
fn bow_tie_profile() -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketch_constraints"] = json!([]);
    wire["model"]["sketch_entities"] = json!([
        line(1, [0.0, 0.0], [10.0, 10.0]),
        line(2, [10.0, 10.0], [0.0, 10.0]),
        line(3, [0.0, 10.0], [10.0, 0.0]),
        line(4, [10.0, 0.0], [0.0, 0.0]),
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_self_intersection_between_endpoints_splits_the_profile() {
    let (document, report) = convert(
        &bow_tie_profile(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    let mut loops: Vec<Vec<String>> =
        serde_json::from_value(document["features"][1]["profiles"]["loops"].clone()).unwrap();
    loops.sort();
    assert_eq!(loops, vec![vec!["l1", "l2", "l3"], vec!["l1", "l3", "l4"]]);
}

#[test]
fn solidcraft_replays_both_lobes_of_a_self_intersecting_profile() {
    let Some(cli) = solidcraft() else { return };
    let volume: f64 = volumes(&cli, &written(&bow_tie_profile()).0).iter().sum();
    assert!((volume - 500.0).abs() < 1e-6, "{volume}");
}

/// A native selection with a known sketch but no decoded region pick. Two nested
/// circles identify one annulus; a third creates a separate material island.
fn nested_native_profile(radii: &[f64]) -> Model {
    let profile = json!({"kind": "sketch_selection", "value": {
        "sketch": SKETCH, "selections": ["inventor:native:profile-selection#1"],
    }});
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketch_constraints"] = json!([]);
    let entities: Vec<Value> = radii
        .iter()
        .enumerate()
        .map(|(i, radius)| {
            entity(
                u32::try_from(i + 1).unwrap(),
                json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": radius}),
            )
        })
        .collect();
    let profiles: Vec<Value> = radii
        .iter()
        .enumerate()
        .map(|(i, _)| json!([{"entity": format!("f3d:model:sketch-entity#{}", i + 1), "reversed": false}]))
        .collect();
    m["sketch_entities"] = json!(entities);
    m["sketches"][0]["profiles"] = json!(profiles);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn native_selections_resolve_one_even_odd_region() {
    let (document, report) = convert(
        &nested_native_profile(&[5.0, 3.0]),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    assert_eq!(
        document["features"][1]["profiles"],
        json!({"type": "whole_boundaries", "profiles": [{"outer": ["c1"], "holes": [["c2"]]}]})
    );
    let (reversed, _) = convert(
        &nested_native_profile(&[3.0, 5.0]),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(
        reversed["features"][1]["profiles"],
        json!({"type": "whole_boundaries", "profiles": [{"outer": ["c2"], "holes": [["c1"]]}]})
    );
    for radii in [&[5.0, 3.0, 1.0][..], &[5.0, 5.0][..], &[5.0, 4.99][..]] {
        let (document, report) = convert(
            &nested_native_profile(radii),
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert_reference_sketches(&document, 1);
        assert_eq!(
            report.outcome().unwrap().features[1].status,
            dutchman_core::FeatureStatus::Blocked
        );
        assert!(!report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("history/profile-region-inferred")));
    }
}

#[test]
fn solidcraft_replays_a_native_annulus_with_its_hole() {
    let Some(cli) = solidcraft() else { return };
    let volume = volumes(&cli, &written(&nested_native_profile(&[5.0, 3.0])).0);
    let expected = std::f64::consts::PI * 16.0 * 10.0;
    assert_eq!(volume.len(), 1);
    assert!((volume[0] - expected).abs() / expected < dutchman_solidcraft::VOLUME_TOLERANCE);
}

/// A synthetic panel with twenty circular holes centred on an unused inner rectangle.
/// The region selects the panel and circles; the inner rectangle is a layout helper.
fn panel_region() -> Model {
    let profile = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
        "regions": [{"kind": "loops", "outer": 0, "holes": (2..22).rev().collect::<Vec<_>>()}]}});
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketch_constraints"] = json!([]);
    let mut entities = vec![
        line(1, [0.0, 0.0], [100.0, 0.0]),
        line(2, [100.0, 0.0], [100.0, 100.0]),
        line(3, [100.0, 100.0], [0.0, 100.0]),
        line(4, [0.0, 100.0], [0.0, 0.0]),
        line(5, [10.0, 10.0], [90.0, 10.0]),
        line(6, [90.0, 10.0], [90.0, 90.0]),
        line(7, [90.0, 90.0], [10.0, 90.0]),
        line(8, [10.0, 90.0], [10.0, 10.0]),
    ];
    let boundary = |keys: Vec<u32>| {
        json!(keys
            .into_iter()
            .map(
                |key| json!({"entity": format!("f3d:model:sketch-entity#{key}"), "reversed": false})
            )
            .collect::<Vec<_>>())
    };
    let mut profiles = vec![boundary((1..5).collect()), boundary((5..9).collect())];
    for side in 0..4 {
        for i in 0..5 {
            let along = 20.0 + f64::from(i) * 15.0;
            let center = match side {
                0 => [along, 10.0],
                1 => [90.0, along],
                2 => [along, 90.0],
                _ => [10.0, along],
            };
            let key = 9 + side * 5 + i;
            entities.push(entity(
                key,
                json!({"kind": "circle",
                "center": {"u": center[0], "v": center[1]}, "radius": 1.0}),
            ));
            profiles.push(boundary(vec![key]));
        }
    }
    m["sketch_entities"] = json!(entities);
    m["sketches"][0]["profiles"] = json!(profiles);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn explicit_regions_keep_unused_layout_lines_out_of_their_holes() {
    let model = panel_region();
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);
    let curves = document["features"][0]["sketch"]["curves"]
        .as_array()
        .unwrap();
    assert_eq!(
        curves.iter().filter(|c| c["construction"] == true).count(),
        4
    );
    assert!(curves[4..8].iter().all(|c| c["construction"] == true));
    assert!(curves[8..].iter().all(|c| c["construction"] == false));
    assert!(report
        .losses()
        .iter()
        .any(|loss| { loss.severity == Severity::Info && loss.detail.contains("4 lines") }));

    // Other selections, including an opaque pick depending on the sketch feature, prevent
    // inference. Suppressed consumers do not change the sketch.
    for profile in [
        whole_sketch(),
        json!({"kind": "native", "value": "unresolved profile"}),
        json!({"kind": "feature", "value": "f3d:model:feature#1"}),
        json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
            "regions": [{"kind": "loops", "outer": 1, "holes": []}]}}),
        json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
        "regions": [{"kind": "trimmed", "outer_boundary": [{"entity": "f3d:model:sketch-entity#9", "parameter_range": [0.0, 1.0], "reversed": false}], "hole_boundaries": []}]}}),
    ] {
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let mut other = wire["model"]["features"][1].clone();
        other["id"] = json!("synthetic:panel:feature#3");
        other["ordinal"] = json!(2);
        other["definition"]["value"]["value"]["profile"]["value"] = profile;
        if other["definition"]["value"]["value"]["profile"]["value"]["kind"] == "feature" {
            other["dependencies"] = json!([]);
        }
        wire["model"]["features"]
            .as_array_mut()
            .unwrap()
            .push(other);
        let mut shared = model.clone();
        shared.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(&shared, &writer);
        if wire["model"]["features"][2]["definition"]["value"]["value"]["profile"]["value"]["kind"]
            == "native"
        {
            assert!(history_line(&report).contains("native profiles are not mapped yet"));
        }
        assert!(document["features"][0]["sketch"]["curves"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["construction"] == false));
        wire["model"]["features"][2]["suppressed"] = json!(true);
        shared.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, _) = convert(&shared, &writer);
        assert!(document["features"][0]["sketch"]["curves"]
            .as_array()
            .unwrap()[4..8]
            .iter()
            .all(|c| c["construction"] == true));
    }
}

#[test]
fn solidcraft_replays_a_panel_region_with_twenty_circular_holes() {
    let Some(cli) = solidcraft() else { return };
    let volume = volumes(&cli, &written(&panel_region()).0);
    let expected = (10_000.0 - 20.0 * std::f64::consts::PI) * 10.0;
    assert_eq!(volume.len(), 1);
    assert!((volume[0] - expected).abs() / expected < dutchman_solidcraft::VOLUME_TOLERANCE);
}

/// The shape cadmpeg 0.6's Inventor decoder gives an extrude: its profile as native profile
/// selections of the sketch, an explicit direction, a symmetric or through-next extent. In a
/// sketch of one profile the selection can only be that profile; multiple loops must
/// identify one material region by even-odd nesting, otherwise the history is refused.
#[test]
fn an_inventor_shaped_extrude_needs_confirmation_for_several_profiles() {
    let inventor_profile = json!({"kind": "sketch_selection",
        "value": {"sketch": SKETCH, "selections": ["inventor:native:profile-selection#1"]}});
    let extent = json!({"kind": "symmetric",
        "side": {"termination": {"kind": "blind", "length": 10.0}}});
    let model = cube_with_history(10.0, inventor_profile.clone(), Some(extent.clone()));
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, _) = convert(&model, &writer);
    assert_eq!(
        types(&document),
        [
            ("sketch".to_owned(), false),
            ("extrude".to_owned(), false),
            ("import".to_owned(), true)
        ]
    );
    // The same sketch with its loop listed twice, as two profiles.
    let mut model = cube_with_history(10.0, inventor_profile, Some(extent));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let profiles = wire["model"]["sketches"][0]["profiles"].clone();
    wire["model"]["sketches"][0]["profiles"] = json!([profiles[0], profiles[0]]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(&model, &writer);
    assert_reference_sketches(&document, 1);
    assert_eq!(
        history_line(&report),
        "flattened: feature \"Block\": its inferred profile region could not be confirmed by \
         the source body check"
    );
    let outcome = report.outcome().unwrap();
    let statuses: Vec<_> = outcome
        .features
        .iter()
        .map(|f| (f.kind.as_str(), f.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("sketch", dutchman_core::FeatureStatus::Mapped),
            ("extrude", dutchman_core::FeatureStatus::Blocked)
        ]
    );
}

/// Two parts with the cube history (and so the same parameter names), `First` placed at the
/// origin and `Second` twice, 30 mm and 60 mm along X. The assembly's own file holds no
/// sketches, as an Inventor assembly's holds none of its parts'.
fn assembly_of_histories() -> Model {
    let part = |name: &str| Model {
        name: name.to_owned(),
        source_file: Some(format!("{name}.ipt")),
        source_step: None,
        ..cube_with_history(10.0, first_profile(), None)
    };
    let at = |x: f64| {
        [
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]
    };
    let placed = |part: usize, x: f64| dutchman_core::Placement {
        hidden: false,
        name: None,
        part,
        transform: at(x),
    };
    Model {
        name: "pair".to_owned(),
        source_file: Some("pair.iam".to_owned()),
        source_step: None,
        assembly: Some(dutchman_core::Assembly {
            own_part: None,
            joints: Vec::new(),
            left_out: Vec::new(),
            held: Vec::new(),
            joint_notes: Vec::new(),
            stored_placements: Vec::new(),
            identities: Vec::new(),
            definitions: Vec::new(),
            parts: vec![part("First"), part("Second")],
            placements: vec![placed(0, 0.0), placed(1, 30.0), placed(1, 60.0)],
        }),
        ir: cadmpeg_ir::examples::unit_cube().unwrap(),
        ..part("pair")
    }
}

#[test]
fn each_sketch_sits_under_the_part_that_owns_it_and_a_consumed_one_stays_shown() {
    let (document, _) = convert(
        &assembly_of_histories(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = document["features"].as_array().unwrap();
    let components = document["components"].as_array().unwrap();
    for name in ["First", "Second"] {
        let id = components.iter().find(|c| c["name"] == name).unwrap()["id"].clone();
        let sketches: Vec<_> = features
            .iter()
            .filter(|f| f["type"] == "sketch" && f["component"] == id)
            .collect();
        assert_eq!(sketches.len(), 1, "{name} holds its sketch");
    }
    assert!(
        !features
            .iter()
            .any(|f| f["type"] == "sketch" && f["component"] == 0),
        "no sketch is left in the assembly root"
    );
    // The extrude of each part is built on its sketch, so both start shown.
    assert!(document.get("visibility").is_none_or(|v| v
        .get("hidden_sketches")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)));
}

#[test]
fn parts_keep_their_histories_inside_an_assembly() {
    let (document, report) = convert(
        &assembly_of_histories(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let components = document["components"].as_array().unwrap();
    let second = components.iter().find(|c| c["name"] == "Second").unwrap()["id"].clone();
    let in_second: Vec<_> = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["component"] == second)
        .map(|f| {
            (
                f["type"].as_str().unwrap().to_owned(),
                f["suppressed"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        in_second,
        [
            ("sketch".to_owned(), false),
            ("extrude".to_owned(), false),
            ("import".to_owned(), true)
        ]
    );
    // Feature ids are unique, and each extrude still points at its own sketch.
    let features = document["features"].as_array().unwrap();
    let mut ids: Vec<u64> = features.iter().map(|f| f["id"].as_u64().unwrap()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), features.len());
    for extrude in features.iter().filter(|f| f["type"] == "extrude") {
        let sketch = features
            .iter()
            .find(|f| f["id"] == extrude["sketch"])
            .unwrap();
        assert_eq!(sketch["component"], extrude["component"]);
    }
    // The second part's parameters are renamed, and its dimension follows.
    let names: Vec<&str> = document["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["d1", "side", "Second_d1", "Second_side"]);
    let second_sketch = features
        .iter()
        .find(|f| f["type"] == "sketch" && f["component"] == second)
        .unwrap();
    let linked: Vec<&str> = second_sketch["sketch"]["constraints"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["param"].as_str())
        .collect();
    assert_eq!(linked, ["Second_side"]);
    let lines: Vec<String> = report
        .losses()
        .iter()
        .filter(|loss| loss.subject == "history")
        .map(|loss| loss.detail.clone())
        .collect();
    assert_eq!(
        lines,
        [
            "First.ipt: 2 of 2 features rebuilt; unverified: verification turned off",
            "Second.ipt: 2 of 2 features rebuilt; unverified: verification turned off"
        ]
    );
    assert_eq!(report.outcome().unwrap().mode, dutchman_core::Mode::History);
}

/// [`assembly_of_histories`] with `First`'s cube green and `Second`'s top face blue.
fn assembly_with_face_colours() -> Model {
    let mut model = assembly_of_histories();
    let assembly = model.assembly.as_mut().unwrap();
    for (part, recolour) in assembly.parts.iter_mut().zip([true, false]) {
        let mut wire = serde_json::to_value(&part.ir).unwrap();
        if recolour {
            wire["geometry"]["bodies"][0]["color"] =
                json!({"r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0});
        } else {
            for face in wire["geometry"]["faces"].as_array_mut().unwrap() {
                if face["id"] == "synthetic:cube:face#top" {
                    face["color"] = json!({"r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0});
                }
            }
        }
        part.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    }
    model
}

#[test]
fn parts_inside_an_assembly_keep_their_own_body_and_face_colours() {
    let (document, report) = convert(
        &assembly_with_face_colours(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    // Both parts name their body "unit cube"; the second's is renamed so each keeps its look.
    let bodies = &document["appearances"]["bodies"];
    assert_eq!(bodies["unit cube"]["color"], json!([0, 255, 0]));
    assert_eq!(bodies["unit cube (Second)"]["color"], json!([204, 51, 26]));
    let second = document["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Second")
        .unwrap()["id"]
        .clone();
    let extrude = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "extrude" && f["component"] == second)
        .unwrap();
    assert_eq!(extrude["body_names"], json!(["unit cube (Second)"]));
    // The face look is on the second part's body, at a point in that part's own frame: on its
    // top face, not moved to where it is placed.
    let faces = document["appearances"]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0]["body"], "unit cube (Second)");
    assert_eq!(faces[0]["look"]["color"], json!([0, 0, 255]));
    let p: Vec<f64> = faces[0]["point"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_f64().unwrap())
        .collect();
    assert!(
        (p[2] - 10.0).abs() < 1e-9 && (0.0..10.0).contains(&p[0]) && (0.0..10.0).contains(&p[1]),
        "{p:?}"
    );
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.subject == "appearance"),
        "{:?}",
        report.losses()
    );
}

#[test]
fn solidcraft_rebuilds_each_parts_history_in_the_assembly() {
    let Some(cli) = solidcraft() else { return };
    let (bytes, _) = written(&assembly_of_histories());
    assert_eq!(volumes(&cli, &bytes), [1000.0, 1000.0, 1000.0]);
    // The second part's own `side` drives only its placements.
    let mut grown = volumes(&cli, &with_param(&bytes, "Second_side", "20 mm"));
    grown.sort_by(f64::total_cmp);
    assert_eq!(grown, [1000.0, 4000.0, 4000.0]);
}

#[test]
fn inspecting_an_assembly_maps_each_parts_history() {
    let mut report = Report::default();
    let outcome = SolidCraftWriter::new().plan(&assembly_of_histories(), &mut report);
    assert_eq!(outcome.mode, dutchman_core::Mode::History);
    let names: Vec<String> = outcome
        .features
        .iter()
        .map(|f| f.name.clone().unwrap_or_default())
        .collect();
    assert_eq!(
        names,
        [
            "First.ipt: Base sketch",
            "First.ipt: Block",
            "Second.ipt: Base sketch",
            "Second.ipt: Block"
        ]
    );
}
/// The cube history followed by a pattern of its extrude, laid out by `layout`.
fn patterned(layout: Value) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Copies", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "pattern", "value": {"seeds": [{"kind": "feature", "value": "f3d:model:feature#2"}], "pattern": layout}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

/// Change a synthetic pattern's operands without passing through a native decoder.
fn pattern_seeds(mut model: Model, seeds: Value) -> Model {
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][2]["definition"]["value"]["value"]["seeds"] = seeds;
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn body_patterns_use_names_assigned_by_earlier_features() {
    for selection in [
        json!({"kind": "bodies", "value": ["synthetic:cube:body#0"]}),
        json!({"kind": "resolved", "value": {
            "bodies": ["synthetic:cube:body#0"], "native": "selection"}}),
        json!({"kind": "resolved_set", "value": {"members": [
            {"body": "synthetic:cube:body#0", "native": "selection"}]}}),
    ] {
        let model = pattern_seeds(
            patterned(
                json!({"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3, "omitted_instances": [], "symmetric": false}),
            ),
            json!([{"kind": "bodies", "value": selection}]),
        );
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
        assert_eq!(document["features"][2]["bodies"], json!(["unit cube"]));
        assert_eq!(document["features"][2]["features"], json!([]));
        if let Some(cli) = solidcraft() {
            assert!((volumes(&cli, &written(&model).0).iter().sum::<f64>() - 3000.0).abs() < 1e-6);
        }
    }
}

#[test]
fn regular_explicit_pattern_locations_map_to_spacing_and_span() {
    let direction = json!({"x": 1.0, "y": 0.0, "z": 0.0});
    for (layout, field, expected) in [
        (
            json!({"kind": "linear_offsets", "direction": direction, "offsets": [0.0, 20.0, 40.0]}),
            "spacing1",
            "20 mm",
        ),
        (
            json!({"kind": "circular_angles",
            "axis_origin": {"x": -20.0, "y": 0.0, "z": 0.0},
            "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0},
            "angles": [0.0, std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_PI_2]}),
            "angle",
            "90 deg",
        ),
    ] {
        let model = patterned(layout);
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
        assert_eq!(document["features"][2]["pattern"][field], expected);
        if let Some(cli) = solidcraft() {
            assert!((volumes(&cli, &written(&model).0).iter().sum::<f64>() - 3000.0).abs() < 1e-6);
        }
    }
    let model = patterned(json!({"kind": "circular_angles",
        "axis_origin": {"x": -20.0, "y": 0.0, "z": 0.0},
        "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0},
        "angles": [0.0, std::f64::consts::FRAC_PI_2, std::f64::consts::PI,
            3.0 * std::f64::consts::FRAC_PI_2]}));
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(document["features"][2]["pattern"]["angle"], "360 deg");
    assert_eq!(document["features"][2]["pattern"]["count"], "4");
    if let Some(cli) = solidcraft() {
        assert!((volumes(&cli, &written(&model).0).iter().sum::<f64>() - 4000.0).abs() < 1e-6);
    }
}

#[test]
fn two_linear_stages_become_a_rectangular_pattern() {
    let model = patterned(json!({"kind": "composite", "stages": [
        {"pattern": {"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3, "omitted_instances": [], "symmetric": false}},
        {"pattern": {"kind": "linear", "direction": {"x": 0.0, "y": 1.0, "z": 0.0}, "spacing": 30.0, "count": 2, "omitted_instances": [], "symmetric": false}},
    ]}));
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
    assert_eq!(
        document["features"][2]["pattern"],
        json!({
        "type": "rectangular", "dir1": [1.0, 0.0, 0.0], "count1": "3", "spacing1": "20 mm",
        "dir2": [0.0, 1.0, 0.0], "count2": "2", "spacing2": "30 mm"})
    );
    if let Some(cli) = solidcraft() {
        assert!((volumes(&cli, &written(&model).0).iter().sum::<f64>() - 6000.0).abs() < 1e-6);
    }
}

#[test]
fn full_turn_endpoints_keep_only_distinct_instances() {
    for (angles, count) in [
        (vec![0.0, std::f64::consts::TAU], 1),
        (vec![0.0, std::f64::consts::PI, std::f64::consts::TAU], 2),
        (
            (0..=4)
                .map(|i| i as f64 * std::f64::consts::FRAC_PI_2)
                .collect(),
            4,
        ),
    ] {
        let model = patterned(json!({"kind": "circular_angles",
            "axis_origin": {"x": -20.0, "y": 0.0, "z": 0.0},
            "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0}, "angles": angles}));
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
        assert_eq!(
            document["features"][2]["pattern"]["count"],
            count.to_string()
        );
        assert_eq!(document["features"][2]["pattern"]["angle"], "360 deg");
        if let Some(cli) = solidcraft() {
            let v = volumes(&cli, &written(&model).0);
            assert_eq!(v.len(), count);
            assert!((v.iter().sum::<f64>() - count as f64 * 1000.0).abs() < 1e-6);
        }
    }
}

#[test]
fn composite_mirrors_multiply_the_previous_instance_set_in_order() {
    let mirror = json!({"pattern": {"kind": "mirror",
        "plane_origin": {"x": 20.0, "y": 0.0, "z": 0.0},
        "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}}});
    let linear = json!({"pattern": {"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 60.0, "count": 3, "omitted_instances": [], "symmetric": false}});
    for (stages, expected) in [
        (
            json!([mirror, linear]),
            vec![0.0, 30.0, 60.0, 90.0, 120.0, 150.0],
        ),
        (
            json!([linear, mirror]),
            vec![-90.0, -30.0, 0.0, 30.0, 60.0, 120.0],
        ),
    ] {
        let model = patterned(json!({"kind": "composite", "stages": stages}));
        let (document, report) = convert(&model, &composite_writer());
        assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
        assert_eq!(document["features"][2]["features"], json!(["Block"]));
        assert_eq!(
            document["features"][3]["features"],
            json!(["Block", "Copies stage 1"])
        );
        assert_eq!(document["features"][3]["name"], "Copies");
        if let Some(cli) = composite_cli() {
            let design = written_with(&model, &composite_writer()).0;
            let v = volumes(&cli, &design);
            assert_eq!(v.len(), 6);
            assert!((v.iter().sum::<f64>() - 6000.0).abs() < 1e-6);
            let mut x: Vec<f64> = bounds(&cli, &design)
                .iter()
                .map(|(min, _)| min[0])
                .collect();
            x.sort_by(f64::total_cmp);
            assert_eq!(x, expected);
        }
    }
}

fn composite_writer() -> SolidCraftWriter {
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_25_1))
}

fn composite_cli() -> Option<PathBuf> {
    let cli = solidcraft()?;
    let version =
        dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()));
    if version < dutchman_solidcraft::SolidCraftVersion::V0_25_1 {
        eprintln!("skipped: composite chains need SolidCraft 0.25 patch 1 or newer");
        return None;
    }
    Some(cli)
}

#[test]
fn compatibility_floor_blocks_composite_chains_but_keeps_single_stages() {
    let mirror = json!({"kind": "mirror",
        "plane_origin": {"x": 20.0, "y": 0.0, "z": 0.0},
        "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}});
    let linear = json!({"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 60.0, "count": 3, "omitted_instances": [], "symmetric": false});
    let writer = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.18.2".parse().unwrap()));
    for stages in [
        json!([{"pattern": mirror}, {"pattern": linear}]),
        json!([{"pattern": linear}, {"pattern": mirror}]),
        json!([{"pattern": mirror}, {"pattern": mirror}]),
    ] {
        let (document, report) = convert(
            &patterned(json!({"kind": "composite", "stages": stages})),
            &writer,
        );
        // The extrusion before the blocked pattern stays as partial history.
        assert_eq!(report.outcome().unwrap().mode, Mode::PartialHistory);
        let feature = report
            .outcome()
            .unwrap()
            .features
            .iter()
            .find(|feature| feature.kind == "pattern")
            .unwrap();
        assert_eq!(
            feature.reason.as_deref(),
            Some("needs SolidCraft 0.25 patch 1 or newer (target 0.18)")
        );
        assert!(history_line(&report).contains(feature.reason.as_deref().unwrap()));
        assert!(!types(&document)
            .iter()
            .any(|(kind, _)| kind == "mirror" || kind == "pattern"));
    }
    for layout in [
        mirror.clone(),
        linear.clone(),
        json!({"kind": "composite", "stages": [{"pattern": mirror}]}),
        json!({"kind": "composite", "stages": [{"pattern": linear}]}),
    ] {
        let (document, report) = convert(&patterned(layout.clone()), &writer);
        assert_eq!(report.outcome().unwrap().mode, Mode::History);
        assert!(types(&document)
            .iter()
            .any(|(kind, _)| kind == "mirror" || kind == "pattern"));
        let mut wire = serde_json::to_value(&patterned(layout.clone()).ir).unwrap();
        wire["model"]["features"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "f3d:model:feature#4", "ordinal": 3, "name": "Later copies", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "pattern", "value": {"seeds": [{"kind": "feature", "value": "f3d:model:feature#3"}], "pattern": {"kind": "mirror", "plane_origin": {"x": 0.0, "y": 20.0, "z": 0.0},
                    "plane_normal": {"x": 0.0, "y": 1.0, "z": 0.0}}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..patterned(layout)
        };
        let (_, report) = convert(&model, &writer);
        // The features before the gated later pattern stay as partial history.
        assert_eq!(report.outcome().unwrap().mode, Mode::PartialHistory);
        assert!(
            history_line(&report).contains("needs SolidCraft 0.25 patch 1 or newer (target 0.18)")
        );
        let (_, report) = convert(&model, &composite_writer());
        assert_eq!(report.outcome().unwrap().mode, Mode::History);
    }
}

#[test]
fn composite_mirrors_chain_multiple_reflections_and_circular_copies() {
    let mirror = json!({"pattern": {"kind": "mirror",
        "plane_origin": {"x": 20.0, "y": 0.0, "z": 0.0},
        "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}}});
    let other = json!({"pattern": {"kind": "mirror",
        "plane_origin": {"x": 0.0, "y": 20.0, "z": 0.0},
        "plane_normal": {"x": 0.0, "y": 1.0, "z": 0.0}}});
    let circular = json!({"pattern": {"kind": "circular",
        "axis_origin": {"x": -1000.0, "y": -1000.0, "z": 0.0},
        "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0},
        "angle": std::f64::consts::FRAC_PI_2, "count": 3}});
    for (stages, count) in [
        (json!([mirror, other]), 4),
        (json!([mirror, other, circular]), 12),
        (json!([circular, mirror]), 6),
    ] {
        let model = patterned(json!({"kind": "composite", "stages": stages}));
        let (_, report) = convert(&model, &composite_writer());
        assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
        assert!(report.outcome().unwrap().features[2].output.ids.len() > 1);
        if let Some(cli) = composite_cli() {
            let v = volumes(&cli, &written_with(&model, &composite_writer()).0);
            assert_eq!(v.len(), count);
            assert!((v.iter().sum::<f64>() - count as f64 * 1000.0).abs() < 1e-5);
        }
    }
}

#[test]
fn composite_mirror_chains_refuse_unrepresentable_stages_and_excessive_products() {
    let mirror = json!({"pattern": {"kind": "mirror",
        "plane_origin": {"x": 20.0, "y": 0.0, "z": 0.0},
        "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}}});
    let model = patterned(json!({"kind": "composite", "stages": [mirror,
        {"pattern": {"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 5001, "omitted_instances": [], "symmetric": false}}]}));
    let (document, report) = convert(&model, &composite_writer());
    assert!(
        history_line(&report).contains("more than 10000 instances"),
        "{}",
        history_line(&report)
    );
    // The sketch and extrusion before the refused pattern stay as partial history beside
    // the imported final shape.
    assert_eq!(report.outcome().unwrap().mode, Mode::PartialHistory);
    assert_eq!(
        types(&document),
        [
            ("sketch".to_owned(), false),
            ("extrude".to_owned(), false),
            ("import".to_owned(), false)
        ]
    );
    let model = pattern_seeds(
        patterned(json!({"kind": "composite", "stages": [mirror, mirror]})),
        json!([{"kind": "bodies", "value": {"kind": "bodies", "value": ["synthetic:cube:body#0"]}}]),
    );
    let (_, report) = convert(&model, &composite_writer());
    assert!(history_line(&report).contains("intermediate copies have no mapped body names"));
}

#[test]
fn later_patterns_can_select_a_whole_composite_mirror_result() {
    let model = patterned(json!({"kind": "composite", "stages": [
        {"pattern": {"kind": "mirror", "plane_origin": {"x": 20.0, "y": 0.0, "z": 0.0},
            "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}}},
        {"pattern": {"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 60.0, "count": 3, "omitted_instances": [], "symmetric": false}}
    ]}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // Colliding source and helper names must still select the right timeline features.
    wire["model"]["features"][0]["name"] = json!("Copies stage 1");
    wire["model"]["features"].as_array_mut().unwrap().push(json!({"id": "f3d:model:feature#4", "ordinal": 3, "name": "Copies", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "pattern", "value": {"seeds": [{"kind": "feature", "value": "f3d:model:feature#3"}], "pattern": {"kind": "linear", "direction": {"x": 0.0, "y": 1.0, "z": 0.0}, "spacing": 30.0, "count": 2, "omitted_instances": [], "symmetric": false}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (document, report) = convert(&model, &composite_writer());
    assert!(history_line(&report).starts_with("4 of 4 features rebuilt"));
    assert_eq!(document["features"][2]["name"], "Copies stage 1 (2)");
    assert_eq!(document["features"][4]["name"], "Copies (2)");
    assert_eq!(
        document["features"][4]["features"],
        json!(["Block", "Copies stage 1 (2)", "Copies"])
    );
    if let Some(cli) = composite_cli() {
        let v = volumes(&cli, &written_with(&model, &composite_writer()).0);
        assert_eq!(v.len(), 12);
        assert!((v.iter().sum::<f64>() - 12000.0).abs() < 1e-6);
    }
}

#[test]
fn unfinished_pattern_replays_keep_the_original_geometry() {
    let Some(cli) = solidcraft() else { return };
    for layout in [
        json!({"kind": "circular_angles", "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0}, "angles": [0.0, std::f64::consts::TAU]}),
        json!({"kind": "composite", "stages": [{"pattern": {"kind": "mirror",
            "plane_origin": {"x": 20.0, "y": 0.0, "z": 0.0},
            "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}}},
            {"pattern": {"kind": "linear", "direction": {"x": 0.0, "y": 1.0, "z": 0.0}, "spacing": 30.0, "count": 2, "omitted_instances": [], "symmetric": false}}]}),
    ] {
        let (document, report) = convert(
            &patterned(layout),
            &SolidCraftWriter::new()
                .verification(Verification::With(cli.clone()))
                .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_25_1))
                .timeout(std::time::Duration::ZERO),
        );
        // The features before the unverified pattern stay as partial history.
        assert_eq!(report.outcome().unwrap().mode, Mode::PartialHistory);
        assert!(
            history_line(&report).contains("pattern instance set could not be verified"),
            "{}",
            history_line(&report)
        );
        assert!(history_line(&report).contains("timed out"));
        // The unverified timeline (sketch, extrusion, then the pattern's mirror or pattern
        // stages) stays hidden beside the visible original shape.
        let kinds: Vec<String> = types(&document).into_iter().map(|(kind, _)| kind).collect();
        assert_eq!(kinds[..2], ["sketch", "extrude"], "{kinds:?}");
        assert_eq!(
            kinds.last().map(String::as_str),
            Some("import"),
            "{kinds:?}"
        );
        let stages = &kinds[2..kinds.len() - 1];
        assert!(!stages.is_empty(), "{kinds:?}");
        assert!(
            stages
                .iter()
                .all(|kind| kind == "mirror" || kind == "pattern"),
            "{kinds:?}"
        );
    }
}

#[test]
fn cc0_freecad_mirror_and_full_turn_match_recipe_volumes() {
    for (file, count, volume) in [
        ("S13c.FCStd", 7, 11660.70799341),
        ("freecad-1.1.4/S13c.FCStd", 7, 11660.70799341),
        ("P08b.FCStd", 5, 13006.19358586),
    ] {
        let cli = if file == "P08b.FCStd" {
            solidcraft()
        } else {
            composite_cli()
        };
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(file);
        let model = dutchman_core::read(
            dutchman_core::Format::FreeCadDocument,
            &path,
            &mut Report::default(),
        )
        .unwrap();
        let (document, report) = convert(&model, &composite_writer());
        assert!(
            history_line(&report).starts_with(&format!("{count} of {count} features rebuilt")),
            "{file}: {}",
            history_line(&report)
        );
        assert!(types(&document).iter().any(|(kind, _)| kind == "pattern"));
        let model = if file == "P08b.FCStd" {
            assert_eq!(document["features"][4]["pattern"]["count"], "7");
            assert_eq!(document["features"][4]["pattern"]["angle"], "360 deg");
            // The pinned decoder resolves Z_Axis as -Y and includes a duplicate endpoint
            // where the recipe has eight distinct holes. Never repair native decoding
            // in the mapper: expose both gaps and isolate them in this neutral test model.
            assert!(
                (document["features"][4]["pattern"]["axis"][1]
                    .as_f64()
                    .unwrap()
                    + 1.0)
                    .abs()
                    < 1e-9
            );
            let mut wire = serde_json::to_value(&model.ir).unwrap();
            for feature in wire["model"]["features"].as_array_mut().unwrap() {
                if feature["name"] == "PolarPattern" {
                    feature["definition"]["value"]["operation"]["value"]["pattern"]["axis_dir"] =
                        json!({"x": 0.0, "y": 0.0, "z": 1.0});
                }
            }
            if let Some(cli) = cli.clone() {
                let axis_only = Model {
                    ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
                    ..model.clone()
                };
                let (_, report) = convert(
                    &axis_only,
                    &SolidCraftWriter::new().verification(Verification::With(cli)),
                );
                // A measured difference removes the unconfirmed body-building recipe.
                assert_eq!(report.outcome().unwrap().mode, Mode::Import);
                assert!(
                    history_line(&report).contains("replay differs"),
                    "{}",
                    history_line(&report)
                );
                assert!(history_line(&report).contains("volume"));
            }
            for feature in wire["model"]["features"].as_array_mut().unwrap() {
                if feature["name"] == "PolarPattern" {
                    feature["definition"]["value"]["operation"]["value"]["pattern"]["angles"] =
                        json!((0..8)
                            .map(|i| i as f64 * std::f64::consts::TAU / 8.0)
                            .collect::<Vec<_>>());
                }
            }
            Model {
                ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
                ..model
            }
        } else {
            model
        };
        if let Some(cli) = cli {
            let (design, report) = written_with(
                &model,
                &SolidCraftWriter::new()
                    .verification(Verification::With(cli.clone()))
                    .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_25_1)),
            );
            assert_eq!(
                report.outcome().unwrap().mode,
                Mode::History,
                "{file}: {}",
                history_line(&report)
            );
            assert!(
                history_line(&report).contains("verified against the original"),
                "{file}: {}",
                history_line(&report)
            );
            let v = volumes(&cli, &design);
            assert_eq!(v.len(), 1);
            assert!((v[0] - volume).abs() / volume < 1e-4, "{file}: {v:?}");
        }
    }
}

/// Reuse the sweep fixture's mapped path sketch, lengthened to contain three copies.
fn path_pattern(whole_sketch: bool) -> Model {
    let mut model = sweep_history(|_| {});
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for row in wire["model"]["sketch_entities"].as_array_mut().unwrap() {
        if row["id"] == "f3d:model:sketch-entity#21" {
            row["geometry"]["end"]["v"] = json!(60.0);
        }
    }
    let features = wire["model"]["features"].as_array_mut().unwrap();
    features[2] = serde_json::to_value(&cube_with_history(10.0, first_profile(), None).ir).unwrap()
        ["model"]["features"][1]
        .clone();
    features[2]["ordinal"] = json!(2);
    let path = if whole_sketch {
        json!({"kind": "sketch", "value": PATH})
    } else {
        json!({"kind": "sketch_curves", "value": {"sketch": PATH,
            "curves": ["f3d:model:sketch-entity#21"]}})
    };
    features.push(json!({"id": "f3d:model:feature#4", "ordinal": 3, "name": "Copies", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "pattern", "value": {"seeds": [{"kind": "feature", "value": "f3d:model:feature#2"}], "pattern": {"kind": "curve_driven", "path": path, "spacing": 20.0, "count": 3}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn path_patterns_use_mapped_sketch_curves_and_arc_length_spacing() {
    for whole_sketch in [false, true] {
        let model = path_pattern(whole_sketch);
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).starts_with("4 of 4 features rebuilt"));
        let pattern = &document["features"][3]["pattern"];
        assert_eq!(pattern["type"], "path");
        assert_eq!(pattern["path_sketch"], document["features"][1]["id"]);
        assert_eq!(pattern["count"], "3");
        assert_eq!(pattern["spacing"], "20 mm");
        assert_eq!(pattern["path"].as_array().unwrap().len(), 1);
        if let Some(cli) = solidcraft() {
            let mut starts: Vec<f64> = bounds(&cli, &written(&model).0)
                .iter()
                .map(|(min, _)| min[2])
                .collect();
            starts.sort_by(f64::total_cmp);
            assert_eq!(starts, [0.0, 20.0, 40.0]);
        }
    }
}

#[test]
fn pattern_refusals_identify_the_unrepresentable_option() {
    for (layout, reason) in [
        (
            json!({"kind": "linear_offsets", "direction": {"x": 1.0, "y": 0.0, "z": 0.0},
            "offsets": [0.0, 20.0, 60.0]}),
            "nonuniform linear offsets",
        ),
        (
            json!({"kind": "circular_angles", "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0}, "angles": [0.0, 0.5, 2.0]}),
            "nonuniform circular angles",
        ),
        (
            json!({"kind": "curve_driven", "path": {"kind": "native", "value": "path"},
            "spacing": 20.0, "count": 3}),
            "pattern paths from native",
        ),
        (
            json!({"kind": "curve_driven", "spacing": 20.0, "count": 3}),
            "its pattern path is not resolved",
        ),
        (
            json!({"kind": "unresolved", "form": "curve_driven"}),
            "cadmpeg does not decode its curve driven pattern layout yet",
        ),
        (
            json!({"kind": "composite", "stages": [
                {"pattern": {"kind": "circular", "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0}, "angle": 1.0, "count": 2}},
                {"pattern": {"kind": "circular", "axis_origin": {"x": 5.0, "y": 0.0, "z": 0.0},
                    "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0}, "angle": 1.0, "count": 2}}
            ]}),
            "2 stages (circular then circular)",
        ),
        (
            json!({"kind": "scale", "center": {"kind": "first_seed_centroid"},
            "final_factor": 2.0, "count": 3}),
            "progressively scaled patterns",
        ),
        (
            json!({"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 10001, "omitted_instances": [], "symmetric": false}),
            "more than 10000 instances",
        ),
    ] {
        let (_, report) = convert(
            &patterned(layout),
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn unresolved_pattern_seeds_and_mixed_seed_kinds_have_specific_refusals() {
    let layout = json!({"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3, "omitted_instances": [], "symmetric": false});
    for (seeds, reason) in [
        (
            json!([{"kind": "bodies", "value": {"kind": "unresolved"}}]),
            "cadmpeg does not resolve which bodies",
        ),
        (
            json!([{"kind": "bodies", "value": {"kind": "native", "value": "body"}}]),
            "cadmpeg does not resolve which bodies",
        ),
        (
            json!([{"kind": "bodies", "value": {"kind": "native_set", "value": ["body"]}}]),
            "cadmpeg does not resolve which bodies",
        ),
        (
            json!([{"kind": "faces", "value": {"kind": "native", "value": "face"}}]),
            "SolidCraft patterns repeat whole features or bodies",
        ),
        (
            json!([{"kind": "feature", "value": "f3d:model:feature#2"},
            {"kind": "bodies", "value": {"kind": "bodies", "value": ["synthetic:cube:body#0"]}}]),
            "SolidCraft requires one seed kind",
        ),
    ] {
        let model = pattern_seeds(patterned(layout.clone()), seeds);
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn patterns_refuse_unnamed_bodies_and_excessive_rectangular_products() {
    let model = pattern_seeds(
        patterned(
            json!({"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3, "omitted_instances": [], "symmetric": false}),
        ),
        json!([{"kind": "bodies", "value": {"kind": "bodies", "value": ["synthetic:cube:body#0"]}}]),
    );
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["outputs"] = json!([]);
    let unnamed = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (_, report) = convert(
        &unnamed,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).contains("body without an earlier mapped name"));

    let too_many = patterned(
        json!({"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 101, "second": {"direction": {"x": 0.0, "y": 1.0, "z": 0.0}, "spacing": 20.0, "count": 100, "symmetric": false}, "omitted_instances": [], "symmetric": false}),
    );
    let (_, report) = convert(
        &too_many,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).contains("more than 10000 instances"));
}

#[test]
fn a_feature_pattern_names_its_seed() {
    let model = patterned(
        json!({"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3, "omitted_instances": [], "symmetric": false}),
    );
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let pattern = &document["features"][2];
    assert_eq!(pattern["type"], "pattern");
    assert_eq!(pattern["features"], json!(["Block"]));
}

#[test]
fn solidcraft_replays_feature_patterns() {
    let Some(cli) = solidcraft() else { return };
    let linear = patterned(
        json!({"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3, "omitted_instances": [], "symmetric": false}),
    );
    let mut copies = volumes(&cli, &written(&linear).0);
    copies.sort_by(f64::total_cmp);
    assert_eq!(copies.iter().sum::<f64>(), 3000.0, "{copies:?}");
    // Spacing is between instances: the third copy spans 40 to 50 mm.
    let mut starts: Vec<f64> = bounds(&cli, &written(&linear).0)
        .iter()
        .map(|(min, _)| min[0])
        .collect();
    starts.sort_by(f64::total_cmp);
    assert_eq!(starts, [0.0, 20.0, 40.0]);

    let around = patterned(json!({"kind": "circular",
        "axis_origin": {"x": -20.0, "y": 0.0, "z": 0.0},
        "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0},
        "angle": std::f64::consts::TAU, "count": 4}));
    let copies = volumes(&cli, &written(&around).0);
    assert!(
        (copies.iter().sum::<f64>() - 4000.0).abs() < 1e-6,
        "{copies:?}"
    );
}

#[test]
fn partial_circular_patterns_include_both_ends() {
    let model = patterned(json!({"kind": "circular",
        "axis_origin": {"x": -20.0, "y": 0.0, "z": 0.0},
        "axis_dir": {"x": 0.0, "y": 0.0, "z": 2.0},
        "angle": std::f64::consts::FRAC_PI_2, "count": 3}));
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
    assert_eq!(document["features"][2]["features"], json!(["Block"]));
    assert_eq!(
        document["features"][2]["pattern"],
        json!({"type": "circular", "origin": [-20.0, 0.0, 0.0],
            "axis": [0.0, 0.0, 1.0], "count": "3", "angle": "90 deg"})
    );

    let Some(cli) = solidcraft() else { return };
    let design = written(&model).0;
    let mut boxes = bounds(&cli, &design);
    boxes.sort_by(|(a, _), (b, _)| b[0].total_cmp(&a[0]));
    let root2 = std::f64::consts::SQRT_2;
    // Rotate the seed's [0, 10]^3 box about (-20, 0, 0) by 0, 45 and 90 degrees.
    // The middle box distinguishes a total span from a per-instance angle.
    let expected = [
        ([0.0, 0.0, 0.0], [10.0, 10.0, 10.0]),
        (
            [-20.0 + 5.0 * root2, 10.0 * root2, 0.0],
            [-20.0 + 15.0 * root2, 20.0 * root2, 10.0],
        ),
        ([-30.0, 20.0, 0.0], [-20.0, 30.0, 10.0]),
    ];
    assert_eq!(boxes.len(), expected.len());
    for ((min, max), (want_min, want_max)) in boxes.iter().zip(expected) {
        for axis in 0..3 {
            assert!((min[axis] - want_min[axis]).abs() < 1e-6, "{boxes:?}");
            assert!((max[axis] - want_max[axis]).abs() < 1e-6, "{boxes:?}");
        }
    }
    let copies = volumes(&cli, &design);
    assert_eq!(copies.len(), 3);
    assert!(copies.iter().all(|volume| (volume - 1000.0).abs() < 1e-6));
}

/// Each body's bounding box (min, max) as SolidCraft evaluates the design.
fn bounds(cli: &Path, design: &[u8]) -> Vec<([f64; 3], [f64; 3])> {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("design.solidcraft");
    let output = dir.path().join("eval.json");
    std::fs::write(&input, design).unwrap();
    let status = std::process::Command::new(cli)
        .arg("eval")
        .arg(&input)
        .arg("--json-out")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let result: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    let corner = |v: &Value| [0, 1, 2].map(|i| v[i].as_f64().unwrap());
    result["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (corner(&b["bbox"]["min"]), corner(&b["bbox"]["max"])))
        .collect()
}

#[test]
fn a_face_colour_becomes_a_face_look_on_that_face() {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for face in wire["geometry"]["faces"].as_array_mut().unwrap() {
        if face["id"] == "synthetic:cube:face#top" {
            face["color"] = json!({"r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0});
        }
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let faces = document["appearances"]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 1, "only the face that differs from its body");
    let look = &faces[0];
    assert_eq!(look["body"], "unit cube");
    assert_eq!(look["look"]["color"], json!([0, 0, 255]));
    let p: Vec<f64> = look["point"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_f64().unwrap())
        .collect();
    assert!(
        (p[2] - 10.0).abs() < 1e-9 && (0.0..10.0).contains(&p[0]) && (0.0..10.0).contains(&p[1]),
        "{p:?}"
    );

    if let Some(cli) = solidcraft() {
        assert_eq!(volumes(&cli, &written(&model).0), [1000.0]);
    }
}

#[test]
fn freecad_periodic_spline_recipes_keep_their_history() {
    // cadmpeg expands compact periodic sketch knots, so both native repros convert in full.
    for name in ["N04a.FCStd", "N04b.FCStd"] {
        let input = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures/freecad-1.1.4-N04")
            .join(name);
        let model = dutchman_core::read(
            dutchman_core::Format::FreeCadDocument,
            &input,
            &mut Report::default(),
        )
        .unwrap();
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("2 of 2 features rebuilt"),
            "{name}: {}",
            history_line(&report)
        );
        let Some(cli) = solidcraft() else { continue };
        let (_, report) = written_with(
            &model,
            &SolidCraftWriter::new().verification(Verification::With(cli)),
        );
        assert_eq!(report.outcome().unwrap().check, Check::Verified, "{name}");
    }
}

fn nonuniform_spline_geometry() -> Value {
    json!({"kind": "nurbs", "curve": {"degree": 3, "periodic": false,
        "knots": [0.,0.,0.,0.,0.3,1.,1.,1.,1.],
        "poles": {"kind": "polynomial", "points": [
            {"u": 0., "v": 0.}, {"u": 1., "v": 4.}, {"u": 4., "v": 6.},
            {"u": 8., "v": 4.}, {"u": 10., "v": 0.}]}}})
}

#[test]
fn nonuniform_spline_profiles_keep_one_closed_loop_and_extrude() {
    for profile in [
        first_profile(),
        json!({"kind": "sketch_entities", "value": {"sketch": SKETCH,
            "entities": ["f3d:model:sketch-entity#1", "f3d:model:sketch-entity#2"]}}),
        json!({"kind": "sketch_regions", "value": {"sketch": SKETCH,
            "regions": [{"kind": "loops", "outer": 0, "holes": []}]}}),
    ] {
        let mut model = cube_with_history(10., profile, None);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let m = &mut wire["model"];
        m["sketch_entities"] = json!([
            entity(1, nonuniform_spline_geometry()),
            line(2, [10., 0.], [0., 0.])
        ]);
        m["sketches"][0]["profiles"] = json!([[{"entity": "f3d:model:sketch-entity#1", "reversed": false}, {"entity": "f3d:model:sketch-entity#2", "reversed": false}]]);
        m["sketch_constraints"] = json!([]);
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("2 of 2 features rebuilt"),
            "{:?}",
            report.losses()
        );
        let sketch = &document["features"][0]["sketch"];
        let curves = sketch["curves"].as_array().unwrap();
        assert_eq!(curves.len(), 3);
        let first = curves[0]["pts"].as_array().unwrap();
        let second = curves[1]["pts"].as_array().unwrap();
        assert_eq!(first.last(), second.first());
        assert_eq!(first.first().unwrap(), &curves[2]["b"]);
        assert_eq!(second.last().unwrap(), &curves[2]["a"]);
        assert!(sketch["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["type"] == "tangent" && c["a"] == 0 && c["b"] == 1));
        assert_eq!(document["features"][1]["type"], "extrude");
        let selection = &document["features"][1]["profiles"];
        assert!(
            selection == &json!({"type": "all"})
                || selection == &json!({"type": "curves", "loops": [["s1", "s2", "l1"]]})
                || selection
                    == &json!({"type": "whole_boundaries", "profiles": [{"outer": ["l1", "s1", "s2"], "holes": []}]}),
            "{selection}"
        );
        if let Some(cli) = solidcraft() {
            let volume = volumes(&cli, &written(&model).0);
            assert_eq!(volume.len(), 1, "{volume:?}");
            assert!(volume[0] > 0.0, "{volume:?}");
        }
    }
}

#[test]
fn nonuniform_spline_sweep_paths_expand_the_source_curve() {
    for whole in [false, true] {
        let mut model = sweep_history(|d| {
            d["path"] = if whole {
                json!({"kind": "sketch", "value": PATH})
            } else {
                json!({"kind": "sketch_curves", "value": {"sketch": PATH, "curves": ["f3d:model:sketch-entity#20"]}})
            };
        });
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        for entity in wire["model"]["sketch_entities"].as_array_mut().unwrap() {
            if entity["sketch"] == PATH {
                entity["id"] = json!("f3d:model:sketch-entity#20");
                entity["geometry"] = nonuniform_spline_geometry();
            }
        }
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("3 of 3 features rebuilt"),
            "{:?}",
            report.losses()
        );
        let sweep = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "sweep")
            .unwrap();
        assert_eq!(sweep["path"], json!(["s1", "s2"]));
    }
}

#[test]
fn nonuniform_spline_constraints_follow_chain_endpoints_and_fixed_pieces() {
    let mut model = bounded_conic_history(nonuniform_spline_geometry(), [0., 0.], [10., 0.]);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let mut tangent = line(3, [10., 0.], [12., -4.]);
    tangent["construction"] = json!(true);
    wire["model"]["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .push(tangent);
    wire["model"]["sketch_constraints"] = json!([
        {"id": "f3d:model:sketch-constraint#1", "sketch": SKETCH,
         "definition": {"kind": "fixed", "entity": "f3d:model:sketch-entity#1"}},
        {"id": "f3d:model:sketch-constraint#2", "sketch": SKETCH,
         "definition": {"kind": "coincident_loci", "loci": [
             {"kind": "end", "value": "f3d:model:sketch-entity#1"},
             {"kind": "start", "value": "f3d:model:sketch-entity#2"}]}},
        {"id": "f3d:model:sketch-constraint#3", "sketch": SKETCH,
         "definition": {"kind": "distance", "entities": ["f3d:model:sketch-entity#1"], "parameter": "f3d:model:parameter#2"}},
        {"id": "f3d:model:sketch-constraint#4", "sketch": SKETCH,
         "definition": {"kind": "tangent_loci", "first": {"kind": "end", "value": "f3d:model:sketch-entity#1"},
             "second": {"kind": "start", "value": "f3d:model:sketch-entity#3"}}},
        {"id": "f3d:model:sketch-constraint#5", "sketch": SKETCH,
         "definition": {"kind": "curvature", "first": "f3d:model:sketch-entity#1", "second": "f3d:model:sketch-entity#3"}}
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{:?}",
        report.losses()
    );
    let curves = document["features"][0]["sketch"]["curves"]
        .as_array()
        .unwrap();
    assert_eq!(curves[0]["fixed"], true);
    assert_eq!(curves[1]["fixed"], true);
    let constraints = document["features"][0]["sketch"]["constraints"]
        .as_array()
        .unwrap();
    for kind in ["tangent", "smooth"] {
        assert!(
            constraints
                .iter()
                .any(|c| c["type"] == kind && c["a"] == 1 && c["b"] == 3),
            "{constraints:?}"
        );
    }
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.severity == dutchman_core::Severity::Info
            && loss
                .detail
                .contains("a constraint on an exact spline chain is not carried")));
    assert!(!report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("constraints not carried")));
}

#[test]
fn nonuniform_spline_projected_copies_keep_every_control_point() {
    let mut model = bounded_conic_history(nonuniform_spline_geometry(), [0., 0.], [10., 0.]);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let mut copy = entity(3, nonuniform_spline_geometry());
    copy["construction"] = json!(true);
    wire["model"]["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .push(copy);
    wire["model"]["sketch_constraints"] = json!([
        {"id": "f3d:model:sketch-constraint#1", "sketch": SKETCH,
         "definition": {"kind": "projected_copy", "source": "f3d:model:sketch-entity#1", "result": "f3d:model:sketch-entity#3"}}
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{:?}",
        report.losses()
    );
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("not carried")),
        "{:?}",
        report.losses()
    );
    let sketch = &document["features"][0]["sketch"];
    assert_eq!(sketch["curves"].as_array().unwrap().len(), 5);
    assert_eq!(sketch["curves"][3]["construction"], true);
    assert_eq!(sketch["curves"][4]["construction"], true);
    let source: Vec<_> = sketch["curves"][0]["pts"]
        .as_array()
        .unwrap()
        .iter()
        .chain(
            sketch["curves"][1]["pts"]
                .as_array()
                .unwrap()
                .iter()
                .skip(1),
        )
        .collect();
    let result: Vec<_> = sketch["curves"][3]["pts"]
        .as_array()
        .unwrap()
        .iter()
        .chain(
            sketch["curves"][4]["pts"]
                .as_array()
                .unwrap()
                .iter()
                .skip(1),
        )
        .collect();
    let constraints = sketch["constraints"].as_array().unwrap();
    for (p, q) in source.into_iter().zip(result) {
        assert!(
            p == q || constraints.iter().any(|c| c["p"] == *p && c["q"] == *q),
            "{p} -> {q}: {constraints:?}"
        );
    }
}

fn periodic_spline_profile(profile: Value) -> Model {
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketches"][0]["profiles"] =
        json!([[{"entity": "f3d:model:sketch-entity#1", "reversed": false}]]);
    wire["model"]["sketch_constraints"] = json!([]);
    wire["model"]["sketch_entities"] = json!([entity(
        1,
        json!({"kind": "nurbs", "curve": {"degree": 2, "periodic": true,
            "knots": [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0],
            "poles": {"kind": "polynomial", "points": [
                {"u": -10.0, "v": -10.0}, {"u": 10.0, "v": -10.0},
                {"u": 10.0, "v": 10.0}, {"u": -10.0, "v": 10.0},
                {"u": -10.0, "v": -10.0}, {"u": 10.0, "v": -10.0}]}}})
    )]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn periodic_spline_profiles_keep_editable_history_and_profile_selections() {
    for profile in [whole_sketch(), first_profile()] {
        let model = periodic_spline_profile(profile);
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert_eq!(report.outcome().unwrap().mode, Mode::History, "{report:?}");
        let sketch = &document["features"][0]["sketch"];
        let curve = &sketch["curves"][0];
        assert_eq!(curve["type"], "spline");
        assert_eq!(curve["degree"], 2);
        assert_eq!(curve["control"], true);
        let pts = curve["pts"].as_array().unwrap();
        assert_eq!(pts.first(), pts.last());
        assert_eq!(pts.len(), 6);
        assert!(!history_line(&report).contains("not mapped"));
    }
}

#[test]
fn weighted_periodic_splines_name_solidcrafts_missing_weights() {
    let mut model = periodic_spline_profile(whole_sketch());
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let curve = &mut wire["model"]["sketch_entities"][0]["geometry"]["curve"];
    let points = curve["poles"]["points"].as_array().unwrap().clone();
    curve["poles"] = json!({"kind": "rational", "points": points.iter().enumerate()
        .map(|(i, p)| json!({"point": p, "weight": if i % 4 == 0 { 2.0 } else { 1.0 }})).collect::<Vec<_>>()});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(report.outcome().unwrap().mode, Mode::Import);
    assert!(history_line(&report).contains("weighted periodic splines are not supported yet: SolidCraft control splines have no weights"), "{report:?}");
}

#[test]
fn solidcraft_extrudes_a_closed_periodic_spline_profile() {
    let Some(cli) = solidcraft() else { return };
    for profile in [whole_sketch(), first_profile()] {
        let model = periodic_spline_profile(profile);
        let (bytes, report) = written(&model);
        assert_eq!(report.outcome().unwrap().mode, Mode::History, "{report:?}");
        let got = volumes(&cli, &bytes);
        // A 20 mm square with four quadratic corners removes 4 × 10²/6 mm².
        let expected = (400.0 - 400.0 / 6.0) * 10.0;
        assert_eq!(got.len(), 1, "{got:?}");
        assert!((got[0] - expected).abs() / expected < 1e-3, "{got:?}");
    }
}

#[test]
fn solidcraft_extrudes_a_spline_profile() {
    let Some(cli) = solidcraft() else { return };
    let mut model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketches"][0]["profiles"] = json!([]);
    wire["model"]["sketch_constraints"] = json!([]);
    wire["model"]["sketch_entities"] = json!([
        line(1, [0.0, 0.0], [30.0, 0.0]),
        entity(
            2,
            json!({"kind": "nurbs", "curve": {"degree": 2, "periodic": false,
            "knots": [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            "poles": {"kind": "polynomial", "points": [
                {"u": 0.0, "v": 0.0}, {"u": 15.0, "v": 30.0}, {"u": 30.0, "v": 0.0}]}}})
        ),
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    // Under a quadratic arch: base × control height / 3 = 300 mm², extruded 10 mm.
    let got = volumes(&cli, &written(&model).0);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!((got[0] - 3000.0).abs() / 3000.0 < 1e-3, "{got:?}");
}

#[test]
fn solidcraft_colours_the_face_of_each_parts_own_body() {
    let Some(cli) = solidcraft() else { return };
    let dir = tempfile::tempdir().unwrap();
    let design = dir.path().join("pair.solidcraft");
    std::fs::write(&design, written(&assembly_with_face_colours()).0).unwrap();
    let script = dir.path().join("looks.json");
    let list = |body: &str| json!({"command": "appearance.list", "params": {"body": body}});
    std::fs::write(
        &script,
        json!([list("unit cube (Second)"), list("unit cube")]).to_string(),
    )
    .unwrap();
    let output = dir.path().join("looks.out.json");
    let status = std::process::Command::new(cli)
        .arg("run")
        .arg(&script)
        .arg("--in")
        .arg(&design)
        .arg("--results")
        .arg("--json-out")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let results: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    let face_colours = |result: &Value| -> Vec<String> {
        result["body"]["faces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|face| face["appearance"]["color"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(face_colours(&results[0]), ["#0000ff"], "{results}");
    assert!(face_colours(&results[1]).is_empty(), "{results}");
}

fn mirrored(origin: [f64; 3], normal: [f64; 3]) -> Model {
    patterned(json!({"kind": "mirror",
        "plane_origin": {"x": origin[0], "y": origin[1], "z": origin[2]},
        "plane_normal": {"x": normal[0], "y": normal[1], "z": normal[2]}}))
}

#[test]
fn a_mirror_of_features_becomes_a_solidcraft_mirror() {
    let (document, report) = convert(
        &mirrored([20.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let mirror = &document["features"][2];
    assert_eq!(mirror["type"], "mirror");
    assert_eq!(mirror["name"], "Copies");
    assert_eq!(mirror["features"], json!(["Block"]));
    let plane = &mirror["plane"]["plane"];
    assert_eq!(mirror["plane"]["type"], "custom");
    assert_eq!(plane["origin"], json!([20.0, 0.0, 0.0]));
    // The plane is spanned by two directions square to the normal.
    for key in ["x_dir", "y_dir"] {
        assert!(plane[key][0].as_f64().unwrap().abs() < 1e-12, "{plane}");
    }
}

#[test]
fn solidcraft_mirrors_the_features_across_the_plane() {
    let Some(cli) = solidcraft() else { return };
    let h = std::f64::consts::FRAC_1_SQRT_2;
    for (origin, normal, copy_min, copy_max) in [
        // Across x = 20: the copy spans 30 to 40 along X.
        (
            [20.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [30.0, 0.0, 0.0],
            [40.0, 10.0, 10.0],
        ),
        // Across x + y = 0: (x, y) goes to (-y, -x).
        (
            [0.0, 0.0, 0.0],
            [h, h, 0.0],
            [-10.0, -10.0, 0.0],
            [0.0, 0.0, 10.0],
        ),
    ] {
        let design = written(&mirrored(origin, normal)).0;
        let total: f64 = volumes(&cli, &design).iter().sum();
        assert!((total - 2000.0).abs() < 1e-6, "{total}");
        let boxes = bounds(&cli, &design);
        let near = |a: [f64; 3], b: [f64; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-6);
        assert!(
            boxes
                .iter()
                .any(|(min, max)| near(*min, copy_min) && near(*max, copy_max)),
            "{normal:?}: {boxes:?}"
        );
    }
}

/// The example cube's model with a history of one primitive feature, `definition`.
fn primitive_history(definition: Value) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    model["features"] = json!([{"id": "f3d:model:feature#2", "ordinal": 0, "name": "Ball", "suppressed": false, "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": definition}, "dependencies": [], "source_properties": {}, "source_content": []}]);
    model["parameters"] = json!([]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

fn sphere() -> Value {
    json!({"kind": "sphere", "value": {"center": {"x": 5.0, "y": 5.0, "z": 5.0}, "radius": 5.0, "op": "new_body"}})
}

fn torus(axis: [f64; 3]) -> Value {
    json!({"kind": "torus", "value": {"center": {"x": 0.0, "y": 0.0, "z": 2.0}, "axis": {"x": axis[0], "y": axis[1], "z": axis[2]}, "major_radius": 10.0, "minor_radius": 2.0, "op": "new_body"}})
}

#[test]
fn spheres_and_tori_become_solidcraft_primitives() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&primitive_history(sphere()), &writer);
    let ball = &document["features"][0];
    assert_eq!(ball["type"], "sphere");
    assert_eq!(ball["name"], "Ball");
    assert_eq!(ball["center"], json!([5.0, 5.0, 5.0]));
    assert_eq!(ball["radius"], "5 mm");
    assert_eq!(ball["operation"], "new_body");
    assert_eq!(ball["body_names"], json!(["unit cube"]));
    assert!(
        history_line(&report).contains("1 of 1 features rebuilt"),
        "{}",
        history_line(&report)
    );

    let (document, _) = convert(&primitive_history(torus([0.0, 0.0, -1.0])), &writer);
    let ring = &document["features"][0];
    assert_eq!(ring["type"], "torus");
    assert_eq!(ring["center"], json!([0.0, 0.0, 2.0]));
    assert_eq!(
        (ring["major"].clone(), ring["minor"].clone()),
        (json!("10 mm"), json!("2 mm"))
    );
}

fn placed_primitive(solid: Value) -> Value {
    // Its frame's z axis along model +y, its origin at (1, 2, 3).
    json!({"kind": "primitive", "value": {"solid": solid, "placement": [[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 2.0], [0.0, -1.0, 0.0, 3.0]], "op": "new_body"}})
}

#[test]
fn placed_cylinders_and_cones_become_solidcraft_primitives() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let full = std::f64::consts::TAU;
    let (document, _) = convert(
        &primitive_history(placed_primitive(
            json!({"kind": "cylinder", "radius": 2.5, "height": 26.0, "angle": full}),
        )),
        &writer,
    );
    let pin = &document["features"][0];
    assert_eq!(pin["type"], "cylinder");
    assert_eq!(pin["base"], json!([1.0, 2.0, 3.0]));
    assert_eq!(pin["axis"], json!([0.0, 1.0, 0.0]));
    assert_eq!(
        (pin["radius"].clone(), pin["height"].clone()),
        (json!("2.5 mm"), json!("26 mm"))
    );

    let cone = placed_primitive(
        json!({"kind": "cone", "radius1": 4.0, "radius2": 1.0, "height": 6.0, "angle": full}),
    );
    // Cones exist from SolidCraft 0.24.1: name that target rather than whichever SolidCraft
    // the machine running the test has.
    let with_cones = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.24.1".parse().unwrap()));
    let (document, _) = convert(&primitive_history(cone.clone()), &with_cones);
    let tip = &document["features"][0];
    assert_eq!(tip["type"], "cone");
    assert_eq!(
        (
            tip["radius"].clone(),
            tip["top_radius"].clone(),
            tip["height"].clone()
        ),
        (json!("4 mm"), json!("1 mm"), json!("6 mm"))
    );

    // A cone needs SolidCraft 0.24.1; a part-turn cylinder has no SolidCraft form.
    let older = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.23.0".parse().unwrap()));
    let (_, report) = convert(&primitive_history(cone), &older);
    assert!(
        history_line(&report).contains("a cone needs SolidCraft 0.24 patch 1 or newer"),
        "{}",
        history_line(&report)
    );
    let (_, report) = convert(
        &primitive_history(placed_primitive(
            json!({"kind": "cylinder", "radius": 2.5, "height": 26.0, "angle": 3.0}),
        )),
        &writer,
    );
    assert!(
        history_line(&report).contains("a part-turn cylinder is not mapped yet"),
        "{}",
        history_line(&report)
    );
}

/// A box (a block) of `size` along its frame's x, y and z from the frame origin, its minimum
/// corner; `placement` gives the frame as three rows of a 3x4 transform.
fn block(size: [f64; 3], placement: [[f64; 4]; 3]) -> Value {
    json!({"kind": "block", "value": {"dimensions": size, "placement": placement, "op": "new_body"}})
}

#[test]
fn boxes_along_the_model_axes_become_solidcraft_boxes() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    // The frame at the origin: the box is the cube itself.
    let identity = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let (document, report) = convert(&primitive_history(block([10.0; 3], identity)), &writer);
    let cube = &document["features"][0];
    assert_eq!(cube["type"], "box");
    assert_eq!(cube["corner"], json!([0.0, 0.0, 0.0]));
    assert_eq!(
        (
            cube["length"].clone(),
            cube["width"].clone(),
            cube["height"].clone()
        ),
        (json!("10 mm"), json!("10 mm"), json!("10 mm"))
    );
    assert_eq!(cube["operation"], "new_body");
    assert!(
        history_line(&report).contains("1 of 1 features rebuilt"),
        "{}",
        history_line(&report)
    );

    // RailCore's box frame: its x along model +y and its y along model -x, from (10, 0, 0).
    // A 2 x 3 x 4 box then spans x 7..10, y 0..2 and z 0..4.
    let turned = [
        [0.0, -1.0, 0.0, 10.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let (document, _) = convert(&primitive_history(block([2.0, 3.0, 4.0], turned)), &writer);
    let slab = &document["features"][0];
    assert_eq!(slab["corner"], json!([7.0, 0.0, 0.0]));
    assert_eq!(
        (
            slab["length"].clone(),
            slab["width"].clone(),
            slab["height"].clone()
        ),
        (json!("3 mm"), json!("2 mm"), json!("4 mm"))
    );
}

#[test]
fn primitives_solidcraft_cannot_place_flatten_the_history() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    for (definition, reason) in [
        (
            torus([1.0, 0.0, 0.0]),
            "a torus about an axis other than Z is not mapped yet",
        ),
        (
            // Turned 45 degrees about z: SolidCraft's box is axis-aligned.
            block(
                [10.0, 10.0, 10.0],
                [
                    [
                        std::f64::consts::FRAC_1_SQRT_2,
                        -std::f64::consts::FRAC_1_SQRT_2,
                        0.0,
                        0.0,
                    ],
                    [
                        std::f64::consts::FRAC_1_SQRT_2,
                        std::f64::consts::FRAC_1_SQRT_2,
                        0.0,
                        0.0,
                    ],
                    [0.0, 0.0, 1.0, 0.0],
                ],
            ),
            "a box turned off the model axes is not mapped yet",
        ),
    ] {
        let (_, report) = convert(&primitive_history(definition), &writer);
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn solidcraft_builds_the_primitives() {
    let Some(cli) = solidcraft() else { return };
    let pi = std::f64::consts::PI;
    let turned = [
        [0.0, -1.0, 0.0, 10.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    for (definition, expected) in [
        (sphere(), 4.0 / 3.0 * pi * 125.0),
        (torus([0.0, 0.0, 1.0]), 2.0 * pi * pi * 10.0 * 4.0),
        (block([2.0, 3.0, 4.0], turned), 24.0),
    ] {
        let volume = volumes(&cli, &written(&primitive_history(definition)).0);
        assert_eq!(volume.len(), 1);
        // SolidCraft measures curved solids numerically (a torus to about 1.5e-4 of the
        // formula); the history check compares SolidCraft with itself, where that cancels.
        assert!(
            (volume[0] - expected).abs() / expected < 1e-3,
            "{volume:?} vs {expected}"
        );
    }
}

/// A coil up Z from the origin, starting along `radial`: 20 mm across, of a 2 mm round
/// section, `extent` long, turning clockwise as CADIR sees it (from its base, along its axis)
/// when `clockwise`.
fn coil(extent: Value, radial: [f64; 3], clockwise: bool) -> Value {
    json!({"kind": "coil", "value": {"construction": {
               "placement": {"kind": "explicit",
                   "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                   "axis": {"x": 0.0, "y": 0.0, "z": 1.0},
                   "radial": {"x": radial[0], "y": radial[1], "z": radial[2]}},
               "diameter": 20.0,
               "extent": extent,
               "section": {"kind": "circular", "diameter": 2.0},
               "section_placement": "center",
               "clockwise": clockwise,
               "taper": 0.0}, "result": {"kind": "new_body"}}})
}

fn turns_and_pitch(revolutions: f64, pitch: f64) -> Value {
    json!({"kind": "revolutions_pitch", "revolutions": revolutions, "pitch": pitch})
}

#[test]
fn coils_become_solidcraft_coils() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(
        &primitive_history(coil(turns_and_pitch(4.0, 5.0), [1.0, 0.0, 0.0], true)),
        &writer,
    );
    let spring = &document["features"][0];
    assert_eq!(spring["type"], "coil");
    assert_eq!(spring["base"], json!([0.0, 0.0, 0.0]));
    assert_eq!(spring["axis"], json!([0.0, 0.0, 1.0]));
    assert_eq!(
        [&spring["diameter"], &spring["turns"], &spring["pitch"]],
        [&json!("20 mm"), &json!("4"), &json!("5 mm")]
    );
    assert_eq!(
        (&spring["section"], &spring["section_size"]),
        (&json!("circular"), &json!("2 mm"))
    );
    // Clockwise seen from the base is anticlockwise seen from above: SolidCraft's default.
    assert_eq!(spring.get("clockwise"), None);
    assert_eq!(spring.get("start_angle"), None);
    assert_eq!(spring.get("position"), None);
    assert!(
        history_line(&report).contains("1 of 1 features rebuilt"),
        "{}",
        history_line(&report)
    );

    let mut other = coil(
        json!({"kind": "revolutions_height", "revolutions": 2.0, "height": 8.0}),
        [0.0, 1.0, 0.0],
        false,
    );
    other["value"]["construction"]["section"] = json!({"kind": "square", "size": 1.5});
    other["value"]["construction"]["section_placement"] = json!("inside");
    other["value"]["result"] = json!({"kind": "boolean", "operation": "join", "targets": {"kind": "bodies", "value": ["synthetic:cube:body#0"]}});
    let (document, _) = convert(&primitive_history(other), &writer);
    let spring = &document["features"][0];
    assert_eq!(spring["clockwise"], true);
    assert_eq!(spring["start_angle"], "90 deg");
    assert_eq!(spring["position"], -1);
    assert_eq!(spring["pitch"], "(8 mm) / (2)");
    assert_eq!(spring["section"], "square");
    assert_eq!(spring["operation"], "join");
}

#[test]
fn coils_map_signed_extents_and_reverse_the_axis_and_turning_flag_together() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    for sign in [1.0, -1.0] {
        for (extent, turns, pitch) in [
            (turns_and_pitch(2.0, sign * 4.0), "2", "4 mm"),
            (
                json!({"kind": "revolutions_height", "revolutions": 2.0, "height": sign * 8.0}),
                "2",
                "(8 mm) / (2)",
            ),
            (
                json!({"kind": "height_pitch", "height": sign * 8.0, "pitch": sign * 4.0}),
                "(8 mm) / (4 mm)",
                "4 mm",
            ),
        ] {
            for clockwise in [true, false] {
                let mut definition = coil(extent.clone(), [0.0, 1.0, 0.0], clockwise);
                definition["value"]["construction"]["placement"]["axis"] =
                    json!({"x": 1.0, "y": 0.0, "z": 0.0});
                let (document, report) = convert(&primitive_history(definition), &writer);
                let spring = &document["features"][0];
                assert_eq!(spring["type"], "coil", "{}", history_line(&report));
                assert_eq!(spring["axis"], json!([sign, 0.0, 0.0]));
                assert_eq!(spring["start_angle"], "-90 deg");
                assert_eq!(spring["turns"], turns);
                assert_eq!(spring["pitch"], pitch);
                assert_eq!(
                    spring.get("clockwise"),
                    (clockwise == (sign < 0.0)).then_some(&json!(true))
                );
            }
        }
    }
}

#[test]
fn coils_solidcraft_cannot_build_flatten_the_history() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let plain = || coil(turns_and_pitch(4.0, 5.0), [1.0, 0.0, 0.0], true);
    let with = |path: &[&str], value: Value| {
        let mut definition = plain();
        let mut at = &mut definition["value"];
        for key in path {
            at = &mut at[*key];
        }
        *at = value;
        definition
    };
    for (definition, reason) in [
        (
            with(&["construction", "taper"], json!(0.1)),
            "a tapered coil is not mapped yet: SolidCraft's coil has no taper option",
        ),
        (
            with(&["construction", "taper"], json!(-0.1)),
            "a tapered coil is not mapped yet: SolidCraft's coil has no taper option",
        ),
        (
            with(
                &["construction", "extent"],
                json!({"kind": "spiral", "revolutions": 3.0, "radial_pitch": 2.0}),
            ),
            "a flat spiral coil is not mapped yet: SolidCraft's coil has no spiral mode",
        ),
        (
            with(
                &["construction", "extent"],
                json!({"kind": "spiral", "revolutions": 3.0, "radial_pitch": -2.0}),
            ),
            "a flat spiral coil is not mapped yet: SolidCraft's coil has no spiral mode",
        ),
        (
            with(
                &["construction", "extent"],
                json!({"kind": "height_pitch", "height": 8.0, "pitch": -4.0}),
            ),
            "its extent is malformed",
        ),
        (
            with(
                &["construction", "section"],
                json!({"kind": "external_triangle", "size": 2.0}),
            ),
            "a coil of triangular section is not mapped yet: SolidCraft's coil only has \
             circular and square sections",
        ),
        (
            with(
                &["construction", "section"],
                json!({"kind": "internal_triangle", "size": 2.0}),
            ),
            "a coil of triangular section is not mapped yet: SolidCraft's coil only has \
             circular and square sections",
        ),
    ] {
        let (document, report) = convert(&primitive_history(definition), &writer);
        assert_reference_sketches(&document, 1);
        let outcome = report.outcome().unwrap();
        assert_eq!(outcome.mode, Mode::Import);
        assert!(matches!(outcome.check, Check::Flattened(_)));
        assert_eq!(outcome.features.len(), 1);
        assert_eq!(outcome.features[0].kind, "coil");
        assert_eq!(outcome.features[0].reason.as_deref(), Some(reason));
        assert_eq!(
            outcome.features[0].status,
            dutchman_core::FeatureStatus::Blocked
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn solidcraft_turns_the_coil_the_way_cadir_says() {
    let Some(cli) = solidcraft() else { return };
    // Quarter turns expose the start direction and turning sense; a full turn would hide
    // both. Include SolidCraft's alternate start frame close to X, and its usual projected-X
    // frame on either side of that threshold. CADIR requires unit placement vectors.
    for (axis, radial) in [
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([1.0, 2.0, 3.0], [2.0, -1.0, 0.0]),
        ([1.0, 0.0, 0.0000005], [0.0, 1.0, 0.0]),
        ([1.0, 0.0, 0.000002], [0.0, 1.0, 0.0]),
    ] {
        let normalize = |v: [f64; 3]| {
            let length = v.iter().map(|c| c * c).sum::<f64>().sqrt();
            v.map(|c| c / length)
        };
        let axis = normalize(axis);
        let radial = normalize(radial);
        for clockwise in [true, false] {
            for pitch in [4.0, -4.0] {
                let mut definition = coil(turns_and_pitch(0.25, pitch), radial, clockwise);
                definition["value"]["construction"]["placement"]["axis"] =
                    json!({"x": axis[0], "y": axis[1], "z": axis[2]});
                let base = [3.0, -2.0, 5.0];
                definition["value"]["construction"]["placement"]["origin"] =
                    json!({"x": base[0], "y": base[1], "z": base[2]});
                let design = written(&primitive_history(definition)).0;
                let boxes = bounds(&cli, &design);
                assert_eq!(boxes.len(), 1, "{boxes:?}");
                let (min, max) = boxes[0];
                // Sample the CADIR helix directly, without using SolidCraft's start-angle frame.
                // Its section is a unit circle in the radial/axis plane at each point.
                let tangent = [
                    axis[1] * radial[2] - axis[2] * radial[1],
                    axis[2] * radial[0] - axis[0] * radial[2],
                    axis[0] * radial[1] - axis[1] * radial[0],
                ];
                let mut expected_min = [f64::INFINITY; 3];
                let mut expected_max = [f64::NEG_INFINITY; 3];
                for step in 0..=1000 {
                    let turns = 0.25 * f64::from(step) / 1000.0;
                    let angle = std::f64::consts::TAU * turns * if clockwise { 1.0 } else { -1.0 };
                    for i in 0..3 {
                        let r = radial[i] * angle.cos() + tangent[i] * angle.sin();
                        let center = base[i] + 10.0 * r + axis[i] * pitch * turns;
                        let half_width = r.hypot(axis[i]);
                        expected_min[i] = expected_min[i].min(center - half_width);
                        expected_max[i] = expected_max[i].max(center + half_width);
                    }
                }
                let case = format!("axis={axis:?}, clockwise={clockwise}, pitch={pitch}");
                for i in 0..3 {
                    assert!(
                        (min[i] - expected_min[i]).abs() < 0.05
                            && (max[i] - expected_max[i]).abs() < 0.05,
                        "{case}: {boxes:?} vs {expected_min:?}, {expected_max:?}"
                    );
                }
                let volume = volumes(&cli, &design);
                assert_eq!(volume.len(), 1);
                // The section's area along the helix: π × 1² × √((π × 20 / 4)² + 1²).
                let pi = std::f64::consts::PI;
                let expected = pi * ((pi * 5.0).powi(2) + 1.0).sqrt();
                assert!(
                    (volume[0] - expected).abs() / expected < 1e-2,
                    "{volume:?} vs {expected}"
                );
            }
        }
    }
}

#[test]
fn a_helix_surface_is_written_to_the_step_and_needs_no_note() {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["geometry"];
    // One of the cube's faces on a coil's swept surface, as Fusion stores a coil's.
    let surface = model["faces"][0]["surface"].clone();
    for row in model["surfaces"].as_array_mut().unwrap() {
        if row["id"] == surface {
            row["geometry"] = json!({"kind": "procedural", "value": {"construction": "synthetic:cube:procedural_surface#0"}});
        }
    }
    model["procedural_surfaces"] = json!([{"id": "synthetic:cube:procedural_surface#0", "surface": surface, "definition": {"kind": "helix", "construction": {
            "angle_range": [-std::f64::consts::PI, std::f64::consts::PI],
            "dimension_range": [0.0, 250.0],
            "path": {"angle_range": [0.0, 25.0],
                     "center": {"x": 0.0, "y": 0.0, "z": 0.0},
                     "major": {"x": 10.0, "y": 0.0, "z": 0.0},
                     "minor": {"x": 0.0, "y": 10.0, "z": 0.0},
                     "pitch": {"x": 0.0, "y": 0.0, "z": 5.0},
                     "apex_factor": 0.0,
                     "axis": {"x": 0.0, "y": 0.0, "z": 1.0}},
            "profile": {"kind": "circle", "angle": 0.0, "radius": 1.0}}}, "record_bounds": [null, null, null, null]}]);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let (_, report) = convert(&model, &SolidCraftWriter::new().import_only(true));
    // cadmpeg writes an evaluable helix surface as a sampled B-spline surface, so nothing is
    // left out and the report names nothing.
    assert!(
        report.losses().iter().all(|loss| loss.subject
            != "step/geometry.procedural-definition-not-written"
            && !loss.detail.contains("has no form cadmpeg writes")),
        "{report:?}"
    );
    assert!(
        report
            .losses()
            .iter()
            .any(|loss| loss.subject == "step/geometry.helix-fitted-to-bspline"),
        "{report:?}"
    );
}

/// The holed cube's history with its sketch and extrude replaced by a base feature holding
/// the cube's body as it is, as Fusion holds a body modelled directly or imported.
fn based_cube() -> Model {
    let mut model = holed_cube(4.0, |_| {});
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    let m = &mut wire["model"];
    let features = m["features"].as_array_mut().unwrap();
    features.retain(|feature| feature["ordinal"].as_u64() >= Some(2));
    features.push(json!({"id": "f3d:model:feature#1", "ordinal": 0, "name": "Base Feature 1", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "base_feature", "value": {"bodies": {"kind": "resolved", "value": {"bodies": [body], "native": "f3d:base#1"}}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

/// Synthetic stored geometry uses the same cube body as the base-feature tests.
fn stored_cube(with_hole: bool) -> Model {
    let mut model = based_cube();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    let features = wire["model"]["features"].as_array_mut().unwrap();
    if !with_hole {
        features.retain(|feature| feature["ordinal"] == 0);
    }
    let base = features.iter_mut().find(|f| f["ordinal"] == 0).unwrap();
    base["definition"] =
        json!({"kind":"operation", "value": {"kind": "stored_geometry", "value": {}}});
    base["outputs"] = json!([body]);
    base["name"] = json!("Stored solid");
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

const STORED_SOLID_MESSAGE: &str =
    "This part holds an imported solid; there is no feature history to bring over";

#[test]
fn stored_geometry_alone_becomes_an_import_without_lost_history() {
    let model = stored_cube(false);
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);
    assert_eq!(
        types(&document),
        [
            ("import".to_owned(), false),
            ("sketch".to_owned(), false),
            ("sketch".to_owned(), false),
        ]
    );
    let base = &document["features"][0];
    assert_eq!(base["name"], "Stored solid");
    assert!(base.get("_step").is_none() && base.get("_bodies").is_none());
    let (bytes, _) = written_with(&model, &writer);
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut step = String::new();
    archive
        .by_name(base["step_blob"].as_str().unwrap())
        .unwrap()
        .read_to_string(&mut step)
        .unwrap();
    assert!(step.contains("MANIFOLD_SOLID_BREP"), "{step}");
    let outcome = report.outcome().unwrap();
    assert_eq!(outcome.mode, Mode::Import);
    assert_eq!(
        outcome.check,
        Check::NotApplicable(STORED_SOLID_MESSAGE.to_owned())
    );
    assert_eq!(outcome.features.len(), 1);
    assert_eq!(
        outcome.features[0].status,
        dutchman_core::FeatureStatus::Mapped
    );
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.severity == Severity::Info && loss.detail == STORED_SOLID_MESSAGE));
    assert!(report
        .losses()
        .iter()
        .all(|loss| !loss.detail.contains("not mapped yet") && !loss.detail.contains("flattened")));
    let mut planned = Report::default();
    assert_eq!(
        outcome.features[0].output.ids,
        [base["id"].as_u64().unwrap()]
    );
    assert_eq!(
        outcome.features[0].output.method,
        dutchman_core::OutputMatch::Emitted
    );
    // Inspection saves no design; all existing outcome fields still agree.
    let mut expected_plan = outcome.clone();
    expected_plan.features[0].output = dutchman_core::FeatureOutput {
        ids: Vec::new(),
        method: dutchman_core::OutputMatch::NotWritten,
    };
    assert_eq!(writer.plan(&model, &mut planned), expected_plan);
    assert!(planned
        .losses()
        .iter()
        .any(|loss| loss.detail == STORED_SOLID_MESSAGE));
}

#[test]
fn stored_geometry_without_bodies_is_refused_specifically() {
    let mut model = stored_cube(false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][0]["outputs"] = json!([]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).contains("it holds no bodies"));
    assert!(!history_line(&report).contains("not mapped yet"));
    assert!(report
        .losses()
        .iter()
        .all(|loss| loss.detail != STORED_SOLID_MESSAGE));
}

#[test]
fn stored_geometry_keeps_the_features_after_it() {
    let (document, report) = convert(
        &stored_cube(true),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(
        types(&document),
        [
            ("import".to_owned(), false),
            ("sketch".to_owned(), false),
            ("hole".to_owned(), false),
            ("sketch".to_owned(), false),
            ("import".to_owned(), true),
        ]
    );
    assert!(document["features"][0]["step_blob"].as_str().is_some());
    assert_eq!(report.outcome().unwrap().mode, Mode::History);
    assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.severity == Severity::Info && loss.detail == STORED_SOLID_MESSAGE));
    assert!(report
        .losses()
        .iter()
        .all(|loss| !loss.detail.contains("not mapped yet") && !loss.detail.contains("flattened")));
}

#[test]
fn a_base_feature_is_imported_where_it_stands_in_the_timeline() {
    let (document, report) = convert(
        &based_cube(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let base = &document["features"][0];
    assert_eq!(base["type"], "import");
    assert_eq!(base["name"], "Base Feature 1");
    assert!(base["step_blob"]
        .as_str()
        .is_some_and(|blob| blob.starts_with("blobs/")));
    assert!(base.get("_step").is_none() && base.get("_bodies").is_none());
    // The hole after it keeps its history; the original geometry still ends the timeline.
    assert_eq!(
        types(&document),
        [
            ("import".to_owned(), false),
            ("sketch".to_owned(), false),
            ("hole".to_owned(), false),
            ("sketch".to_owned(), false),
            ("import".to_owned(), true),
        ]
    );
}

#[test]
fn pasted_copies_are_imported_where_the_paste_stands_in_the_timeline() {
    // The base feature replaced by a copy/paste whose output, the copy, is the cube's body.
    let mut model = based_cube();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    let features = wire["model"]["features"].as_array_mut().unwrap();
    let paste = features.iter_mut().find(|f| f["ordinal"] == 0).unwrap();
    paste["definition"] = json!({"kind": "operation", "value": {"kind": "insert_bodies",
        "value": {"bodies": {"kind": "resolved", "value": {"native": "f3d:paste#1"}}}}});
    paste["outputs"] = json!([body]);
    paste["name"] = json!("Paste 1");
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let paste = &document["features"][0];
    assert_eq!(paste["type"], "import");
    assert_eq!(paste["name"], "Paste 1");
    assert_eq!(
        types(&document)[1..3],
        [("sketch".to_owned(), false), ("hole".to_owned(), false),]
    );
}

/// The based cube with a later shell of its body, which changes it, and, when `nested`, the
/// base feature's own body as the source keeps it: a history body with a nest of its own.
fn based_cube_changed_later(nested: bool) -> Model {
    let mut model = based_cube();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#9", "ordinal": 9, "name": "Shell", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "shell", "value": {"bodies": cube_body(), "removed_faces": cube_faces(&["top"]), "thickness": 1.0, "outward": false}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": [body]}));
    if nested {
        let renamed = |value: &Value| -> Value {
            let text = serde_json::to_string(value).unwrap();
            serde_json::from_str(&text.replace("synthetic:cube:", "synthetic:nest:")).unwrap()
        };
        let mut topology = serde_json::Map::new();
        topology.insert("body".to_owned(), renamed(&wire["geometry"]["bodies"][0]));
        for arena in [
            "regions", "shells", "faces", "loops", "coedges", "edges", "vertices", "surfaces",
            "curves", "pcurves", "points",
        ] {
            let rows = wire["geometry"].get(arena).map_or(json!([]), renamed);
            topology.insert(arena.to_owned(), rows);
        }
        wire["model"]["history_bodies"] = json!([{
            "id": "synthetic:history-body:nest#h0",
            "produced_by": "f3d:model:feature#1",
            "state": {"ordinal": 1, "identifier": 7},
            "topology": topology,
        }]);
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_base_feature_imports_its_own_body_when_the_source_keeps_it() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let base = |report: &Report| {
        report
            .outcome()
            .unwrap()
            .features
            .iter()
            .find(|f| f.name.as_deref() == Some("Base Feature 1"))
            .cloned()
            .unwrap()
    };
    // Without the base feature's own body, the shell after it would apply twice.
    let (_, report) = convert(&based_cube_changed_later(false), &writer);
    assert!(
        base(&report)
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("stored only as the design ends")),
        "{}",
        history_line(&report)
    );
    // With it, the base feature imports that body and the shell is judged on its own.
    let (document, report) = convert(&based_cube_changed_later(true), &writer);
    let imported = base(&report);
    assert_eq!(
        imported.status,
        dutchman_core::FeatureStatus::Mapped,
        "{}",
        history_line(&report)
    );
    assert!(!imported.final_body && !imported.reapplied);
    let first = &document["features"][0];
    assert_eq!(first["type"], "import");
    assert_eq!(first["name"], "Base Feature 1");
    assert!(first["step_blob"]
        .as_str()
        .is_some_and(|blob| blob.starts_with("blobs/")));
    assert!(first.get("_step").is_none() && first.get("_bodies").is_none());
    // A second history body for the same base feature leaves it unclear which is its own.
    let mut doubled = based_cube_changed_later(true);
    let mut wire = serde_json::to_value(&doubled.ir).unwrap();
    let mut second = wire["model"]["history_bodies"][0].clone();
    second["id"] = json!("synthetic:history-body:nest#h1");
    wire["model"]["history_bodies"]
        .as_array_mut()
        .unwrap()
        .push(second);
    doubled.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(&doubled, &writer);
    assert!(base(&report)
        .reason
        .as_deref()
        .is_some_and(|reason| reason.contains("stored only as the design ends")));
}

/// A missing recipe with only synthetic cube topology, optionally retained at its producer.
fn fallback_cube(stored: bool, later: bool, op: &str) -> Model {
    let mut model = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["definition"]["value"]["value"]["profile"] = json!({"kind": "planar", "value": {"kind": "unresolved", "value": "synthetic:external:profile#1"}});
    wire["model"]["features"][1]["definition"]["value"]["value"]["op"] = json!(op);
    if !later {
        wire["model"]["features"]
            .as_array_mut()
            .unwrap()
            .retain(|f| f["ordinal"].as_u64() < Some(2));
    }
    if stored {
        let nests = serde_json::to_value(&based_cube_changed_later(true).ir).unwrap();
        wire["model"]["history_bodies"] = nests["model"]["history_bodies"].clone();
        wire["model"]["history_bodies"][0]["produced_by"] = json!("f3d:model:feature#2");
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[cfg(unix)]
#[test]
fn a_missing_recipe_imports_its_stored_result_and_a_later_fillet_maps() {
    let model = fallback_cube(true, true, "new_body");
    let cli = fake_cli(&[("unit cube", prism_edges(&SQUARE, 0.0, 10.0))], 0);
    let (document, report) = convert(&model, &replaying(&cli.path));
    let rows = &report.outcome().unwrap().features;
    assert_eq!(
        rows[1].status,
        dutchman_core::FeatureStatus::Mapped,
        "{report:?}"
    );
    assert_eq!(
        rows[2].status,
        dutchman_core::FeatureStatus::Mapped,
        "{report:?}"
    );
    assert!(rows[1].reason.is_none());
    let features = document["features"].as_array().unwrap();
    let imported = features.iter().find(|f| f["name"] == "Block").unwrap();
    assert_eq!(imported["type"], "import");
    assert!(imported["step_blob"].as_str().is_some());
    assert!(features
        .iter()
        .any(|f| f["type"] == "fillet" && f["body"] == "unit cube"));
    let loss = report
        .losses()
        .iter()
        .find(|loss| loss.code.as_deref() == Some("history/body-fallback-imported"))
        .unwrap();
    assert_eq!(loss.severity, Severity::Degraded);
    assert!(loss.subject.contains("Block"));
    assert!(loss.detail.contains("unresolved"));
}

#[test]
fn missing_recipes_without_a_stored_result_and_cuts_still_stop() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    for model in [
        fallback_cube(false, true, "new_body"),
        fallback_cube(true, true, "cut"),
    ] {
        let (_, report) = convert(&model, &writer);
        assert_eq!(
            report.outcome().unwrap().features[1].status,
            dutchman_core::FeatureStatus::Blocked,
            "{report:?}"
        );
        assert!(report
            .losses()
            .iter()
            .all(|loss| loss.code.as_deref() != Some("history/body-fallback-imported")));
    }
}

#[test]
fn a_missing_recipe_uses_final_geometry_only_without_later_body_changes() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&fallback_cube(false, false, "new_body"), &writer);
    assert_eq!(
        report.outcome().unwrap().features[1].status,
        dutchman_core::FeatureStatus::Mapped,
        "{report:?}"
    );
    assert!(document["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "import" && f["name"] == "Block"));
    let loss = report
        .losses()
        .iter()
        .find(|loss| loss.code.as_deref() == Some("history/body-fallback-imported"))
        .unwrap();
    assert_eq!(loss.severity, Severity::Degraded);
    assert!(loss.subject.contains("Block"));
    assert!(loss.detail.contains("unresolved"));
    // The final cube exists, but a later fillet would modify it: refuse that source.
    let (_, report) = convert(&fallback_cube(false, true, "new_body"), &writer);
    assert_eq!(
        report.outcome().unwrap().features[1].status,
        dutchman_core::FeatureStatus::Blocked,
        "{report:?}"
    );
}

#[test]
fn a_stored_result_reports_an_external_profile_only_for_the_affected_feature() {
    let mut model = fallback_cube(true, true, "new_body");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // An unresolved carrier alone says nothing about whether it is external.
    wire["model"]["features"][1]["definition"]["value"]["value"]["profile"] =
        json!({"kind": "planar", "value": {"kind": "unresolved", "value": "synthetic:profile#1"}});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    for (subject, detail, external) in [
        ("source", "feature \"Block\": profile sketch absent", true),
        ("source", "feature \"Other\": profile sketch absent", false),
        ("feature \"Block\"", "Profile sketch absent", true),
        (
            "source",
            "Sketch-profile operand names a sketch absent from this design",
            true,
        ),
    ] {
        let mut report = Report::default();
        report.push_coded(
            Severity::Degraded,
            subject,
            "f3d/feature.extrude-profile-sketch-absent",
            detail,
        );
        SolidCraftWriter::new()
            .verification(Verification::Off)
            .write(&model, &mut Vec::new(), &mut report)
            .unwrap();
        assert_eq!(
            report
                .losses()
                .iter()
                .filter(|loss| loss.code.as_deref()
                    == Some("f3d/feature.extrude-profile-sketch-absent"))
                .count(),
            1,
            "{report:?}"
        );
        let loss = report
            .losses()
            .iter()
            .find(|loss| loss.code.as_deref() == Some("history/body-fallback-imported"));
        assert_eq!(loss.is_some(), external, "{report:?}");
        assert_eq!(
            report.outcome().unwrap().features[1].status,
            if external {
                dutchman_core::FeatureStatus::Mapped
            } else {
                dutchman_core::FeatureStatus::Blocked
            },
            "{report:?}"
        );
    }
}

#[test]
fn an_ordinary_loft_refusal_keeps_its_reason_with_a_stored_result() {
    let mut model = loft_history(10.0, |definition| {
        definition["guidance"] = json!({"kind": "guides", "value": [
            {"kind": "edges", "value": []}
        ]});
    });
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let producer = wire["model"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["definition"]["value"]["kind"] == "loft")
        .unwrap()["id"]
        .clone();
    let nests = serde_json::to_value(&based_cube_changed_later(true).ir).unwrap();
    wire["model"]["history_bodies"] = nests["model"]["history_bodies"].clone();
    wire["model"]["history_bodies"][0]["produced_by"] = producer;
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("a guide rail of topological edges is not mapped yet"),
        "{report:?}"
    );
    assert!(report
        .losses()
        .iter()
        .all(|loss| loss.code.as_deref() != Some("history/body-fallback-imported")));
}

#[test]
fn measured_inference_precedes_an_eligible_stored_result() {
    let mut model = unresolved_square_selection(&[]);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let nests = serde_json::to_value(&based_cube_changed_later(true).ir).unwrap();
    wire["model"]["history_bodies"] = nests["model"]["history_bodies"].clone();
    wire["model"]["history_bodies"][0]["produced_by"] = wire["model"]["features"][1]["id"].clone();
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let mut report = Report::default();
    report.push_coded(
        Severity::Degraded,
        "feature \"Block\"",
        "f3d/feature.extrude-profile-sketch-absent",
        "Profile sketch absent",
    );
    let mut bytes = Vec::new();
    SolidCraftWriter::new()
        .evaluator(std::sync::Arc::new(RecordedEvaluator::default()))
        .write(&model, &mut bytes, &mut report)
        .unwrap();
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(document["features"][1]["type"], "extrude");
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("history/profile-region-inferred")));
    assert!(report
        .losses()
        .iter()
        .all(|loss| loss.code.as_deref() != Some("history/body-fallback-imported")));
}

#[test]
fn a_sketch_with_no_timeline_row_is_written_before_the_extrude_that_uses_it() {
    // The source keeps the cube's sketch only in its sketch arena: no sketch feature names it.
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .retain(|feature| feature["name"] != "Base sketch");
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let outcome = report.outcome().unwrap();
    let block = outcome
        .features
        .iter()
        .find(|f| f.name.as_deref() == Some("Block"))
        .unwrap();
    assert_eq!(
        block.status,
        dutchman_core::FeatureStatus::Mapped,
        "{}",
        history_line(&report)
    );
    let kinds: Vec<&str> = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|f| f["type"].as_str())
        .collect();
    let sketch = kinds.iter().position(|kind| *kind == "sketch").unwrap();
    let extrude = kinds.iter().position(|kind| *kind == "extrude").unwrap();
    assert!(sketch < extrude, "{kinds:?}");
}

#[test]
fn a_base_feature_proven_to_hold_no_bodies_builds_nothing() {
    // A second base feature the source proves empty, after the one holding the cube.
    let mut model = based_cube();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#9", "ordinal": 9, "name": "Base Feature 2", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "base_feature", "value": {"bodies": {"kind": "resolved", "value": {"bodies": [], "native": "f3d:base#2"}}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let outcome = report.outcome().unwrap();
    let empty = outcome
        .features
        .iter()
        .find(|f| f.name.as_deref() == Some("Base Feature 2"))
        .unwrap();
    assert_eq!(empty.status, dutchman_core::FeatureStatus::NotNeeded);
    assert_eq!(
        types(&document)
            .iter()
            .filter(|(kind, _)| kind == "import")
            .count(),
        2,
        "the base body and the original geometry only"
    );
}

#[test]
fn a_base_feature_holding_every_body_writes_no_second_copy() {
    let mut model = based_cube();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .retain(|feature| feature["ordinal"] == 0);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, _) = convert(&model, &writer);
    assert_eq!(types(&document)[0], ("import".to_owned(), false));
    let features = document["features"].as_array().unwrap();
    assert!(features
        .iter()
        .all(|f| f["name"] != "Original geometry (imported)"));
    let (bytes, _) = written_with(&model, &writer);
    let archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let blobs = archive
        .file_names()
        .filter(|name| name.as_ref().is_ok_and(|n| n.starts_with("blobs/")));
    assert_eq!(blobs.count(), 1, "only the base feature's STEP");
}

#[test]
fn solidcraft_drills_into_the_imported_base_body() {
    let Some(cli) = solidcraft() else { return };
    let bore = std::f64::consts::PI * 2.0_f64.powi(2);
    let got = volumes(&cli, &written(&based_cube()).0);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!((got[0] - (1000.0 - bore * 10.0)).abs() < 1.0, "{got:?}");
}

#[test]
fn a_base_feature_alone_is_verified_against_the_original() {
    let Some(cli) = solidcraft() else { return };
    let mut model = based_cube();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .retain(|feature| feature["ordinal"] == 0);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::With(cli)),
    );
    assert_eq!(
        history_line(&report),
        "1 of 1 features rebuilt; verified against the original in SolidCraft"
    );
}

/// The example cube's history with its extrude replaced by a sheet-metal base flange of the
/// same square, `thickness` thick on `side` of the sketch.
fn flanged_cube(thickness: f64, side: &str) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for feature in wire["model"]["features"].as_array_mut().unwrap() {
        if feature["definition"]["value"]["kind"] == "extrude" {
            feature["name"] = json!("BaseFlange 1");
            feature["definition"] = json!({"kind":"operation", "value": {"kind": "sheet_metal_base_flange", "value": {"profile": whole_sketch(), "thickness": thickness, "side": side}}});
        }
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_base_flange_becomes_a_solidcraft_sheet_under_a_rule_of_its_thickness() {
    let (document, report) = convert(
        &flanged_cube(10.0, "forward"),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let flange = &document["features"][1];
    assert_eq!(flange["type"], "sheet_base");
    assert_eq!(flange["rule"], "Sheet 10 mm");
    assert_eq!(flange["profiles"], json!({"type": "all"}));
    assert!(flange.get("flip").is_none());
    assert_eq!(document["sheet"]["active"], "Sheet 10 mm");
    assert_eq!(document["sheet"]["rules"][0]["thickness"], "10 mm");
    // The other side of the sketch flips it; both sides at once is not mapped yet.
    let (document, _) = convert(
        &flanged_cube(10.0, "reverse"),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(document["features"][1]["flip"], true);
    let (_, report) = convert(
        &flanged_cube(10.0, "symmetric"),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("thick on both sides of its sketch is not mapped yet"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn solidcraft_builds_the_base_flange_as_the_original() {
    let Some(cli) = solidcraft() else { return };
    let (_, report) = convert(
        &flanged_cube(10.0, "forward"),
        &SolidCraftWriter::new().verification(Verification::With(cli)),
    );
    assert_eq!(
        history_line(&report),
        "2 of 2 features rebuilt; verified against the original in SolidCraft"
    );
}

/// Synthetic open section; the saved cube is deliberately unrelated to the section, so
/// conversion tests cannot accidentally select curves from the final body.
fn contour_flange_model(change: Value) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketches"][0]["profiles"] = json!([]);
    m["sketch_entities"] = json!([
        line(1, [0.0, 0.0], [20.0, 0.0]),
        line(2, [20.0, 0.0], [20.0, 15.0]),
        line(3, [20.0, 15.0], [0.0, 0.0]),
        line(4, [50.0, 0.0], [60.0, 0.0]),
    ]);
    m["sketch_constraints"] = json!([]);
    let mut definition = json!({
        "profile": {"kind": "sketch_entities", "value": {"sketch": SKETCH,
            "entities": ["f3d:model:sketch-entity#1", "f3d:model:sketch-entity#2"]}},
        "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
        "extent": {"kind": "one_sided", "side": {
            "termination": {"kind": "blind", "length": 10.0}}},
        "thickness": 2.0, "inside_radius": 2.0, "neutral_factor": 0.4, "side": "right",
    });
    for (key, value) in change.as_object().unwrap() {
        definition[key] = value.clone();
    }
    m["features"][1]["name"] = json!("Contour flange");
    m["features"][1]["definition"] = json!({
        "kind": "operation", "value": {
            "kind": "sheet_metal_contour_flange", "value": {"construction": definition},
        },
    });
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn contour_flanges_keep_the_open_section_bend_rule_and_signed_travel() {
    for (side, direction, length, reverse) in [
        ("right", 1.0, 10.0, false),
        ("left", 1.0, -10.0, true),
        ("right", -1.0, 10.0, true),
        ("left", -1.0, -10.0, false),
    ] {
        let model = contour_flange_model(json!({"side": side,
            "direction": {"x": 0.0, "y": 0.0, "z": direction},
            "extent": {"kind": "one_sided", "side": {
                "termination": {"kind": "blind", "length": length}}}}));
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("2 of 2 features rebuilt"),
            "{report:?}"
        );
        let sketch = &document["features"][0];
        let flange = &document["features"][1];
        assert_eq!(flange["type"], "sheet_contour");
        assert_eq!(flange["sketch"], sketch["id"]);
        assert_eq!(flange["curves"], json!(["l1", "l2"]));
        assert_eq!(flange["distance"], "10 mm");
        assert_eq!(flange["flip"].as_bool().unwrap_or(false), side == "left");
        assert_eq!(flange["reverse"].as_bool().unwrap_or(false), reverse);
        let rule = &document["sheet"]["rules"][0];
        assert_eq!(flange["rule"], rule["name"]);
        assert_eq!(rule["thickness"], "2 mm");
        assert_eq!(rule["bend_radius"], "2 mm");
        assert_eq!(rule["k_factor"], "0.4");
    }
}

#[test]
fn contour_flanges_keep_arc_entities_in_source_order() {
    let mut model = contour_flange_model(json!({}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketch_entities"][1] = entity(
        2,
        json!({
            "kind": "arc", "center": {"u": 20.0, "v": 5.0}, "radius": 5.0,
            "start_angle": -std::f64::consts::FRAC_PI_2, "end_angle": 0.0,
        }),
    );
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{report:?}"
    );
    assert_eq!(document["features"][1]["curves"], json!(["l1", "a1"]));
}

#[test]
fn contour_flanges_report_unexpressible_sections_and_keep_the_source_geometry() {
    for (change, reason) in [
        (json!({"side": "centered"}), "centered on its profile"),
        (
            json!({"direction": {"x": 1.0, "y": 0.0, "z": 1.0}}),
            "oblique to its sketch",
        ),
        (
            json!({"extent": {"kind": "symmetric", "side": {
            "termination": {"kind": "blind", "length": 10.0}}}}),
            "symmetric contour flange",
        ),
        (
            json!({"extent": {"kind": "one_sided", "side": {
            "termination": {"kind": "through_all"}}}}),
            "length ends through_all",
        ),
        (
            json!({"extent": {"kind": "one_sided", "side": {
            "termination": {"kind": "blind", "length": 10.0}, "draft": 0.1}}}),
            "drafted contour flange",
        ),
        (
            json!({"profile": {"kind": "sketch_entities", "value": {"sketch": SKETCH,
            "entities": ["f3d:model:sketch-entity#1", "f3d:model:sketch-entity#4"]}}}),
            "do not form an open chain",
        ),
        (
            json!({"profile": {"kind": "sketch_entities", "value": {"sketch": SKETCH,
            "entities": ["f3d:model:sketch-entity#1", "f3d:model:sketch-entity#2",
                "f3d:model:sketch-entity#3"]}}}),
            "closed profile",
        ),
    ] {
        let (document, report) = convert(
            &contour_flange_model(change),
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).contains(reason), "{report:?}");
        assert!(document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature["type"] == "import" && feature["suppressed"] == false));
    }
}

#[test]
fn solidcraft_replays_contour_flange_side_and_travel_at_the_expected_coordinates() {
    let Some(cli) = solidcraft() else { return };
    // A straight 20 mm section makes a 20 x 2 x 10 mm sheet. Its position independently
    // proves the material side and signed sweep, even when volume alone cannot.
    for (side, length) in [
        ("right", 10.0),
        ("left", 10.0),
        ("right", -10.0),
        ("left", -10.0),
    ] {
        let model = contour_flange_model(json!({"side": side,
            "profile": {"kind": "sketch_entities", "value": {"sketch": SKETCH,
                "entities": ["f3d:model:sketch-entity#1"]}},
            "extent": {"kind": "one_sided", "side": {
                "termination": {"kind": "blind", "length": length}}}}));
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("contour.sc");
        std::fs::write(&design, written(&model).0).unwrap();
        let actual = sheet_measurements(&cli, &design, &dir.path().join("actual.json"));
        assert!(
            (actual["volume_mm3"].as_f64().unwrap() - 400.0).abs() < 1e-6,
            "{actual}"
        );
        let (near_y, far_y) = if side == "left" {
            (0.0, 2.0)
        } else {
            (-2.0, 0.0)
        };
        let (near_z, far_z) = if length < 0.0 {
            (-10.0, 0.0)
        } else {
            (0.0, 10.0)
        };
        for (corner, expected) in [
            ("min", [0.0, near_y, near_z]),
            ("max", [20.0, far_y, far_z]),
        ] {
            for (axis, at) in expected.into_iter().enumerate() {
                assert!(
                    (actual["bbox"][corner][axis].as_f64().unwrap() - at).abs() < 1e-6,
                    "{side} {length}: {actual}"
                );
            }
        }
    }
}

#[test]
fn solidcraft_replays_a_contour_bend_with_the_source_radius_and_thickness() {
    let Some(cli) = solidcraft() else { return };
    // The opens-in floor can replay straight contours, but not rounded contour bends.
    let version =
        dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()));
    if version <= "0.18.2".parse().unwrap() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let design = dir.path().join("bent-contour.sc");
    std::fs::write(&design, written(&contour_flange_model(json!({}))).0).unwrap();
    let actual = sheet_measurements(&cli, &design, &dir.path().join("actual.json"));
    // The right-side material turns around an inside radius of 2 mm. The straight
    // portions are 18 and 13 mm; the bend is a quarter annulus of radii 2 and 4 mm.
    let section_area = 2.0 * (18.0 + 13.0) + std::f64::consts::PI * (16.0 - 4.0) / 4.0;
    let expected_volume = section_area * 10.0;
    assert!(
        (actual["volume_mm3"].as_f64().unwrap() - expected_volume).abs() < 1e-6,
        "{actual}"
    );
    for (corner, expected) in [("min", [0.0, -2.0, 0.0]), ("max", [22.0, 15.0, 10.0])] {
        for (axis, at) in expected.into_iter().enumerate() {
            assert!(
                (actual["bbox"][corner][axis].as_f64().unwrap() - at).abs() < 1e-6,
                "{actual}"
            );
        }
    }
}

/// A base sheet followed by a synthetic operation on its front top edge.
fn sheet_operation(mut definition: Value) -> Model {
    let mut model = flanged_cube(10.0, "forward");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    definition["value"]["edges"] = json!({"kind": "edges", "value": ["synthetic:cube:edge#4"]});
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Sheet wall", "suppressed": false, "definition": {"kind": "operation", "value": definition}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn edge_flanges_refuse_final_body_edge_selections() {
    // The selected edge exists only on the final body, where it has become the bend.
    let model = sheet_operation(
        json!({"kind": "sheet_metal_edge_flange", "value": {"height": {"kind": "distance", "value": 30.0}, "angle": std::f64::consts::FRAC_PI_2, "height_datum": "outer_faces", "bend_position": "outside", "width": {"kind": "full_edge"}, "bend_radius": 2.0}}),
    );
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("needs this feature's input edge geometry"),
        "{}",
        history_line(&report)
    );
}

/// The final body is deliberately still the original cube. The two symmetric input edges
/// have their own geometry, unrelated to final-body edge numbering.
fn input_edge_flange(back: bool, position: &str, angle: f64, radius: f64) -> Model {
    let mut model = flanged_cube(1.0, "forward");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let state = "test:sheet:feature-input#wall";
    let edges = [
        "test:sheet:historical-edge#front",
        "test:sheet:historical-edge#back",
    ];
    let faces = [
        "test:sheet:historical-face#top",
        "test:sheet:historical-face#rim",
    ];
    let point = |x, y, z| json!({"x": x, "y": y, "z": z});
    let geometry = [0, 1].map(|i| {
        json!({
            "edge": edges[i], "interior": point(5.0, 10.0 * i as f64, 1.0),
            "start": point(0.0, 10.0 * i as f64, 1.0),
            "end": point(10.0, 10.0 * i as f64, 1.0),
            "tangent": point(1.0, 0.0, 0.0), "faces": faces,
        })
    });
    wire["model"]["feature_input_topologies"] = json!([{"id": state, "input_of": "f3d:model:feature#3", "edges": edges, "faces": faces, "edge_geometry": geometry, "bodies": [], "vertices": []}]);
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Sheet wall", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sheet_metal_edge_flange", "value": {"edges": {"kind": "historical", "value": {
                    "state": state, "edges": [edges[usize::from(back)]], "native": "synthetic edge",
                }}, "height": {"kind": "distance", "value": 30.0}, "angle": angle, "height_datum": "outer_faces", "bend_position": position, "width": {"kind": "full_edge"}, "bend_radius": radius}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn edge_flanges_pick_their_own_input_edges_and_keep_dimensions() {
    for back in [false, true] {
        for position in ["inside", "outside"] {
            let (document, report) = convert(
                &input_edge_flange(back, position, std::f64::consts::FRAC_PI_2, 2.0),
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            assert!(
                history_line(&report).starts_with("3 of 3 features rebuilt"),
                "{}",
                history_line(&report)
            );
            let flange = &document["features"][2];
            assert_eq!(flange["type"], "sheet_flange");
            assert_eq!(
                flange["edges"],
                json!([[5.0, if back { 10.0 } else { 0.0 }, 1.0]])
            );
            assert_eq!(flange["height"], "30 mm");
            assert_eq!(flange["angle"], "90 deg");
            assert_eq!(flange["radius"], "2 mm");
            assert_eq!(flange["position"], position);
            assert_eq!(
                document["sheet"]["rules"][0]["corner_relief_shape"],
                "notch"
            );
            assert_eq!(document["sheet"]["rules"][0]["relief_width"], "Thickness");
        }
    }
}

#[test]
fn edge_flanges_refuse_unproven_picks_and_unexpressible_options() {
    let mutations = [
        (
            "/model/feature_input_topologies/0/input_of",
            json!("f3d:model:feature#2"),
            "does not belong",
        ),
        (
            "/model/feature_input_topologies/0/edge_geometry",
            json!([]),
            "input topology has no edge geometry",
        ),
        (
            "/model/feature_input_topologies/0/edges",
            json!([]),
            "input topology has no edges",
        ),
        (
            "/model/feature_input_topologies/0/edge_geometry/0/interior",
            json!({"x": 5.0, "y": 5.0, "z": 1.0}),
            "exactly one rebuilt edge",
        ),
        (
            "/model/feature_input_topologies/0/edge_geometry/0/interior",
            json!({"x": 0.0, "y": 0.0, "z": 1.0}),
            "exactly one rebuilt edge",
        ),
        (
            "/model/feature_input_topologies/0/edge_geometry/0/end",
            json!({"x": 9.0, "y": 0.0, "z": 1.0}),
            "vertices or tangent",
        ),
        (
            "/model/feature_input_topologies/0/edge_geometry/0/tangent",
            json!({"x": 0.0, "y": 1.0, "z": 0.0}),
            "vertices or tangent",
        ),
        (
            "/model/features/2/definition/value/value/height_datum",
            json!("inner_faces"),
            "inner faces",
        ),
        (
            "/model/features/2/definition/value/value/bend_position",
            json!("adjacent"),
            "bend position",
        ),
        (
            "/model/features/2/definition/value/value/bend_position",
            json!("tangent_to_side"),
            "bend position",
        ),
        (
            "/model/features/2/definition/value/value/width",
            json!({"kind": "symmetric", "width": 5.0}),
            "part of its edge",
        ),
        (
            "/model/features/2/definition/value/value/height",
            json!({"kind": "to_object", "value": {
                "target": {"kind": "native", "value": "synthetic"}, "offset": 0.0,
            }}),
            "ending at an object",
        ),
    ];
    for (path, value, reason) in mutations {
        let mut model = input_edge_flange(false, "outside", std::f64::consts::FRAC_PI_2, 2.0);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        *wire.pointer_mut(path).unwrap() = value;
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{path}: {}",
            history_line(&report)
        );
    }
}

#[test]
fn edge_flanges_refuse_bases_without_a_proven_rectangular_body() {
    for extra_body in [false, true] {
        let mut model = input_edge_flange(false, "outside", std::f64::consts::FRAC_PI_2, 2.0);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        if extra_body {
            let mut other = wire["model"]["features"][1].clone();
            other["id"] = json!("f3d:model:feature#4");
            other["ordinal"] = json!(2);
            other["name"] = json!("Another sheet");
            wire["model"]["features"][2]["ordinal"] = json!(3);
            wire["model"]["features"]
                .as_array_mut()
                .unwrap()
                .push(other);
        } else {
            // The base now has an oblique side; a bounding rectangle alone is not proof.
            wire["model"]["sketch_entities"][0]["geometry"]["end"]["u"] = json!(9.0);
            wire["model"]["sketch_entities"][1]["geometry"]["start"]["u"] = json!(9.0);
        }
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let reason = if extra_body {
            "a base sheet that earlier features changed"
        } else {
            "a base sheet that is not one plain rectangle"
        };
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

/// SolidCraft's complete measurements, used to distinguish symmetric but misplaced flanges.
fn sheet_measurements(cli: &Path, input: &Path, output: &Path) -> Value {
    let status = std::process::Command::new(cli)
        .arg("eval")
        .arg(input)
        .arg("--json-out")
        .arg(output)
        .status()
        .unwrap();
    assert!(status.success());
    let result: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    for entry in result["timeline"].as_array().into_iter().flatten() {
        assert!(entry["error"].is_null(), "{entry}");
    }
    assert_eq!(result["bodies"].as_array().unwrap().len(), 1, "{result}");
    result["bodies"][0].clone()
}

#[test]
fn solidcraft_replays_input_edge_flanges_with_the_expected_shape() {
    let Some(cli) = solidcraft() else { return };
    for (back, position, degrees, radius, both) in [
        (false, "outside", 90.0_f64, 2.0, false),
        (true, "outside", 90.0, 2.0, false),
        (false, "inside", 60.0, 1.5, false),
        (true, "inside", 120.0, 3.0, false),
        (false, "outside", 90.0, 2.0, true),
    ] {
        let mut model = input_edge_flange(back, position, degrees.to_radians(), radius);
        let mut points = vec![[5.0, if back { 10.0 } else { 0.0 }, 1.0]];
        if both {
            let mut wire = serde_json::to_value(&model.ir).unwrap();
            wire["model"]["features"][2]["definition"]["value"]["value"]["edges"]["value"]
                ["edges"] = json!([
                "test:sheet:historical-edge#front",
                "test:sheet:historical-edge#back",
            ]);
            model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
            points.push([5.0, 10.0, 1.0]);
            let (document, report) = convert(
                &model,
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
            assert_eq!(document["features"][2]["edges"], json!(points));
        }
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("mapped.solidcraft");
        std::fs::write(&design, written(&model).0).unwrap();
        // Build the expected body independently with documented commands and an explicit
        // point on the intended edge. In particular the back case must grow toward +Y.
        let reference = json!([
            {"command": "sketch.create", "params": {"plane": "XY", "name": "Base"}},
            {"command": "sketch.rectangle.two_point", "params": {"p0": [0, 0], "p1": [10, 10]}},
            {"command": "sketch.finish", "params": {}},
            {"command": "sheet.manage_rules", "params": {"name": "Sheet 1 mm", "thickness": "1 mm", "active": true}},
            {"command": "sheet.flange", "params": {"sketch": "Base"}},
            {"command": "sheet.flange", "params": {
                "edges": points, "height": "30 mm",
                "angle": format!("{degrees} deg"), "radius": format!("{radius} mm"), "position": position,
            }},
        ]);
        let script = dir.path().join("expected.json");
        std::fs::write(&script, serde_json::to_vec(&reference).unwrap()).unwrap();
        let actual = sheet_measurements(&cli, &design, &dir.path().join("actual.json"));
        let expected = sheet_measurements(&cli, &script, &dir.path().join("reference.json"));
        let volume = expected["volume_mm3"].as_f64().unwrap();
        assert!((actual["volume_mm3"].as_f64().unwrap() - volume).abs() / volume < 1e-4);
        assert_eq!(actual["faces"], expected["faces"]);
        for corner in ["min", "max"] {
            for axis in 0..3 {
                let actual_at = actual["bbox"][corner][axis].as_f64().unwrap();
                let expected_at = expected["bbox"][corner][axis].as_f64().unwrap();
                assert!(
                    (actual_at - expected_at).abs() < 1e-6,
                    "{back} {position}: {actual} != {expected}"
                );
            }
        }
        if !both && degrees == 90.0 && position == "outside" {
            let corner = if back { "max" } else { "min" };
            let y = if back { 13.0 } else { -3.0 };
            assert!((actual["bbox"][corner][1].as_f64().unwrap() - y).abs() < 1e-6);
            // 0.44 merges the plate, bend and flange side faces (one plane) into one face on each side: 14 becomes 10.
            let version =
                dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()));
            let faces = if version < "0.44.0".parse().unwrap() {
                14
            } else {
                10
            };
            assert_eq!(actual["faces"], faces);
        }
    }
}

#[test]
fn hems_explain_the_form_or_dimension_that_cannot_be_expressed() {
    for (form, direction, reason) in [
        (
            json!({"kind": "flat", "length": 20.0}),
            "forward",
            "developed length",
        ),
        (
            json!({"kind": "open", "gap": 2.0, "length": 20.0}),
            "reverse",
            "developed length",
        ),
        (
            json!({"kind": "gap_length", "gap": 2.0, "length": 20.0}),
            "forward",
            "unresolved flat-or-open",
        ),
        (
            json!({"kind": "rolled", "radius": 2.0, "angle": 1.0}),
            "forward",
            "rolled hem",
        ),
        (
            json!({"kind": "teardrop", "gap": 2.0, "length": 20.0, "radius": 2.0}),
            "forward",
            "teardrop hem",
        ),
        (
            json!({"kind": "flat", "length": 20.0}),
            "unresolved",
            "unresolved fold direction",
        ),
    ] {
        let model = sheet_operation(
            json!({"kind": "sheet_metal_hem", "value": {"bend_position": "adjacent", "form": form, "direction": direction, "bend_radius": 2.0}}),
        );
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

const PLATE: &str = "f3d:model:sketch#41";

/// A 1 mm base sheet, then a plate sketched square to its front edge from `near` to 20 mm
/// up, attached to that edge, then the plate's bend of `radius`. `extra` adds a line to the
/// plate sketch, which makes it more than one rectangle.
fn attached_plate(near: f64, radius: f64, extra: bool) -> Model {
    let mut model = flanged_cube(1.0, "forward");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    let point = |x, y, z| json!({"x": x, "y": y, "z": z});
    let edges = [
        "test:sheet:historical-edge#front",
        "test:sheet:historical-edge#back",
    ];
    let faces = [
        "test:sheet:historical-face#top",
        "test:sheet:historical-face#rim",
    ];
    let geometry = [0, 1].map(|i| {
        json!({
            "edge": edges[i], "interior": point(5.0, 10.0 * i as f64, 1.0),
            "start": point(0.0, 10.0 * i as f64, 1.0),
            "end": point(10.0, 10.0 * i as f64, 1.0),
            "tangent": point(1.0, 0.0, 0.0), "faces": faces,
        })
    });
    let state = |name: &str, owner: &str| json!({"id": format!("test:sheet:feature-input#{name}"), "input_of": owner, "edges": edges, "faces": faces, "edge_geometry": geometry, "bodies": [], "vertices": []});
    m["feature_input_topologies"] = json!([
        state("plate", "f3d:model:feature#4"),
        state("bend", "f3d:model:feature#5"),
    ]);
    let attachment = |name: &str| {
        json!({"kind": "historical", "value": {
            "state": format!("test:sheet:feature-input#{name}"),
            "edges": [edges[0]], "native": "synthetic edge",
        }})
    };
    m["sketches"].as_array_mut().unwrap().push(json!({
        "id": PLATE, "name": "Plate",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": -1.0, "z": 0.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [],
    }));
    let (lo, hi) = (near, 20.0);
    let mut lines = vec![
        line(41, [0.0, lo], [10.0, lo]),
        line(42, [10.0, lo], [10.0, hi]),
        line(43, [10.0, hi], [0.0, hi]),
        line(44, [0.0, hi], [0.0, lo]),
    ];
    if extra {
        lines.push(line(45, [0.0, lo], [5.0, hi]));
    }
    for mut l in lines {
        l["sketch"] = json!(PLATE);
        m["sketch_entities"].as_array_mut().unwrap().push(l);
    }
    let features = m["features"].as_array_mut().unwrap();
    features.push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Plate sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": PLATE}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    features.push(json!({"id": "f3d:model:feature#4", "ordinal": 3, "name": "Plate", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sheet_metal_attached_plate", "value": {"profile": {"kind": "sketch", "value": PLATE}, "thickness": 1.0, "side": "forward", "attachment": attachment("plate")}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    features.push(json!({"id": "f3d:model:feature#5", "ordinal": 4, "name": "Bend", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sheet_metal_bend", "value": {"plate": "f3d:model:feature#4", "attachment": attachment("bend"), "inside_radius": radius, "relief": {"form": "style_default", "width": 1.0, "depth": 0.5,
                       "minimum_remnant": 0.0, "seam_gap": 0.0}, "transition": {"form": "style_default", "radius": 0.0}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn an_attached_plate_and_its_bend_become_one_edge_flange() {
    let (document, report) = convert(
        &attached_plate(3.0, 2.0, false),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("4 of 4 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let flanges: Vec<_> = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["type"] == "sheet_flange")
        .collect();
    assert_eq!(flanges.len(), 1);
    let flange = flanges[0];
    assert_eq!(flange["edges"], json!([[5.0, 0.0, 1.0]]));
    assert_eq!(flange["height"], "20 mm");
    assert_eq!(flange["angle"], "90 deg");
    assert_eq!(flange["radius"], "2 mm");
    assert_eq!(flange["position"], "inside");
}

#[test]
fn a_plate_starting_at_the_outer_face_is_the_same_edge_flange() {
    // Inventor stores a flange measured from where the outer faces meet: its plate starts at
    // the sheet's outer face, not where the bend ends. The flange's height is the same.
    let (document, report) = convert(
        &attached_plate(0.0, 2.0, false),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("4 of 4 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let flange = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "sheet_flange")
        .unwrap();
    assert_eq!(flange["height"], "20 mm");
    assert_eq!(flange["radius"], "2 mm");
}

#[test]
fn an_attached_plate_that_is_not_a_plain_edge_flange_is_refused() {
    for (model, reason) in [
        (attached_plate(3.0, 2.0, true), "not one rectangle"),
        (
            attached_plate(4.0, 2.0, false),
            "does not start where its 90 degree bend ends",
        ),
    ] {
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let line = history_line(&report);
        assert!(line.contains(reason), "{line}");
    }
}

/// Synthetic polygonal base and a square plate spanning its selected actual edge. The
/// source B-rep remains the test cube; these tests inspect/replay the unverified history.
fn polygon_attached_plate(points: &[[f64; 2]], edge: usize, top: bool) -> Model {
    let mut model = attached_plate(3.0, 2.0, false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketch_constraints"] = json!([]);
    m["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .retain(|row| row["sketch"] == PLATE);
    let mut members = Vec::new();
    for (i, (&a, &b)) in points.iter().zip(points.iter().cycle().skip(1)).enumerate() {
        let row = line(i as u32 + 1, a, b);
        members.push(json!({"entity": row["id"], "reversed": false}));
        m["sketch_entities"].as_array_mut().unwrap().push(row);
    }
    m["sketches"][0]["profiles"] = json!([members]);
    let (a, b) = (points[edge], points[(edge + 1) % points.len()]);
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length = dx.hypot(dy);
    let (ux, uy) = (dx / length, dy / length);
    let sign = if top { 1.0 } else { -1.0 };
    let point = |x, y, z| json!({"x": x, "y": y, "z": z});
    m["sketches"][1]["placement"]["frame"] = json!({
        "origin": point(a[0], a[1], if top { 0.0 } else { 1.0 }),
        "normal": point(sign * uy, -sign * ux, 0.0),
        "u_axis": point(ux, uy, 0.0),
    });
    for row in m["sketch_entities"].as_array_mut().unwrap() {
        if row["sketch"] == PLATE {
            for endpoint in ["start", "end"] {
                if row["geometry"][endpoint]["u"] == 10.0 {
                    row["geometry"][endpoint]["u"] = json!(length);
                }
            }
        }
    }
    for state in m["feature_input_topologies"].as_array_mut().unwrap() {
        state["edge_geometry"][0]["start"] = point(a[0], a[1], if top { 1.0 } else { 0.0 });
        state["edge_geometry"][0]["end"] = point(b[0], b[1], if top { 1.0 } else { 0.0 });
        state["edge_geometry"][0]["interior"] = point(
            (a[0] + b[0]) / 2.0,
            (a[1] + b[1]) / 2.0,
            if top { 1.0 } else { 0.0 },
        );
        state["edge_geometry"][0]["tangent"] = point(ux, uy, 0.0);
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

const L_BASE: [[f64; 2]; 6] = [
    [0.0, 0.0],
    [30.0, 0.0],
    [30.0, 10.0],
    [10.0, 10.0],
    [10.0, 30.0],
    [0.0, 30.0],
];

#[test]
fn translated_attached_plates_keep_rectangular_and_concave_base_edges() {
    for (points, edge) in [
        (vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], 0),
        (
            L_BASE.iter().map(|[x, y]| [x / 80.0, y / 80.0]).collect(),
            3,
        ),
    ] {
        for reverse in [false, true] {
            let mut points = points.clone();
            let edge = if reverse {
                points.reverse();
                (points.len() * 2 - edge - 2) % points.len()
            } else {
                edge
            };
            for offset in [0.0, 100_000_000.0, 100_000_001.0] {
                let translated: Vec<_> = points
                    .iter()
                    .map(|[x, y]| [x + offset, y + offset])
                    .collect();
                for top in [false, true] {
                    let mut model = polygon_attached_plate(&translated, edge, top);
                    if points.len() > 4 {
                        // Inset the plate at the concave edge: the flush-face proof must
                        // use the outward winding sign, not just admit a nonzero area.
                        let mut wire = serde_json::to_value(&model.ir).unwrap();
                        wire["model"]["sketches"][1]["placement"]["frame"]["origin"]["x"] =
                            json!(translated[edge][0] - 1.0);
                        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
                    }
                    let (document, report) = convert(
                        &model,
                        &SolidCraftWriter::new().verification(Verification::Off),
                    );
                    assert!(
                        history_line(&report).starts_with("4 of 4 features rebuilt"),
                        "reverse={reverse}, offset={offset}, top={top}: {}",
                        history_line(&report)
                    );
                    let flange = document["features"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|f| f["type"] == "sheet_flange")
                        .unwrap();
                    let (a, b) = (translated[edge], translated[(edge + 1) % translated.len()]);
                    assert_eq!(
                        flange["edges"],
                        json!([[
                            (a[0] + b[0]) / 2.0,
                            (a[1] + b[1]) / 2.0,
                            if top { 1.0 } else { 0.0 },
                        ]])
                    );
                    assert_eq!(flange["height"], "20 mm");
                    assert_eq!(flange["position"], "inside");
                }
            }
        }
    }
}

#[test]
fn attached_plates_use_the_actual_concave_base_edge_on_either_face_and_winding() {
    for reverse in [false, true] {
        let mut points = L_BASE.to_vec();
        if reverse {
            points.reverse();
        }
        // The inside vertical edge, pointing outward toward +X. A direction from the
        // bounding-box centre would point toward -X instead. Put the plate 1 mm inward
        // to make that wrong sign observable in the flush-face proof.
        let edge = if reverse { 1 } else { 3 };
        for top in [false, true] {
            let mut model = polygon_attached_plate(&points, edge, top);
            let mut wire = serde_json::to_value(&model.ir).unwrap();
            wire["model"]["sketches"][1]["placement"]["frame"]["origin"]["x"] = json!(9.0);
            model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
            let (document, report) = convert(
                &model,
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            assert!(
                history_line(&report).starts_with("4 of 4 features rebuilt"),
                "{}",
                history_line(&report)
            );
            let flange = document["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["type"] == "sheet_flange")
                .unwrap();
            assert_eq!(
                flange["edges"],
                json!([[10.0, 20.0, if top { 1.0 } else { 0.0 }]])
            );
            assert_eq!(flange["height"], "20 mm");
            assert_eq!(flange["position"], "inside");
        }
    }
}

#[test]
fn attached_plates_accept_explicit_single_base_profile_selections() {
    for profile in [
        first_profile(),
        json!({"kind":"sketch_regions", "value":{"sketch":SKETCH, "regions":[{"kind":"loops", "outer":0, "holes":[]}]}}),
    ] {
        let mut model = polygon_attached_plate(&L_BASE, 3, true);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["features"][1]["definition"]["value"]["value"]["profile"] = profile;
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("4 of 4 features rebuilt"),
            "{}",
            history_line(&report)
        );
        let flange = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "sheet_flange")
            .unwrap();
        assert_eq!(flange["edges"], json!([[10.0, 20.0, 1.0]]));
    }
}

#[test]
fn attached_plates_refuse_unproven_nonrectangular_base_picks() {
    for (field, point, reason) in [
        (
            "interior",
            json!({"x":30.0,"y":20.0,"z":1.0}),
            "exactly one rebuilt edge",
        ),
        (
            "end",
            json!({"x":10.0,"y":29.0,"z":1.0}),
            "vertices or tangent",
        ),
        (
            "tangent",
            json!({"x":1.0,"y":0.0,"z":0.0}),
            "vertices or tangent",
        ),
    ] {
        let mut model = polygon_attached_plate(&L_BASE, 3, true);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["feature_input_topologies"][0]["edge_geometry"][0][field] = point;
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn attached_plates_name_base_profiles_that_cannot_be_proven() {
    for points in [
        // Crossing perimeter.
        vec![[0.0, 0.0], [30.0, 30.0], [0.0, 30.0], [30.0, 0.0]],
        // Two consecutive collinear sides would merge in SolidCraft.
        vec![
            [0.0, 0.0],
            [15.0, 0.0],
            [30.0, 0.0],
            [30.0, 30.0],
            [0.0, 30.0],
        ],
    ] {
        let model = polygon_attached_plate(&points, 0, true);
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report)
                .contains("one closed, non-crossing base perimeter with distinct straight edges"),
            "{}",
            history_line(&report)
        );
    }
    let mut model = polygon_attached_plate(&L_BASE, 3, true);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // A second disjoint closed region is not the single base body this proof requires.
    for (i, (a, b)) in [
        ([40.0, 0.0], [50.0, 0.0]),
        ([50.0, 0.0], [45.0, 10.0]),
        ([45.0, 10.0], [40.0, 0.0]),
    ]
    .into_iter()
    .enumerate()
    {
        wire["model"]["sketch_entities"]
            .as_array_mut()
            .unwrap()
            .push(line(10 + i as u32, a, b));
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report)
            .contains("one closed, non-crossing base perimeter with distinct straight edges"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn attached_plates_name_curved_base_profiles_and_thickness_edge_picks() {
    let mut model = polygon_attached_plate(&L_BASE, 3, true);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .push(entity(
            10,
            json!({
                "kind": "circle", "center": {"u": 5.0, "v": 5.0}, "radius": 2.0,
            }),
        ));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("curved, disconnected, or collinear base edges"),
        "{}",
        history_line(&report)
    );

    let mut model = polygon_attached_plate(&L_BASE, 3, true);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["feature_input_topologies"][0]["edge_geometry"][0] = json!({
        "edge": "test:sheet:historical-edge#front",
        "start": {"x":10.0,"y":10.0,"z":0.0},
        "end": {"x":10.0,"y":10.0,"z":1.0},
        "interior": {"x":10.0,"y":10.0,"z":0.5},
        "tangent": {"x":0.0,"y":0.0,"z":1.0},
        "faces": ["test:sheet:historical-face#top", "test:sheet:historical-face#rim"],
    });
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("select a thickness edge"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn solidcraft_replays_attached_plates_on_nonrectangular_base_edges() {
    let Some(cli) = solidcraft() else { return };
    for (points, edge, pick) in [
        (L_BASE.to_vec(), 3, [10.0, 20.0, 1.0]),
        (L_BASE.iter().copied().rev().collect(), 1, [10.0, 20.0, 0.0]),
        // Notch across the far side, attaching to its bottom edge.
        (
            vec![
                [0.0, 0.0],
                [40.0, 0.0],
                [40.0, 30.0],
                [30.0, 30.0],
                [30.0, 10.0],
                [10.0, 10.0],
                [10.0, 30.0],
                [0.0, 30.0],
            ],
            4,
            [20.0, 10.0, 1.0],
        ),
    ] {
        let model = polygon_attached_plate(&points, edge, pick[2] == 1.0);
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("mapped.sc");
        let (bytes, report) = written(&model);
        assert!(
            history_line(&report).starts_with("4 of 4 features rebuilt"),
            "{}",
            history_line(&report)
        );
        std::fs::write(&design, bytes).unwrap();
        // Independent command construction uses the explicit intended profile and pick.
        let reference = json!([
            {"command":"sketch.create", "params":{"plane":"XY", "name":"Base"}},
            {"command":"sketch.line", "params":{"points":points, "closed":true}},
            {"command":"sketch.finish", "params":{}},
            {"command":"sheet.manage_rules", "params":{"name":"Sheet 1 mm", "thickness":"1 mm", "active":true}},
            {"command":"sheet.flange", "params":{"sketch":"Base"}},
            {"command":"sheet.flange", "params":{"edges":[pick], "height":"20 mm", "angle":"90 deg", "radius":"2 mm", "position":"inside"}},
        ]);
        let script = dir.path().join("expected.json");
        std::fs::write(&script, serde_json::to_vec(&reference).unwrap()).unwrap();
        let actual = sheet_measurements(&cli, &design, &dir.path().join("actual.json"));
        let expected = sheet_measurements(&cli, &script, &dir.path().join("reference.json"));
        let volume = expected["volume_mm3"].as_f64().unwrap();
        assert!((actual["volume_mm3"].as_f64().unwrap() - volume).abs() / volume < 1e-4);
        assert_eq!(actual["faces"], expected["faces"]);
        for corner in ["min", "max"] {
            for axis in 0..3 {
                assert!(
                    (actual["bbox"][corner][axis].as_f64().unwrap()
                        - expected["bbox"][corner][axis].as_f64().unwrap())
                    .abs()
                        < 1e-6,
                    "{actual} != {expected}"
                );
            }
        }
    }
}

/// The example cube under a timeline of `features`, each `(name, definition)`.
fn cube_under(features: &[(&str, Value)]) -> Model {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut wire = serde_json::to_value(&ir).unwrap();
    wire["model"]["features"] = features
        .iter()
        .enumerate()
        .map(|(i, (name, definition))| {
            json!({"id": format!("inventor:model:feature#{i}"), "ordinal": i, "name": name, "suppressed": false, "definition": {"kind": "operation", "value": definition}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []})
        })
        .collect();
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "cube".to_owned(),
        source_file: Some("cube.ipt".to_owned()),
        source_step: None,
        assembly: None,
    }
}

#[test]
fn a_corner_that_leaves_its_body_unchanged_needs_no_feature() {
    let corner = json!({"kind": "sheet_metal_corner", "value": {"bend": "inventor:model:feature#0", "effect": "unchanged"}});
    let model = cube_under(&[("Corner1", corner)]);
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let outcome = report.outcome().unwrap();
    let row = outcome
        .features
        .iter()
        .find(|f| f.name.as_deref() == Some("Corner1"))
        .unwrap();
    assert_eq!(
        row.status,
        dutchman_core::FeatureStatus::NotNeeded,
        "{row:?}"
    );
}

#[test]
fn a_design_cadmpeg_decodes_no_feature_of_has_no_history_to_flatten() {
    // An Inventor part made of a base solid: its one feature is kept in its native form.
    let native =
        json!({"kind": "native", "value": {"kind": "InventorClass:0843c7c2", "parameters": {}}});
    let model = cube_under(&[("Base1", native.clone())]);
    let mut report = Report::default();
    let outcome = SolidCraftWriter::new().plan(&model, &mut report);
    assert_eq!(outcome.mode, Mode::Import);
    assert_eq!(
        outcome.check,
        Check::NotApplicable(
            "no feature history cadmpeg decodes: its one feature is kept in its native form"
                .to_owned()
        )
    );
    let model = cube_under(&[("Base1", native.clone()), ("Composite1", native.clone())]);
    let mut report = Report::default();
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .write(&model, &mut Vec::new(), &mut report)
        .unwrap();
    let written = report.outcome().unwrap();
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail.starts_with("not written: no feature history")));
    assert_eq!(
        written.check,
        Check::NotApplicable(
            "no feature history cadmpeg decodes: its 2 features are kept in their native form"
                .to_owned()
        )
    );
    // A native feature after a valid extrusion retains that editable prefix.
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    if let Some(features) = wire["model"]["features"].as_array_mut() {
        features.push(
            json!({"id": "f3d:model:feature#9", "ordinal": 9, "name": "Base1", "suppressed": false, "definition": {"kind": "operation", "value": native}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
        );
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let mut report = Report::default();
    let outcome = SolidCraftWriter::new().plan(&model, &mut report);
    assert!(
        matches!(outcome.check, Check::Partial(_)),
        "{:?}",
        outcome.check
    );
    assert_eq!(outcome.mode, Mode::PartialHistory);
    assert_eq!(
        outcome
            .features
            .iter()
            .filter(|f| f.status == dutchman_core::FeatureStatus::Mapped)
            .count(),
        2
    );
}

/// A design of user parameters and no geometry, as a Fusion design other designs take their
/// parameters from: `W1` = 20 mm, `half` = W1 / 2 and an angle.
fn parameters_alone() -> Model {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut wire = serde_json::to_value(&ir).unwrap();
    for home in ["geometry", "model"] {
        if let Some(arenas) = wire[home].as_object_mut() {
            for rows in arenas.values_mut() {
                if rows.is_array() {
                    *rows = json!([]);
                }
            }
        }
    }
    wire["model"]["parameters"] = json!([
        {"id": "f3d:model:parameter#1", "ordinal": 0, "name": "W1", "expression": "20 mm", "value": {"kind": "length", "value": 20.0}, "dependencies": [], "properties": {}},
        {"id": "f3d:model:parameter#2", "ordinal": 1, "name": "half", "expression": "W1 / 2", "value": {"kind": "length", "value": 10.0}, "dependencies": ["f3d:model:parameter#1"], "properties": {}},
        {"id": "f3d:model:parameter#3", "ordinal": 2, "name": "tilt", "expression": "15 deg", "value": {"kind": "angle", "value": 15.0_f64.to_radians()}, "dependencies": [], "properties": {}},
    ]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "Parameters".to_owned(),
        source_file: Some("Parameters.f3d".to_owned()),
        source_step: None,
        assembly: None,
    }
}

#[test]
fn a_design_of_parameters_alone_is_written_with_its_parameters() {
    let model = parameters_alone();
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    assert_eq!(writer.convertible(&model), Ok(()));
    let reason = "the design holds 3 parameters and no geometry".to_owned();
    let mut report = Report::default();
    let planned = writer.plan(&model, &mut report);
    assert_eq!(planned.check, Check::NotApplicable(reason.clone()));
    let (document, report) = convert(&model, &writer);
    assert_eq!(
        document["params"],
        json!([
            {"name": "W1", "expr": "20 mm", "unit": "mm", "model": false},
            {"name": "half", "expr": "W1 / 2", "unit": "mm", "model": false},
            {"name": "tilt", "expr": "15 deg", "unit": "deg", "model": false},
        ])
    );
    assert_eq!(document["features"], json!([]));
    let outcome = report.outcome().unwrap();
    assert_eq!(outcome.mode, Mode::Import);
    assert_eq!(outcome.check, Check::NotApplicable(reason));
    assert!(report.losses().iter().any(|loss| loss.subject == "geometry"
        && loss.detail == "none: the design holds 3 parameters and no geometry"));

    // A design with neither geometry nor parameters still has nothing to write.
    let mut empty = parameters_alone();
    let mut wire = serde_json::to_value(&empty.ir).unwrap();
    wire["model"]["parameters"] = json!([]);
    empty.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    assert!(writer.convertible(&empty).is_err());
}

#[test]
fn solidcraft_opens_a_design_of_parameters_alone() {
    let Some(cli) = solidcraft() else { return };
    assert_eq!(
        volumes(&cli, &written(&parameters_alone()).0),
        Vec::<f64>::new()
    );
}

/// Change only the extrusion definition of the synthetic cube history.
fn changed_extrude(change: impl FnOnce(&mut Value)) -> Model {
    let mut model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    change(&mut wire["model"]["features"][1]["definition"]["value"]["value"]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn one_sided_extrude_draft_keeps_its_sign_in_both_directions() {
    for direction in ["profile_normal", "reversed_profile_normal"] {
        for degrees in [-10.0_f64, 10.0] {
            let model = changed_extrude(|definition| {
                definition["direction"] = json!({"kind": direction});
                definition["extent"]["side"]["draft"] = json!(degrees.to_radians());
            });
            let (document, report) = convert(
                &model,
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
            let extent = &document["features"][1]["extent"];
            assert_eq!(extent["taper"], format!("{degrees} deg"));
            let expected_direction = if direction == "profile_normal" {
                "positive"
            } else {
                "negative"
            };
            assert_eq!(extent["direction"], expected_direction);
        }
    }
}

#[test]
fn solidcraft_rebuilds_drafted_extrude_volumes() {
    let Some(cli) = solidcraft() else { return };
    for direction in ["profile_normal", "reversed_profile_normal"] {
        for degrees in [-10.0_f64, 10.0] {
            let model = changed_extrude(|definition| {
                definition["direction"] = json!({"kind": direction});
                definition["extent"]["side"]["draft"] = json!(degrees.to_radians());
            });
            let (bytes, _) = written(&model);
            // Positive taper grows a 10 mm square outward as it travels 10 mm.
            let top = 10.0 + 20.0 * degrees.to_radians().tan();
            let expected = 10.0 / 3.0 * (100.0 + 10.0 * top + top * top);
            let actual = volumes(&cli, &bytes);
            assert_eq!(actual.len(), 1);
            assert!(
                (actual[0] - expected).abs() < 1e-4,
                "{actual:?}, {expected}"
            );
        }
    }
}

/// A resolved parallel plane end, optionally also present as a placed datum helper.
fn plane_ended_extrude(z: f64, datum: bool, change: impl FnOnce(&mut Value)) -> Model {
    let frame = json!({
        "origin": {"x": 37.0, "y": -23.0, "z": z},
        "normal": {"x": 0.0, "y": 0.0, "z": -2.0},
        "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
    });
    let mut model = changed_extrude(|definition| {
        definition["extent"]["side"]["termination"] = json!({
            "kind": "to_plane", "plane": frame, "native": "synthetic plane"
        });
        change(definition);
    });
    if datum {
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["features"][1]["ordinal"] = json!(2);
        wire["model"]["features"][1]["dependencies"] = json!(["f3d:model:feature#3"]);
        wire["model"]["features"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "f3d:model:feature#3", "ordinal": 1, "name": "End plane", "suppressed": false, "dependencies": [], "outputs": [], "definition": {"kind": "operation", "value": {"kind": "datum_plane", "value": {"frame": frame}}}, "source_properties": {}, "source_content": []}));
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    }
    model
}

#[test]
fn parallel_plane_ended_extrudes_use_free_to_object_points() {
    for datum in [false, true] {
        for reverse in [false, true] {
            let z = if reverse { -10.0 } else { 10.0 };
            let model = plane_ended_extrude(z, datum, |definition| {
                if reverse {
                    definition["direction"] = json!({"kind": "reversed_profile_normal"});
                }
            });
            let (document, report) = convert(
                &model,
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            assert!(
                history_line(&report).starts_with("2 of 2 features rebuilt"),
                "{}",
                history_line(&report)
            );
            assert_eq!(
                document["features"][1]["extent"]["to"],
                json!([37.0, -23.0, z])
            );
            assert_eq!(document["features"][1]["extent"]["distance"], "0 mm");
            if let Some(cli) = solidcraft() {
                let actual = volumes(&cli, &written(&model).0);
                assert_eq!(actual.len(), 1);
                assert!((actual[0] - 1000.0).abs() < 1e-4, "{actual:?}");
            }
        }
    }
}

#[test]
fn plane_ended_extrudes_follow_a_placed_sketch_normal() {
    let mut model = plane_ended_extrude(10.0, false, |_| {});
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketches"][0]["placement"]["frame"] = json!({
        "origin": {"x": 5.0, "y": 2.0, "z": 3.0},
        "normal": {"x": 1.0, "y": 0.0, "z": 0.0},
        "u_axis": {"x": 0.0, "y": 1.0, "z": 0.0}
    });
    wire["model"]["features"][1]["definition"]["value"]["value"]["extent"]["side"]["termination"]
        ["plane"] = json!({
        "origin": {"x": 15.0, "y": -23.0, "z": 37.0},
        "normal": {"x": 2.0, "y": 0.0, "z": 0.0},
        "u_axis": {"x": 0.0, "y": 0.0, "z": 1.0}
    });
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    assert_eq!(
        document["features"][1]["extent"]["to"],
        json!([15.0, -23.0, 37.0])
    );
    if let Some(cli) = solidcraft() {
        let actual = volumes(&cli, &written(&model).0);
        assert_eq!(actual.len(), 1);
        assert!((actual[0] - 1000.0).abs() < 1e-4, "{actual:?}");
    }
}

#[test]
fn plane_ended_extrudes_keep_start_offsets_and_two_sided_travel() {
    let model = plane_ended_extrude(10.0, false, |definition| {
        definition["start"] = json!({"kind": "offset_profile_plane", "offset": 3.0});
        let first = definition["extent"]["side"].clone();
        definition["extent"] = json!({"kind": "two_sided", "first": first,
            "second": {"termination": {"kind": "blind", "length": 2.0}}});
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    assert_eq!(document["features"][1]["extent"]["start_offset"], "3 mm");
    assert_eq!(document["features"][1]["extent"]["distance2"], "2 mm");
    if let Some(cli) = solidcraft() {
        let actual = volumes(&cli, &written(&model).0);
        assert_eq!(actual.len(), 1);
        assert!((actual[0] - 900.0).abs() < 1e-4, "{actual:?}");
    }
}

#[test]
fn unsupported_plane_and_vertex_extrude_ends_name_the_blocker() {
    for (termination, reason) in [
        (
            json!({"kind": "to_plane", "native": "synthetic tilted plane", "plane": {
                "origin": {"x": 0.0, "y": 0.0, "z": 10.0},
                "normal": {"x": 0.0, "y": 1.0, "z": 1.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
            }}),
            "terminating plane not parallel",
        ),
        (
            json!({"kind": "to_vertex", "vertex": {"kind": "native", "value": "synthetic vertex"}}),
            "to-vertex extrude needs a resolved input vertex position",
        ),
    ] {
        let model = changed_extrude(|definition| {
            definition["extent"]["side"]["termination"] = termination;
        });
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let line = history_line(&report);
        assert!(line.contains("Block") && line.contains(reason), "{line}");
    }
    for (z, change, reason) in [
        (-10.0, "wrong_side", "not along the selected extrude side"),
        (10.0, "symmetric", "symmetric extrude to an object"),
        (
            10.0,
            "negative_end",
            "non-blind extrude end against the sketch normal",
        ),
    ] {
        let model = plane_ended_extrude(z, false, |definition| {
            let side = definition["extent"]["side"].clone();
            if change == "symmetric" {
                definition["extent"] = json!({"kind": "symmetric", "side": side});
            } else if change == "negative_end" {
                definition["extent"] = json!({"kind": "two_sided",
                    "first": {"termination": {"kind": "blind", "length": 2.0}}, "second": side});
            }
        });
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

/// An earlier cube followed by another extrusion using its top face as an end or start.
fn face_ended_extrude(from_face: bool, offset: f64, resolved: bool) -> Model {
    let mut model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let selection = if resolved {
        json!({"kind": "resolved", "value": {
            "faces": ["synthetic:cube:face#top"], "native": "synthetic top face"
        }})
    } else {
        json!({"kind": "faces", "value": ["synthetic:cube:face#top"]})
    };
    let mut next = wire["model"]["features"][1].clone();
    next["id"] = json!("f3d:model:feature#3");
    next["ordinal"] = json!(2);
    next["name"] = json!("Face extent");
    next["outputs"] = json!([]);
    next["dependencies"] = json!(["f3d:model:feature#2"]);
    if from_face {
        next["definition"]["value"]["value"]["start"] = json!({
            "kind": "from_face", "face": selection, "offset": offset
        });
        next["definition"]["value"]["value"]["extent"]["side"]["termination"]["length"] =
            json!(2.0);
    } else {
        next["definition"]["value"]["value"]["start"] =
            json!({"kind": "offset_profile_plane", "offset": 12.0});
        next["definition"]["value"]["value"]["direction"] =
            json!({"kind": "reversed_profile_normal"});
        next["definition"]["value"]["value"]["extent"]["side"]["termination"] = json!({
            "kind": "to_face", "face": selection, "offset": offset
        });
    }
    wire["model"]["features"].as_array_mut().unwrap().push(next);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

/// [`face_ended_extrude`] with its second extrude ending at a plane at height `z`, whose
/// source (`face_surface`, `work_plane`, ...) is `source`.
fn extrude_ending_at_plane(z: f64, source: Option<&str>) -> Model {
    let mut model = face_ended_extrude(false, 0.0, false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let mut termination = json!({
        "kind": "to_plane", "native": "synthetic plane", "plane": {
            "origin": {"x": 0.0, "y": 5.0, "z": z},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
        }
    });
    if let Some(source) = source {
        termination["source"] = json!(source);
    }
    wire["model"]["features"][2]["definition"]["value"]["value"]["extent"]["side"]["termination"] =
        termination;
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn plane_ended_extrudes_after_bodies_end_on_a_face_surface() {
    // The plane is the surface of a face: a SolidCraft pick there ends at the same plane.
    let (document, report) = convert(
        &extrude_ending_at_plane(10.0, Some("face_surface")),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features rebuilt"),
        "{}",
        history_line(&report)
    );
    assert_eq!(
        document["features"][2]["extent"]["to"],
        json!([0.0, 5.0, 10.0])
    );
}

#[test]
fn plane_started_extrudes_start_at_the_plane_offset() {
    // The fixture's second extrude starts 12 mm above the sketch and runs down to the top
    // face; the same start given as a plane gives the same start offset.
    let mut model = face_ended_extrude(false, 0.0, false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][2]["definition"]["value"]["value"]["start"] = json!({
        "kind": "from_plane", "native": "synthetic plane", "source": "face_surface", "plane": {
            "origin": {"x": 0.0, "y": 0.0, "z": 12.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
        }
    });
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let line = history_line(&report);
    assert!(!line.contains("extrude starts are not mapped"), "{line}");
    assert_eq!(
        document["features"][2]["extent"]["start_offset"], "12 mm",
        "{line}\n{}",
        document["features"]
    );
}

#[test]
fn plane_ended_extrudes_after_bodies_need_input_face_geometry() {
    for (z, source) in [(10.0, None), (5.0, None), (10.0, Some("work_plane"))] {
        let model = extrude_ending_at_plane(z, source);
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let line = history_line(&report);
        assert!(
            line.contains("Face extent")
                && line.contains("CADIR does not supply the input face geometry"),
            "{line}"
        );
        assert_partial(&document, &report, 1);
    }
}

#[test]
fn resolved_face_ends_and_starts_use_solidcraft_picks() {
    for resolved in [false, true] {
        for from in [false, true] {
            let (document, report) = convert(
                &face_ended_extrude(from, -0.5, resolved),
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            assert!(
                history_line(&report).starts_with("3 of 3 features rebuilt"),
                "{}",
                history_line(&report)
            );
            let extent = &document["features"][2]["extent"];
            let pick = &extent[if from { "from" } else { "to" }];
            assert_eq!(pick[2], 10.0);
            assert!(pick[0].as_f64().unwrap() > 0.0 && pick[0].as_f64().unwrap() < 10.0);
            assert!(pick[1].as_f64().unwrap() > 0.0 && pick[1].as_f64().unwrap() < 10.0);
            if from {
                assert_eq!(extent["start_offset"], "-0.5 mm");
                assert_eq!(extent["distance"], "2 mm");
            } else {
                assert_eq!(extent["start_offset"], "12 mm");
                assert_eq!(extent["to_offset"], "-0.5 mm");
                assert_eq!(extent["distance"], "0 mm");
            }
        }
    }
}

#[test]
fn solidcraft_rebuilds_face_ended_and_face_started_extrudes() {
    let Some(cli) = solidcraft() else { return };
    for (from, expected) in [(false, 150.0), (true, 200.0)] {
        let (bytes, report) = written(&face_ended_extrude(from, -0.5, false));
        assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
        let mut actual = volumes(&cli, &bytes);
        actual.sort_by(f64::total_cmp);
        assert_eq!(actual.len(), 2);
        assert!((actual[0] - expected).abs() < 1e-4, "{actual:?}");
        assert!((actual[1] - 1000.0).abs() < 1e-4, "{actual:?}");
    }
}

#[test]
fn reverse_start_offsets_and_two_sided_distances_follow_the_normal() {
    let model = changed_extrude(|definition| {
        definition["direction"] = json!({"kind": "reversed_profile_normal"});
        definition["start"] = json!({"kind": "offset_profile_plane", "offset": 3.0});
        definition["extent"] = json!({"kind": "two_sided",
            "first": {"termination": {"kind": "blind", "length": 7.0}},
            "second": {"termination": {"kind": "blind", "length": 2.0}}});
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    assert_eq!(
        document["features"][1]["extent"],
        json!({
            "direction": "positive", "distance": "2 mm", "distance2": "7 mm", "start_offset": "3 mm"
        })
    );
    if let Some(cli) = solidcraft() {
        let (bytes, _) = written(&model);
        let actual = volumes(&cli, &bytes);
        assert_eq!(actual.len(), 1);
        assert!((actual[0] - 900.0).abs() < 1e-4);
    }
}

#[test]
fn equal_two_sided_revolves_become_symmetric() {
    let model = ring_history(json!({"kind": "two_sided",
        "first": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2},
        "second": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2}}));
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features rebuilt"));
    assert_eq!(document["features"][1]["symmetric"], true);
    assert_eq!(document["features"][1]["angle"], "180 deg");
    if let Some(cli) = solidcraft() {
        let (bytes, _) = written(&model);
        let actual = volumes(&cli, &bytes);
        assert_eq!(actual.len(), 1);
        assert!((actual[0] - 375.0 * std::f64::consts::PI).abs() < 1e-3);
    }
}

#[test]
fn unsupported_end_combinations_keep_specific_refusals() {
    for kind in ["symmetric", "two_sided"] {
        let model = changed_extrude(|definition| {
            let side = json!({"termination": {"kind": "blind", "length": 4.0}, "draft": 0.1});
            definition["extent"] = if kind == "symmetric" {
                json!({"kind": kind, "side": side})
            } else {
                json!({"kind": kind, "first": side, "second": side})
            };
        });
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).contains("draft on a symmetric or two-sided extrude"));
    }
    let model = ring_history(json!({"kind": "one_sided", "termination": {
        "kind": "to_face", "face": {"kind": "faces", "value": ["synthetic:cube:face#top"]}
    }}));
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).contains("CADIR does not state which way it turns"));
}

#[test]
fn face_selections_that_cannot_be_placed_stop_the_history() {
    for (selection, reason) in [
        (
            json!({"kind": "native", "value": "synthetic top face"}),
            "a native face reference is not supported yet",
        ),
        (
            json!({"kind": "faces", "value": []}),
            "without exactly one face",
        ),
        (
            json!({"kind": "faces", "value": ["synthetic:cube:face#top", "synthetic:cube:face#bottom"]}),
            "without exactly one face",
        ),
        (
            json!({"kind": "faces", "value": ["synthetic:cube:face#front"]}),
            "not parallel to its sketch",
        ),
    ] {
        let mut model = face_ended_extrude(false, 0.0, false);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let termination = &mut wire["model"]["features"][2]["definition"]["value"]["value"]
            ["extent"]["side"]["termination"];
        termination["face"] = selection;
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert_partial(&document, &report, 1);
        assert_eq!(
            types(&document),
            [
                ("sketch".to_owned(), false),
                ("extrude".to_owned(), false),
                ("import".to_owned(), false),
            ]
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
    let model = changed_extrude(|definition| {
        definition["extent"]["side"]["termination"] = json!({"kind": "to_face",
            "face": {"kind": "faces", "value": ["synthetic:cube:face#top"]}});
    });
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).contains("not on an earlier named body"));
}

#[test]
fn a_face_at_an_angle_to_the_sketch_is_gated_to_solidcraft_0_44() {
    // The front face is perpendicular to the sketch: older targets name the release that
    // reaches slanted faces, and 0.44 still refuses a face the sweep runs along.
    let detected = dutchman_solidcraft::SolidCraftVersion::detected(&Verification::Auto);
    let detected_reason = if dutchman_solidcraft::VersionGate::SlantedFaceExtent.supported(detected)
    {
        "runs along the travel"
    } else {
        "needs SolidCraft 0.44 or newer"
    };
    for (target, reason) in [
        (Some("0.18.2"), "needs SolidCraft 0.44 or newer"),
        (Some("0.43.0"), "needs SolidCraft 0.44 or newer"),
        (Some("0.44.0"), "runs along the travel"),
        (None, detected_reason),
    ] {
        let mut model = face_ended_extrude(false, 0.0, false);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["features"][2]["definition"]["value"]["value"]["extent"]["side"]
            ["termination"]["face"] =
            json!({"kind": "faces", "value": ["synthetic:cube:face#front"]});
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let writer = SolidCraftWriter::new()
            .verification(Verification::Off)
            .target_version(target.map(|t| t.parse().unwrap()));
        let (_, report) = convert(&model, &writer);
        assert!(
            history_line(&report).contains(reason),
            "{target:?}: {}",
            history_line(&report)
        );
    }
}

/// A small tilted sketch above a cube; its sweep meets the horizontal top obliquely.
fn slanted_face_extent(from: bool, historical: bool) -> Model {
    let mut model = face_ended_extrude(from, 0.0, false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let sketch = "test:slanted:sketch#2";
    let mut placed = wire["model"]["sketches"][0].clone();
    placed["id"] = json!(sketch);
    placed["placement"]["frame"] = json!({
        "origin": {"x": 3.0, "y": 6.0, "z": 14.0},
        "normal": {"x": 0.0, "y": 0.6, "z": 0.8},
        "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
    });
    let mut curves = vec![
        line(11, [0.0, 0.0], [4.0, 0.0]),
        line(12, [4.0, 0.0], [4.0, 4.0]),
        line(13, [4.0, 4.0], [0.0, 4.0]),
        line(14, [0.0, 4.0], [0.0, 0.0]),
    ];
    placed["profiles"] = json!([curves
        .iter()
        .map(|curve| { json!({"entity": curve["id"], "reversed": false}) })
        .collect::<Vec<_>>()]);
    for curve in &mut curves {
        curve["sketch"] = json!(sketch);
    }
    wire["model"]["sketches"]
        .as_array_mut()
        .unwrap()
        .push(placed);
    wire["model"]["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .extend(curves);
    let mut feature = wire["model"]["features"][0].clone();
    feature["id"] = json!("test:slanted:feature#sketch");
    feature["ordinal"] = json!(2);
    feature["definition"]["value"]["value"]["sketch"]["sketch"] = json!(sketch);
    wire["model"]["features"][2]["ordinal"] = json!(3);
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(feature);
    let definition = &mut wire["model"]["features"][2]["definition"]["value"]["value"];
    definition["profile"] = json!({"kind": "planar", "value": {"kind": "sketch", "value": sketch}});
    if !from {
        definition["start"] = json!({"kind": "profile_plane"});
    }
    if historical {
        // The same top face resolved only from input boundary samples, as Onshape supplies it.
        let source = face_extrude(true, |_| {});
        let source = serde_json::to_value(&source.ir).unwrap();
        wire["model"]["feature_input_topologies"] =
            source["model"]["feature_input_topologies"].clone();
        let selection = json!({"kind": "historical", "value": {
            "state": "test:face:feature-input#3", "faces": ["test:face:historical-face#target"],
            "native": "synthetic top face"
        }});
        let definition = &mut wire["model"]["features"][2]["definition"]["value"]["value"];
        if from {
            definition["start"]["face"] = selection;
        } else {
            definition["extent"]["side"]["termination"]["face"] = selection;
        }
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn slanted_face_starts_and_ends_map_only_on_supported_targets() {
    for from in [false, true] {
        for historical in [false, true] {
            let model = slanted_face_extent(from, historical);
            for target in ["0.43.0", "0.44.0", "0.44.1"] {
                let writer = SolidCraftWriter::new()
                    .verification(Verification::Off)
                    .target_version(Some(target.parse().unwrap()));
                let (document, report) = convert(&model, &writer);
                let line = history_line(&report);
                if target == "0.43.0" {
                    assert!(line.contains("needs SolidCraft 0.44 or newer"), "{line}");
                } else {
                    assert!(line.starts_with("4 of 4 features rebuilt"), "{line}");
                    let extent = &document["features"][3]["extent"];
                    assert_eq!(extent[if from { "from" } else { "to" }][2], 10.0);
                }
            }
        }
    }
}

#[test]
fn solidcraft_rebuilds_a_slanted_face_end_with_the_expected_volume() {
    let Some(cli) = solidcraft() else { return };
    // Use the CLI's actual release: detected() intentionally caps the default output target.
    let version = dutchman_solidcraft::solidcraft_version(&Verification::With(cli.clone()))
        .and_then(|line| {
            line.split_whitespace()
                .last()?
                .trim_start_matches('v')
                .parse::<dutchman_solidcraft::SolidCraftVersion>()
                .ok()
        })
        .expect("the configured SolidCraft CLI must report its release");
    if !dutchman_solidcraft::VersionGate::SlantedFaceExtent.supported(version) {
        return;
    }
    for historical in [false, true] {
        let writer = SolidCraftWriter::new()
            .verification(Verification::Off)
            .target_version(Some("0.44.0".parse().unwrap()));
        let (bytes, report) = written_with(&slanted_face_extent(false, historical), &writer);
        assert!(history_line(&report).starts_with("4 of 4 features rebuilt"));
        let mut actual = volumes(&cli, &bytes);
        actual.sort_by(f64::total_cmp);
        assert_eq!(actual.len(), 2, "{actual:?}");
        // Profile area 16; mean travel (14 - 0.6 * 2 - 10) / 0.8 = 3.5.
        assert!((actual[0] - 56.0).abs() < 1e-4, "{actual:?}");
        assert!((actual[1] - 1000.0).abs() < 1e-4, "{actual:?}");
    }
}

#[test]
fn a_two_sided_extrude_can_end_at_a_face_along_the_positive_normal() {
    let mut model = face_ended_extrude(false, 0.0, false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let definition = &mut wire["model"]["features"][2]["definition"]["value"]["value"];
    let first = definition["extent"]["side"].clone();
    definition["direction"] = json!({"kind": "profile_normal"});
    definition["start"] = json!({"kind": "offset_profile_plane", "offset": 5.0});
    definition["extent"] = json!({"kind": "two_sided", "first": first,
        "second": {"termination": {"kind": "blind", "length": 2.0}}});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("3 of 3 features rebuilt"));
    assert_eq!(document["features"][2]["extent"]["distance2"], "2 mm");
    assert_eq!(document["features"][2]["extent"]["to"][2], 10.0);
    if let Some(cli) = solidcraft() {
        let (bytes, _) = written(&model);
        let mut actual = volumes(&cli, &bytes);
        actual.sort_by(f64::total_cmp);
        assert_eq!(actual.len(), 2);
        assert!((actual[0] - 700.0).abs() < 1e-4, "{actual:?}");
    }
}

#[test]
fn a_face_end_does_not_silently_reverse_the_extrude() {
    let mut model = face_ended_extrude(false, 0.0, false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][2]["definition"]["value"]["value"]["direction"] =
        json!({"kind": "profile_normal"});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).contains("not along the selected extrude side"));
}

/// Synthetic sketch additions on the example cube; every relation is tested alone so
/// another relation cannot hide a missing mapping by already determining the sketch.
fn sketch_relation_history(definition: Value) -> Model {
    let mut model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let rows = wire["model"]["sketch_entities"].as_array_mut().unwrap();
    let mut extra = vec![
        entity(
            5,
            json!({"kind": "circle", "center": {"u": 20.0, "v": 20.0}, "radius": 2.0}),
        ),
        entity(
            6,
            json!({"kind": "circle", "center": {"u": 20.0, "v": 20.0}, "radius": 2.0}),
        ),
        entity(
            7,
            json!({"kind": "ellipse", "center": {"u": 20.0, "v": 20.0},
            "major_angle": 0.0, "major_radius": 4.0, "minor_radius": 2.0}),
        ),
        entity(
            8,
            json!({"kind": "point", "position": {"u": 5.0, "v": 5.0}}),
        ),
        entity(
            9,
            json!({"kind": "arc", "center": {"u": 10.0, "v": 2.0},
            "radius": 2.0, "start_angle": -std::f64::consts::FRAC_PI_2, "end_angle": 0.0}),
        ),
        line(10, [10.0, 0.0], [20.0, 0.0]),
    ];
    for row in &mut extra {
        row["construction"] = json!(true);
    }
    rows.extend(extra);
    wire["model"]["sketch_constraints"] = json!([{
        "id": "f3d:model:sketch-constraint#1", "sketch": SKETCH, "definition": definition,
    }]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn sketch_locus(kind: &str, key: u32) -> Value {
    json!({"kind": kind, "value": format!("f3d:model:sketch-entity#{key}")})
}

#[test]
fn additional_sketch_relations_keep_their_intent_and_cube_volume() {
    let id = |key| format!("f3d:model:sketch-entity#{key}");
    let parameter = "f3d:model:parameter#2";
    let cases = vec![
        (
            json!({"kind": "same_coordinate", "relation": {
            "first": sketch_locus("start", 1), "second": sketch_locus("end", 3), "axis": "u"}}),
            "vertical_points",
        ),
        (
            json!({"kind": "point_coordinate_values", "point": sketch_locus("entity", 8),
            "values": [5.0, 5.0]}),
            "fix",
        ),
        (
            json!({"kind": "point_coordinate_values", "point": sketch_locus("start", 1),
            "values": [0.0, 0.0]}),
            "coincident",
        ),
        (
            json!({"kind": "at_intersection", "point": sketch_locus("end", 1),
            "first": id(1), "second": id(2)}),
            "point_on_curve",
        ),
        (
            json!({"kind": "coradial", "first": id(5), "second": id(6)}),
            "concentric",
        ),
        (
            json!({"kind": "tangent_loci", "first": sketch_locus("end", 1),
            "second": sketch_locus("start", 9)}),
            "tangent",
        ),
        (
            json!({"kind": "curvature", "first": id(1), "second": id(10)}),
            "smooth",
        ),
        (
            json!({"kind": "point_symmetric", "first": sketch_locus("start", 1),
            "second": sketch_locus("start", 3), "center": sketch_locus("entity", 8)}),
            "midpoint",
        ),
        (
            json!({"kind": "polygon", "polygon": {"entities": [id(1), id(2), id(3), id(4)]}}),
            "equal",
        ),
        (
            json!({"kind": "distance_loci_value", "first": sketch_locus("start", 1),
            "second": sketch_locus("end", 1), "distance": 10.0, "parameter": parameter}),
            "distance",
        ),
        (
            json!({"kind": "repeated_length", "entities": [id(1), id(2)], "parameter": parameter}),
            "length",
        ),
        (
            json!({"kind": "repeated_distance", "measurements": [
            {"kind": "horizontal", "first": sketch_locus("start", 1), "second": sketch_locus("end", 2)},
            {"kind": "vertical", "first": sketch_locus("start", 4), "second": sketch_locus("end", 10)}],
            "parameter": parameter}),
            "distance_x",
        ),
        (
            json!({"kind": "arc_angle", "entity": id(9), "angle": std::f64::consts::FRAC_PI_2}),
            "arc_sweep",
        ),
        (
            json!({"kind": "polar_distance", "first": sketch_locus("start", 1),
            "second": sketch_locus("end", 1), "distance": 10.0, "angle": 0.0,
            "distance_parameter": parameter}),
            "length",
        ),
        (
            json!({"kind": "equal_distance", "first": {"first": sketch_locus("start", 1), "second": sketch_locus("end", 1)},
            "second": {"first": sketch_locus("start", 2), "second": sketch_locus("end", 2)}}),
            "equal",
        ),
        (
            json!({"kind": "midpoint_coordinate", "first": sketch_locus("start", 1),
            "second": sketch_locus("end", 1), "axis": "u", "value": 5.0}),
            "midpoint",
        ),
        (
            json!({"kind": "parallel_line_set_distance", "first": [id(1), id(10)],
            "second": [id(3)], "parameter": parameter}),
            "parallel",
        ),
    ];
    let cli = solidcraft();
    for (definition, expected) in cases {
        let model = sketch_relation_history(definition.clone());
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert_eq!(
            document["features"][0]["type"],
            "sketch",
            "{definition}: {:?}",
            report.losses()
        );
        assert!(
            !report
                .losses()
                .iter()
                .any(|loss| loss.detail.contains("constraints not carried")),
            "{definition}: {:?}",
            report.losses()
        );
        let constraints = document["features"][0]["sketch"]["constraints"]
            .as_array()
            .unwrap();
        assert_eq!(constraints[0]["type"], expected, "{definition}");
        if expected == "coincident" {
            // A point held at the sketch origin ties to SolidCraft's origin point.
            assert_eq!(constraints[0]["q"], 0, "{definition}: {constraints:?}");
        }
        if definition["parameter"] == parameter || definition["distance_parameter"] == parameter {
            assert!(
                constraints.iter().any(|c| c["param"] == "side"),
                "{definition}: {constraints:?}"
            );
        }
        if let Some(cli) = &cli {
            let got = volumes(cli, &written(&model).0);
            assert_eq!(got.len(), 1, "{definition}: {got:?}");
            assert!((got[0] - 1000.0).abs() < 1e-6, "{definition}: {got:?}");
        }
    }
}

#[test]
fn fixed_curves_hold_the_radius_and_ellipse_radius_dimensions_use_the_minor_radius() {
    for key in [5, 7] {
        let model = sketch_relation_history(
            json!({"kind": "fixed", "entity": format!("f3d:model:sketch-entity#{key}")}),
        );
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(!report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("constraints not carried")));
        assert_eq!(
            document["features"][0]["sketch"]["curves"][key - 1]["fixed"],
            true
        );
        if let Some(cli) = solidcraft() {
            assert_eq!(volumes(&cli, &written(&model).0), [1000.0]);
        }
    }
    for kind in ["radius", "repeated_radius", "repeated_diameter"] {
        let mut definition = json!({"kind": kind, "parameter": "f3d:model:parameter#3"});
        if kind == "radius" {
            definition["entity"] = json!("f3d:model:sketch-entity#7");
        } else {
            definition["entities"] =
                json!(["f3d:model:sketch-entity#5", "f3d:model:sketch-entity#6"]);
        }
        let mut model = sketch_relation_history(definition);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let value = if kind == "repeated_diameter" {
            4.0
        } else {
            2.0
        };
        wire["model"]["parameters"].as_array_mut().unwrap().push(json!({"id": "f3d:model:parameter#3", "name": "round_size", "expression": format!("{value} mm"), "value": {"kind": "length", "value": value}, "ordinal": 0, "dependencies": [], "properties": {}}));
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(!report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("constraints not carried")));
        let constraints = document["features"][0]["sketch"]["constraints"]
            .as_array()
            .unwrap();
        assert_eq!(constraints.len(), if kind == "radius" { 1 } else { 2 });
        for c in constraints {
            assert_eq!(c["value"], value);
            assert_eq!(c["param"], "round_size");
        }
        if let Some(cli) = solidcraft() {
            assert_eq!(volumes(&cli, &written(&model).0), [1000.0]);
        }
    }
}

fn bounded_conic_history(geometry: Value, start: [f64; 2], end: [f64; 2]) -> Model {
    let mut model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketches"][0]["profiles"] = json!([]);
    wire["model"]["sketch_constraints"] = json!([]);
    wire["model"]["sketch_entities"] = json!([entity(1, geometry), line(2, end, start)]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn bounded_conics_and_weighted_beziers_preserve_their_shape() {
    let h = 0.5_f64;
    let cases = vec![
        (
            json!({"kind": "ellipse", "center": {"u": 0.0, "v": 0.0}, "major_angle": 0.0,
            "major_radius": 4.0, "minor_radius": 2.0, "bounds": [0.0, std::f64::consts::FRAC_PI_2]}),
            [4.0, 0.0],
            [0.0, 2.0],
            20.0 * (std::f64::consts::PI - 2.0),
        ),
        (
            json!({"kind": "parabola", "vertex": {"u": 0.0, "v": 0.0}, "axis_angle": 0.0,
            "focal_length": 1.0, "bounds": [-2.0, 2.0]}),
            [1.0, -2.0],
            [1.0, 2.0],
            80.0 / 3.0,
        ),
        (
            json!({"kind": "hyperbola", "center": {"u": 0.0, "v": 0.0}, "major_angle": 0.0,
            "major_radius": 4.0, "minor_radius": 2.0, "bounds": [-h, h]}),
            [4.0 * h.cosh(), -2.0 * h.sinh()],
            [4.0 * h.cosh(), 2.0 * h.sinh()],
            80.0 * (h.sinh() * h.cosh() - h),
        ),
        (
            json!({"kind": "nurbs", "curve": {"degree": 2, "periodic": false,
            "knots": [0.0, 0.0, 0.0, 1.0, 1.0, 1.0], "poles": {"kind": "rational", "points": [
                {"point": {"u": 4.0, "v": 0.0}, "weight": 1.0},
                {"point": {"u": 4.0, "v": 2.0}, "weight": std::f64::consts::FRAC_1_SQRT_2},
                {"point": {"u": 0.0, "v": 2.0}, "weight": 1.0}]}}}),
            [4.0, 0.0],
            [0.0, 2.0],
            20.0 * (std::f64::consts::PI - 2.0),
        ),
    ];
    let cli = solidcraft();
    for (geometry, start, end, expected_volume) in cases {
        let model = bounded_conic_history(geometry.clone(), start, end);
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let sketch = &document["features"][0]["sketch"];
        assert_eq!(
            document["features"][0]["type"],
            "sketch",
            "{geometry}: {:?}",
            report.losses()
        );
        assert_eq!(sketch["curves"][0]["type"], "conic");
        assert_eq!(sketch["curves"][0]["a"], sketch["curves"][1]["b"]);
        assert_eq!(sketch["curves"][0]["b"], sketch["curves"][1]["a"]);
        let rho = sketch["curves"][0]["rho"].as_f64().unwrap();
        let weight = match geometry["kind"].as_str().unwrap() {
            "parabola" => 1.0,
            "hyperbola" => h.cosh(),
            _ => std::f64::consts::FRAC_1_SQRT_2,
        };
        assert!((rho - weight / (1.0 + weight)).abs() < 1e-12);
        let apex = usize::try_from(sketch["curves"][0]["apex"].as_u64().unwrap()).unwrap();
        let expected_apex = match geometry["kind"].as_str().unwrap() {
            "parabola" => [-1.0, 0.0],
            "hyperbola" => [4.0 / h.cosh(), 0.0],
            _ => [4.0, 2.0],
        };
        for (index, expected) in expected_apex.into_iter().enumerate() {
            assert!(
                (sketch["points"][apex]["pos"][index].as_f64().unwrap() - expected).abs() < 1e-12
            );
        }
        if let Some(cli) = &cli {
            let got = volumes(cli, &written(&model).0);
            assert_eq!(got.len(), 1, "{geometry}: {got:?}");
            assert!(
                (got[0] - expected_volume).abs() / expected_volume < 1e-3,
                "{geometry}: {got:?} vs {expected_volume}"
            );
        }
    }
}

#[test]
fn static_pattern_and_projected_copy_relations_map_to_point_and_radius_associations() {
    let id = |key| format!("f3d:model:sketch-entity#{key}");
    let cases = [
        json!({"kind": "projected_copy", "source": id(5), "result": id(6)}),
        json!({"kind": "rectangular_pattern", "pattern": {
            "directions": [{"direction": [1.0, 0.0], "spacing": 10.0},
                {"direction": [0.0, 1.0], "spacing": 10.0}],
            "rows": [[{"entities": [id(5)]}], [{"entities": [id(6)]}]],
        }}),
        json!({"kind": "circular_pattern", "pattern": {"center": id(8), "angle": std::f64::consts::PI,
            "seed": [id(5)], "instances": [{"angle": std::f64::consts::PI, "entities": [id(6)]}],
        }}),
    ];
    let cli = solidcraft();
    for definition in cases {
        let kind = definition["kind"].as_str().unwrap();
        let mut model = sketch_relation_history(definition.clone());
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        if kind == "rectangular_pattern" {
            wire["model"]["sketch_entities"][5]["geometry"]["center"] =
                json!({"u": 30.0, "v": 20.0});
        } else if kind == "circular_pattern" {
            wire["model"]["sketch_entities"][5]["geometry"]["center"] =
                json!({"u": -10.0, "v": -10.0});
        }
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            !report
                .losses()
                .iter()
                .any(|loss| loss.detail.contains("constraints not carried")),
            "{definition}: {:?}",
            report.losses()
        );
        let constraints = document["features"][0]["sketch"]["constraints"]
            .as_array()
            .unwrap();
        assert!(constraints.iter().any(|c| c["type"] == "pattern_radius"));
        if kind != "projected_copy" {
            let point = constraints
                .iter()
                .find(|c| c["type"] == "pattern_point")
                .unwrap();
            if kind == "rectangular_pattern" {
                assert_eq!(point["offset"], json!([10.0, 0.0]));
            } else {
                assert!(point["center"].is_number());
                assert_eq!(point["angle"], std::f64::consts::PI);
            }
        }
        if let Some(cli) = &cli {
            assert_eq!(volumes(cli, &written(&model).0), [1000.0]);
        }
    }
}

#[test]
fn vertical_axis_angles_use_a_fixed_construction_axis_and_keep_the_parameter() {
    let mut model = sketch_relation_history(json!({"kind": "angle_to_axis", "axis": "vertical",
        "entity": "f3d:model:sketch-entity#1", "parameter": "f3d:model:parameter#3"}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["parameters"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:parameter#3", "name": "theta", "expression": "90 deg", "value": {"kind": "angle", "value": std::f64::consts::FRAC_PI_2}, "ordinal": 0, "dependencies": [], "properties": {}}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(!report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("constraints not carried")));
    let sketch = &document["features"][0]["sketch"];
    assert_eq!(sketch["constraints"][0]["type"], "angle");
    assert_eq!(sketch["constraints"][0]["param"], "theta");
    assert_eq!(
        sketch["curves"].as_array().unwrap().last().unwrap()["fixed"],
        true
    );
    if let Some(cli) = solidcraft() {
        assert_eq!(volumes(&cli, &written(&model).0), [1000.0]);
    }
}

#[test]
fn solver_only_sketch_relations_are_counted_without_flattening() {
    let mut model =
        sketch_relation_history(json!({"kind": "scalar_equality", "first": 1, "second": 2}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketch_constraints"].as_array_mut().unwrap().push(json!({
        "id": "f3d:model:sketch-constraint#2", "sketch": SKETCH,
        "definition": {"kind": "angle_difference", "first": 1, "second": 2, "difference": 3, "value": 0.0},
    }));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(document["features"][0]["type"], "sketch");
    let loss = report
        .losses()
        .iter()
        .find(|loss| loss.detail.contains("2 of 2 constraints not carried"))
        .unwrap();
    assert_eq!(loss.severity, Severity::Degraded);
    assert!(loss
        .detail
        .contains("solver scalar identities have no sketch entity binding"));
}

#[test]
fn equal_weight_rational_splines_are_polynomial_control_splines() {
    let model = bounded_conic_history(
        json!({"kind": "nurbs", "curve": {
        "degree": 3, "periodic": false, "knots": [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
        "poles": {"kind": "rational", "points": [
            {"point": {"u": 0.0, "v": 0.0}, "weight": 2.0},
            {"point": {"u": 10.0, "v": 20.0}, "weight": 2.0},
            {"point": {"u": 20.0, "v": 20.0}, "weight": 2.0},
            {"point": {"u": 30.0, "v": 0.0}, "weight": 2.0},
        ]}}}),
        [0.0, 0.0],
        [30.0, 0.0],
    );
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(
        document["features"][0]["type"],
        "sketch",
        "{:?}",
        report.losses()
    );
    let curve = &document["features"][0]["sketch"]["curves"][0];
    assert_eq!(curve["type"], "spline");
    assert_eq!(curve["control"], true);
    assert_eq!(curve["degree"], 3);
    assert_eq!(curve["pts"].as_array().unwrap().len(), 4);
    if let Some(cli) = solidcraft() {
        let got = volumes(&cli, &written(&model).0);
        assert_eq!(got.len(), 1);
        assert!((got[0] - 3000.0).abs() / 3000.0 < 1e-3, "{got:?}");
    }
}

#[test]
fn major_elliptical_arcs_are_exact_conic_chains_in_a_whole_sketch() {
    let geometry = json!({"kind": "ellipse", "center": {"u": 0.0, "v": 0.0}, "major_angle": 0.0,
        "major_radius": 4.0, "minor_radius": 2.0, "bounds": [0.0, 3.0 * std::f64::consts::FRAC_PI_2]});
    let model = bounded_conic_history(geometry, [4.0, 0.0], [0.0, -2.0]);
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(
        document["features"][0]["type"],
        "sketch",
        "{:?}",
        report.losses()
    );
    let curves = document["features"][0]["sketch"]["curves"]
        .as_array()
        .unwrap();
    assert_eq!(curves.len(), 5);
    for pair in curves[..4].windows(2) {
        assert_eq!(pair[0]["type"], "conic");
        assert_eq!(pair[0]["b"], pair[1]["a"]);
    }
    assert_eq!(curves[0]["a"], curves[4]["b"]);
    assert_eq!(curves[3]["b"], curves[4]["a"]);
    if let Some(cli) = solidcraft() {
        let got = volumes(&cli, &written(&model).0);
        let expected = 40.0 * (3.0 * std::f64::consts::FRAC_PI_2 + 1.0);
        assert_eq!(got.len(), 1);
        assert!(
            (got[0] - expected).abs() / expected < 1e-3,
            "{got:?} vs {expected}"
        );
    }
}

#[test]
fn virtual_relations_are_counted_and_inactive_or_disabled_relations_are_quiet() {
    let mut model = sketch_relation_history(json!({"kind": "disabled"}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketch_constraints"]
        .as_array_mut()
        .unwrap()
        .extend([
            json!({"id": "f3d:model:sketch-constraint#2", "sketch": SKETCH, "virtual_space": true,
            "definition": {"kind": "horizontal", "entity": "f3d:model:sketch-entity#1"}}),
            json!({"id": "f3d:model:sketch-constraint#3", "sketch": SKETCH, "active": false,
            "definition": {"kind": "scalar_equality", "first": 1, "second": 2}}),
        ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(document["features"][0]["type"], "sketch");
    assert!(document["features"][0]["sketch"]["constraints"]
        .as_array()
        .unwrap()
        .is_empty());
    let loss = report
        .losses()
        .iter()
        .find(|loss| loss.detail.contains("constraints not carried"))
        .unwrap();
    assert!(loss.detail.starts_with("1 of 2 constraints not carried"));
    assert!(loss.detail.contains("virtual sketch space"));
}

// Body features use only synthetic CADIR and the example cube's topology.
fn modifying(definition: Value) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Modify", "suppressed": false, "definition": {"kind": "operation", "value": definition}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

fn cube_faces(names: &[&str]) -> Value {
    json!({"kind": "faces", "value": names.iter()
        .map(|name| format!("synthetic:cube:face#{name}")).collect::<Vec<_>>()})
}

fn cube_body() -> Value {
    json!({"kind": "bodies", "value": ["synthetic:cube:body#0"]})
}

fn body_mirror_definition() -> Value {
    json!({"kind": "pattern", "value": {"seeds": [{"kind": "bodies", "value": cube_body()}], "pattern": {"kind": "mirror", "plane_origin": {"x": 20.0, "y": 0.0, "z": 0.0},
            "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}}}})
}

#[test]
fn body_mirrors_select_named_bodies_and_keep_the_plane() {
    let (document, report) = convert(
        &modifying(body_mirror_definition()),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let mirror = &document["features"][2];
    assert_eq!(mirror["type"], "mirror", "{}", history_line(&report));
    assert_eq!(mirror["bodies"], document["features"][1]["body_names"]);
    assert_eq!(mirror["features"], json!([]));
    assert_eq!(mirror["combine"], false);
    assert_eq!(mirror["plane"]["plane"]["origin"], json!([20.0, 0.0, 0.0]));
}

fn combined_spheres(operation: &str, keep_tools: bool) -> Model {
    let base = primitive_history(sphere());
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["geometry"];
    // A second, independent synthetic topology, with its own arena identities.
    for arena in [
        "points", "vertices", "curves", "edges", "coedges", "loops", "surfaces", "faces", "shells",
        "regions", "bodies",
    ] {
        let text = serde_json::to_string(&model[arena]).unwrap();
        let mut rows: Vec<Value> =
            serde_json::from_str(&text.replace("synthetic:cube:", "synthetic:tool:")).unwrap();
        if arena == "bodies" {
            rows[0]["name"] = json!("Tool");
        }
        model[arena].as_array_mut().unwrap().extend(rows);
    }
    let model = &mut wire["model"];
    let mut sphere = sphere();
    sphere["value"]["center"]["x"] = json!(10.0);
    model["features"].as_array_mut().unwrap().extend([
        json!({"id": "f3d:model:feature#3", "ordinal": 1, "name": "Tool ball", "suppressed": false, "outputs": ["synthetic:tool:body#0"], "definition": {"kind": "operation", "value": sphere}, "dependencies": [], "source_properties": {}, "source_content": []}),
        json!({"id": "f3d:model:feature#4", "ordinal": 2, "name": "Boolean", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "combine", "value": {"operands": {"target": cube_body(), "tools": {"kind": "bodies", "value": ["synthetic:tool:body#0"]}}, "op": operation, "keep_tools": keep_tools, "destination": "target_component"}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
    ]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

/// The example cube split by a datum plane at `x` across X, then `after` (if any). The split's
/// result bodies are the cube and, when `shifted`, a copy of it moved 10 mm along X: on either
/// side of a plane through the cube's +X face.
fn split_cube(x: f64, shifted: bool, after: Option<Value>) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["geometry"];
    let mut outputs = vec![json!("synthetic:cube:body#0")];
    if shifted {
        for arena in [
            "points", "vertices", "curves", "edges", "coedges", "loops", "surfaces", "faces",
            "shells", "regions", "bodies",
        ] {
            let text = serde_json::to_string(&model[arena]).unwrap();
            let mut rows: Vec<Value> =
                serde_json::from_str(&text.replace("synthetic:cube:", "synthetic:piece:")).unwrap();
            if arena == "points" {
                for row in &mut rows {
                    let moved = row["position"]["x"].as_f64().unwrap() + 10.0;
                    row["position"]["x"] = json!(moved);
                }
            }
            model[arena].as_array_mut().unwrap().extend(rows);
        }
        outputs.insert(0, json!("synthetic:piece:body#0"));
    }
    let frame = json!({"origin": {"x": x, "y": 0.0, "z": 0.0},
        "normal": {"x": 1.0, "y": 0.0, "z": 0.0}, "u_axis": {"x": 0.0, "y": 1.0, "z": 0.0}});
    let model = &mut wire["model"];
    let features = model["features"].as_array_mut().unwrap();
    features.extend([
        json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Cutting plane", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "datum_plane", "value": {"frame": frame}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
        json!({"id": "f3d:model:feature#4", "ordinal": 3, "name": "Split", "suppressed": false, "outputs": outputs, "definition": {"kind": "operation", "value": {"kind": "split_body", "value": {"targets": cube_body(), "tool": {"kind": "plane", "value": {"plane": "f3d:model:feature#3"}}}}}, "dependencies": [], "source_properties": {}, "source_content": []}),
    ]);
    if let Some(definition) = after {
        features.push(
            json!({"id": "f3d:model:feature#5", "ordinal": 4, "name": "After", "suppressed": false, "definition": {"kind": "operation", "value": definition}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
        );
    }
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

/// The largest X any of the example cube's points has.
fn cube_right(model: &Model) -> f64 {
    let wire = serde_json::to_value(&model.ir).unwrap();
    wire["geometry"]["points"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["position"]["x"].as_f64())
        .fold(f64::MIN, f64::max)
}

#[test]
fn a_body_split_by_a_datum_plane_keeps_each_piece_named() {
    let right = cube_right(&split_cube(0.0, false, None));
    // The moved copy lies on the side the normal points to and keeps the cube's name; the
    // cube itself, behind the plane, becomes the new piece. A mirror after the split names
    // the cube's body, so it mirrors the new piece.
    let mirror = json!({"kind": "pattern", "value": {"seeds": [{"kind": "bodies", "value": cube_body()}], "pattern": {"kind": "mirror", "plane_origin": {"x": 40.0, "y": 0.0, "z": 0.0},
            "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}}}});
    let (document, report) = convert(
        &split_cube(right, true, Some(mirror)),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let made = document["features"][1]["body_names"][0].as_str().unwrap();
    let split = &document["features"][2];
    assert_eq!(split["type"], "split", "{}", history_line(&report));
    assert_eq!(split["name"], "Split");
    assert_eq!(split["body"], made);
    assert_eq!(split["plane"]["type"], "custom");
    assert_eq!(split["plane"]["plane"]["origin"], json!([right, 0.0, 0.0]));
    let piece = split["body_names"][0].as_str().unwrap();
    assert_ne!(piece, made);
    let mirror = &document["features"][3];
    assert_eq!(mirror["type"], "mirror", "{}", history_line(&report));
    assert_eq!(mirror["bodies"], json!([piece]));
}

/// New history paths target the current release window even when the replay job
/// checks an older opening floor. Older-target flattening is tested separately.
fn split_writer() -> SolidCraftWriter {
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_40_0))
}

fn split_history_cli() -> Option<PathBuf> {
    let cli = solidcraft()?;
    let version =
        dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()));
    if version < dutchman_solidcraft::SolidCraftVersion::V0_40_0 {
        eprintln!("skipped: new Split history paths need SolidCraft 0.40 or newer");
        return None;
    }
    Some(cli)
}

/// A cube that exists only before its Split: the timeline holds the datum plane and a Split
/// through the cube's middle, whose target is the historical cube, kept by the source as a
/// history body consumed by the Split with a nest of its own. `kept`: whether the source
/// keeps that history body.
fn split_of_a_consumed_cube(kept: bool) -> Model {
    let base = split_cube(0.0, false, None);
    let middle = cube_right(&base) - 5.0;
    let mut wire = serde_json::to_value(&split_cube(middle, false, None).ir).unwrap();
    let mut topology = serde_json::Map::new();
    for arena in [
        "regions", "shells", "faces", "loops", "coedges", "edges", "vertices", "surfaces",
        "curves", "pcurves", "points",
    ] {
        let text = serde_json::to_string(&wire["geometry"][arena]).unwrap();
        let rows: Value =
            serde_json::from_str(&text.replace("synthetic:cube:", "synthetic:nest:")).unwrap();
        topology.insert(arena.to_owned(), rows);
    }
    let text = serde_json::to_string(&wire["geometry"]["bodies"][0]).unwrap();
    let nest_body: Value =
        serde_json::from_str(&text.replace("synthetic:cube:", "synthetic:nest:")).unwrap();
    topology.insert("body".to_owned(), nest_body);
    let features = wire["model"]["features"].as_array_mut().unwrap();
    features.retain(|feature| feature["ordinal"].as_u64() >= Some(2));
    let split = features.iter_mut().find(|f| f["name"] == "Split").unwrap();
    split["definition"]["value"]["value"]["targets"] = json!({"kind": "historical", "value": {
        "state": "synthetic:history-input:state#0", "bodies": ["synthetic:history-input:body#0"], "native": "n"}});
    if kept {
        wire["model"]["history_bodies"] = json!([{
            "id": "synthetic:history-body:nest#h0",
            "consumed_by": "f3d:model:feature#4",
            "state": {"ordinal": 3, "identifier": 7},
            "topology": topology,
        }]);
    }
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

#[test]
fn a_split_of_a_body_that_exists_only_earlier_imports_it_first() {
    let writer = split_writer();
    let split_status = |report: &Report| {
        report
            .outcome()
            .unwrap()
            .features
            .iter()
            .find(|f| f.name.as_deref() == Some("Split"))
            .cloned()
            .unwrap()
    };
    let (_, report) = convert(&split_of_a_consumed_cube(false), &writer);
    assert_ne!(
        split_status(&report).status,
        dutchman_core::FeatureStatus::Mapped,
        "{}",
        history_line(&report)
    );
    let (document, report) = convert(&split_of_a_consumed_cube(true), &writer);
    assert_eq!(
        split_status(&report).status,
        dutchman_core::FeatureStatus::Mapped,
        "{}",
        history_line(&report)
    );
    let features = document["features"].as_array().unwrap();
    let at = |kind: &str| features.iter().position(|f| f["type"] == kind).unwrap();
    assert!(at("import") < at("split"), "{features:?}");
    let import = &features[at("import")];
    assert!(import["step_blob"]
        .as_str()
        .is_some_and(|blob| blob.starts_with("blobs/")));
    assert_eq!(features[at("split")]["body"], import["body_names"][0]);
}

/// The consumed cube made by a Base Feature: one history body, produced by the Base Feature and
/// consumed by the Split, which works on the body the Base Feature's import already named.
fn split_of_a_based_cube() -> Model {
    let model = split_of_a_consumed_cube(true);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["history_bodies"][0]["produced_by"] = json!("f3d:model:feature#1");
    wire["model"]["features"].as_array_mut().unwrap().insert(
        0,
        json!({"id": "f3d:model:feature#1", "ordinal": 0, "name": "Base Feature 1",
            "suppressed": false,
            "definition": {"kind": "operation", "value": {"kind": "base_feature", "value": {"bodies": {"kind": "unresolved"}}}},
            "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
    );
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

/// The same, but the Split's target is native: the source does not resolve which body it
/// splits, and the history body it consumed says so instead.
#[test]
fn a_split_with_a_native_target_splits_the_body_it_consumed() {
    let model = split_of_a_based_cube();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let split = wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["name"] == "Split")
        .unwrap();
    split["definition"]["value"]["value"]["targets"] = json!({"kind": "native", "value": "n"});
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let writer = split_writer();
    let (document, report) = convert(&model, &writer);
    let features = document["features"].as_array().unwrap();
    let imports: Vec<&Value> = features
        .iter()
        .filter(|f| f["type"] == "import" && f["suppressed"] == false)
        .collect();
    assert_eq!(imports.len(), 1, "{features:?}\n{}", history_line(&report));
    let split = features.iter().find(|f| f["type"] == "split").unwrap();
    assert_eq!(split["body"], imports[0]["body_names"][0]);
}

#[test]
fn a_split_of_a_base_features_body_does_not_import_it_twice() {
    let writer = split_writer();
    let (document, report) = convert(&split_of_a_based_cube(), &writer);
    let features = document["features"].as_array().unwrap();
    let imports: Vec<&Value> = features
        .iter()
        .filter(|f| f["type"] == "import" && f["suppressed"] == false)
        .collect();
    assert_eq!(imports.len(), 1, "{features:?}\n{}", history_line(&report));
    let split = features.iter().find(|f| f["type"] == "split").unwrap();
    assert_eq!(split["body"], imports[0]["body_names"][0]);
}

#[test]
fn a_split_reuses_a_base_import_selected_by_the_final_body_identity() {
    let model = split_of_a_based_cube();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][0]["definition"]["value"]["value"]["bodies"] = json!({"kind": "resolved", "value": {"bodies": ["synthetic:cube:body#0"], "native": "synthetic selection"}});
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (document, report) = convert(&model, &split_writer());
    let features = document["features"].as_array().unwrap();
    let imports: Vec<_> = features
        .iter()
        .filter(|f| f["type"] == "import" && f["suppressed"] == false)
        .collect();
    assert_eq!(imports.len(), 1, "{}", history_line(&report));
    let split = features.iter().find(|f| f["type"] == "split").unwrap();
    assert_eq!(split["body"], imports[0]["body_names"][0]);
}

#[test]
fn a_split_with_several_consumed_history_bodies_is_refused_by_name() {
    let model = split_of_a_consumed_cube(true);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let mut other = wire["model"]["history_bodies"][0].clone();
    other["id"] = json!("synthetic:history-body:other#h1");
    wire["model"]["history_bodies"]
        .as_array_mut()
        .unwrap()
        .push(other);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (document, report) = convert(&model, &split_writer());
    assert!(
        history_line(&report).contains("several consumed history bodies"),
        "{}",
        history_line(&report)
    );
    assert!(!document["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "split"));
}

#[test]
fn solidcraft_splits_a_base_features_body_into_two_halves() {
    let Some(cli) = split_history_cli() else {
        return;
    };
    let (design, _) = written(&split_of_a_based_cube());
    let mut volume = volumes(&cli, &design);
    volume.sort_by(f64::total_cmp);
    assert_eq!(volume.len(), 2, "{volume:?}");
    assert!(
        volume.iter().all(|v| (v - 500.0).abs() / 500.0 < 1e-6),
        "{volume:?}"
    );
}

/// The split cube whose tool is one face of the input topology: a square on the plane x = 5
/// (`lift` raises one corner off it, so the face is not flat), or `selection` instead.
fn split_by_face(lift: f64, selection: Option<Value>) -> Model {
    let model = split_cube(5.0, false, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let state = "test:split:feature-input#4";
    let face = "test:split:historical-face#tool";
    let side = |k: usize| format!("test:split:historical-face#side{k}");
    let corners = [
        (0.0, 0.0, 0.0),
        (10.0, 0.0, 0.0),
        (10.0, 10.0, lift),
        (0.0, 10.0, 0.0),
    ];
    let point = |c: (f64, f64, f64)| json!({"x": 5.0 + c.2, "y": c.0, "z": c.1});
    let geometry: Vec<Value> = (0..4)
        .map(|k| {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let chord = [b.2 - a.2, b.0 - a.0, b.1 - a.1];
            let length = (chord[0].powi(2) + chord[1].powi(2) + chord[2].powi(2)).sqrt();
            let tangent = chord.map(|c| c / length);
            json!({
                "edge": format!("test:split:historical-edge#{k}"),
                "interior": point(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0, (a.2 + b.2) / 2.0)),
                "start": point(a), "end": point(b),
                "tangent": {"x": tangent[0], "y": tangent[1], "z": tangent[2]},
                "faces": [face, side(k)],
            })
        })
        .collect();
    let edges: Vec<String> = (0..4)
        .map(|k| format!("test:split:historical-edge#{k}"))
        .collect();
    let mut faces: Vec<String> = (0..4).map(side).collect();
    faces.push(face.to_owned());
    wire["model"]["feature_input_topologies"] = json!([{
        "id": state, "input_of": "f3d:model:feature#4", "edges": edges, "faces": faces,
        "edge_geometry": geometry, "bodies": [], "vertices": [],
    }]);
    let selection = selection.unwrap_or_else(|| {
        json!({"kind": "historical", "value": {
            "state": state, "faces": [face], "native": "synthetic selection"}})
    });
    let features = wire["model"]["features"].as_array_mut().unwrap();
    let split = features.iter_mut().find(|f| f["name"] == "Split").unwrap();
    split["definition"]["value"]["value"]["tool"] = json!({"kind": "faces", "value": selection});
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn a_body_split_by_a_flat_face_splits_along_its_plane() {
    let writer = split_writer();
    let (document, report) = convert(&split_by_face(0.0, None), &writer);
    let features = document["features"].as_array().unwrap();
    let split = features
        .iter()
        .find(|f| f["type"] == "split")
        .unwrap_or_else(|| panic!("{}", history_line(&report)));
    assert_eq!(split["plane"]["plane"]["origin"][0], json!(5.0));
}

/// The split cube whose tool is two separate faces of the input topology, sharing no edge: a
/// rectangle on the plane x = 5 and another on the plane x = `second`.
fn split_by_two_faces(second: f64) -> Model {
    let model = split_cube(5.0, false, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let state = "test:split:feature-input#4";
    let mut geometry: Vec<Value> = Vec::new();
    let mut edges: Vec<String> = Vec::new();
    let mut faces: Vec<String> = Vec::new();
    let mut tools: Vec<String> = Vec::new();
    for (n, (x, y0, y1)) in [(5.0, 0.0, 4.0), (second, 6.0, 10.0)]
        .into_iter()
        .enumerate()
    {
        let face = format!("test:split:historical-face#tool{n}");
        let corners: [(f64, f64); 4] = [(y0, 0.0), (y1, 0.0), (y1, 10.0), (y0, 10.0)];
        let point = |c: (f64, f64)| json!({"x": x, "y": c.0, "z": c.1});
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let length = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
            let edge = format!("test:split:historical-edge#{n}{k}");
            let side = format!("test:split:historical-face#side{n}{k}");
            geometry.push(json!({
                "edge": edge,
                "interior": point(((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0)),
                "start": point(a), "end": point(b),
                "tangent": {"x": 0.0, "y": (b.0 - a.0) / length, "z": (b.1 - a.1) / length},
                "faces": [face, side],
            }));
            edges.push(edge);
            faces.push(side);
        }
        faces.push(face.clone());
        tools.push(face);
    }
    wire["model"]["feature_input_topologies"] = json!([{
        "id": state, "input_of": "f3d:model:feature#4", "edges": edges, "faces": faces,
        "edge_geometry": geometry, "bodies": [], "vertices": [],
    }]);
    let features = wire["model"]["features"].as_array_mut().unwrap();
    let split = features.iter_mut().find(|f| f["name"] == "Split").unwrap();
    split["definition"]["value"]["value"]["tool"] = json!({"kind": "faces", "value": {
        "kind": "historical", "value": {
            "state": state, "faces": tools, "native": "synthetic selection"}}});
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn a_body_split_by_two_separate_coplanar_faces_splits_along_their_plane() {
    let writer = split_writer();
    let (document, report) = convert(&split_by_two_faces(5.0), &writer);
    let features = document["features"].as_array().unwrap();
    let split = features
        .iter()
        .find(|f| f["type"] == "split")
        .unwrap_or_else(|| panic!("{}", history_line(&report)));
    assert_eq!(split["plane"]["plane"]["origin"][0], json!(5.0));
    assert!(
        !history_line(&report).contains("do not lie on one plane"),
        "{}",
        history_line(&report)
    );
}

/// Two rectangles on x = 5 that meet at y = 4 and share their boundary edge.
fn split_by_adjacent_faces() -> Model {
    let model = split_by_two_faces(5.0);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let state = &mut wire["model"]["feature_input_topologies"][0];
    let geometry = state["edge_geometry"].as_array_mut().unwrap();
    for row in geometry.iter_mut().skip(4) {
        for field in ["start", "interior", "end"] {
            let point = &mut row[field];
            let y = point["y"].as_f64().unwrap();
            point["y"] = json!(4.0 + (y - 6.0) * 1.5);
        }
    }
    geometry[1]["faces"] = json!([
        "test:split:historical-face#tool0",
        "test:split:historical-face#tool1"
    ]);
    geometry.retain(|row| row["edge"] != "test:split:historical-edge#13");
    state["edges"]
        .as_array_mut()
        .unwrap()
        .retain(|id| id != "test:split:historical-edge#13");
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn a_body_split_by_adjacent_coplanar_faces_splits_along_their_plane() {
    let (document, report) = convert(&split_by_adjacent_faces(), &split_writer());
    let split = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "split")
        .unwrap_or_else(|| panic!("{}", history_line(&report)));
    assert_eq!(split["plane"]["plane"]["origin"][0], json!(5.0));
}

#[test]
fn solidcraft_splits_a_body_by_coplanar_faces_into_two_halves() {
    let Some(cli) = split_history_cli() else {
        return;
    };
    for model in [split_by_two_faces(5.0), split_by_adjacent_faces()] {
        let (design, _) = written(&model);
        let volume = volumes(&cli, &design);
        assert_eq!(volume.len(), 2, "{volume:?}");
        assert!(
            volume.iter().all(|v| (v - 500.0).abs() / 500.0 < 1e-6),
            "{volume:?}"
        );
    }
}

#[test]
fn new_split_paths_flatten_with_a_named_reason_below_the_release_window() {
    let writer = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.18.2".parse().unwrap()));
    for (model, reason) in [
        (
            split_of_a_consumed_cube(true),
            "history bodies need SolidCraft 0.40 or newer",
        ),
        (
            split_by_face(0.0, None),
            "splitting by input faces needs SolidCraft 0.40 or newer",
        ),
    ] {
        let (document, report) = convert(&model, &writer);
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
        assert!(document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature["type"] == "import" && feature["suppressed"] == false));
    }
}

#[test]
fn a_split_by_separate_faces_on_different_planes_is_refused_for_that_reason() {
    let writer = split_writer();
    let line = history_line(&convert(&split_by_two_faces(6.0), &writer).1);
    assert!(line.contains("do not lie on one plane"), "{line}");
}

#[test]
fn a_split_by_a_face_that_is_not_flat_or_not_one_is_refused_for_that_reason() {
    let writer = split_writer();
    let reason = |model: &Model| history_line(&convert(model, &writer).1);
    assert!(
        reason(&split_by_face(5.0, None)).contains("a face that is not flat is not supported yet"),
        "{}",
        reason(&split_by_face(5.0, None))
    );
    let two = json!({"kind": "historical", "value": {
        "state": "test:split:feature-input#4",
        "faces": ["test:split:historical-face#tool", "test:split:historical-face#side0"],
        "native": "synthetic selection"}});
    // The second face has only one boundary edge, so it cannot establish a plane.
    assert!(
        reason(&split_by_face(0.0, Some(two.clone()))).contains("does not span a plane"),
        "{}",
        reason(&split_by_face(0.0, Some(two)))
    );
    let native = json!({"kind": "native", "value": "synthetic selection"});
    assert!(reason(&split_by_face(0.0, Some(native))).contains("native"));
}

#[test]
fn solidcraft_splits_a_body_by_a_flat_face_into_two_halves() {
    let Some(cli) = split_history_cli() else {
        return;
    };
    let (design, _) = written(&split_by_face(0.0, None));
    let mut volume = volumes(&cli, &design);
    volume.sort_by(f64::total_cmp);
    assert_eq!(volume.len(), 2, "{volume:?}");
    assert!(
        volume.iter().all(|v| (v - 500.0).abs() / 500.0 < 1e-6),
        "{volume:?}"
    );
}

#[test]
fn solidcraft_splits_the_imported_body_into_two_halves() {
    let Some(cli) = split_history_cli() else {
        return;
    };
    let (design, _) = written(&split_of_a_consumed_cube(true));
    let mut volume = volumes(&cli, &design);
    volume.sort_by(f64::total_cmp);
    assert_eq!(volume.len(), 2, "{volume:?}");
    assert!(
        volume.iter().all(|v| (v - 500.0).abs() / 500.0 < 1e-6),
        "{volume:?}"
    );
}

#[test]
fn split_pieces_that_cannot_be_told_apart_are_not_selected_later() {
    // The plane cuts through the cube, so its vertices lie on both sides: neither piece keeps
    // the cube's identity, and a later feature selecting it stops the history.
    let model = split_cube(0.0, false, None);
    let middle = cube_right(&model) - 5.0;
    let mirror = json!({"kind": "pattern", "value": {"seeds": [{"kind": "bodies", "value": cube_body()}], "pattern": {"kind": "mirror", "plane_origin": {"x": 40.0, "y": 0.0, "z": 0.0},
            "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}}}});
    let (document, report) = convert(
        &split_cube(middle, false, Some(mirror)),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(document["features"][2]["type"], "split");
    assert!(
        history_line(&report).contains("it selects a body that an earlier feature has not named"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn splitting_by_faces_of_the_final_model_is_refused_by_name() {
    let mut model = split_cube(0.0, false, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let split = wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["name"] == "Split")
        .unwrap();
    split["definition"]["value"]["value"]["tool"] =
        json!({"kind": "faces", "value": cube_faces(&["top"])});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report)
            .contains("a splitting face selection in this form is not supported yet"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn solidcraft_splits_the_body_in_two() {
    let Some(cli) = solidcraft() else { return };
    let model = split_cube(0.0, false, None);
    let middle = cube_right(&model) - 4.0;
    let got = volumes(&cli, &written(&split_cube(middle, false, None)).0);
    assert_eq!(got.len(), 2, "{got:?}");
    let mut got = got;
    got.sort_by(f64::total_cmp);
    assert!((got[0] - 400.0).abs() < 1.0, "{got:?}");
    assert!((got[1] - 600.0).abs() < 1.0, "{got:?}");
}

#[test]
fn booleans_map_target_tools_operation_and_retention() {
    for operation in ["join", "cut", "intersect"] {
        for keep_tools in [false, true] {
            let (document, report) = convert(
                &combined_spheres(operation, keep_tools),
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            let combine = &document["features"][2];
            assert_eq!(combine["type"], "combine", "{}", history_line(&report));
            assert_eq!(combine["target"], document["features"][0]["body_names"][0]);
            assert_eq!(combine["tools"], json!(["Tool"]));
            assert_eq!(combine["operation"], operation);
            assert_eq!(combine["keep_tools"], keep_tools);
        }
    }
}

#[test]
fn a_boolean_whose_operands_keep_a_native_member_per_body_maps_those_bodies() {
    let mut model = combined_spheres("cut", false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let boolean = wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["name"] == "Boolean")
        .unwrap();
    boolean["definition"]["value"]["value"]["operands"] = json!({
        "target": {"kind": "resolved_set", "value": {"members": [
            {"body": "synthetic:cube:body#0", "native": "body:1"}]}},
        "tools": {"kind": "resolved_set", "value": {"members": [
            {"body": "synthetic:tool:body#0", "native": "body:2"}]}}});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let combine = &document["features"][2];
    assert_eq!(combine["type"], "combine", "{}", history_line(&report));
    assert_eq!(combine["target"], document["features"][0]["body_names"][0]);
    assert_eq!(combine["tools"], json!(["Tool"]));
    assert_eq!(combine["operation"], "cut");
}

#[test]
fn a_boolean_referencing_a_removed_tool_reports_a_history_blocker() {
    let mut model = combined_spheres("cut", false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // Match core's consumed-body filtering: preserve history, remove the tool's topology.
    for (arena, values) in wire["geometry"].as_object_mut().unwrap() {
        if arena == "features" {
            continue;
        }
        if let Some(rows) = values.as_array_mut() {
            rows.retain(|row| {
                !row["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("synthetic:tool:"))
            });
        }
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(report.outcome().unwrap().features.len(), 3);
    assert!(
        report.losses().iter().any(|loss| loss
            .detail
            .contains("it selects a body that an earlier feature has not named")),
        "{}",
        history_line(&report)
    );
}

#[test]
fn unsupported_modify_options_explain_what_stops_history() {
    let mut cases = Vec::new();
    // Shell and draft faces exist only on the final body, which the feature has changed.
    cases.push((
        json!({"kind": "shell", "value": {"bodies": cube_body(), "removed_faces": cube_faces(&["top"]), "thickness": 1.0, "outward": false}}),
        "shell requires faces from its own input topology",
    ));
    cases.push((
        json!({"kind": "draft", "value": {"faces": cube_faces(&["right"]), "anchor": {"kind": "neutral_plane", "plane": cube_faces(&["bottom"]),
            "pull": {"direction": {"x": 0.0, "y": 0.0, "z": 1.0}}}, "angle": 0.1, "outward": false}}),
        "draft waits on input face geometry",
    ));
    cases.push((
        json!({"kind": "mirror_shape", "value": {"source": cube_body(), "plane_origin": {"x": 20.0, "y": 0.0, "z": 0.0}, "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}}}),
        "SolidCraft mirrors keep the original body",
    ));
    cases.push((
        json!({"kind": "cosmetic_thread", "value": {"face": cube_faces(&["right"]), "diameter": 8.0, "extent": {"kind": "through"}}}),
        "cosmetic threads require a resolved diameter and pitch",
    ));
    for family in ["thread", "detailed_thread"] {
        cases.push((
            json!({"kind": "unresolved", "value": {"family": family}}),
            "threads whose construction is unresolved are not supported yet",
        ));
    }
    cases.push((
        json!({"kind": "combine", "value": {"operands": {
        "target": {"kind": "native", "value": "Body1"},
        "tools": {"kind": "native", "value": "Body2"}}, "op": "cut", "keep_tools": false, "destination": "target_component"}}),
        "a native body reference is not supported yet",
    ));
    for (definition, reason) in cases {
        let (_, report) = convert(
            &modifying(definition),
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn solidcraft_replays_body_mirror_and_booleans() {
    let Some(cli) = solidcraft() else { return };
    let mirror = volumes(&cli, &written(&modifying(body_mirror_definition())).0);
    assert_eq!(mirror.len(), 2);
    assert!(
        mirror.iter().all(|v| (v - 1000.0).abs() < 1e-6),
        "{mirror:?}"
    );
    // Two radius-5 spheres with centres 5 mm apart; overlap = 625π/12 mm³. SolidCraft measures
    // curved booleans to about one part in ten thousand, so the check is relative, as for spheres.
    let sphere = 500.0 * std::f64::consts::PI / 3.0;
    let overlap = 625.0 * std::f64::consts::PI / 12.0;
    for (operation, expected) in [
        ("join", 2.0 * sphere - overlap),
        ("cut", sphere - overlap),
        ("intersect", overlap),
    ] {
        let volume = volumes(&cli, &written(&combined_spheres(operation, false)).0);
        assert_eq!(volume.len(), 1);
        assert!(
            (volume[0] - expected).abs() / expected < 1e-3,
            "{operation}: {volume:?} vs {expected}"
        );
    }
}

#[test]
fn later_body_mirrors_require_boolean_tools_to_be_retained() {
    for keep_tools in [false, true] {
        let base = combined_spheres("join", keep_tools);
        let mut wire = serde_json::to_value(&base.ir).unwrap();
        let mut mirror = body_mirror_definition();
        mirror["value"]["seeds"][0]["value"] = json!({"kind": "resolved", "value": {
            "bodies": ["synthetic:tool:body#0"], "native": "Tool"}});
        wire["model"]["features"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "f3d:model:feature#5", "ordinal": 3, "name": "Mirror tool", "suppressed": false, "definition": {"kind": "operation", "value": mirror}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..base
        };
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        if keep_tools {
            assert_eq!(
                document["features"][3]["type"],
                "mirror",
                "{}",
                history_line(&report)
            );
            assert_eq!(document["features"][3]["bodies"], json!(["Tool"]));
        } else {
            assert!(history_line(&report).contains("earlier feature has not named"));
        }
    }
}

/// The stored B-rep deliberately stays an unblended cube. Only the feature's own
/// historical input samples locate the two symmetric edges; no final edge id is used.
fn input_blend(kind: &str, dimension: Value, opposite: bool) -> Model {
    blend_on(
        cube_with_history(10.0, first_profile(), None),
        kind,
        dimension,
        opposite,
        0.0,
    )
}

/// The blend of [`input_blend`] on the cube extruded symmetrically about its sketch (Z from
/// -5 to 5 mm): a base no rebuild without SolidCraft knew the edges of.
fn symmetric_blend(kind: &str, dimension: Value, opposite: bool) -> Model {
    let extent = json!({"kind": "symmetric",
        "side": {"termination": {"kind": "blind", "length": 10.0}}});
    blend_on(
        cube_with_history(10.0, first_profile(), Some(extent)),
        kind,
        dimension,
        opposite,
        -5.0,
    )
}

/// `base` with a blend of one of its two symmetric vertical edges, which run from `z0` to
/// `z0 + 10` mm: at (0, 0), or at (10, 10) when `opposite`.
fn blend_on(base: Model, kind: &str, dimension: Value, opposite: bool, z0: f64) -> Model {
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let mut group = json!({"edges": {"kind": "historical", "value": {
        "state": "f3d:model:feature-input-topology#1",
        "edges": [if opposite { "opposite" } else { "near" }],
        "native": "synthetic input edge",
    }}});
    group[if kind == "fillet" { "radius" } else { "spec" }] = dimension;
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Edge treatment", "suppressed": false, "dependencies": ["f3d:model:feature#2"], "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": {"kind": kind, "value": {"groups": [group]}}}, "source_properties": {}, "source_content": []}));
    if kind == "chamfer" {
        wire["model"]["features"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap()["definition"]["value"]["value"]["flip_direction"] = json!(false);
    }
    let sample = |edge: &str, xy: f64| {
        json!({
            "edge": edge, "interior": {"x": xy, "y": xy, "z": z0 + 5.0},
            "tangent": {"x": 0.0, "y": 0.0, "z": 1.0},
            "start": {"x": xy, "y": xy, "z": z0},
            "end": {"x": xy, "y": xy, "z": z0 + 10.0},
            "faces": if edge == "near" { json!(["front", "left"]) } else { json!(["back", "right"]) },
        })
    };
    wire["model"]["feature_input_topologies"] = json!([{"id": "f3d:model:feature-input-topology#1", "input_of": "f3d:model:feature#3", "edges": ["near", "opposite"], "faces": ["front", "left", "back", "right"], "edge_geometry": [sample("near", 0.0), sample("opposite", 10.0)], "bodies": [], "vertices": []}]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

/// Entirely synthetic closed input circle. The saved example body is deliberately
/// unrelated: selection must use the historical input, never final-body edge numbers.
fn closed_circle_blend(hole: bool, seam: f64) -> Model {
    closed_circle_blend_from(hole, seam, false)
}

fn closed_circle_blend_from(hole: bool, seam: f64, sketch_based: bool) -> Model {
    let base = if hole {
        holed_cube(4.0, |_| {})
    } else {
        primitive_history(json!({"kind": "primitive", "value": {
            "solid": {"kind": "cylinder", "radius": 5.0, "height": 10.0,
                "angle": std::f64::consts::TAU},
            "placement": [[1,0,0,0],[0,1,0,0],[0,0,1,0]], "op": "new_body"
        }}))
    };
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let m = &mut wire["model"];
    if sketch_based {
        if hole {
            m["features"][3]["definition"]["value"] = json!({"kind": "extrude", "value": {
                "profile": {"kind": "planar", "value": {"kind": "sketch", "value": HOLES}},
                "direction": {"kind": "explicit", "vector": {"x": 0.0, "y": 0.0, "z": -1.0}},
                "start": {"kind": "profile_plane"},
                "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 10.0}}},
                "op": "cut"
            }});
        } else {
            let cylinder = cube_with_history(10.0, whole_sketch(), None);
            *m = serde_json::to_value(&cylinder.ir).unwrap()["model"].clone();
            m["sketch_entities"] = json!([entity(
                1,
                json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 5.0})
            )]);
            m["sketch_constraints"] = json!([]);
            m["sketches"][0]["profiles"] =
                json!([[{"entity": "f3d:model:sketch-entity#1", "reversed": false}]]);
        }
    }
    let features = m["features"].as_array_mut().unwrap();
    let previous = features.last().unwrap()["id"].clone();
    let ordinal = features.len();
    let kind = if hole { "chamfer" } else { "fillet" };
    let dimension = if hole {
        json!({"spec": {"kind": "distance", "distance": 0.5}})
    } else {
        json!({"radius": {"kind": "constant", "radius": 1.0}})
    };
    let mut group = dimension;
    group["edges"] = json!({"kind": "historical", "value": {
        "state": "synthetic:closed:feature-input-topology#0", "edges": ["synthetic:closed:edge#0"], "native": "synthetic rim"}});
    let mut definition = json!({"groups": [group]});
    if hole {
        definition["flip_direction"] = json!(false);
    }
    features.push(
        json!({"id": "synthetic:closed:feature#0", "ordinal": ordinal,
        "name": "Rim treatment", "suppressed": false, "dependencies": [previous],
        "outputs": ["synthetic:cube:body#0"], "source_properties": {}, "source_content": [],
        "definition": {"kind": "operation", "value": {"kind": kind, "value": definition}}}),
    );
    let (c, r) = if hole { (5.0, 2.0) } else { (0.0, 5.0) };
    let point = |a: f64| json!({"x": c + r * a.cos(), "y": c + r * a.sin(), "z": 10.0});
    m["feature_input_topologies"] = json!([{
        "id": "synthetic:closed:feature-input-topology#0", "input_of": "synthetic:closed:feature#0",
        "edges": ["synthetic:closed:edge#0"], "faces": [], "bodies": [], "vertices": [],
        "edge_geometry": [{"edge": "synthetic:closed:edge#0", "start": point(seam), "end": point(seam),
            "interior": point(seam + std::f64::consts::PI),
            "tangent": {"x": seam.sin(), "y": -seam.cos(), "z": 0.0}, "faces": []}]
    }]);
    // Exact historical carrier in a hidden synthetic input body, separate from final geometry.
    let source = "synthetic:closed:body#0";
    let old = json!("synthetic:cube:body#0");
    for feature in wire["model"]["features"].as_array_mut().unwrap() {
        for output in feature["outputs"].as_array_mut().unwrap() {
            if *output == old {
                *output = json!(source);
            }
        }
    }
    let g = &mut wire["geometry"];
    g["bodies"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": source, "kind": "wire", "visible": false}));
    g["regions"].as_array_mut().unwrap().push(json!({"id": "synthetic:closed:region#0", "body": source, "shells": ["synthetic:closed:shell#0"]}));
    g["shells"].as_array_mut().unwrap().push(json!({"id": "synthetic:closed:shell#0", "region": "synthetic:closed:region#0", "members": [{"kind": "wire_edge", "id": "synthetic:closed:edge#0"}]}));
    g["edges"].as_array_mut().unwrap().push(json!({"id": "synthetic:closed:edge#0", "carrier": {"curve": "synthetic:closed:curve#0", "param_range": [seam, seam + std::f64::consts::TAU]}, "ends": {"start": "synthetic:closed:vertex#0", "end": "synthetic:closed:vertex#0"}}));
    g["vertices"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "synthetic:closed:vertex#0", "point": "synthetic:closed:point#0"}));
    g["points"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "synthetic:closed:point#0", "position": point(seam)}));
    g["curves"].as_array_mut().unwrap().push(json!({"id": "synthetic:closed:curve#0", "geometry": {"kind": "solved", "value": {
        "kind": "circle", "center": {"x": c, "y": c, "z": 10.0}, "axis": {"x": 0.0, "y": 0.0, "z": 1.0}, "ref_direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "radius": r
    }}}));
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

#[test]
fn closed_circle_blends_keep_older_targets_and_non_circular_curves_refused() {
    let base = closed_circle_blend(false, 0.0);
    let older = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.43.0".parse().unwrap()));
    let (_, report) = convert(&base, &older);
    assert!(
        history_line(&report).contains("closed input circles need SolidCraft 0.44 patch 1"),
        "{report:?}"
    );
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    wire["model"]["feature_input_topologies"][0]["edge_geometry"][0]["tangent"] =
        json!({"x": 1.0, "y": 0.0, "z": 0.0});
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let writer = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.44.1".parse().unwrap()));
    let (_, report) = convert(&model, &writer);
    assert!(
        history_line(&report).contains("closed input curves other than proven circles"),
        "{report:?}"
    );
}

#[test]
fn closed_blends_distinguish_missing_historical_carriers_from_non_circular_curves() {
    let base = closed_circle_blend(true, 0.71);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    // A historical identity is independent of the geometry topology's identities.
    // Keep the same samples, but do not give this selection an exact stored carrier.
    let edge = json!("synthetic:historical:edge#0");
    let model = &mut wire["model"];
    let state = &mut model["feature_input_topologies"][0];
    state["edges"] = json!([edge]);
    state["edge_geometry"][0]["edge"] = edge.clone();
    model["features"]
        .as_array_mut()
        .unwrap()
        .last_mut()
        .unwrap()["definition"]["value"]["value"]["groups"][0]["edges"]["value"]["edges"] =
        json!([edge]);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let writer = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.45.0".parse().unwrap()));
    let (_, report) = convert(&model, &writer);
    assert!(
        history_line(&report).contains("no resolved historical curve carrier"),
        "{report:?}"
    );
    assert!(
        !history_line(&report).contains("closed input curves other than proven circles"),
        "{report:?}"
    );
}

#[test]
fn closed_circle_blends_refuse_ellipses_whose_samples_alias_a_circle() {
    let base = closed_circle_blend(false, 0.0);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let curve = wire["geometry"]["curves"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|row| row["id"] == "synthetic:closed:curve#0")
        .unwrap();
    curve["geometry"]["value"] = json!({"kind": "ellipse", "center": {"x": 0.0, "y": 0.0, "z": 10.0},
        "axis": {"x": 0.0, "y": 0.0, "z": 1.0}, "major_direction": {"x": 1.0, "y": 0.0, "z": 0.0},
        "major_radius": 5.0, "minor_radius": 3.0});
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let writer = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.44.1".parse().unwrap()));
    let (_, report) = convert(&model, &writer);
    assert!(
        history_line(&report).contains("closed input curves other than proven circles"),
        "{report:?}"
    );
}

/// The new replay cases target the tested release; the older opens-in CI remains intact.
fn closed_blend_cli() -> Option<PathBuf> {
    let cli = solidcraft()?;
    let version = std::process::Command::new(&cli)
        .arg("--version")
        .output()
        .unwrap();
    let text = String::from_utf8(version.stdout).unwrap();
    let version: dutchman_solidcraft::SolidCraftVersion =
        text.split_whitespace().last().unwrap().parse().unwrap();
    if version < dutchman_solidcraft::SolidCraftVersion::V0_44_1 {
        eprintln!("skipped: closed circle replay needs SolidCraft 0.44.1 or newer");
        return None;
    }
    Some(cli)
}

#[test]
fn solidcraft_replays_closed_circle_fillets_and_hole_chamfers_across_seams() {
    let Some(cli) = closed_blend_cli() else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let writer = replaying(&cli).target_version(Some("0.44.1".parse().unwrap()));
    for (hole, sketch_based) in [(false, false), (true, false), (false, true), (true, true)] {
        for seam in [0.0, 0.71, 2.5] {
            let (bytes, report) =
                written_with(&closed_circle_blend_from(hole, seam, sketch_based), &writer);
            let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
            let mut text = String::new();
            archive
                .by_name("document.json")
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            let document: Value = serde_json::from_str(&text).unwrap();
            let kind = if hole { "chamfer" } else { "fillet" };
            let blend = document["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["type"] == kind)
                .unwrap_or_else(|| panic!("{report:?}"));
            assert!(!blend["edges"].as_array().unwrap().is_empty(), "{blend}");
            assert!(
                !history_line(&report).starts_with("flattened"),
                "{report:?}"
            );
            let actual_path = dir.path().join("actual.sc");
            std::fs::write(&actual_path, bytes).unwrap();
            let actual = blend_measurement(&cli, &actual_path, &dir.path().join("actual.json"));
            // Replay the reference at a point on the circle, independently of the source seam.
            let script = if hole {
                json!([
                    {"command": "solid.box", "params": {"length": "10 mm", "width": "10 mm", "height": "10 mm", "body_name": "Reference"}},
                    {"command": "solid.hole", "params": {"position": [5,5,10], "direction": [0,0,-1], "diameter": "4 mm", "targets": ["Reference"]}},
                    {"command": "solid.chamfer", "params": {"edges": [[7,5,10]], "distance": "0.5 mm", "body": "Reference"}}
                ])
            } else {
                json!([
                    {"command": "solid.cylinder", "params": {"radius": "5 mm", "height": "10 mm", "body_name": "Reference"}},
                    {"command": "solid.fillet", "params": {"edges": [[5,0,10]], "radius": "1 mm", "body": "Reference"}}
                ])
            };
            let expected_path = dir.path().join("expected.json");
            std::fs::write(&expected_path, script.to_string()).unwrap();
            let expected = blend_measurement(
                &cli,
                &expected_path,
                &dir.path().join("expected-measured.json"),
            );
            for key in ["volume_mm3", "area_mm2"] {
                let a = actual[key].as_f64().unwrap();
                let b = expected[key].as_f64().unwrap();
                assert!(
                    (a - b).abs() <= 1e-5 * b.abs().max(1.0),
                    "{key}: {actual} vs {expected}"
                );
            }
            for axis in 0..3 {
                assert!(
                    (actual["center_of_mass"][axis].as_f64().unwrap()
                        - expected["center_of_mass"][axis].as_f64().unwrap())
                    .abs()
                        < 1e-5
                );
                for end in ["min", "max"] {
                    assert!(
                        (actual["bbox"][end][axis].as_f64().unwrap()
                            - expected["bbox"][end][axis].as_f64().unwrap())
                        .abs()
                            < 1e-5
                    );
                }
            }
        }
    }
}

/// A closed boss edge and a hole rim use the same carrier proof, with opposite
/// material sides. Check the actual removed ring independently of edge seam placement.
#[test]
fn solidcraft_chamfers_closed_boss_and_hole_edges() {
    let Some(cli) = closed_blend_cli() else {
        return;
    };
    let writer = replaying(&cli).target_version(Some("0.45.0".parse().unwrap()));
    let setback: f64 = 0.5;
    for hole in [false, true] {
        for seam in [0.0, 0.71, 2.5] {
            let base = closed_circle_blend_from(hole, seam, true);
            let mut wire = serde_json::to_value(&base.ir).unwrap();
            let operation = &mut wire["model"]["features"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap()["definition"]["value"];
            operation["kind"] = json!("chamfer");
            let group = &mut operation["value"]["groups"][0];
            group.as_object_mut().unwrap().remove("radius");
            group["spec"] = json!({"kind": "distance", "distance": setback});
            operation["value"]["flip_direction"] = json!(false);
            let model = Model {
                ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
                ..base
            };
            let (bytes, report) = written_with(&model, &writer);
            let expected_features = if hole { 5 } else { 3 };
            assert!(
                history_line(&report).starts_with(&format!(
                    "{expected_features} of {expected_features} features rebuilt"
                )),
                "{}",
                history_line(&report)
            );
            let measured = evaluated(&cli, &bytes);
            let [body] = measured["bodies"].as_array().unwrap().as_slice() else {
                panic!("expected one chamfered body: {measured}");
            };
            let pi = std::f64::consts::PI;
            let radius = if hole { 2.0 } else { 5.0 };
            let original = if hole {
                1000.0 - pi * radius * radius * 10.0
            } else {
                pi * radius * radius * 10.0
            };
            // Pappus: triangle area s²/2, centroid radius r ± s/3.
            let sign = if hole { 1.0 } else { -1.0 };
            let removed = pi * setback * setback * (radius + sign * setback / 3.0);
            let expected = original - removed;
            assert!(
                (body["volume_mm3"].as_f64().unwrap() - expected).abs() < 1e-6 * expected,
                "hole={hole}, seam={seam}: {body}"
            );
            let depth =
                setback * (4.0 * radius + sign * setback) / (4.0 * (3.0 * radius + sign * setback));
            let z = (5.0 * original - (10.0 - depth) * removed) / expected;
            for (axis, wanted) in [
                (0, if hole { 5.0 } else { 0.0 }),
                (1, if hole { 5.0 } else { 0.0 }),
                (2, z),
            ] {
                assert!(
                    (body["center_of_mass"][axis].as_f64().unwrap() - wanted).abs() < 1e-6,
                    "hole={hole}, seam={seam}: {body}"
                );
            }
        }
    }
}

#[test]
fn solidcraft_replays_closed_circle_work_with_a_connected_straight_edge_chain() {
    let Some(cli) = closed_blend_cli() else {
        return;
    };
    let base = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    model["feature_input_topologies"][0]["edge_geometry"] = json!([
        {"edge": "near", "start": {"x": 0, "y": 0, "z": 10},
            "end": {"x": 10, "y": 0, "z": 10}, "interior": {"x": 5, "y": 0, "z": 10},
            "tangent": {"x": 1, "y": 0, "z": 0}, "faces": ["front", "left"]},
        {"edge": "opposite", "start": {"x": 10, "y": 0, "z": 10},
            "end": {"x": 10, "y": 10, "z": 10}, "interior": {"x": 10, "y": 5, "z": 10},
            "tangent": {"x": 0, "y": 1, "z": 0}, "faces": ["back", "right"]}
    ]);
    model["features"][2]["definition"]["value"]["value"]["groups"][0]["edges"]["value"]["edges"] =
        json!(["near", "opposite"]);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let (document, report) = convert(&model, &replaying(&cli));
    assert_eq!(
        document["features"][2]["edges"],
        json!([[5.0, 0.0, 10.0], [10.0, 5.0, 10.0]]),
        "{report:?}"
    );
    let (bytes, _) = written_with(&model, &replaying(&cli));
    let dir = tempfile::tempdir().unwrap();
    let actual_path = dir.path().join("actual.sc");
    std::fs::write(&actual_path, bytes).unwrap();
    let actual = blend_measurement(&cli, &actual_path, &dir.path().join("actual.json"));
    let script = json!([
        {"command": "solid.box", "params": {"length": "10 mm", "width": "10 mm", "height": "10 mm", "body_name": "Reference"}},
        {"command": "solid.fillet", "params": {"edges": [[5,0,10],[10,5,10]], "radius": "1 mm", "body": "Reference"}}
    ]);
    let expected_path = dir.path().join("expected.json");
    std::fs::write(&expected_path, script.to_string()).unwrap();
    let expected = blend_measurement(
        &cli,
        &expected_path,
        &dir.path().join("expected-measured.json"),
    );
    assert!(
        (actual["volume_mm3"].as_f64().unwrap() - expected["volume_mm3"].as_f64().unwrap()).abs()
            < 1e-5
    );
    assert_eq!(actual["faces"], expected["faces"]);
}

/// Synthetic prisms on opposite sides of one sketch, kept as separate bodies. Their
/// stored example B-rep is deliberately unchanged, as in `input_blend`: the input
/// topology and SolidCraft's replay, never that final B-rep, select the blend edges.
fn multibody_blend(kind: &str, dimension: Value) -> Model {
    let base = input_blend(kind, dimension, false);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let m = &mut wire["geometry"];
    let mut body = m["bodies"][0].clone();
    body["id"] = json!("synthetic:lower:body#0");
    body["name"] = json!("Lower block");
    let mut region = m["regions"][0].clone();
    region["id"] = json!("synthetic:lower:region#0");
    region["body"] = json!("synthetic:lower:body#0");
    m["regions"].as_array_mut().unwrap().push(region);
    m["bodies"].as_array_mut().unwrap().push(body);
    let m = &mut wire["model"];
    let mut extrude = m["features"][1].clone();
    extrude["id"] = json!("synthetic:lower:feature#0");
    extrude["ordinal"] = json!(2);
    extrude["name"] = json!("Lower extrusion");
    extrude["outputs"] = json!(["synthetic:lower:body#0"]);
    extrude["definition"]["value"]["value"]["direction"] =
        json!({"kind": "explicit", "vector": {"x": 0.0, "y": 0.0, "z": -1.0}});
    m["features"].as_array_mut().unwrap().insert(2, extrude);
    m["features"][3]["ordinal"] = json!(3);
    m["features"][3]["outputs"] = json!(["synthetic:cube:body#0", "synthetic:lower:body#0"]);
    m["features"][3]["definition"]["value"]["value"]["groups"][0]["edges"]["value"]["edges"] =
        json!(["lower", "near"]);
    let state = &mut m["feature_input_topologies"][0];
    state["edges"].as_array_mut().unwrap().push(json!("lower"));
    let mut lower = state["edge_geometry"][1].clone();
    lower["edge"] = json!("lower");
    lower["start"]["z"] = json!(-10.0);
    lower["end"]["z"] = json!(0.0);
    lower["interior"]["z"] = json!(-5.0);
    state["edge_geometry"].as_array_mut().unwrap().push(lower);
    // A later blend proves that replay refreshes after the split. Its untouched edge
    // on the upper prism still has the original input geometry.
    let mut later = m["features"][3].clone();
    later["id"] = json!("synthetic:later:feature#0");
    later["ordinal"] = json!(4);
    later["name"] = json!("Later treatment");
    later["outputs"] = json!(["synthetic:cube:body#0"]);
    later["definition"]["value"]["value"]["groups"][0]["edges"]["value"]["state"] =
        json!("synthetic:later:feature-input-topology#0");
    later["definition"]["value"]["value"]["groups"][0]["edges"]["value"]["edges"] =
        json!(["opposite"]);
    let mut later_state = m["feature_input_topologies"][0].clone();
    later_state["id"] = json!("synthetic:later:feature-input-topology#0");
    later_state["input_of"] = json!("synthetic:later:feature#0");
    m["feature_input_topologies"]
        .as_array_mut()
        .unwrap()
        .push(later_state);
    m["features"].as_array_mut().unwrap().push(later);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

#[test]
fn solidcraft_splits_multibody_blends_and_continues_history() {
    let Some(cli) = solidcraft() else { return };
    for target in ["0.18.2", "0.40.1"] {
        for (kind, dimension) in blend_cases() {
            let model = multibody_blend(kind, dimension);
            let (bytes, report) = written_with(
                &model,
                &replaying(&cli).target_version(Some(target.parse().unwrap())),
            );
            let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
            let mut text = String::new();
            archive
                .by_name("document.json")
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            let document: Value = serde_json::from_str(&text).unwrap();
            let features = document["features"].as_array().unwrap();
            let blends: Vec<_> = features.iter().filter(|f| f["type"] == kind).collect();
            assert_eq!(blends.len(), 3, "{report:?}");
            assert_eq!(blends[0]["body"], "Lower block");
            assert_eq!(blends[0]["edges"], json!([[10.0, 10.0, -5.0]]));
            assert_eq!(blends[0]["name"], "Edge treatment");
            assert_eq!(blends[1]["body"], "unit cube");
            assert_eq!(blends[1]["edges"], json!([[0.0, 0.0, 5.0]]));
            assert_eq!(blends[1]["name"], "Edge treatment (2)");
            assert_eq!(blends[2]["name"], "Later treatment");
            assert_eq!(blends[2]["body"], "unit cube");
            let outcomes = &report.outcome().unwrap().features;
            assert_eq!(outcomes.len(), 5);
            assert_eq!(outcomes[3].output.ids.len(), 2);
            assert_output_links(&document, &report);
            assert!(
                outcomes
                    .iter()
                    .all(|f| f.status == dutchman_core::FeatureStatus::Mapped),
                "{report:?}"
            );
            assert_eq!(
                outcomes
                    .iter()
                    .filter(|f| f.name.as_deref() == Some("Edge treatment"))
                    .count(),
                1
            );
            // Run the resulting file through SolidCraft too: every emitted feature must
            // succeed, and both bodies must have lost material from the original prisms.
            let measured = evaluated(&cli, &bytes);
            assert_eq!(measured["bodies"].as_array().unwrap().len(), 2);
            for body in measured["bodies"].as_array().unwrap() {
                assert!(body["volume_mm3"].as_f64().unwrap() < 1000.0, "{measured}");
            }
        }
    }
}

#[test]
fn multibody_blends_keep_unsupported_dimensions_refused() {
    for (kind, dimension, reason) in [
        (
            "fillet",
            json!({"kind": "variable", "points": [
            {"parameter": 0.0, "radius": 1.0}, {"parameter": 1.0, "radius": 2.0}]}),
            "proven chain direction",
        ),
        (
            "fillet",
            json!({"kind": "chordal", "chord_length": 1.0}),
            "other than constant-radius",
        ),
        (
            "fillet",
            json!({"kind": "asymmetric", "offset_one": 1.0, "offset_two": 2.0}),
            "other than constant-radius",
        ),
        (
            "fillet",
            json!({"kind": "unresolved"}),
            "other than constant-radius",
        ),
        (
            "chamfer",
            json!({"kind": "two_distances", "first": 1.0, "second": 2.0}),
            "replay",
        ),
    ] {
        let (_, report) = convert(
            &multibody_blend(kind, dimension),
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).contains(reason), "{report:?}");
    }
}

fn blend_cases() -> Vec<(&'static str, Value)> {
    vec![
        ("fillet", json!({"kind": "constant", "radius": 1.0})),
        ("chamfer", json!({"kind": "distance", "distance": 1.0})),
        (
            "chamfer",
            json!({"kind": "two_distances", "first": 1.0, "second": 1.0}),
        ),
        (
            "chamfer",
            json!({"kind": "distance_angle", "distance": 1.0,
                           "angle": std::f64::consts::FRAC_PI_4}),
        ),
    ]
}

#[cfg(unix)]
/// The edges SolidCraft's `model.edges` reports for a prism: the polygon `ring` (on XY)
/// extruded from `z0` to `z1`.
fn prism_edges(ring: &[[f64; 2]], z0: f64, z1: f64) -> Vec<Value> {
    let edge = |a: [f64; 3], b: [f64; 3]| {
        let length = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2) + (b[2] - a[2]).powi(2)).sqrt();
        let mid = [0, 1, 2].map(|i| (a[i] + b[i]) / 2.0);
        json!({"start": a, "end": b, "mid": mid, "length": length})
    };
    let mut edges = Vec::new();
    for (i, p) in ring.iter().enumerate() {
        let q = ring[(i + 1) % ring.len()];
        edges.push(edge([p[0], p[1], z0], [q[0], q[1], z0]));
        edges.push(edge([q[0], q[1], z1], [p[0], p[1], z1]));
        edges.push(edge([p[0], p[1], z0], [p[0], p[1], z1]));
    }
    edges
}

#[cfg(unix)]
const SQUARE: [[f64; 2]; 4] = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];

/// A stand-in SolidCraft CLI that answers a replay with `bodies` (name, edges), after
/// sleeping `delay` seconds; the folder holding it lives as long as the value.
#[cfg(unix)]
struct FakeCli {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

#[cfg(unix)]
fn fake_cli(bodies: &[(&str, Vec<Value>)], delay: u32) -> FakeCli {
    fake_cli_with_faces(bodies, &[], delay)
}

/// [`fake_cli`], answering a replay that asks for faces with `faces`, one list per body.
#[cfg(unix)]
fn fake_cli_with_faces(bodies: &[(&str, Vec<Value>)], faces: &[Vec<Value>], delay: u32) -> FakeCli {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let mut with_faces = vec![json!({"features": 3, "marker": 2})];
    for ((name, edges), faces) in bodies.iter().zip(faces) {
        with_faces.push(json!({"body": name, "edges": edges}));
        with_faces.push(json!({"faces": faces}));
    }
    std::fs::write(
        dir.path().join("faces.json"),
        Value::from(with_faces).to_string(),
    )
    .unwrap();
    let inspect = json!([{"features": 3, "marker": 2}, {
        "bodies": bodies.iter().map(|(name, _)| json!({"name": name})).collect::<Vec<_>>(),
        "timeline": [],
    }]);
    let mut edges = vec![json!({"features": 3, "marker": 2})];
    edges.extend(
        bodies
            .iter()
            .map(|(name, edges)| json!({"body": name, "edges": edges})),
    );
    std::fs::write(dir.path().join("bodies.json"), inspect.to_string()).unwrap();
    std::fs::write(
        dir.path().join("edges.json"),
        Value::from(edges).to_string(),
    )
    .unwrap();
    let mut measured = vec![json!({"features": 3, "marker": 2})];
    for (_, edges) in bodies {
        for edge in edges.iter().filter(|edge| edge.get("index").is_some()) {
            measured.push(
                edge.get("measurement")
                    .cloned()
                    .unwrap_or(json!({"items": []})),
            );
        }
    }
    std::fs::write(
        dir.path().join("measure.json"),
        Value::from(measured).to_string(),
    )
    .unwrap();
    let path = dir.path().join("solidcraft-cli");
    let folder = dir.path().display();
    std::fs::write(
        &path,
        format!(
            "#!/bin/sh\nfor last; do out=\"$last\"; done\n\
             case \"$1\" in --version) echo 'solidcraft-cli 0.19.0'; exit 0;; esac\n\
             sleep {delay}\n\
             if grep -q inspect.measure \"$2\"; then cat '{folder}/measure.json' > \"$out\"; \
             elif grep -q model.faces \"$2\"; then cat '{folder}/faces.json' > \"$out\"; \
             elif grep -q model.edges \"$2\"; then cat '{folder}/edges.json' > \"$out\"; \
             else cat '{folder}/bodies.json' > \"$out\"; fi\n"
        ),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    FakeCli { _dir: dir, path }
}

/// A writer that finds blend edges with `cli` and checks nothing else.
fn replaying(cli: &Path) -> SolidCraftWriter {
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .replay(Verification::With(cli.to_path_buf()))
}

#[cfg(unix)]
#[test]
fn blends_select_symmetric_edges_by_their_own_input_geometry() {
    for (symmetric, z0) in [(false, 0.0), (true, -5.0)] {
        let cli = fake_cli(&[("unit cube", prism_edges(&SQUARE, z0, z0 + 10.0))], 0);
        for (kind, dimension) in blend_cases() {
            for opposite in [false, true] {
                let model = if symmetric {
                    symmetric_blend(kind, dimension.clone(), opposite)
                } else {
                    input_blend(kind, dimension.clone(), opposite)
                };
                let (document, report) = convert(&model, &replaying(&cli.path));
                assert!(
                    !history_line(&report).starts_with("flattened"),
                    "{report:?}"
                );
                let blend = &document["features"][2];
                assert_eq!(blend["type"], kind);
                assert_eq!(blend["body"], "unit cube");
                let xy = if opposite { 10.0 } else { 0.0 };
                assert_eq!(blend["edges"], json!([[xy, xy, z0 + 5.0]]));
                if kind == "fillet" {
                    assert_eq!(blend["radius"], "1 mm");
                    assert_eq!(blend["style"], json!({"type": "constant"}));
                } else {
                    assert_eq!(blend["distance"], "1 mm");
                    assert!(blend.get("distance2").is_none());
                    assert!(blend.get("angle").is_none());
                }
            }
        }
    }
}

#[test]
fn blends_without_a_solidcraft_to_replay_the_history_are_refused() {
    let model = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_partial(&document, &report, 1);
    assert_eq!(
        types(&document),
        [
            ("sketch".to_owned(), false),
            ("extrude".to_owned(), false),
            ("import".to_owned(), false)
        ]
    );
    assert!(
        history_line(&report).contains(
            "feature \"Edge treatment\": its input edges are found only on SolidCraft's replay \
             of the history before it, and there is none: verification turned off"
        ),
        "{report:?}"
    );
    let missing = Path::new("/nonexistent/solidcraft-cli");
    let (_, report) = convert(&model, &replaying(missing));
    assert!(history_line(&report).contains("cannot run"), "{report:?}");
    // Inspecting says the same, without running anything.
    let mut report = Report::default();
    let outcome = SolidCraftWriter::new()
        .verification(Verification::Off)
        .plan(&model, &mut report);
    assert_eq!(outcome.mode, Mode::PartialHistory);
    let blend = outcome.features.last().unwrap();
    assert!(
        blend
            .reason
            .as_deref()
            .unwrap_or("")
            .contains("there is none"),
        "{blend:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_replay_that_runs_out_of_time_refuses_the_blend() {
    let cli = fake_cli(&[("unit cube", prism_edges(&SQUARE, 0.0, 10.0))], 5);
    let model = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    let started = std::time::Instant::now();
    let (document, report) = convert(
        &model,
        &replaying(&cli.path).timeout(std::time::Duration::from_millis(300)),
    );
    assert!(started.elapsed() < std::time::Duration::from_secs(4));
    assert_partial(&document, &report, 1);
    assert_eq!(
        types(&document),
        [
            ("sketch".to_owned(), false),
            ("extrude".to_owned(), false),
            ("import".to_owned(), false)
        ]
    );
    assert!(history_line(&report).contains("timed out"), "{report:?}");
}

#[cfg(unix)]
#[test]
fn blends_refuse_missing_foreign_or_ambiguous_input_geometry() {
    let cube = fake_cli(&[("unit cube", prism_edges(&SQUARE, 0.0, 10.0))], 0);
    let base = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    let check = |cli: &Path, change: fn(&mut Value), reason: &str| {
        let mut wire = serde_json::to_value(&base.ir).unwrap();
        change(&mut wire["model"]);
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..base.clone()
        };
        let (document, report) = convert(&model, &replaying(cli));
        let types = types(&document);
        assert_partial(&document, &report, 1);
        assert_eq!(document["features"][1]["type"], "extrude");
        assert!(!types.iter().any(|(kind, _)| kind == "fillet"), "{types:?}");
        assert!(history_line(&report).contains(reason), "{report:?}");
    };
    let cli = cube.path.as_path();
    check(
        cli,
        |m| m["feature_input_topologies"] = json!([]),
        "input topology is missing",
    );
    check(
        cli,
        |m| m["feature_input_topologies"][0]["input_of"] = json!("f3d:model:feature#2"),
        "does not uniquely belong",
    );
    check(
        cli,
        |m| m["feature_input_topologies"][0]["edge_geometry"] = json!([]),
        "input edge geometry is missing",
    );
    check(
        cli,
        |m| {
            m["feature_input_topologies"][0]["edge_geometry"][0]
                .as_object_mut()
                .unwrap()
                .remove("tangent");
        },
        "tangent is missing",
    );
    check(
        cli,
        |m| m["feature_input_topologies"][0]["edges"] = json!(["opposite"]),
        "not a unique member",
    );
    check(
        cli,
        |m| {
            m["feature_input_topologies"][0]["edge_geometry"][0]["interior"] =
                json!({"x": 0.0, "y": 0.0, "z": 0.0})
        },
        "cannot be proven",
    );
    // A midpoint on no rebuilt edge, and ends or a tangent other than the rebuilt edge's.
    check(
        cli,
        |m| {
            m["feature_input_topologies"][0]["edge_geometry"][0]["interior"] =
                json!({"x": 5.0, "y": 5.0, "z": 5.0})
        },
        "cannot be proven",
    );
    check(
        cli,
        |m| m["feature_input_topologies"][0]["edge_geometry"][0]["end"]["z"] = json!(9.0),
        "cannot be proven",
    );
    check(
        cli,
        |m| {
            m["feature_input_topologies"][0]["edge_geometry"][0]["tangent"] =
                json!({"x": 1.0, "y": 0.0, "z": 0.0})
        },
        "cannot be proven",
    );
    // A curve that is neither a line nor a circular arc.
    check(
        cli,
        |m| {
            m["feature_input_topologies"][0]["edge_geometry"][0]["interior"] =
                json!({"x": 1.0, "y": 1.0, "z": 4.0})
        },
        "other than circular arcs",
    );
    check(
        cli,
        |m| {
            m["features"][2]["definition"]["value"]["value"]["groups"][0]["edges"] =
                json!({"kind": "edges", "value": ["synthetic:cube:edge#8"]})
        },
        "require edges from their own feature input topology",
    );
    check(
        cli,
        |m| {
            m["features"][2]["definition"]["value"]["value"]["groups"][0]["edges"] = json!({
                "kind": "resolved", "value": {
                    "edges": ["synthetic:cube:edge#8"], "native": "synthetic base selection"}})
        },
        "not proven members of an earlier stored feature result",
    );
    check(
        cli,
        |m| {
            m["features"][2]["definition"]["value"]["value"]["groups"][0]["edges"] =
                json!({"kind": "native", "value": "feature-property:93"})
        },
        "a native edge reference is not supported yet",
    );
    check(
        cli,
        |m| {
            let edges = &mut m["features"][2]["definition"]["value"]["value"]["groups"][0]["edges"];
            let value = edges["value"].clone();
            *edges = json!({"kind": "historical_partial", "value": {"state": value["state"],
                "edges": [], "unresolved": ["op:1"], "native": value["native"]}})
        },
        "some of its edges are not resolved in its input topology",
    );
    // Two bodies in the same place: the midpoint lies on an edge of each.
    let twins = fake_cli(
        &[
            ("unit cube", prism_edges(&SQUARE, 0.0, 10.0)),
            ("unit cube (2)", prism_edges(&SQUARE, 0.0, 10.0)),
        ],
        0,
    );
    check(twins.path.as_path(), |_| {}, "cannot be proven");
}

#[test]
fn blends_refuse_unsupported_dimensions() {
    for (kind, dimension, reason) in [
        (
            "fillet",
            json!({"kind": "variable", "points": [
            {"parameter": 0.0, "radius": 1.0}, {"parameter": 1.0, "radius": 2.0}]}),
            "proven chain direction",
        ),
        (
            "fillet",
            json!({"kind": "asymmetric", "offset_one": 1.0, "offset_two": 2.0}),
            "other than constant-radius",
        ),
    ] {
        let (_, report) = convert(
            &input_blend(kind, dimension, false),
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).contains(reason), "{report:?}");
    }
}

/// Unequal dimensions now reach the edge and face proof, but still need a replay.
#[test]
fn blends_refuse_unsupported_dimensions_and_unordered_reference_faces() {
    for spec in [
        json!({"kind": "two_distances", "first": 1.0, "second": 2.0}),
        json!({"kind": "distance_angle", "distance": 1.0, "angle": 0.6}),
    ] {
        let (_, report) = convert(
            &input_blend("chamfer", spec, false),
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).contains("replay"), "{report:?}");
    }
}

/// A prism face: its name, outward normal, a point on it, and its ring of (edge, forward) uses.
type PrismFace = (String, [f64; 3], [f64; 3], Vec<(String, bool)>);

/// The prism of the polygon `ring` (counter-clockwise on the local UV plane) from W = `z0` to
/// `z1`. `place` turns local (U, V, W) into model coordinates and must be a rotation.
struct Prism<'a> {
    ring: &'a [[f64; 2]],
    z0: f64,
    z1: f64,
    place: fn([f64; 3]) -> [f64; 3],
}

/// `base` with its source body replaced by one prism: planar faces, straight edges, each edge
/// used once each way.
fn with_prism_body(
    base: Model,
    ring: &[[f64; 2]],
    z0: f64,
    z1: f64,
    place: fn([f64; 3]) -> [f64; 3],
) -> Model {
    with_prism_bodies(
        base,
        &[Prism {
            ring,
            z0,
            z1,
            place,
        }],
    )
}

/// `base` with the geometry of its bodies replaced by `prisms`, the `k`th in the `k`th region
/// and body, each in a shell of its own.
fn with_prism_bodies(base: Model, prisms: &[Prism]) -> Model {
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let template = wire["geometry"]["shells"][0].clone();
    let mut arenas: std::collections::BTreeMap<&str, Vec<Value>> = Default::default();
    let mut shells = Vec::new();
    for (k, prism) in prisms.iter().enumerate() {
        let region = &mut wire["geometry"]["regions"][k];
        let shell = json!(format!("synthetic:prism{k}:shell#0"));
        region["shells"] = json!([shell]);
        let mut row = template.clone();
        row["id"] = shell.clone();
        row["region"] = region["id"].clone();
        shells.push(row);
        for (arena, rows) in prism_rows(prism, k, &shell) {
            arenas.entry(arena).or_default().extend(rows);
        }
    }
    let geometry = &mut wire["geometry"];
    geometry["shells"] = Value::from(shells);
    for (arena, rows) in arenas {
        geometry[arena] = Value::from(rows);
    }
    let mut ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    ir.finalize();
    Model { ir, ..base }
}

/// The geometry rows of prism `k` in `shell`, by arena.
fn prism_rows(prism: &Prism, k: usize, shell: &Value) -> Vec<(&'static str, Vec<Value>)> {
    let Prism {
        ring,
        z0,
        z1,
        place,
    } = *prism;
    let n = ring.len();
    let id = |kind: &str, key: String| json!(format!("synthetic:prism{k}:{kind}#{key}"));
    let corner = |i: usize, top: bool| {
        let [x, y] = ring[i % n];
        [x, y, if top { z1 } else { z0 }]
    };
    let vertex = |i: usize, top: bool| {
        id(
            "vertex",
            format!("{}{}", i % n, if top { "t" } else { "b" }),
        )
    };
    let p3 = |p: [f64; 3]| {
        let p = place(p);
        json!({"x": p[0], "y": p[1], "z": p[2]})
    };
    let (mut points, mut vertices, mut curves, mut edges) = (vec![], vec![], vec![], vec![]);
    for i in 0..n {
        for top in [false, true] {
            let key = format!("{i}{}", if top { "t" } else { "b" });
            points.push(json!({"id": id("point", key.clone()), "position": p3(corner(i, top))}));
            vertices.push(json!({"id": vertex(i, top), "point": id("point", key)}));
        }
    }
    // Bottom edge `b<i>` and top edge `t<i>` run from corner i to i + 1, side edge `v<i>` up.
    let mut edge = |key: String, from: (usize, bool), to: (usize, bool)| {
        let (a, b) = (corner(from.0, from.1), corner(to.0, to.1));
        let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let length = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        curves.push(
            json!({"id": id("curve", key.clone()), "geometry": {"kind": "solved", "value": {
            "kind": "line", "origin": p3(a), "direction": p3(d.map(|c| c / length))}}}),
        );
        edges.push(json!({"id": id("edge", key.clone()),
            "carrier": {"curve": id("curve", key), "param_range": [0.0, length]},
            "ends": {"start": vertex(from.0, from.1), "end": vertex(to.0, to.1)}}));
    };
    for i in 0..n {
        edge(format!("b{i}"), (i, false), (i + 1, false));
        edge(format!("t{i}"), (i, true), (i + 1, true));
        edge(format!("v{i}"), (i, false), (i, true));
    }
    // Each face's ring runs counter-clockwise about its outward normal.
    let mut faces: Vec<PrismFace> = vec![
        (
            "bottom".to_owned(),
            [0.0, 0.0, -1.0],
            corner(0, false),
            (0..n).rev().map(|i| (format!("b{i}"), false)).collect(),
        ),
        (
            "top".to_owned(),
            [0.0, 0.0, 1.0],
            corner(0, true),
            (0..n).map(|i| (format!("t{i}"), true)).collect(),
        ),
    ];
    for i in 0..n {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let length = (dx * dx + dy * dy).sqrt();
        faces.push((
            format!("side{i}"),
            [dy / length, -dx / length, 0.0],
            corner(i, false),
            vec![
                (format!("b{i}"), true),
                (format!("v{}", (i + 1) % n), true),
                (format!("t{i}"), false),
                (format!("v{i}"), false),
            ],
        ));
    }
    let (mut surfaces, mut loops, mut coedges, mut face_rows) = (vec![], vec![], vec![], vec![]);
    let mut uses: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for (name, normal, origin, ring) in &faces {
        let u_axis = if normal[2].abs() > 0.5 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        surfaces.push(
            json!({"id": id("surface", name.clone()), "geometry": {"kind": "solved", "value": {
            "kind": "plane", "origin": p3(*origin), "normal": p3(*normal), "u_axis": p3(u_axis)}}}),
        );
        let ids: Vec<String> = (0..ring.len())
            .map(|use_| format!("synthetic:prism{k}:coedge#{name}{use_}"))
            .collect();
        for ((edge, forward), coedge) in ring.iter().zip(&ids) {
            uses.entry(edge.clone()).or_default().push(coedge.clone());
            coedges.push(json!({"id": coedge, "owner_loop": id("loop", name.clone()), "edge": id("edge", edge.clone()),
                "radial_next": coedge, "sense": if *forward { "forward" } else { "reversed" }, "pcurves": []}));
        }
        loops.push(
            json!({"id": id("loop", name.clone()), "face": id("face", name.clone()),
            "boundary": {"kind": "ring", "coedges": ids, "vertex_uses": []}}),
        );
        face_rows.push(json!({"id": id("face", name.clone()), "shell": shell, "surface": id("surface", name.clone()),
            "sense": "forward", "loops": {"kind": "unspecified", "loops": [id("loop", name.clone())]}}));
    }
    for coedge in &mut coedges {
        let edge = coedge["edge"]
            .as_str()
            .unwrap()
            .trim_start_matches(&format!("synthetic:prism{k}:edge#"))
            .to_owned();
        let pair = &uses[&edge];
        assert_eq!(pair.len(), 2);
        let own = coedge["id"].as_str().unwrap().to_owned();
        coedge["radial_next"] = json!(pair.iter().find(|other| **other != own).unwrap());
    }
    vec![
        ("points", points),
        ("vertices", vertices),
        ("curves", curves),
        ("edges", edges),
        ("surfaces", surfaces),
        ("loops", loops),
        ("coedges", coedges),
        ("faces", face_rows),
    ]
}

/// The example cube with the chamfer `spec` on its top front edge, from (0, 0, 10) to
/// (10, 0, 10) between the front face (Y = 0) and the top face (Z = 10), selected by its own
/// input geometry. With `cut`, the source body shows the chamfer: the setbacks `(on_top,
/// on_front)` taken off the top and the front face. Without, it stays the uncut cube.
fn chamfered_cube(spec: Value, cut: Option<(f64, f64)>) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let group = json!({"spec": spec, "edges": {"kind": "historical", "value": {
        "state": "f3d:model:feature-input-topology#1", "edges": ["top-front"],
        "native": "synthetic input edge"}}});
    wire["model"]["features"].as_array_mut().unwrap().push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Edge treatment", "suppressed": false, "dependencies": ["f3d:model:feature#2"], "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": {"kind": "chamfer", "value": {"groups": [group], "flip_direction": false}}}, "source_properties": {}, "source_content": []}));
    let sample = json!({"edge": "top-front", "interior": {"x": 5.0, "y": 0.0, "z": 10.0},
        "tangent": {"x": 1.0, "y": 0.0, "z": 0.0},
        "start": {"x": 0.0, "y": 0.0, "z": 10.0}, "end": {"x": 10.0, "y": 0.0, "z": 10.0},
        "faces": ["front", "top"]});
    wire["model"]["feature_input_topologies"] = json!([{"id": "f3d:model:feature-input-topology#1", "input_of": "f3d:model:feature#3", "edges": ["top-front"], "faces": ["front", "top"], "edge_geometry": [sample], "bodies": [], "vertices": []}]);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let Some((on_top, on_front)) = cut else {
        return model;
    };
    // The cross-section on the side face (X = 0), counter-clockwise in (Y, Z), run along X.
    let ring = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 10.0],
        [on_top, 10.0],
        [0.0, 10.0 - on_front],
    ];
    with_prism_body(model, &ring, 0.0, 10.0, |[u, v, w]| [w, u, v])
}

/// Unequal chamfers, with the setbacks `(on_top, on_front)` each one cuts when its distance
/// runs along the front face, and when it runs along the top face.
fn asymmetric_chamfers() -> Vec<(Value, (f64, f64))> {
    let tan30 = std::f64::consts::FRAC_PI_6.tan();
    let angle =
        json!({"kind": "distance_angle", "distance": 1.0, "angle": std::f64::consts::FRAC_PI_6});
    let distances = json!({"kind": "two_distances", "first": 1.0, "second": 3.0});
    vec![
        (angle.clone(), (tan30, 1.0)),
        (angle, (1.0, tan30)),
        (distances.clone(), (3.0, 1.0)),
        (distances, (1.0, 3.0)),
    ]
}

/// SolidCraft cuts an unequal chamfer on the face the source body shows it on: the history
/// rebuilds and verifies, and the corner taken off sits where the source has it, which the
/// centre of mass tells apart from the same chamfer the other way round.
#[test]
fn solidcraft_cuts_unequal_chamfers_on_the_faces_the_source_shows() {
    let Some(cli) = solidcraft() else { return };
    let dir = tempfile::tempdir().unwrap();
    for (spec, (on_top, on_front)) in asymmetric_chamfers() {
        let model = chamfered_cube(spec.clone(), Some((on_top, on_front)));
        let writer = SolidCraftWriter::new()
            .verification(Verification::With(cli.clone()))
            .replay(Verification::With(cli.clone()));
        let (bytes, report) = written_with(&model, &writer);
        assert_eq!(
            history_line(&report),
            "3 of 3 features rebuilt; verified against the original in SolidCraft",
            "{spec} cut ({on_top}, {on_front})"
        );
        let path = dir.path().join("chamfered.sc");
        std::fs::write(&path, bytes).unwrap();
        let (document, _) = convert(&model, &writer);
        let chamfer = &document["features"][2];
        assert_eq!(chamfer["type"], "chamfer");
        assert_eq!(chamfer["distance"], "1 mm");
        if spec["kind"] == "two_distances" {
            assert_eq!(chamfer["distance2"], "3 mm");
            assert!(chamfer.get("angle").is_none());
        } else {
            let degrees: f64 = chamfer["angle"]
                .as_str()
                .unwrap()
                .trim_end_matches(" deg")
                .parse()
                .unwrap();
            assert!((degrees - 30.0).abs() < 1e-9, "{chamfer}");
            assert!(chamfer.get("distance2").is_none());
        }
        let body = blend_measurement(&cli, &path, &dir.path().join("chamfered.json"));
        let cut = on_top * on_front / 2.0 * 10.0;
        let volume = 1000.0 - cut;
        assert!(
            (body["volume_mm3"].as_f64().unwrap() - volume).abs() < 1e-6,
            "{body}"
        );
        let center = [
            5.0,
            (5000.0 - cut * on_top / 3.0) / volume,
            (5000.0 - cut * (10.0 - on_front / 3.0)) / volume,
        ];
        for (axis, wanted) in center.iter().enumerate() {
            let found = body["center_of_mass"][axis].as_f64().unwrap();
            assert!(
                (found - wanted).abs() < 1e-6,
                "{spec} cut ({on_top}, {on_front}): {body}"
            );
        }
    }
}

/// Which face takes an unequal chamfer's distance is proven from the source body and from
/// SolidCraft's own cut; without either, the chamfer is refused, saying which is missing.
#[cfg(unix)]
#[test]
fn unequal_chamfers_need_their_face_on_the_source_body_and_in_solidcraft() {
    // Every replay answers with the uncut cube, the chamfer's own included.
    let cli = fake_cli(&[("unit cube", prism_edges(&SQUARE, 0.0, 10.0))], 0);
    for (spec, cut) in asymmetric_chamfers() {
        let (_, report) = convert(&chamfered_cube(spec.clone(), None), &replaying(&cli.path));
        let line = history_line(&report);
        assert!(
            line.contains("which face takes the distance"),
            "{spec}: {line}"
        );
        let (_, report) = convert(
            &chamfered_cube(spec.clone(), Some(cut)),
            &replaying(&cli.path),
        );
        let line = history_line(&report);
        assert!(line.contains("either way round"), "{spec}: {line}");
    }
}

/// The example cube (Z 0 to 10) and a block below it (Z -10 to 0), separate bodies from one
/// sketch, with one chamfer of 1 mm and 3 mm on the cube's top front edge and the block's
/// bottom front edge. The source body has the 1 mm on the top face of the cube and on the
/// bottom face of the block, and the 3 mm on their front faces (Y = 0). SolidCraft measures
/// from the face facing up most, so the cube's chamfer takes no flip and the block's does.
fn chamfered_stack() -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let g = &mut wire["geometry"];
    let mut body = g["bodies"][0].clone();
    body["id"] = json!("synthetic:lower:body#0");
    body["name"] = json!("Lower block");
    let mut region = g["regions"][0].clone();
    region["id"] = json!("synthetic:lower:region#0");
    region["body"] = json!("synthetic:lower:body#0");
    g["regions"].as_array_mut().unwrap().push(region);
    g["bodies"].as_array_mut().unwrap().push(body);
    let m = &mut wire["model"];
    let mut lower = m["features"][1].clone();
    lower["id"] = json!("synthetic:lower:feature#0");
    lower["ordinal"] = json!(2);
    lower["name"] = json!("Lower extrusion");
    lower["outputs"] = json!(["synthetic:lower:body#0"]);
    lower["definition"]["value"]["value"]["direction"] =
        json!({"kind": "explicit", "vector": {"x": 0.0, "y": 0.0, "z": -1.0}});
    m["features"].as_array_mut().unwrap().push(lower);
    let spec = json!({"kind": "two_distances", "first": 1.0, "second": 3.0});
    let group = json!({"spec": spec, "edges": {"kind": "historical", "value": {
        "state": "f3d:model:feature-input-topology#1", "edges": ["top-front", "bottom-front"],
        "native": "synthetic input edge"}}});
    m["features"].as_array_mut().unwrap().push(json!({"id": "f3d:model:feature#3", "ordinal": 3, "name": "Edge treatment", "suppressed": false, "dependencies": ["f3d:model:feature#2", "synthetic:lower:feature#0"], "outputs": ["synthetic:cube:body#0", "synthetic:lower:body#0"], "definition": {"kind": "operation", "value": {"kind": "chamfer", "value": {"groups": [group], "flip_direction": false}}}, "source_properties": {}, "source_content": []}));
    let sample = |edge: &str, z: f64, faces: [&str; 2]| {
        json!({"edge": edge, "interior": {"x": 5.0, "y": 0.0, "z": z},
            "tangent": {"x": 1.0, "y": 0.0, "z": 0.0},
            "start": {"x": 0.0, "y": 0.0, "z": z}, "end": {"x": 10.0, "y": 0.0, "z": z},
            "faces": faces})
    };
    m["feature_input_topologies"] = json!([{"id": "f3d:model:feature-input-topology#1", "input_of": "f3d:model:feature#3", "edges": ["top-front", "bottom-front"], "faces": ["front", "top", "lower-front", "bottom"], "edge_geometry": [sample("top-front", 10.0, ["front", "top"]), sample("bottom-front", -10.0, ["lower-front", "bottom"])], "bodies": [], "vertices": []}]);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    // Cross-sections on the side face (X = 0), counter-clockwise in (Y, Z), run along X.
    let upper = [
        [0.0, 0.0],
        [10.0, 0.0],
        [10.0, 10.0],
        [1.0, 10.0],
        [0.0, 7.0],
    ];
    let below = [
        [1.0, -10.0],
        [10.0, -10.0],
        [10.0, 0.0],
        [0.0, 0.0],
        [0.0, -7.0],
    ];
    let along_x = |[u, v, w]: [f64; 3]| [w, u, v];
    with_prism_bodies(
        model,
        &[
            Prism {
                ring: &upper,
                z0: 0.0,
                z1: 10.0,
                place: along_x,
            },
            Prism {
                ring: &below,
                z0: 0.0,
                z1: 10.0,
                place: along_x,
            },
        ],
    )
}

/// One unequal chamfer on two bodies whose faces need opposite flips in SolidCraft: each body's
/// chamfer is settled on its own, and both are cut where the source has them.
#[test]
fn solidcraft_flips_an_unequal_chamfer_per_body() {
    let Some(cli) = solidcraft() else { return };
    let writer = SolidCraftWriter::new()
        .verification(Verification::With(cli.clone()))
        .replay(Verification::With(cli.clone()));
    let model = chamfered_stack();
    let (bytes, report) = written_with(&model, &writer);
    assert_eq!(
        history_line(&report),
        "4 of 4 features rebuilt; verified against the original in SolidCraft"
    );
    let (document, _) = convert(&model, &writer);
    let chamfers: Vec<_> = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["type"] == "chamfer")
        .map(|f| (f["body"].as_str().unwrap(), f.get("flip").cloned()))
        .collect();
    assert_eq!(
        chamfers,
        [("unit cube", None), ("Lower block", Some(json!(true)))],
        "{document}"
    );
    // 1 mm × 3 mm off each 1000 mm³ body: 1 mm into the top or bottom face, 3 mm down the front.
    let measured = evaluated(&cli, &bytes);
    let volume = 985.0;
    for (name, y, z) in [
        ("unit cube", 5000.0 - 15.0 / 3.0, 5000.0 - 15.0 * 9.0),
        ("Lower block", 5000.0 - 15.0 / 3.0, -5000.0 + 15.0 * 9.0),
    ] {
        let body = measured["bodies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["name"] == name)
            .unwrap();
        assert!(
            (body["volume_mm3"].as_f64().unwrap() - volume).abs() < 1e-6,
            "{body}"
        );
        for (axis, wanted) in [(0, 5.0), (1, y / volume), (2, z / volume)] {
            let found = body["center_of_mass"][axis].as_f64().unwrap();
            assert!((found - wanted).abs() < 1e-6, "{name}: {body}");
        }
    }
}

/// The radius of [`blended_hole`]'s hole.
const BORE: f64 = 2.5;

/// [`holed_cube`] with a hole of radius 2.5 mm about (5, 5), and a `kind` blend of `dimension`
/// on the hole's top edge, selected by its own input geometry: a closed edge from (5, 7.5, 10)
/// all around, its midpoint opposite. `circles` (Z, radius) are circles about the hole's axis
/// the source's final body has, edges of their own beside the example cube's B-rep: where an
/// unequal chamfer cut it.
fn blended_hole(kind: &str, dimension: Value, circles: &[(f64, f64)]) -> Model {
    let base = holed_cube(2.0 * BORE, |_| {});
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let mut group = json!({"edges": {"kind": "historical", "value": {
        "state": "f3d:model:feature-input-topology#1", "edges": ["synthetic:rim:edge#hole-top"],
        "native": "synthetic input edge"}}});
    group[if kind == "fillet" { "radius" } else { "spec" }] = dimension;
    let mut definition = json!({"groups": [group]});
    if kind == "chamfer" {
        definition["flip_direction"] = json!(false);
    }
    wire["model"]["features"].as_array_mut().unwrap().push(json!({"id": "f3d:model:feature#5", "ordinal": 4, "name": "Edge treatment", "suppressed": false, "dependencies": ["f3d:model:feature#4"], "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": {"kind": kind, "value": definition}}, "source_properties": {}, "source_content": []}));
    let xyz = |x: f64, y: f64, z: f64| json!({"x": x, "y": y, "z": z});
    // Keep the source seam distinct from the measured replay seam.
    let start = xyz(5.0, 5.0 + BORE, 10.0);
    let sample = json!({"edge": "synthetic:rim:edge#hole-top", "interior": xyz(5.0, 5.0 - BORE, 10.0),
        "tangent": xyz(1.0, 0.0, 0.0), "start": start, "end": start,
        "faces": ["synthetic:rim:face#hole-wall", "synthetic:rim:face#top"]});
    wire["model"]["feature_input_topologies"] = json!([{"id": "f3d:model:feature-input-topology#1", "input_of": "f3d:model:feature#5", "edges": ["synthetic:rim:edge#hole-top"], "faces": ["synthetic:rim:face#hole-wall", "synthetic:rim:face#top"], "edge_geometry": [sample], "bodies": [], "vertices": []}]);
    // Exact input carrier belongs to an earlier hidden result, independently of final edges.
    let source = "synthetic:rim:body#0";
    for feature in wire["model"]["features"].as_array_mut().unwrap() {
        for output in feature["outputs"].as_array_mut().unwrap() {
            if *output == json!("synthetic:cube:body#0") {
                *output = json!(source);
            }
        }
    }
    let g = &mut wire["geometry"];
    // The stored circle's owner must name the body returned by the replay.
    g["bodies"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": source, "name": "unit cube", "kind": "wire", "visible": false}));
    g["regions"].as_array_mut().unwrap().push(json!({"id": "synthetic:rim:region#0", "body": source, "shells": ["synthetic:rim:shell#0"]}));
    g["shells"].as_array_mut().unwrap().push(json!({"id": "synthetic:rim:shell#0", "region": "synthetic:rim:region#0", "members": [{"kind": "wire_edge", "id": "synthetic:rim:edge#hole-top"}]}));
    g["edges"].as_array_mut().unwrap().push(json!({"id": "synthetic:rim:edge#hole-top", "carrier": {"curve": "synthetic:rim:curve#0", "param_range": [0.0, std::f64::consts::TAU]}, "ends": {"start": "synthetic:rim:vertex#0", "end": "synthetic:rim:vertex#0"}}));
    g["vertices"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "synthetic:rim:vertex#0", "point": "synthetic:rim:point#0"}));
    g["points"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "synthetic:rim:point#0", "position": start}));
    g["curves"].as_array_mut().unwrap().push(json!({"id": "synthetic:rim:curve#0", "geometry": {"kind": "solved", "value": {
        "kind": "circle", "center": xyz(5.0, 5.0, 10.0), "axis": xyz(0.0, 0.0, 1.0), "ref_direction": xyz(0.0, 1.0, 0.0), "radius": BORE
    }}}));
    for (i, &(z, radius)) in circles.iter().enumerate() {
        let id = |arena: &str| format!("synthetic:cut:{arena}#{i}");
        g["points"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": id("point"), "position": xyz(5.0 + radius, 5.0, z)}));
        g["vertices"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": id("vertex"), "point": id("point")}));
        g["curves"].as_array_mut().unwrap().push(json!({"id": id("curve"), "geometry": {"kind": "solved", "value": {"kind": "circle",
            "center": xyz(5.0, 5.0, z), "axis": xyz(0.0, 0.0, 1.0), "ref_direction": xyz(1.0, 0.0, 0.0), "radius": radius}}}));
        g["edges"].as_array_mut().unwrap().push(json!({"id": id("edge"), "carrier": {"curve": id("curve"), "param_range": [0.0, std::f64::consts::TAU]},
            "ends": {"start": id("vertex"), "end": id("vertex")}}));
    }
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

/// Chamfers on [`blended_hole`]'s top edge, each with the setbacks it cuts across the top face
/// and down the hole's wall.
fn hole_chamfers() -> Vec<(Value, (f64, f64))> {
    let tan30 = std::f64::consts::FRAC_PI_6.tan();
    let angle =
        json!({"kind": "distance_angle", "distance": 1.0, "angle": std::f64::consts::FRAC_PI_6});
    let distances = json!({"kind": "two_distances", "first": 1.0, "second": 2.0});
    vec![
        (json!({"kind": "distance", "distance": 1.0}), (1.0, 1.0)),
        (
            json!({"kind": "distance_angle", "distance": 1.0, "angle": std::f64::consts::FRAC_PI_4}),
            (1.0, 1.0),
        ),
        (distances.clone(), (1.0, 2.0)),
        (distances, (2.0, 1.0)),
        (angle.clone(), (tan30, 1.0)),
        (angle, (1.0, tan30)),
    ]
}

/// Blends on the top edge of a hole, a full circle: SolidCraft finds the edge wherever it seams
/// it, cuts each unequal chamfer the way round the source's circles show, and takes off the ring
/// the source has. The ring taken off turns a cross-section about the hole's axis: its volume
/// is `2π ∫ ρ dA` and its centroid's depth below the top `∫ w ρ dA / ∫ ρ dA`, with `ρ` the
/// distance from the axis and `w` the depth.
#[test]
fn solidcraft_blends_the_full_circle_around_a_hole() {
    use std::f64::consts::PI;
    let Some(cli) = closed_blend_cli() else {
        return;
    };
    let holed = 1000.0 - PI * BORE * BORE * 10.0;
    // `removed` mm³ taken off, its centroid `depth` below the top face.
    let check = |model: &Model, removed: f64, depth: f64, label: &str| {
        let (bytes, report) = written_with(
            model,
            &replaying(&cli).target_version(Some("0.44.1".parse().unwrap())),
        );
        assert!(
            history_line(&report).starts_with("5 of 5 features rebuilt"),
            "{label}: {}",
            history_line(&report)
        );
        let measured = evaluated(&cli, &bytes);
        let [body] = measured["bodies"].as_array().unwrap().as_slice() else {
            panic!("{label}: one body: {measured}");
        };
        let volume = holed - removed;
        let found = body["volume_mm3"].as_f64().unwrap();
        assert!((found - volume).abs() < 1e-6 * volume, "{label}: {body}");
        let z = (5.0 * holed - (10.0 - depth) * removed) / volume;
        for (axis, wanted) in [(0, 5.0), (1, 5.0), (2, z)] {
            let found = body["center_of_mass"][axis].as_f64().unwrap();
            assert!((found - wanted).abs() < 1e-6, "{label}: {body}");
        }
    };
    for (spec, (across, down)) in hole_chamfers() {
        let circles = [(10.0, BORE + across), (10.0 - down, BORE)];
        let model = blended_hole("chamfer", spec.clone(), &circles);
        // A right triangle with legs `across` and `down`: ∫ ρ dA = A (r + a / 3) and
        // ∫ w ρ dA = r a b² / 6 + a² b² / 24.
        let area = across * down / 2.0;
        let removed = 2.0 * PI * area * (BORE + across / 3.0);
        let depth = down * (4.0 * BORE + across) / (4.0 * (3.0 * BORE + across));
        check(
            &model,
            removed,
            depth,
            &format!("{spec} ({across}, {down})"),
        );
    }
    // A 1 mm fillet takes off the corner outside a quarter circle: area 1 - π/4, centroid
    // (10 - 3π) / (12 - 3π) mm from the corner along both faces, and ∫ u w dA = 19/24 - π/4
    // with `u` out from the wall.
    let area = 1.0 - PI / 4.0;
    let corner = (10.0 - 3.0 * PI) / (12.0 - 3.0 * PI);
    let depth = (BORE * area * corner + 19.0 / 24.0 - PI / 4.0) / (area * (BORE + corner));
    let model = blended_hole("fillet", json!({"kind": "constant", "radius": 1.0}), &[]);
    check(&model, 2.0 * PI * area * (BORE + corner), depth, "fillet");
}

/// What SolidCraft reports before [`blended_hole`]'s blend: the cube's edges and the hole's two
/// circles, seamed elsewhere than the source's, with their reported midpoints at the test's
/// expected blend picks and the length SolidCraft 0.44.1 reports (short of the circumference),
/// and the faces of the top one; the hole's wall is a
/// cylinder only `with_cylinder`.
#[cfg(unix)]
fn holed_replay(with_cylinder: bool) -> (Vec<Value>, Vec<Value>) {
    let mut edges = prism_edges(&SQUARE, 0.0, 10.0);
    for (z, cap) in [(10.0, "top"), (0.0, "bottom")] {
        edges.push(
            json!({"start": [5.0 + BORE, 5.0, z], "end": [5.0 + BORE, 5.0, z],
            "mid": [5.0 - BORE, 5.0, z], "length": 15.682587880594888,
            "name": format!("wall|{cap}"), "index": edges.len(),
            "measurement": {"items": [{"type": "edge", "round": "circle", "radius": BORE,
                "center": [5.0, 5.0, z], "axis": [0.0, 0.0, 1.0],
                "length_mm": std::f64::consts::TAU * BORE, "circumference": std::f64::consts::TAU * BORE}]}}),
        );
    }
    let cylinder = with_cylinder
        .then(|| json!({"origin": [5.0, 5.0, 0.0], "axis": [0.0, 0.0, 1.0], "radius": BORE}));
    let faces = vec![
        json!({"name": "top", "area": 80.4, "plane_normal": [0.0, 0.0, 1.0], "point": [5.0, 1.0, 10.0]}),
        json!({"name": "wall", "area": 157.0, "plane_normal": null, "cylinder": cylinder, "point": [5.0 - BORE, 5.0, 4.0]}),
        json!({"name": "bottom", "area": 80.4, "plane_normal": [0.0, 0.0, -1.0], "point": [5.0, 1.0, 0.0]}),
    ];
    (edges, faces)
}

/// Equal blends on a full circle need only the circle; a 45-degree chamfer also its faces, a
/// plane and a coaxial cylinder; an unequal one also the source's circles where it cut and
/// SolidCraft's own cut there. Without one, the blend is refused, saying which is missing.
#[cfg(unix)]
#[test]
fn blends_on_a_full_circle_need_the_circle_its_faces_and_the_source_cut() {
    let (edges, faces) = holed_replay(true);
    let cli = fake_cli_with_faces(&[("unit cube", edges.clone())], &[faces], 0);
    let (_, no_cylinder) = holed_replay(false);
    let plain = fake_cli_with_faces(&[("unit cube", edges)], &[no_cylinder], 0);
    let line = |model: &Model, cli: &FakeCli| {
        history_line(
            &convert(
                model,
                &replaying(&cli.path).target_version(Some("0.44.1".parse().unwrap())),
            )
            .1,
        )
    };
    for (kind, dimension) in blend_cases() {
        let model = blended_hole(kind, dimension.clone(), &[]);
        let (document, report) = convert(
            &model,
            &replaying(&cli.path).target_version(Some("0.44.1".parse().unwrap())),
        );
        assert!(
            !history_line(&report).starts_with("flattened"),
            "{dimension}: {report:?}"
        );
        let blend = &document["features"][4];
        assert_eq!(blend["type"], kind, "{report:?}");
        assert_eq!(blend["body"], "unit cube");
        assert_eq!(blend["edges"], json!([[5.0 - BORE, 5.0, 10.0]]));
        if dimension["kind"] == "distance_angle" {
            let refused = line(&model, &plain);
            assert!(refused.contains("non-perpendicular"), "{refused}");
        }
    }
    for (spec, (across, down)) in hole_chamfers().into_iter().skip(2) {
        let unshown = line(&blended_hole("chamfer", spec.clone(), &[]), &cli);
        assert!(
            unshown.contains("which face takes the distance"),
            "{spec}: {unshown}"
        );
        let shown = blended_hole(
            "chamfer",
            spec.clone(),
            &[(10.0, BORE + across), (10.0 - down, BORE)],
        );
        // The replay after it answers with the uncut hole.
        let uncut = line(&shown, &cli);
        assert!(uncut.contains("either way round"), "{spec}: {uncut}");
        let unproven = line(&shown, &plain);
        assert!(unproven.contains("coaxial cylinder"), "{spec}: {unproven}");
    }
}

/// Full SolidCraft measurements, including topology and spatial information.
fn blend_measurement(cli: &Path, input: &Path, output: &Path) -> Value {
    let status = std::process::Command::new(cli)
        .arg("eval")
        .arg(input)
        .arg("--json-out")
        .arg(output)
        .status()
        .unwrap();
    assert!(status.success());
    let measured: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    for entry in measured["timeline"].as_array().unwrap() {
        assert!(entry["error"].is_null(), "{entry}");
    }
    assert_eq!(measured["bodies"].as_array().unwrap().len(), 1);
    measured["bodies"][0].clone()
}

/// The blend found on SolidCraft's own replay of the history before it, on the one-sided and
/// on the symmetric cube, against the same blend made directly in SolidCraft: same volume,
/// bounding box, face count and centre of mass, the last telling the symmetric edges apart.
#[test]
fn solidcraft_blends_the_input_edge_and_matches_the_expected_body() {
    let Some(cli) = solidcraft() else { return };
    let dir = tempfile::tempdir().unwrap();
    for (symmetric, z0) in [(false, 0.0), (true, -5.0)] {
        for (kind, dimension) in blend_cases() {
            for opposite in [false, true] {
                let model = if symmetric {
                    symmetric_blend(kind, dimension.clone(), opposite)
                } else {
                    input_blend(kind, dimension.clone(), opposite)
                };
                let (bytes, report) = written_with(&model, &replaying(&cli));
                assert!(
                    !history_line(&report).starts_with("flattened"),
                    "{report:?}"
                );
                let actual_path = dir.path().join("actual.solidcraft");
                std::fs::write(&actual_path, bytes).unwrap();
                let actual = blend_measurement(&cli, &actual_path, &dir.path().join("actual.json"));
                let xy = if opposite { 10.0 } else { 0.0 };
                let mut params = json!({"edges": [[xy, xy, z0 + 5.0]], "body": "Reference"});
                if kind == "fillet" {
                    params["radius"] = json!("1 mm");
                } else {
                    params["distance"] = json!("1 mm");
                    match dimension["kind"].as_str().unwrap() {
                        "two_distances" => params["distance2"] = json!("1 mm"),
                        "distance_angle" => params["angle"] = json!("45 deg"),
                        _ => {}
                    }
                }
                let script = json!([
                    {"command": "solid.box", "params": {"length": "10 mm", "width": "10 mm",
                        "height": "10 mm", "corner": [0, 0, z0], "body_name": "Reference"}},
                    {"command": format!("solid.{kind}"), "params": params},
                ]);
                let expected_path = dir.path().join("expected-script.json");
                std::fs::write(&expected_path, script.to_string()).unwrap();
                let expected =
                    blend_measurement(&cli, &expected_path, &dir.path().join("expected.json"));
                let volume = actual["volume_mm3"].as_f64().unwrap();
                let wanted = expected["volume_mm3"].as_f64().unwrap();
                assert!((volume - wanted).abs() / wanted < dutchman_solidcraft::VOLUME_TOLERANCE);
                let analytic = if kind == "fillet" {
                    1000.0 - 10.0 * (1.0 - std::f64::consts::FRAC_PI_4)
                } else {
                    995.0
                };
                assert!((volume - analytic).abs() < 0.05, "{actual}");
                assert_eq!(
                    actual["faces"].as_u64().unwrap(),
                    expected["faces"].as_u64().unwrap()
                );
                assert_eq!(actual["faces"].as_u64().unwrap(), 7);
                for axis in 0..3 {
                    for corner in ["min", "max"] {
                        assert!(
                            (actual["bbox"][corner][axis].as_f64().unwrap()
                                - expected["bbox"][corner][axis].as_f64().unwrap())
                            .abs()
                                < 1e-6
                        );
                    }
                    assert!(
                        (actual["center_of_mass"][axis].as_f64().unwrap()
                            - expected["center_of_mass"][axis].as_f64().unwrap())
                        .abs()
                            < 1e-5
                    );
                }
                // Volume, bounding box and face count are identical for these symmetric
                // alternatives. The centre of mass proves which corner lost material.
                for axis in 0..2 {
                    let center = actual["center_of_mass"][axis].as_f64().unwrap();
                    assert!(
                        if opposite {
                            center < 4.999
                        } else {
                            center > 5.001
                        },
                        "{actual}"
                    );
                }
            }
        }
    }
}

/// Two bodies where the cube is, made by extruding its profile twice: SolidCraft's replay
/// gives each the edge the blend's midpoint is on, so the blend is refused.
#[test]
fn solidcraft_refuses_a_blend_whose_midpoint_lies_on_two_rebuilt_edges() {
    let Some(cli) = solidcraft() else { return };
    let base = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    let mut earlier = model["features"][1].clone();
    earlier["id"] = json!("f3d:model:feature#4");
    earlier["ordinal"] = json!(3);
    model["features"].as_array_mut().unwrap().push(earlier);
    model["features"][2]["ordinal"] = json!(4);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let (_, report) = written_with(&model, &replaying(&cli));
    assert!(
        history_line(&report).contains("cannot be proven"),
        "{report:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_distance_angle_chamfer_requires_perpendicular_input_faces() {
    let base = input_blend(
        "chamfer",
        json!({"kind": "distance_angle", "distance": 1.0,
               "angle": std::f64::consts::FRAC_PI_4}),
        false,
    );
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    // A parallelogram has the same selected vertical edge, but oblique support faces.
    let entities = &mut wire["model"]["sketch_entities"];
    entities[1]["geometry"]["end"]["u"] = json!(20.0);
    entities[2]["geometry"]["start"]["u"] = json!(20.0);
    entities[2]["geometry"]["end"]["u"] = json!(10.0);
    entities[3]["geometry"]["start"]["u"] = json!(10.0);
    wire["model"]["sketch_constraints"] = json!([]);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let parallelogram = [[0.0, 0.0], [10.0, 0.0], [20.0, 10.0], [10.0, 10.0]];
    let cli = fake_cli(&[("unit cube", prism_edges(&parallelogram, 0.0, 10.0))], 0);
    let (_, report) = convert(&model, &replaying(&cli.path));
    assert!(
        history_line(&report).contains("non-perpendicular input faces"),
        "{report:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_fillet_of_tangency_weight_one_maps_as_a_circular_fillet() {
    let base = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    let cli = fake_cli(&[("unit cube", prism_edges(&SQUARE, 0.0, 10.0))], 0);
    let writer = replaying(&cli.path);
    for (weight, maps) in [(1.0, true), (0.999, false), (2.0, false)] {
        let mut wire = serde_json::to_value(&base.ir).unwrap();
        wire["model"]["features"][2]["definition"]["value"]["value"]["groups"][0]
            ["tangency_weight"] = json!(weight);
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..base.clone()
        };
        let (document, report) = convert(&model, &writer);
        let noted = report.losses().iter().any(|loss| {
            loss.severity == Severity::Info
                && loss.detail == "tangency weight 1.0 treated as a circular fillet"
        });
        if maps {
            assert_eq!(document["features"][2]["type"], "fillet", "{report:?}");
            assert_eq!(document["features"][2]["radius"], "1 mm");
            assert_eq!(
                document["features"][2]["style"],
                json!({"type": "constant"})
            );
            assert!(noted, "{report:?}");
        } else {
            assert!(
                history_line(&report)
                    .contains("fillets with a tangency weight are not supported yet"),
                "{weight}: {report:?}"
            );
            assert!(!noted, "{report:?}");
        }
    }
}

#[cfg(unix)]
#[test]
fn an_opening_check_that_runs_out_of_time_leaves_the_design_unverified() {
    let host = std::sync::Arc::new(RecordedEvaluator::default());
    let writer = SolidCraftWriter::new()
        .evaluator(host)
        .opening_timeout(std::time::Duration::ZERO);
    let (_, report) = convert(&cube_with_history(10.0, first_profile(), None), &writer);
    match &report.outcome().unwrap().check {
        Check::Unverified(why) => assert!(
            why.contains("SolidCraft didn't finish checking the written design"),
            "{why}"
        ),
        other => panic!("expected unverified, got {other:?}"),
    }
}

#[test]
fn a_fillet_remains_mapped_when_standing_up() {
    let model = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    // This mapping fixture stores the pre-fillet cube. Recorded measurements isolate
    // the coordinate-space contract; mismatching real geometry is covered separately.
    let host = std::sync::Arc::new(RecordedEvaluator {
        require_untransformed: true,
        ..RecordedEvaluator::default()
    });
    let writer = SolidCraftWriter::new()
        .evaluator(host.clone())
        .stand_up(Some(true));
    let (document, report) = convert(&model, &writer);
    let features = document["features"].as_array().unwrap();
    assert!(features.iter().any(|f| f["type"] == "fillet"), "{report:?}");
    assert_eq!(report.outcome().unwrap().check, Check::Verified);
    assert_eq!(
        *host.calls.lock().unwrap(),
        [
            "bodies",
            "edges",
            "original",
            "history",
            "verification-parameters",
            "opening"
        ]
    );
    assert_eq!(
        document["occurrences"][0]["transform"],
        json!([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ])
    );
}

#[cfg(unix)]
#[test]
fn blends_keep_multiple_picks_and_groups_and_refuse_repeated_geometry_and_weights() {
    let base = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    wire["model"]["features"][2]["definition"]["value"]["value"]["groups"][0]["edges"]["value"]
        ["edges"] = json!(["near", "opposite"]);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base.clone()
    };
    let cli = fake_cli(&[("unit cube", prism_edges(&SQUARE, 0.0, 10.0))], 0);
    let writer = replaying(&cli.path);
    let (document, _) = convert(&model, &writer);
    assert_eq!(
        document["features"][2]["edges"],
        json!([[0.0, 0.0, 5.0], [10.0, 10.0, 5.0]])
    );
    assert_eq!(document["features"][2]["body"], "unit cube");
    let sample = wire["model"]["feature_input_topologies"][0]["edge_geometry"][0].clone();
    wire["model"]["feature_input_topologies"][0]["edge_geometry"][1] = sample;
    wire["model"]["feature_input_topologies"][0]["edge_geometry"][1]["edge"] = json!("opposite");
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base.clone()
    };
    let (_, report) = convert(&model, &writer);
    assert!(
        history_line(&report).contains("repeats a rebuilt edge"),
        "{report:?}"
    );
    for multiple_groups in [false, true] {
        let mut wire = serde_json::to_value(&base.ir).unwrap();
        let groups = &mut wire["model"]["features"][2]["definition"]["value"]["value"]["groups"];
        if multiple_groups {
            let group = groups[0].clone();
            groups.as_array_mut().unwrap().push(group);
        } else {
            groups[0]["tangency_weight"] = json!(0.5);
        }
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..base.clone()
        };
        let (_, report) = convert(&model, &writer);
        if multiple_groups {
            // Several radius groups map, one blend per group.
            assert!(
                !history_line(&report).contains("independently dimensioned groups"),
                "{report:?}"
            );
        } else {
            assert!(
                history_line(&report).contains("tangency weight"),
                "{report:?}"
            );
        }
    }
}

#[test]
fn signed_axis_dimensions_keep_their_expression_and_drive_positive_magnitudes() {
    for (kind, edge, axis) in [
        ("horizontal_distance", 1, "x"),
        ("vertical_distance", 2, "y"),
    ] {
        let mut model = cube_with_history(10.0, first_profile(), None);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let m = &mut wire["model"];
        let id = format!("f3d:model:sketch-entity#{edge}");
        m["sketch_constraints"][4]["definition"] = json!({
            "kind": kind,
            "first": {"kind": "end", "value": id},
            "second": {"kind": "start", "value": id},
            "parameter": "f3d:model:parameter#2",
        });
        m["sketch_entities"].as_array_mut().unwrap().push(entity(
            5,
            json!({"kind": "point", "position": {"u": 0.0, "v": 0.0}}),
        ));
        m["sketch_constraints"].as_array_mut().unwrap().push(json!({
            "id": "f3d:model:sketch-constraint#9", "sketch": SKETCH,
            "definition": {"kind": "fixed", "entity": "f3d:model:sketch-entity#5"},
        }));
        m["parameters"][1]["expression"] = json!("-span");
        m["parameters"][1]["value"]["value"] = json!(-10.0);
        m["parameters"][1]["dependencies"] = json!(["f3d:model:parameter#3"]);
        m["parameters"].as_array_mut().unwrap().push(json!({"id": "f3d:model:parameter#3", "name": "span", "expression": "10 mm", "value": {"kind": "length", "value": 10.0}, "ordinal": 0, "dependencies": [], "properties": {}}));
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, _) = convert(&model, &SolidCraftWriter::new());
        let dimension = &document["features"][0]["sketch"]["constraints"][4];
        assert_eq!(dimension["type"], format!("distance_{axis}"));
        assert_eq!(dimension["value"], 10.0);
        assert_eq!(dimension["p"], if edge == 1 { 2 } else { 3 });
        assert_eq!(dimension["q"], if edge == 1 { 1 } else { 2 });
        assert_ne!(dimension["param"], "side");
        let params = document["params"].as_array().unwrap();
        assert_eq!(
            params.iter().find(|p| p["name"] == "side").unwrap()["expr"],
            "-span"
        );
        assert_eq!(
            params
                .iter()
                .find(|p| p["name"] == dimension["param"])
                .unwrap()["expr"],
            "-(side)"
        );
        let Some(cli) = solidcraft() else { continue };
        let (bytes, _) = written(&model);
        assert_eq!(volumes(&cli, &bytes), [1000.0]);
        let edited = evaluated(&cli, &with_param(&bytes, "span", "20 mm"));
        assert_eq!(edited["bodies"][0]["volume_mm3"], 4000.0);
        assert_eq!(edited["bodies"][0]["bbox"]["min"], json!([0.0, 0.0, 0.0]));
        assert_eq!(
            edited["bodies"][0]["bbox"]["max"],
            json!([20.0, 20.0, 10.0])
        );
    }
}

#[test]
fn used_datum_helpers_keep_the_resolved_dependent_sketch_and_replay() {
    let base = cube_with_history(10.0, first_profile(), None);
    for definition in [
        json!({"kind": "datum_point", "value": {"position": {"x": 0.0, "y": 0.0, "z": 0.0}}}),
        json!({"kind": "datum_coordinate_system", "value": {"frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "x_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
            "y_axis": {"x": 0.0, "y": 1.0, "z": 0.0},
            "z_axis": {"x": 0.0, "y": 0.0, "z": 1.0}}}}),
    ] {
        let mut wire = serde_json::to_value(&base.ir).unwrap();
        let features = wire["model"]["features"].as_array_mut().unwrap();
        for feature in features.iter_mut() {
            feature["ordinal"] = json!(feature["ordinal"].as_u64().unwrap() + 1);
        }
        features[0]["dependencies"] = json!(["f3d:model:feature#datum"]);
        features.insert(
            0,
            json!({"id": "f3d:model:feature#datum", "ordinal": 0, "name": "Attachment", "suppressed": false, "definition": {"kind": "operation", "value": definition}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
        );
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..base.clone()
        };
        let (document, report) = convert(&model, &SolidCraftWriter::default());
        assert_eq!(
            types(&document),
            [
                ("sketch".to_owned(), false),
                ("extrude".to_owned(), false),
                ("import".to_owned(), true)
            ]
        );
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("its resolved placement is carried")));
        if std::env::var_os(CLI_ENV).is_some() {
            assert_eq!(report.outcome().unwrap().check, Check::Verified);
        }
    }
}

#[test]
fn freecad_cc0_negative_distance_replays_without_flattening() {
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/P09a.FCStd");
    let model = dutchman_core::read(
        dutchman_core::Format::FreeCadDocument,
        &input,
        &mut Report::default(),
    )
    .unwrap();
    let (document, _) = convert(&model, &SolidCraftWriter::new());
    let sketch = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Sketch001")
        .unwrap();
    let constraints = sketch["sketch"]["constraints"].as_array().unwrap();
    let dimension = constraints
        .iter()
        .find(|c| c["type"] == "distance_x")
        .unwrap();
    assert_eq!(dimension["value"], 2.5);
    let magnitude = document["params"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == dimension["param"])
        .unwrap();
    assert_eq!(magnitude["expr"], "-(Constraint9)");
    let Some(cli) = solidcraft() else { return };
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::With(cli.clone())),
    );
    assert!(
        history_line(&report).contains("verified against the original"),
        "{}",
        history_line(&report)
    );
    let (bytes, _) = written(&model);
    let actual = volumes(&cli, &bytes);
    assert_eq!(actual.len(), 1);
    assert!((actual[0] - 6250.0).abs() < 1e-6);
}

#[test]
fn helper_info_survives_mapping_and_replay_fallbacks() {
    for blocked in [true, false] {
        let base = cube_with_history(if blocked { 10.0 } else { 20.0 }, first_profile(), None);
        let mut wire = serde_json::to_value(&base.ir).unwrap();
        let features = wire["model"]["features"].as_array_mut().unwrap();
        features.push(json!({"id": "f3d:model:feature#unused", "ordinal": 4, "name": "Unused point", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "datum_point", "value": {"position": {"x": 0.0, "y": 0.0, "z": 0.0}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
        if blocked {
            features.push(json!({"id": "f3d:model:feature#unknown", "ordinal": 3, "name": "Later", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "native", "value": {"kind": "Mystery", "parameters": {}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
        }
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..base
        };
        let (_, report) = convert(&model, &SolidCraftWriter::default());
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.severity == Severity::Info
                && loss.detail == "helper not converted: nothing uses it"));
        if blocked {
            assert!(matches!(report.outcome().unwrap().check, Check::Partial(_)));
        } else if std::env::var_os(CLI_ENV).is_some() {
            assert_eq!(report.outcome().unwrap().mode, Mode::Import);
            assert!(matches!(
                report.outcome().unwrap().check,
                Check::Flattened(_)
            ));
        }
    }
}

/// A valid extrusion, then a blocker, a resolved sketch, and a later extrusion.
fn partial_history_model() -> Model {
    let mut model = reference_sketch_history();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["ordinal"] = json!(3);
    wire["model"]["features"][2]["ordinal"] = json!(2);
    let cube = cube_with_history(10.0, first_profile(), None);
    let cube = serde_json::to_value(&cube.ir).unwrap();
    let mut extrude = cube["model"]["features"][1].clone();
    extrude["id"] = json!("f3d:model:feature#4");
    extrude["ordinal"] = json!(1);
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(extrude.clone());
    extrude["id"] = json!("f3d:model:feature#5");
    extrude["ordinal"] = json!(4);
    extrude["name"] = json!("Later extrusion");
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(extrude);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn assert_partial(document: &Value, report: &Report, sketches: usize) {
    assert_eq!(report.outcome().unwrap().mode, Mode::PartialHistory);
    assert!(matches!(report.outcome().unwrap().check, Check::Partial(_)));
    let features = document["features"].as_array().unwrap();
    let original = features.last().unwrap();
    assert_eq!(original["type"], "import");
    assert_eq!(original["suppressed"], false);
    assert!(original.get("component").is_none());
    let component = &document["components"][0]["id"];
    assert!(features
        .iter()
        .take(features.len() - 1)
        .all(|f| &f["component"] == component && f["suppressed"] == false));
    assert_eq!(
        document["visibility"]["hidden_occurrences"],
        json!([[document["occurrences"][0]["id"]]])
    );
    assert_eq!(
        features.iter().filter(|f| f["type"] == "sketch").count(),
        sketches
    );
    assert!(report.losses().iter().any(|l| l.subject == "history"
        && l.severity == Severity::Degraded
        && l.detail.contains("imported final shape")));
    let ids: std::collections::BTreeSet<_> =
        features.iter().map(|f| f["id"].as_u64().unwrap()).collect();
    assert_eq!(ids.len(), features.len());
    assert!(ids
        .iter()
        .all(|id| *id < document["next_id"].as_u64().unwrap()));
}

#[test]
fn partial_history_appearance_bindings_reach_the_visible_original() {
    for named_prefix in [false, true] {
        let mut model = partial_history_model();
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let body = wire["geometry"]["bodies"][0]["id"].clone();
        if !named_prefix {
            for feature in wire["model"]["features"].as_array_mut().unwrap() {
                if feature["definition"]["value"]["kind"] == "extrude" {
                    feature["outputs"] = json!([]);
                }
            }
        }
        wire["model"]["appearances"] = json!([
            {"id": "synthetic:test:appearance#red", "base_color": {"r":1.0,"g":0.0,"b":0.0,"a":1.0}, "properties": {}, "textures": []},
            {"id": "synthetic:test:appearance#blue", "base_color": {"r":0.0,"g":0.0,"b":1.0,"a":1.0}, "properties": {}, "textures": []}
        ]);
        wire["model"]["appearance_bindings"] = json!([
            {"id": "synthetic:test:binding#body", "target": {"entity": {"kind":"body","value":body}}, "appearance": "synthetic:test:appearance#red", "channels": {}},
            {"id": "synthetic:test:binding#face", "target": {"entity": {"kind":"face","value":"synthetic:cube:face#top"}}, "appearance": "synthetic:test:appearance#blue", "channels": {}}
        ]);
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (doc, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert_partial(&doc, &report, 2);
        let original = doc["features"].as_array().unwrap().last().unwrap();
        let name = original["body_names"][0].as_str().unwrap();
        assert_eq!(
            doc["appearances"]["bodies"][name]["color"],
            json!([255, 0, 0])
        );
        let face = doc["appearances"]["faces"]
            .as_array()
            .unwrap()
            .iter()
            .find(|face| face["body"] == name)
            .unwrap();
        assert_eq!(face["look"]["color"], json!([0, 0, 255]));
        assert_eq!(name.contains("Original geometry"), named_prefix);
        assert!(
            !report
                .losses()
                .iter()
                .any(|loss| loss.subject == "appearance"),
            "{:?}",
            report.losses()
        );
        if let Some(cli) = solidcraft() {
            let dir = tempfile::tempdir().unwrap();
            let design = dir.path().join("partial.solidcraft");
            let script = dir.path().join("looks.json");
            let output = dir.path().join("looks.out.json");
            std::fs::write(
                &design,
                written_with(
                    &model,
                    &SolidCraftWriter::new().verification(Verification::Off),
                )
                .0,
            )
            .unwrap();
            std::fs::write(
                &script,
                json!([
                    {"command":"document.inspect","params":{"measure":false}},
                    {"command":"appearance.list","params":{"body":name}}
                ])
                .to_string(),
            )
            .unwrap();
            assert!(std::process::Command::new(cli)
                .arg("run")
                .arg(&script)
                .arg("--in")
                .arg(&design)
                .arg("--results")
                .arg("--json-out")
                .arg(&output)
                .status()
                .unwrap()
                .success());
            let results: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
            let bodies = results[0]["bodies"].as_array().unwrap();
            assert_eq!(bodies.len(), 2, "{results}");
            assert!(
                bodies
                    .iter()
                    .any(|b| b["name"] == name && b["visible"] == true),
                "{results}"
            );
            assert_eq!(
                results[1]["body"]["appearance"]["color"], "#ff0000",
                "{results}"
            );
            // SolidCraft 0.18 lists every face; newer releases list only faces with their own
            // look. Either way exactly one face is blue.
            let faces = results[1]["body"]["faces"].as_array().unwrap();
            let blue = faces
                .iter()
                .filter(|face| face["appearance"]["color"] == "#0000ff")
                .count();
            assert_eq!(blue, 1, "{results}");
            assert!(
                faces.iter().all(|face| matches!(
                    face["appearance"]["color"].as_str(),
                    Some("#0000ff" | "#ff0000")
                )),
                "{results}"
            );
        }
    }
}

fn with_later_blocker(mut model: Model) -> Model {
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#99", "ordinal": 99, "name": "Later blocker", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "native", "value": {"kind": "Mystery", "parameters": {}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn partial_entry_recesses_keep_the_target_in_parts_and_assemblies() {
    for version in ["0.18.2", "0.25.1"] {
        for form in [
            json!({"kind": "counterbore", "diameter": 8.0, "depth": 2.0}),
            json!({"kind": "countersink", "diameter": 8.0, "angle": std::f64::consts::FRAC_PI_2}),
        ] {
            let part = with_later_blocker(holed_cube(4.0, |hole| {
                hole["shape"]["construction"]["value"]["kind"] = form;
                hole["extent"] = json!({"kind": "blind", "length": 6.0});
                hole["bottom"] = json!({"kind": "flat"});
            }));
            let mut assembly = assembly_of_histories();
            assembly.assembly.as_mut().unwrap().parts[1].ir = part.ir.clone();
            for model in [&part, &assembly] {
                let writer = SolidCraftWriter::new()
                    .verification(Verification::Auto)
                    .target_version(Some(version.parse().unwrap()));
                let (document, report) = convert(model, &writer);
                assert_eq!(report.outcome().unwrap().mode, Mode::PartialHistory);
                let Check::Partial(why) = &report.outcome().unwrap().check else {
                    panic!("{report:?}");
                };
                if solidcraft().is_some() {
                    assert!(why.contains("retained features replay"), "{why}");
                }
                let hole = document["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|feature| feature["type"] == "hole")
                    .unwrap();
                let tip = &hole["hole"]["tip_angle"];
                if version
                    .parse::<dutchman_solidcraft::SolidCraftVersion>()
                    .unwrap()
                    >= dutchman_solidcraft::SolidCraftVersion::V0_22_1
                {
                    assert_eq!(tip, "180");
                } else {
                    assert!(tip.is_null());
                }
                let component = &hole["component"];
                assert!(document["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| &c["id"] == component && c["name"] == "Editable history (partial)"));
            }
        }
    }
}

#[test]
fn partial_helices_keep_the_target_in_parts_and_assemblies() {
    let replay = helix_cli().map_or(Verification::Off, Verification::With);
    let part = with_later_blocker(helical_history(|_| {}));
    let mut assembly = assembly_of_histories();
    assembly.assembly.as_mut().unwrap().parts[1].ir = part.ir.clone();
    for model in [&part, &assembly] {
        for version in ["0.18.2", "0.25.1"] {
            let writer = SolidCraftWriter::new()
                .verification(replay.clone())
                .target_version(Some(version.parse().unwrap()));
            let (document, report) = convert(model, &writer);
            let coils: Vec<_> = document["features"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["type"] == "coil")
                .collect();
            if version == "0.18.2" {
                assert!(coils.is_empty());
                assert!(report.outcome().unwrap().features.iter().any(|f| f
                    .reason
                    .as_deref()
                    .is_some_and(|why| why.contains("needs SolidCraft 0.25 patch 1"))));
            } else {
                assert_eq!(coils.len(), 1);
                assert_eq!(report.outcome().unwrap().mode, Mode::PartialHistory);
                let Check::Partial(why) = &report.outcome().unwrap().check else {
                    panic!("{report:?}");
                };
                if helix_cli().is_some() {
                    assert!(why.contains("retained features replay"), "{why}");
                }
                assert!(document["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["id"] == coils[0]["component"]
                        && c["name"] == "Editable history (partial)"));
            }
        }
    }
}

#[test]
fn a_blocker_keeps_the_prefix_and_later_resolved_sketches() {
    let model = partial_history_model();
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);
    assert_partial(&document, &report, 2);
    assert_eq!(
        types(&document),
        [
            ("sketch".to_owned(), false),
            ("extrude".to_owned(), false),
            ("sketch".to_owned(), false),
            ("import".to_owned(), false)
        ]
    );
    let outcome = report.outcome().unwrap();
    assert_eq!(
        outcome
            .features
            .iter()
            .filter(|f| f.status == dutchman_core::FeatureStatus::Mapped)
            .count(),
        3
    );
    assert_eq!(
        outcome.features.last().unwrap().status,
        dutchman_core::FeatureStatus::Blocked
    );
    assert_eq!(
        writer.plan(&model, &mut Report::default()).mode,
        Mode::PartialHistory
    );
    let (import, _) = convert(&model, &writer.import_only(true));
    assert_eq!(
        document["features"].as_array().unwrap().last().unwrap()["step_blob"],
        import["features"][0]["step_blob"]
    );
}

#[test]
fn partial_history_keeps_a_sketch_without_a_timeline_row() {
    let mut model = partial_history_model();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .retain(|feature| feature["ordinal"] != 3);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_partial(&document, &report, 2);
    assert_eq!(document["features"][1]["type"], "extrude");
    assert_eq!(
        report
            .outcome()
            .unwrap()
            .features
            .iter()
            .filter(|feature| feature.kind == "sketch")
            .count(),
        1
    );
}

#[test]
fn partial_history_is_not_required_to_keep_sketches_without_timeline_rows() {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"] = json!([]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);
    assert_reference_sketches(&document, 1);
    assert_eq!(report.outcome().unwrap().mode, Mode::Import);
    assert!(matches!(
        report.outcome().unwrap().check,
        Check::NotApplicable(_)
    ));
    wire["model"]["sketches"][0]["placement"] = json!({"kind": "unresolved"});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(&model, &writer);
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("sketch left out") && loss.detail.contains("plane")));
}

#[test]
fn a_later_unresolved_sketch_is_reported_without_losing_the_prefix() {
    let mut model = partial_history_model();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketches"][1]["placement"] = json!({"kind": "unresolved"});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_partial(&document, &report, 1);
    assert!(report
        .losses()
        .iter()
        .any(|l| l.detail.contains("sketch left out") && l.detail.contains("plane")));
}

#[test]
fn partial_history_replays_beside_the_unchanged_original() {
    let Some(cli) = solidcraft() else { return };
    let model = partial_history_model();
    let writer = SolidCraftWriter::new().verification(Verification::With(cli.clone()));
    let (document, report) = convert(&model, &writer);
    assert_partial(&document, &report, 2);
    assert!(history_line(&report).contains("retained features replay in SolidCraft"));
    // eval includes hidden bodies: one editable 1000 mm³ cube and the original cube.
    let (bytes, _) = written_with(&model, &writer);
    assert_eq!(volumes(&cli, &bytes), [1000.0, 1000.0]);
}

#[test]
fn cc0_freecad_subshape_binder_keeps_the_earlier_pad() {
    if !dutchman_core::Format::FreeCadDocument.can_convert() {
        return;
    }
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/F16b.FCStd");
    let mut report = Report::default();
    let model =
        dutchman_core::read(dutchman_core::Format::FreeCadDocument, &input, &mut report).unwrap();
    let writer = SolidCraftWriter::new();
    let (document, report) = convert(&model, &writer);
    assert_partial(&document, &report, 2);
    let features = document["features"].as_array().unwrap();
    assert_eq!(
        features.iter().filter(|f| f["type"] == "extrude").count(),
        1
    );
    assert!(history_line(&report).contains("DatumReference"));
    if let Some(cli) = solidcraft() {
        assert!(history_line(&report).contains("retained features replay in SolidCraft"));
        let (bytes, _) = written_with(&model, &writer);
        let mut measured = volumes(&cli, &bytes);
        measured.sort_by(f64::total_cmp);
        // The hidden prefix is 6000 mm³. The visible original is two solids,
        // 3000 + 6000 mm³, as the recipe's CC0 manifest records.
        assert_eq!(measured.len(), 3);
        for (actual, expected) in measured.iter().zip([3000.0, 6000.0, 6000.0]) {
            assert!((actual - expected).abs() < 1e-6, "{measured:?}");
        }
        let dir = tempfile::tempdir().unwrap();
        let design = dir.path().join("F16b.solidcraft");
        let script = dir.path().join("visibility.json");
        let output = dir.path().join("visibility.out.json");
        std::fs::write(&design, &bytes).unwrap();
        std::fs::write(
            &script,
            json!([
                {"command": "document.inspect", "params": {"measure": true}},
                {"command": "inspect.silhouette", "params": {}}
            ])
            .to_string(),
        )
        .unwrap();
        let status = std::process::Command::new(&cli)
            .arg("run")
            .arg(&script)
            .arg("--in")
            .arg(&design)
            .arg("--results")
            .arg("--json-out")
            .arg(&output)
            .status()
            .unwrap();
        assert!(status.success());
        let results: Value = serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        let bodies = results[0]["bodies"].as_array().unwrap();
        assert_eq!(bodies.len(), 3);
        let visible: Vec<_> = bodies.iter().filter(|b| b["visible"] == true).collect();
        assert_eq!(visible.len(), 2);
        assert_eq!(
            visible
                .iter()
                .map(|b| b["volume_mm3"].as_f64().unwrap())
                .sum::<f64>(),
            9000.0
        );
        assert!(visible
            .iter()
            .all(|b| b["name"].as_str().unwrap().starts_with("F16b")));
        let hidden: Vec<_> = bodies.iter().filter(|b| b["visible"] == false).collect();
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0]["name"], "Body1");
        assert_eq!(hidden[0]["volume_mm3"], 6000.0);
        assert_eq!(results[1]["bodies"], 2);
    }
}

#[test]
fn a_partial_component_is_hidden_in_every_assembly_placement() {
    let mut model = assembly_of_histories();
    let second = &mut model.assembly.as_mut().unwrap().parts[1];
    second.ir = partial_history_model().ir;
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);
    assert_eq!(report.outcome().unwrap().mode, Mode::PartialHistory);
    assert!(matches!(report.outcome().unwrap().check, Check::Partial(_)));
    let components = document["components"].as_array().unwrap();
    let second = &components.iter().find(|c| c["name"] == "Second").unwrap()["id"];
    let partial = &components
        .iter()
        .find(|c| c["name"] == "Editable history (partial)")
        .unwrap()["id"];
    let features = document["features"].as_array().unwrap();
    let prefix: Vec<_> = features
        .iter()
        .filter(|f| &f["component"] == partial)
        .collect();
    assert_eq!(prefix.len(), 3);
    assert!(prefix.iter().any(|f| f["type"] == "extrude"));
    assert!(prefix.iter().all(|f| f["suppressed"] == false));
    let original = features
        .iter()
        .find(|f| &f["component"] == second && f["type"] == "import")
        .unwrap();
    assert_eq!(original["suppressed"], false);
    let hidden = document["visibility"]["hidden_occurrences"]
        .as_array()
        .unwrap();
    assert_eq!(hidden.len(), 2);
    let occurrences = document["occurrences"].as_array().unwrap();
    for path in hidden {
        assert_eq!(path.as_array().unwrap().len(), 2);
        assert_eq!(
            occurrences.iter().find(|o| o["id"] == path[0]).unwrap()["component"],
            *second
        );
        assert_eq!(
            occurrences.iter().find(|o| o["id"] == path[1]).unwrap()["component"],
            *partial
        );
    }
    let mut ids: Vec<_> = features
        .iter()
        .chain(components)
        .chain(occurrences)
        .map(|v| v["id"].as_u64().unwrap())
        .collect();
    let count = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), count);
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail == "2 editable-history component placement(s) are placed hidden"));
    assert!(!report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("the source hides")));
    assert_eq!(document["timeline_groups"].as_array().unwrap().len(), 2);

    // Source visibility and retained-history visibility share the same path builder,
    // while their report counts retain their distinct meanings.
    model.assembly.as_mut().unwrap().placements[1].hidden = true;
    let (document, report) = convert(&model, &writer);
    assert_eq!(
        document["visibility"]["hidden_occurrences"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    for detail in [
        "1 component placement(s) the source hides are placed hidden",
        "2 editable-history component placement(s) are placed hidden",
    ] {
        assert!(report.losses().iter().any(|loss| loss.detail == detail));
    }
}

#[test]
fn partial_assembly_history_keeps_reference_sketches_without_a_body_prefix() {
    let mut model = assembly_of_histories();
    model.assembly.as_mut().unwrap().parts[1].ir = reference_sketch_history().ir;
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);
    let component = &document["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Second")
        .unwrap()["id"];
    let features: Vec<_> = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| &f["component"] == component)
        .collect();
    assert_eq!(
        features
            .iter()
            .map(|f| f["type"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["sketch", "sketch", "import"]
    );
    assert_eq!(features.last().unwrap()["suppressed"], false);
    let ids: Vec<_> = features
        .iter()
        .filter(|f| f["type"] == "sketch")
        .map(|f| f["id"].clone())
        .collect();
    assert_eq!(document["visibility"]["hidden_sketches"], json!(ids));
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.subject.contains("Second.ipt")
            && loss.detail.contains("kept 2 sketches for reference")));
    if let Some(cli) = solidcraft() {
        let (bytes, _) = written_with(&model, &writer);
        assert_eq!(volumes(&cli, &bytes), [1000.0, 1000.0, 1000.0]);
    }
}

#[test]
fn a_partial_component_is_hidden_through_repeated_subassemblies() {
    let mut inner = assembly_of_histories();
    inner.assembly.as_mut().unwrap().parts[1].ir = partial_history_model().ir;
    let mut model = assembly_of_histories();
    let assembly = model.assembly.as_mut().unwrap();
    assembly.parts = vec![inner];
    for placement in &mut assembly.placements {
        placement.part = 0;
    }
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);
    assert_eq!(report.outcome().unwrap().mode, Mode::PartialHistory);
    let hidden = document["visibility"]["hidden_occurrences"]
        .as_array()
        .unwrap();
    // Three subassembly placements, each with two placements of the partial part.
    assert_eq!(hidden.len(), 6);
    let occurrences = document["occurrences"].as_array().unwrap();
    let components = document["components"].as_array().unwrap();
    for path in hidden {
        let path = path.as_array().unwrap();
        assert_eq!(path.len(), 3);
        let mut parent = json!(0);
        for id in path {
            let occurrence = occurrences.iter().find(|o| &o["id"] == id).unwrap();
            assert_eq!(occurrence["parent"], parent);
            parent = occurrence["component"].clone();
        }
        assert_eq!(
            components.iter().find(|c| c["id"] == parent).unwrap()["name"],
            "Editable history (partial)"
        );
    }
}

#[test]
fn a_later_sketch_that_fails_replay_does_not_discard_the_valid_body_prefix() {
    let Some(cli) = solidcraft() else { return };
    for (bound, blocked) in [(true, true), (false, true), (false, false)] {
        let mut model = partial_history_model();
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        if !bound {
            wire["model"]["features"]
                .as_array_mut()
                .unwrap()
                .retain(|feature| feature["ordinal"] != 3);
        }
        if !blocked {
            wire["model"]["features"]
                .as_array_mut()
                .unwrap()
                .retain(|feature| feature["ordinal"].as_u64().unwrap() <= 1);
        }
        wire["model"]["parameters"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": "f3d:model:parameter#99", "name": "bad_dimension", "expression": "-5 mm", "value": {"kind": "length", "value": -5.0}, "ordinal": 0, "dependencies": [], "properties": {}}));
        wire["model"]["sketch_constraints"].as_array_mut().unwrap().push(json!({
        "id": "f3d:model:sketch-constraint#99", "sketch": TOP,
        "definition": {"kind": "distance", "entities": ["f3d:model:sketch-entity#11"], "parameter": "f3d:model:parameter#99"}
    }));
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::With(cli.clone())),
        );
        assert_partial(&document, &report, 2);
        assert_eq!(document["features"][1]["type"], "extrude");
        assert_eq!(document["features"][2]["sketch"]["constraints"], json!([]));
        if bound {
            assert!(
                report
                    .losses()
                    .iter()
                    .any(|loss| loss.subject.contains("Top sketch")
                        && loss.detail.contains("not carried after replay failed")),
                "{report:?}"
            );
        } else {
            let line = history_line(&report);
            if blocked {
                assert!(line.contains("native is not mapped yet"), "{line}");
            }
            assert!(line.contains("not carried after replay failed"), "{line}");
        }
    }
}

#[test]
fn partial_history_is_independent_of_the_native_source_format() {
    for extension in ["FCStd", "f3d", "ipt", "sldprt"] {
        let mut model = partial_history_model();
        model.source_file = Some(format!("cube.{extension}"));
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert_partial(&document, &report, 2);
        assert_eq!(
            document["features"].as_array().unwrap().last().unwrap()["file"],
            format!("cube.{extension}")
        );
        assert_eq!(document["features"][1]["type"], "extrude");
    }
}

/// CC0 recipe geometry with the named sketch binding restored in the neutral model.
/// cadmpeg currently exposes the spreadsheet and pad formulas but omits the named
/// sketch constraint expression. This isolates the writer's contract from that decoder gap.
fn recipe_with_neutral_sheet_binding(recipe: &str, sheet: &str, alias: &str) -> Model {
    let input = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "tests/fixtures/recipe-{recipe}-freecad-0.21.2.FCStd"
    ));
    let mut model = dutchman_core::read(
        dutchman_core::Format::FreeCadDocument,
        &input,
        &mut Report::default(),
    )
    .unwrap();
    let cell = model
        .ir
        .model
        .parameters
        .iter()
        .find(|p| p.name == alias)
        .unwrap()
        .id
        .clone();
    let dimension = model
        .ir
        .model
        .parameters
        .iter_mut()
        .find(|p| {
            p.properties
                .get("source_name")
                .is_some_and(|name| name == "width")
        })
        .unwrap();
    dimension.expression = format!("{sheet}.{alias}");
    dimension.dependencies = vec![cell].try_into().unwrap();
    model
}

#[test]
fn a_cc0_sheet_edit_drives_geometry_with_a_synthetic_neutral_sketch_binding() {
    let Some(cli) = solidcraft() else { return };
    let (recipe, sheet, alias, edit, expected) = ("N11a", "Dimensions", "w", "45 mm", 9000.0);
    let model = recipe_with_neutral_sheet_binding(recipe, sheet, alias);
    let (bytes, report) = written_with(
        &model,
        &SolidCraftWriter::new().verification(Verification::With(cli.clone())),
    );
    assert_eq!(
        report.outcome().unwrap().check,
        Check::Verified,
        "{recipe}: {:?}",
        report.losses()
    );
    assert_eq!(volumes(&cli, &bytes), [6000.0], "{recipe}");
    let actual = volumes(&cli, &with_param(&bytes, alias, edit));
    assert_eq!(actual.len(), 1, "{recipe}");
    assert!(
        (actual[0] - expected).abs() < 0.01,
        "{recipe}: {actual:?}, expected {expected}"
    );
}

#[test]
fn spreadsheet_function_expressions_evaluate_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // The sketch's side parameter drives two dimensions, so doubling it quadruples volume.
    let side = wire["model"]["parameters"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["name"] == "side")
        .unwrap();
    side["expression"] =
        json!("min(max(20 mm, abs(-30 mm)), sqrt(400) * 1 mm) * sin(30) + 0 * pi * 1 mm");
    side.as_object_mut().unwrap().remove("value");
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let bytes = written(&model).0;
    let initial = volumes(&cli, &bytes);
    assert_eq!(initial.len(), 1);
    assert!((initial[0] - 1000.0).abs() < 0.01, "{initial:?}");
    let grown = with_param(&bytes, "side", "max(20 mm, 1 in)");
    let actual = volumes(&cli, &grown);
    assert_eq!(actual.len(), 1);
    assert!((actual[0] - 25.4 * 25.4 * 10.0).abs() < 0.01, "{actual:?}");
}

#[test]
fn cc0_spreadsheet_formulas_keep_their_decoded_parameter_dependencies() {
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/recipe-P12a-freecad-0.21.2.FCStd");
    let model = dutchman_core::read(
        dutchman_core::Format::FreeCadDocument,
        &input,
        &mut Report::default(),
    )
    .unwrap();
    let (doc, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let params = doc["params"].as_array().unwrap();
    assert!(params
        .iter()
        .any(|p| p["name"] == "plate_w" && p["expr"] == "20 mm" && p["model"] == false));
    assert!(params
        .iter()
        .any(|p| p["name"] == "Length" && p["expr"] == "plate_w / 2"));
    assert!(!params.iter().any(|p| p["name"] == "A1"));
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.subject.starts_with("parameter ") && loss.severity != Severity::Info),
        "{:?}",
        report.losses()
    );
}

#[test]
fn sketch_choice_keeps_required_profiles_but_not_reference_sketches() {
    let writer = SolidCraftWriter::new()
        .sketches(false)
        .verification(Verification::Off);
    for before_blocker in [false, true] {
        let mut model = partial_history_model();
        if before_blocker {
            let mut wire = serde_json::to_value(&model.ir).unwrap();
            wire["model"]["features"][1]["ordinal"] = json!(2);
            wire["model"]["features"][2]["ordinal"] = json!(3);
            model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        }
        let (document, report) = convert(&model, &writer);
        assert_partial(&document, &report, 1);
        assert_eq!(
            types(&document),
            [
                ("sketch".to_owned(), false),
                ("extrude".to_owned(), false),
                ("import".to_owned(), false)
            ]
        );
    }
    for model in [loft_history(10.0, |_| {}), sweep_history(|_| {})] {
        let (document, _) = convert(&model, &writer);
        assert_eq!(
            document["features"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["type"] == "sketch")
                .count(),
            2,
            "section, profile and path sketches stay"
        );
    }
}

#[test]
fn a_face_extrude_reapplied_to_a_base_body_stored_as_the_design_ends_is_refused() {
    let model = face_extrude(true, |_| {});
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    let features = wire["model"]["features"].as_array_mut().unwrap();
    // The cube comes from a base feature holding the body as the design ends, and the face
    // extrude joins onto that same body.
    let first = features.iter_mut().find(|f| f["name"] == "Block").unwrap();
    first["definition"] = json!({"kind":"operation", "value": {"kind": "base_feature", "value": {"bodies": {"kind": "resolved", "value": {"bodies": [body.clone()], "native": "f3d:base#1"}}}}});
    first["name"] = json!("Base Feature 1");
    let extrude = features
        .iter_mut()
        .find(|f| f["name"] == "Face extrude")
        .unwrap();
    extrude["outputs"] = json!([body]);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("re-applied to a base body stored only as the design ends"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn face_extrudes_on_imported_history_bases_require_matching_results() {
    let Some(cli) = solidcraft() else { return };
    for (inward, checked) in [(false, true), (true, true), (true, false)] {
        let mut model = face_extrude(false, |definition| {
            definition["extent"]["side"]["termination"]["length"] = json!(3.0);
            if inward {
                definition["direction"] = json!({"kind": "reversed_profile_normal"});
            }
        });
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let body = wire["geometry"]["bodies"][0]["id"].clone();
        let base = &mut wire["model"]["features"][1];
        base["definition"] = json!({"kind": "operation", "value": {
            "kind": "base_feature", "value": {"bodies": {"kind": "resolved", "value": {
                "bodies": [body], "native": "synthetic:base#1"
            }}}
        }});
        let nests = serde_json::to_value(&based_cube_changed_later(true).ir).unwrap();
        wire["model"]["history_bodies"] = nests["model"]["history_bodies"].clone();
        wire["model"]["history_bodies"][0]["produced_by"] = json!("f3d:model:feature#2");
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let writer = SolidCraftWriter::new()
            .replay(Verification::With(cli.clone()))
            .verification(if checked {
                Verification::With(cli.clone())
            } else {
                Verification::Off
            });
        let (document, report) = convert(&model, &writer);
        let outcome = report
            .outcome()
            .unwrap()
            .features
            .iter()
            .find(|feature| feature.name.as_deref() == Some("Face extrude"))
            .unwrap();
        if inward && checked {
            assert_eq!(
                outcome.status,
                dutchman_core::FeatureStatus::Mapped,
                "{report:?}"
            );
        } else {
            assert_eq!(
                outcome.status,
                dutchman_core::FeatureStatus::Blocked,
                "{report:?}"
            );
            assert!(
                outcome.reason.as_deref().unwrap().contains(if checked {
                    "matching source result check"
                } else {
                    "verification turned off"
                }),
                "inward={inward} checked={checked}: {report:?}"
            );
            if checked {
                assert!(
                    outcome.reason.as_deref().unwrap().contains("1.500 mm"),
                    "{report:?}"
                );
            }
            assert!(!document["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|feature| { feature["name"] == "Face extrude" }));
        }
        assert!(document["features"]
            .as_array()
            .unwrap()
            .iter()
            .all(|feature| { feature.get("_base_face_result").is_none() }));
    }
}

/// The example cube (10 mm, built by the base extrude) with one more extrude that takes its
/// top (`up`) or bottom face as it was before that extrude: a historical face, known only by
/// the geometry of the edges around it. `change` edits the extrude's definition.
fn face_extrude(up: bool, change: impl FnOnce(&mut Value)) -> Model {
    let model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let z = if up { 10.0 } else { 0.0 };
    let point = |x: f64, y: f64| json!({"x": x, "y": y, "z": z});
    let state = "test:face:feature-input#3";
    let face = "test:face:historical-face#target";
    let side = |k: usize| format!("test:face:historical-face#side{k}");
    let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let geometry: Vec<Value> = (0..4)
        .map(|k| {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let tangent = [(b.0 - a.0) / 10.0, (b.1 - a.1) / 10.0];
            json!({
                "edge": format!("test:face:historical-edge#{k}"),
                "interior": point((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
                "start": point(a.0, a.1), "end": point(b.0, b.1),
                "tangent": {"x": tangent[0], "y": tangent[1], "z": 0.0},
                "faces": [face, side(k)],
            })
        })
        .collect();
    let edges: Vec<String> = (0..4)
        .map(|k| format!("test:face:historical-edge#{k}"))
        .collect();
    let mut faces: Vec<String> = (0..4).map(side).collect();
    faces.push(face.to_owned());
    wire["model"]["feature_input_topologies"] = json!([{"id": state, "input_of": "f3d:model:feature#3", "edges": edges, "faces": faces, "edge_geometry": geometry, "bodies": [], "vertices": []}]);
    let mut definition = json!({"kind": "extrude", "value": {"profile": {"kind":"planar", "value": {"kind": "historical_faces", "value": {
            "state": state, "faces": [face], "native": ["synthetic profile group"],
        }}}, "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"}, "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 5.0}}}, "op": "join"}});
    let mut payload = definition["value"].take();
    change(&mut payload);
    if payload.get("kind").is_some() {
        definition = payload;
    } else {
        definition["value"] = payload;
    }
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Face extrude", "suppressed": false, "dependencies": ["f3d:model:feature#2"], "definition": {"kind": "operation", "value": definition}, "source_properties": {}, "source_content": [], "outputs": []}));
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

/// The example cube with a delete-face feature whose face is the top of the cube as it was
/// before the feature, or a native selection.
fn face_delete(selection: impl FnOnce(&str, &str) -> Value, heal: bool) -> Model {
    face_extrude(true, |definition| {
        *definition = json!({"kind": "delete_face", "value": {"faces": selection("test:face:feature-input#3", "test:face:historical-face#target"), "heal": heal}});
    })
}

#[test]
fn a_historical_face_is_deleted_by_a_point_inside_it() {
    for heal in [true, false] {
        let model = face_delete(
            |state, face| {
                json!({"kind": "historical", "value": {
                    "state": state, "faces": [face], "native": "synthetic selection"}})
            },
            heal,
        );
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let feature = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "delete_face")
            .unwrap_or_else(|| panic!("{}", history_line(&report)));
        assert_eq!(feature["heal"], json!(heal));
        let points = feature["faces"].as_array().unwrap();
        assert_eq!(points.len(), 1);
        let p: Vec<f64> = points[0]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        assert!((p[2] - 10.0).abs() < 1e-9, "{p:?}");
        assert!(
            p[0] > 1.0 && p[0] < 9.0 && p[1] > 1.0 && p[1] < 9.0,
            "a point well inside the top face: {p:?}"
        );
    }
}

fn face_move(motion: Value) -> Model {
    face_extrude(true, |definition| {
        *definition = json!({"kind": "move_face", "value": {"faces": {"kind": "historical", "value": {
                "state": "test:face:feature-input#3",
                "faces": ["test:face:historical-face#target"], "native": "synthetic selection"}}, "motion": motion}});
    })
}

#[test]
fn a_face_moved_along_its_normal_becomes_an_offset_face_at_a_point_inside_it() {
    let model = face_move(json!({"kind": "offset", "distance": 2.5}));
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let feature = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "offset_face")
        .unwrap_or_else(|| panic!("{}", history_line(&report)));
    assert_eq!(feature["distance"], json!("2.5 mm"));
    let points = feature["faces"].as_array().unwrap();
    assert_eq!(points.len(), 1);
    let p: Vec<f64> = points[0]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert!((p[2] - 10.0).abs() < 1e-9, "{p:?}");
    assert!(
        p[0] > 1.0 && p[0] < 9.0 && p[1] > 1.0 && p[1] < 9.0,
        "{p:?}"
    );
}

#[test]
fn offset_face_body_binding_preserves_older_targets() {
    let model = face_move(json!({"kind": "offset", "distance": 2.0}));
    for (target, bound) in [("0.18.2", false), ("0.44.1", false), ("0.45.0", true)] {
        let (doc, report) = convert(
            &model,
            &SolidCraftWriter::new()
                .target_version(Some(target.parse().unwrap()))
                .verification(Verification::Off),
        );
        let features = doc["features"].as_array().unwrap();
        let offset = features
            .iter()
            .find(|f| f["type"] == "offset_face")
            .unwrap_or_else(|| panic!("{}", history_line(&report)));
        assert_eq!(offset.get("body").is_some(), bound, "{target}: {offset}");
        if bound {
            let extrude = features.iter().find(|f| f["type"] == "extrude").unwrap();
            assert_eq!(offset["body"], extrude["body_names"][0]);
        }
    }
}

#[test]
fn offset_faces_refuse_input_topology_without_unique_body_ownership() {
    let mut model = face_move(json!({"kind": "offset", "distance": 2.0}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["feature_input_topologies"][0]["bodies"] = json!([
        "test:face:historical-body#one",
        "test:face:historical-body#two"
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (doc, report) = convert(
        &model,
        &SolidCraftWriter::new()
            .target_version(Some("0.45.0".parse().unwrap()))
            .verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("several bodies without face ownership"),
        "{}",
        history_line(&report)
    );
    assert!(!doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "offset_face"));
}

#[test]
fn solidcraft_offsets_coincident_local_faces_on_their_own_bodies() {
    let Some(cli) = solidcraft() else { return };
    let mut model = assembly_of_histories();
    let first = face_move(json!({"kind": "offset", "distance": 2.0}));
    let mut second = face_move(json!({"kind": "offset", "distance": 1.0}));
    let mut wire = serde_json::to_value(&second.ir).unwrap();
    // The earlier body's offset top and the later body's input top both lie
    // at z=12 in their components' local frames. Placements stay separate.
    wire["model"]["features"][1]["definition"]["value"]["value"]["extent"]["side"]["termination"]
        ["length"] = json!(12.0);
    wire["model"]["parameters"][0]["expression"] = json!("12 mm");
    wire["model"]["parameters"][0]["value"]["value"] = json!(12.0);
    for edge in wire["model"]["feature_input_topologies"][0]["edge_geometry"]
        .as_array_mut()
        .unwrap()
    {
        for key in ["start", "interior", "end"] {
            edge[key]["z"] = json!(12.0);
        }
    }
    second.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let assembly = model.assembly.as_mut().unwrap();
    assembly.parts = vec![
        Model {
            name: "First".into(),
            source_file: Some("First.f3d".into()),
            ..first
        },
        Model {
            name: "Second".into(),
            source_file: Some("Second.f3d".into()),
            ..second
        },
    ];
    assembly.placements.truncate(2);
    let (bytes, report) = written_with(
        &model,
        &replaying(&cli).target_version(Some("0.45.0".parse().unwrap())),
    );
    assert!(
        history_line(&report).contains("features rebuilt"),
        "{}",
        history_line(&report)
    );
    let mut actual = volumes(&cli, &bytes);
    actual.sort_by(f64::total_cmp);
    assert_eq!(actual.len(), 2);
    for (got, expected) in actual.iter().zip([1200.0, 1300.0]) {
        assert!((got - expected).abs() < 1e-3, "{actual:?}");
    }
}

#[test]
fn solidcraft_offsets_an_annular_face_from_its_interior() {
    let Some(cli) = solidcraft() else { return };
    let mut model = nested_native_profile(&[10.0, 4.0]);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let template =
        serde_json::to_value(&face_move(json!({"kind": "offset", "distance": 2.0})).ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(template["model"]["features"][2].clone());
    let mut topology = template["model"]["feature_input_topologies"][0].clone();
    let face = "test:face:historical-face#target";
    topology["edges"] = json!([
        "test:face:historical-edge#outer",
        "test:face:historical-edge#inner"
    ]);
    topology["faces"] = json!([
        face,
        "test:face:historical-face#outer",
        "test:face:historical-face#inner"
    ]);
    topology["edge_geometry"] = json!([10.0, 4.0]
        .into_iter()
        .enumerate()
        .map(|(k, r)| {
            let side = if k == 0 { "outer" } else { "inner" };
            json!({"edge": format!("test:face:historical-edge#{side}"),
            "start": {"x": r, "y": 0.0, "z": 10.0}, "end": {"x": r, "y": 0.0, "z": 10.0},
            "interior": {"x": -r, "y": 0.0, "z": 10.0}, "tangent": {"x": 0.0, "y": 1.0, "z": 0.0},
            "faces": [face, format!("test:face:historical-face#{side}")]})
        })
        .collect::<Vec<_>>());
    wire["model"]["feature_input_topologies"] = json!([topology]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let writer = replaying(&cli).target_version(Some("0.45.0".parse().unwrap()));
    let (doc, report) = convert(&model, &writer);
    let offset = doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "offset_face")
        .unwrap_or_else(|| panic!("{}", history_line(&report)));
    let p = offset["faces"][0].as_array().unwrap();
    let radius = p[0].as_f64().unwrap().hypot(p[1].as_f64().unwrap());
    assert!(radius > 6.0 && radius < 8.0, "{p:?}");
    let actual = volumes(&cli, &written_with(&model, &writer).0);
    let expected = std::f64::consts::PI * 84.0 * 12.0;
    assert_eq!(actual.len(), 1);
    assert!(
        (actual[0] - expected).abs() / expected < dutchman_solidcraft::VOLUME_TOLERANCE,
        "{actual:?}"
    );
}

#[test]
fn rotated_and_slid_faces_are_refused_for_their_motion() {
    let rotate = json!({"kind": "rotate",
        "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
        "axis_dir": {"x": 1.0, "y": 0.0, "z": 0.0}, "angle": 0.5});
    let slide = json!({"kind": "translate",
        "direction": {"x": 0.0, "y": 0.0, "z": 1.0}, "distance": 2.0});
    for (motion, reason) in [
        (rotate, "rotating faces is not supported yet"),
        (
            slide,
            "translating faces along a direction is not supported yet",
        ),
    ] {
        let (_, report) = convert(
            &face_move(motion),
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn a_native_face_deletion_is_refused_for_its_selection() {
    let model = face_delete(|_, _| json!({"kind": "native", "value": "synthetic"}), true);
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("a native face reference is not supported yet"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn a_historical_face_extrude_without_a_replay_waits_for_one() {
    let (_, report) = convert(
        &face_extrude(true, |_| {}),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("replay of the history before it"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn a_historical_face_without_edge_geometry_is_refused_for_that_reason() {
    let model = face_extrude(true, |_| {});
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["feature_input_topologies"][0]["edge_geometry"] = json!([]);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("carries no edge geometry for the selected face"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn solidcraft_extrudes_a_historical_face_along_its_outward_normal() {
    let Some(cli) = solidcraft() else { return };
    let join = |model: Model| {
        let (bytes, report) = written_with(&model, &replaying(&cli));
        assert!(
            history_line(&report).starts_with("3 of 3 features rebuilt"),
            "{}",
            history_line(&report)
        );
        volumes(&cli, &bytes)
    };
    // Out of the top, and out of the bottom: the sketch is on the outward side either way.
    for up in [true, false] {
        let volume = join(face_extrude(up, |_| {}));
        assert_eq!(volume.len(), 1);
        assert!((volume[0] - 1500.0).abs() < 1e-3, "{up}: {volume:?}");
    }
    // Against the profile normal, a cut goes into the cube.
    for up in [true, false] {
        let volume = join(face_extrude(up, |definition| {
            definition["op"] = json!("cut");
            definition["direction"] = json!({"kind": "reversed_profile_normal"});
            definition["extent"]["side"]["termination"]["length"] = json!(3.0);
        }));
        assert!((volume[0] - 700.0).abs() < 1e-3, "{up}: {volume:?}");
    }
}

/// `face_extrude`, then a chamfer of an edge of the extruded block's end face: the one along X
/// at Y = 0, which no face of the cube can merge with.
fn face_extrude_then_chamfer(up: bool) -> Model {
    let model = face_extrude(up, |_| {});
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let z = if up { 15.0 } else { -5.0 };
    let state = "test:face:feature-input#4";
    wire["model"]["feature_input_topologies"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": state, "input_of": "f3d:model:feature#4", "edges": ["corner"], "faces": ["front", "end"], "edge_geometry": [{
                "edge": "corner", "interior": {"x": 5.0, "y": 0.0, "z": z},
                "tangent": {"x": 1.0, "y": 0.0, "z": 0.0},
                "start": {"x": 0.0, "y": 0.0, "z": z}, "end": {"x": 10.0, "y": 0.0, "z": z},
                "faces": ["front", "end"],
            }], "bodies": [], "vertices": []}));
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#4", "ordinal": 3, "name": "Corner chamfer", "suppressed": false, "dependencies": ["f3d:model:feature#3"], "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": {"kind": "chamfer", "value": {"groups": [{
                "edges": {"kind": "historical", "value": {
                    "state": state, "edges": ["corner"], "native": "synthetic input edge",
                }},
                "spec": {"kind": "distance", "distance": 1.0},
            }], "flip_direction": false}}}, "source_properties": {}, "source_content": []}));
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn a_blend_after_a_historical_face_extrude_sees_the_extrude_on_its_outward_side() {
    let Some(cli) = solidcraft() else { return };
    // Out of the top and out of the bottom: one of the two face sketches has to be turned to
    // the outward side, and the chamfer's edge exists only once it is.
    for up in [true, false] {
        let (bytes, report) = written_with(&face_extrude_then_chamfer(up), &replaying(&cli));
        assert!(
            history_line(&report).starts_with("4 of 4 features rebuilt"),
            "{up}: {}",
            history_line(&report)
        );
        let volume = volumes(&cli, &bytes);
        assert_eq!(volume.len(), 1);
        // The cube and the block, less the 1 mm chamfer along the block's 10 mm edge.
        assert!((volume[0] - 1495.0).abs() < 1e-3, "{up}: {volume:?}");
    }
}

#[test]
fn sketch_entity_distances_keep_expressions_driving_geometry() {
    for operands in [
        json!(["f3d:model:sketch-entity#1", "f3d:model:sketch-entity#3"]),
        json!(["f3d:model:sketch-entity#5", "f3d:model:sketch-entity#3"]),
        json!(["f3d:model:sketch-entity#3", "f3d:model:sketch-entity#5"]),
        json!(["f3d:model:sketch-entity#5", "f3d:model:sketch-entity#6"]),
        json!(["f3d:model:sketch-entity#7", "f3d:model:sketch-entity#5"]),
    ] {
        let mut model = cube_with_history(10.0, first_profile(), None);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let m = &mut wire["model"];
        m["sketch_entities"].as_array_mut().unwrap().extend([
            entity(
                5,
                json!({"kind": "point", "position": {"u": 0.0, "v": 0.0}}),
            ),
            entity(
                6,
                json!({"kind": "point", "position": {"u": 10.0, "v": 0.0}}),
            ),
        ]);
        let mut carrier = line(7, [0.0, 20.0], [0.0, 10.0]);
        carrier["construction"] = json!(true);
        m["sketch_entities"].as_array_mut().unwrap().push(carrier);
        m["sketch_constraints"][4]["definition"]["entities"] = operands;
        m["parameters"][1]["expression"] = json!("Width / 2");
        m["parameters"][1]["dependencies"] = json!(["f3d:model:parameter#3"]);
        m["parameters"].as_array_mut().unwrap().push(json!({"id": "f3d:model:parameter#3", "name": "Width", "expression": "20 mm", "value": {"kind": "length", "value": 20.0}, "ordinal": 0, "dependencies": [], "properties": {}}));
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, _) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(document["params"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "side" && p["expr"] == "Width / 2"));
        assert!(document["features"][0]["sketch"]["constraints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["param"] == "side"));
        if let Some(cli) = solidcraft() {
            let bytes = written(&model).0;
            assert!((volumes(&cli, &bytes)[0] - 1000.0).abs() < 0.01);
            let edited = volumes(&cli, &with_param(&bytes, "Width", "30 mm"));
            assert!((edited[0] - 2250.0).abs() < 0.01, "{edited:?}");
        }
    }
}

/// `cube_with_history` with the extrude's own parameter `d1` (an AlongDistance, as the source
/// kind names it) reading a user parameter `Height`.
fn cube_with_height_parameter(sign: f64) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let parameters = wire["model"]["parameters"].as_array_mut().unwrap();
    parameters[0]["expression"] = json!("Height");
    parameters[0]["value"]["value"] = json!(sign * 10.0);
    parameters[0]["dependencies"] = json!(["f3d:model:parameter#3"]);
    parameters[0]["properties"] = json!({"source_kind": "AlongDistance"});
    parameters.push(json!({"id": "f3d:model:parameter#3", "name": "Height", "expression": "10 mm", "value": {"kind": "length", "value": 10.0}, "ordinal": 0, "dependencies": [], "properties": {}}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn an_extrude_distance_follows_the_parameter_that_sets_it() {
    for sign in [1.0, -1.0] {
        let model = cube_with_height_parameter(sign);
        let (document, _) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let distance = document["features"][1]["extent"]["distance"]
            .as_str()
            .unwrap();
        assert_eq!(distance, if sign > 0.0 { "d1" } else { "abs(d1)" });
        if let Some(cli) = solidcraft() {
            let bytes = written(&model).0;
            assert!((volumes(&cli, &bytes)[0] - 1000.0).abs() < 0.01);
            let edited = volumes(&cli, &with_param(&bytes, "Height", "20 mm"));
            assert!((edited[0] - 2000.0).abs() < 0.01, "{edited:?}");
        }
    }
}

#[test]
fn an_extrude_distance_keeps_its_value_when_no_single_parameter_matches() {
    let mut model = cube_with_height_parameter(1.0);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["parameters"][0]["value"]["value"] = json!(7.0);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(document["features"][1]["extent"]["distance"], "10 mm");
}

#[test]
fn a_partial_revolve_angle_follows_the_parameter_that_sets_it() {
    let mut model = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2}}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["parameters"] = json!([
        {"id": "f3d:model:parameter#1", "owner": "f3d:model:feature#2", "name": "d1", "expression": "Turn", "dependencies": ["f3d:model:parameter#2"], "value": {"kind": "angle", "value": std::f64::consts::FRAC_PI_2}, "properties": {"source_kind": "AlongAngle"}, "ordinal": 0},
        {"id": "f3d:model:parameter#2", "name": "Turn", "expression": "90 deg", "value": {"kind": "angle", "value": std::f64::consts::FRAC_PI_2}, "ordinal": 0, "dependencies": [], "properties": {}},
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let angle = document["features"][1]["angle"].as_str().unwrap();
    assert!(angle == "d1" || angle == "-(d1)", "{angle}");
    if let Some(cli) = solidcraft() {
        let bytes = written(&model).0;
        let ring = std::f64::consts::PI * (10.0_f64.powi(2) - 5.0_f64.powi(2)) * 10.0;
        let quarter = volumes(&cli, &bytes)[0];
        assert!((quarter - ring / 4.0).abs() / ring < 1e-3, "{quarter}");
        let half = volumes(&cli, &with_param(&bytes, "Turn", "180 deg"))[0];
        assert!((half - ring / 2.0).abs() / ring < 1e-3, "{half}");
    }
}

#[test]
fn selecting_nested_loops_leaves_the_inner_one_a_hole() {
    let mut model = nested_native_profile(&[5.0, 3.0]);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["definition"]["value"]["value"]["profile"]["value"] =
        json!({"kind": "sketch_profiles", "value": {"sketch": SKETCH, "profiles": [1, 0]}});
    // End where the synthetic final cube has no face: this test is about the written profile,
    // not the end-cap comparison.
    wire["model"]["features"][1]["definition"]["value"]["value"]["extent"]["side"]["termination"]
        ["length"] = json!(5.0);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(
        document["features"][1]["profiles"],
        json!({"type": "whole_boundaries", "profiles": [{"outer": ["c1"], "holes": [["c2"]]}]})
    );
    if let Some(cli) = solidcraft() {
        let volume = volumes(&cli, &written(&model).0);
        let expected = std::f64::consts::PI * 16.0 * 5.0;
        assert!((volume[0] - expected).abs() / expected < 1e-3, "{volume:?}");
    }
}

#[test]
fn a_loose_corner_is_held_so_a_parameter_edit_moves_only_what_it_drives() {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // The square is drawn away from the origin and nothing pins where: its corner floats.
    for entity in wire["model"]["sketch_entities"].as_array_mut().unwrap() {
        for end in ["start", "end"] {
            for axis in ["u", "v"] {
                let at = entity["geometry"][end][axis].as_f64().unwrap();
                entity["geometry"][end][axis] = json!(at + 5.0);
            }
        }
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let constraints = document["features"][0]["sketch"]["constraints"]
        .as_array()
        .unwrap();
    let holds: Vec<&Value> = constraints
        .iter()
        .filter(|c| {
            matches!(c["type"].as_str(), Some("distance_x" | "distance_y"))
                && c.get("param").is_none()
        })
        .collect();
    assert_eq!(holds.len(), 2, "{constraints:?}");
    assert!(holds.iter().all(|c| c["value"].as_f64().unwrap() >= 5.0));
    if let Some(cli) = solidcraft() {
        let bytes = written(&model).0;
        let edited = volumes(&cli, &with_param(&bytes, "side", "20 mm"));
        assert!((edited[0] - 4000.0).abs() < 0.01, "{edited:?}");
    }
}

/// A block, profiled on XZ and extruded, for a sketch on its end: the profile is 10 wide and
/// `Height` tall with its top left corner tied to the sketch origin by a point entity there.
fn block_with_height_parameter() -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketches"][0]["placement"]["frame"]["normal"] = json!({"x": 0.0, "y": 1.0, "z": 0.0});
    // On XZ, sketch v runs along -Z.
    m["sketch_entities"] = json!([
        line(1, [0.0, 0.0], [10.0, 0.0]),
        line(2, [10.0, 0.0], [10.0, -10.0]),
        line(3, [10.0, -10.0], [0.0, -10.0]),
        line(4, [0.0, -10.0], [0.0, 0.0]),
        entity(
            11,
            json!({"kind": "point", "position": {"u": 0.0, "v": 0.0}})
        ),
    ]);
    let constraint = |key: u32, definition: Value| {
        json!({"id": format!("f3d:model:sketch-constraint#{key}"), "sketch": SKETCH,
               "definition": definition})
    };
    let at = |key: u32| format!("f3d:model:sketch-entity#{key}");
    let locus = |kind: &str, key: u32| json!({"kind": kind, "value": at(key)});
    m["sketch_constraints"] = json!([
        constraint(1, json!({"kind": "horizontal", "entity": at(1)})),
        constraint(2, json!({"kind": "vertical", "entity": at(2)})),
        constraint(3, json!({"kind": "horizontal", "entity": at(3)})),
        constraint(4, json!({"kind": "vertical", "entity": at(4)})),
        constraint(
            5,
            json!({"kind": "distance", "entities": [at(1), at(3)],
            "parameter": "f3d:model:parameter#2"})
        ),
        constraint(
            6,
            json!({"kind": "coincident_loci",
            "loci": [locus("start", 1), locus("entity", 11)]})
        ),
        constraint(
            7,
            json!({"kind": "coincident_loci",
            "loci": [locus("end", 4), locus("entity", 11)]})
        ),
    ]);
    m["parameters"] = json!([
        {"id": "f3d:model:parameter#2", "owner": "f3d:model:feature#1", "ordinal": 0, "name": "d2", "expression": "Height", "dependencies": ["f3d:model:parameter#3"], "value": {"kind": "length", "value": 10.0}, "properties": {}},
        {"id": "f3d:model:parameter#3", "name": "Height", "expression": "10 mm", "value": {"kind": "length", "value": 10.0}, "ordinal": 0, "dependencies": [], "properties": {}},
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_sketch_on_the_end_of_a_feature_moves_with_the_dimensions_that_place_it() {
    let mut model = block_with_height_parameter();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    let pocket = "f3d:model:sketch#2";
    // The pocket sketch lies at z = Height, drawn with its axes turned half a turn.
    m["sketches"].as_array_mut().unwrap().push(json!({
        "id": pocket, "name": "Pocket",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 10.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": -1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [[{"entity": "f3d:model:sketch-entity#10", "reversed": false}]],
    }));
    let mut circle = entity(
        10,
        json!({"kind": "circle", "center": {"u": -4.0, "v": -3.0}, "radius": 1.0}),
    );
    circle["sketch"] = json!(pocket);
    m["sketch_entities"].as_array_mut().unwrap().push(circle);
    m["features"].as_array_mut().unwrap().push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Pocket sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": pocket}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    m["features"].as_array_mut().unwrap().push(json!({"id": "f3d:model:feature#4", "ordinal": 3, "name": "Pocket", "suppressed": false, "dependencies": ["f3d:model:feature#2", "f3d:model:feature#3"], "definition": {"kind": "operation", "value": {"kind": "extrude", "value": {"profile": {"kind":"planar", "value": {"kind": "sketch_profiles", "value": {"sketch": pocket, "profiles": [0]}}}, "direction": {"kind": "reversed_profile_normal"}, "start": {"kind": "profile_plane"}, "extent": {"kind": "one_sided",
                "side": {"termination": {"kind": "blind", "length": 2.0}}}, "op": "cut"}}}, "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let sketch = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Pocket sketch")
        .unwrap();
    assert_eq!(sketch["plane"]["type"], "offset", "{sketch}");
    assert_eq!(sketch["plane"]["base"]["name"], "XY");
    let distance = sketch["plane"]["distance"].as_str().unwrap();
    assert!(distance.contains("d2"), "{distance}");
    // Turned half a turn, the circle's centre keeps its place on the model.
    assert_eq!(sketch["sketch"]["points"][1]["pos"], json!([4.0, 3.0]));
    if let Some(cli) = solidcraft() {
        // Block 10 x 10 x 10 less the 2 mm deep pocket of radius 1: and the pocket still
        // opens on the top face after the block is made taller.
        let pocket_volume = std::f64::consts::PI * 2.0;
        let bytes = written(&model).0;
        let first = volumes(&cli, &bytes);
        assert!(
            (first[0] - (1000.0 - pocket_volume)).abs() < 0.05,
            "{first:?}"
        );
        let taller = volumes(&cli, &with_param(&bytes, "Height", "20 mm"));
        assert!(
            (taller[0] - (2000.0 - pocket_volume)).abs() < 0.05,
            "{taller:?}"
        );
    }
}

#[test]
fn an_empty_base_region_does_not_consume_an_import_name() {
    let mut model = based_cube();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    let empty = json!({"id": "test:model:region#empty", "body": body, "shells": []});
    wire["geometry"]["regions"]
        .as_array_mut()
        .unwrap()
        .insert(0, empty);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(
        document["features"][0]["name"],
        "Base Feature 1",
        "{}",
        history_line(&report)
    );
    assert_eq!(
        document["features"][0]["body_names"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(document["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "hole"));
    assert!(!history_line(&report).contains("appearance targets"));
}

#[test]
fn an_omitted_base_region_does_not_discard_history_or_shift_body_colours() {
    let mut model = based_cube();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    // A nonempty region whose shell is unavailable: the exporter omits it.
    wire["geometry"]["regions"].as_array_mut().unwrap().insert(0,
        json!({"id": "test:model:region#omitted", "body": body, "shells": ["test:model:shell#missing"]}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let base = &document["features"][0];
    assert_eq!(base["name"], "Base Feature 1", "{}", history_line(&report));
    assert!(base.get("body_names").is_none());
    assert!(document["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "hole"));
}

#[test]
fn an_offset_datum_with_independent_sketch_consumers_does_not_stop_history() {
    let model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for feature in wire["model"]["features"].as_array_mut().unwrap() {
        feature["ordinal"] = json!(feature["ordinal"].as_u64().unwrap() + 1);
    }
    wire["model"]["features"][0]["dependencies"] = json!(["test:model:feature#offset"]);
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "test:model:feature#offset", "ordinal": 0, "name": "Offset datum", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "datum_offset_plane", "value": {"distance": -7.0}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    let changed = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (document, report) = convert(
        &changed,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let outcome = report.outcome().unwrap();
    assert_eq!(
        outcome.features[0].status,
        dutchman_core::FeatureStatus::NotNeeded,
        "{}",
        history_line(&report)
    );
    assert!(document["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "extrude"));
    let mut unresolved = serde_json::to_value(&changed.ir).unwrap();
    unresolved["model"]["sketches"][0]["placement"] = json!({"kind": "unresolved"});
    let changed = Model {
        ir: cadmpeg_ir::CadIr::from_json(&unresolved.to_string()).unwrap(),
        ..changed
    };
    let (_, report) = convert(
        &changed,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(
        report.outcome().unwrap().features[0].status,
        dutchman_core::FeatureStatus::Blocked
    );
    assert!(
        history_line(&report).contains("offset plane reference is not resolved"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn a_body_split_uses_the_signed_offset_datum_frame() {
    let model = split_cube(0.0, false, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][2]["definition"] = json!({"kind":"operation", "value": {"kind": "datum_offset_plane", "value": {"reference": {"kind": "resolved_plane", "frame": {
            "origin": {"x": 9.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 2.0, "y": 0.0, "z": 0.0},
            "u_axis": {"x": 0.0, "y": 3.0, "z": 0.0}}}, "distance": -5.0}}});
    let changed = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (document, report) = convert(
        &changed,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let split = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "split")
        .unwrap_or_else(|| panic!("{}", history_line(&report)));
    assert_eq!(split["plane"]["plane"]["origin"], json!([4.0, 0.0, 0.0]));
    if let Some(cli) = solidcraft() {
        let mut got = volumes(
            &cli,
            &written_with(
                &changed,
                &SolidCraftWriter::new().verification(Verification::Off),
            )
            .0,
        );
        got.sort_by(f64::total_cmp);
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(
            (got[0] - 400.0).abs() < 1.0 && (got[1] - 600.0).abs() < 1.0,
            "{got:?}"
        );
    }
}

fn historical_shell(outward: bool) -> Model {
    face_extrude(true, |definition| {
        *definition = json!({"kind": "shell", "value": {"removed_faces": {
            "kind": "historical", "value": {"state": "test:face:feature-input#3",
                "faces": ["test:face:historical-face#target"], "native": "synthetic selection"}}, "thickness": 1.0, "outward": outward}});
    })
}

#[test]
fn shells_map_historical_input_faces_and_signed_direction() {
    for outward in [false, true] {
        let model = historical_shell(outward);
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let shell = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "shell")
            .unwrap_or_else(|| panic!("{}", history_line(&report)));
        assert_eq!(
            shell["direction"],
            if outward { "outside" } else { "inside" }
        );
        assert_eq!(shell["thickness"], "1 mm");
        assert_eq!(shell["faces"].as_array().unwrap().len(), 1);
        assert_eq!(shell["faces"][0][2], 10.0);
        assert!(shell["body"].is_string());
        if let Some(cli) = solidcraft() {
            let got = volumes(
                &cli,
                &written_with(
                    &model,
                    &SolidCraftWriter::new().verification(Verification::Off),
                )
                .0,
            );
            let expected = if outward {
                12.0 * 12.0 * 11.0 - 1000.0
            } else {
                1000.0 - 8.0 * 8.0 * 9.0
            };
            assert_eq!(got.len(), 1, "{got:?}");
            assert!((got[0] - expected).abs() < 1.0, "{got:?} vs {expected}");
        }
    }
}

#[test]
fn shells_require_their_own_historical_input_faces() {
    let model = historical_shell(false);
    let original = serde_json::to_value(&model.ir).unwrap();
    for missing_geometry in [true, false] {
        let mut wire = original.clone();
        if missing_geometry {
            wire["model"]["feature_input_topologies"][0]["edge_geometry"] = json!([]);
        } else {
            wire["model"]["feature_input_topologies"][0]["input_of"] = json!("f3d:model:feature#2");
        }
        let changed = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model.clone()
        };
        let (document, report) = convert(
            &changed,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(!document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["type"] == "shell"));
        assert!(
            history_line(&report).contains(if missing_geometry {
                "no edge geometry"
            } else {
                "does not uniquely belong"
            }),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn a_body_the_source_leaves_unnamed_gets_a_name_later_features_can_target() {
    let model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["geometry"]["bodies"][0]
        .as_object_mut()
        .unwrap()
        .remove("name");
    let changed = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (document, report) = convert(
        &changed,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let extrude = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "extrude")
        .unwrap_or_else(|| panic!("{}", history_line(&report)));
    assert_eq!(
        extrude["body_names"],
        json!(["Body 1"]),
        "{}",
        history_line(&report)
    );
}

#[test]
fn a_join_before_any_body_makes_and_names_the_first_body() {
    let model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["geometry"]["bodies"][0]
        .as_object_mut()
        .unwrap()
        .remove("name");
    for feature in wire["model"]["features"].as_array_mut().unwrap() {
        if feature["definition"]["value"]["value"]["op"] == "new_body" {
            feature["definition"]["value"]["value"]["op"] = json!("join");
        }
    }
    let changed = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    };
    let (document, report) = convert(
        &changed,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let extrude = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "extrude")
        .unwrap_or_else(|| panic!("{}", history_line(&report)));
    assert_eq!(
        extrude["operation"],
        "new_body",
        "{}",
        history_line(&report)
    );
    assert_eq!(
        extrude["body_names"],
        json!(["Body 1"]),
        "{}",
        history_line(&report)
    );
}

#[test]
fn through_all_cuts_require_a_body_before_the_cut_in_ordinal_order() {
    const CUT_SKETCH: &str = "test:model:sketch#cut";
    for with_boss in [false, true] {
        let model = cube_with_history(10.0, whole_sketch(), None);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let mut cut = wire["model"]["features"][1].clone();
        cut["id"] = json!("test:model:feature#cut");
        cut["name"] = json!("Through cut");
        cut["ordinal"] = json!(3);
        cut["outputs"] = json!([]);
        cut["dependencies"] = if with_boss {
            json!(["test:model:feature#cut-sketch", "f3d:model:feature#2"])
        } else {
            json!(["test:model:feature#cut-sketch"])
        };
        cut["definition"]["value"]["value"]["op"] = json!("cut");
        cut["definition"]["value"]["value"]["extent"] =
            json!({"kind": "symmetric", "side": {"termination": {"kind": "through_all"}}});
        cut["definition"]["value"]["value"]["profile"]["value"] =
            json!({"kind": "sketch", "value": CUT_SKETCH});
        let mut circle = entity(
            5,
            json!({"kind": "circle", "center": {"u": 5.0, "v": 5.0}, "radius": 1.0}),
        );
        circle["sketch"] = json!(CUT_SKETCH);
        wire["model"]["sketch_entities"]
            .as_array_mut()
            .unwrap()
            .push(circle);
        let placement = wire["model"]["sketches"][0]["placement"].clone();
        wire["model"]["sketches"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id": CUT_SKETCH,
            "name": "Cut sketch", "placement": placement,
            "profiles": [[{"entity": "f3d:model:sketch-entity#5", "reversed": false}]]}));
        let features = wire["model"]["features"].as_array_mut().unwrap();
        if !with_boss {
            features.retain(|f| f["ordinal"] == 0);
        }
        features.push(json!({"id": "test:model:feature#cut-sketch", "ordinal": 2, "name": "Cut sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": CUT_SKETCH}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
        features.push(cut);
        features.reverse();
        let changed = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model
        };
        let (document, report) = convert(
            &changed,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        let cuts: Vec<_> = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["operation"] == "cut")
            .collect();
        if with_boss {
            assert_eq!(cuts.len(), 1, "{}", history_line(&report));
            let features = document["features"].as_array().unwrap();
            assert_eq!(features[1]["name"], "Block");
            assert_eq!(features[2]["name"], "Cut sketch");
            assert_eq!(features[3]["name"], "Through cut");
            if let Some(cli) = solidcraft() {
                let got = volumes(
                    &cli,
                    &written_with(
                        &changed,
                        &SolidCraftWriter::new().verification(Verification::Off),
                    )
                    .0,
                );
                assert_eq!(got.len(), 1);
                assert!(
                    (got[0] - (1000.0 - std::f64::consts::PI * 10.0)).abs() < 1.0,
                    "{got:?}"
                );
            }
        } else {
            assert!(cuts.is_empty());
            assert!(
                history_line(&report).contains("through-all cut has no earlier named body"),
                "{}",
                history_line(&report)
            );
        }
    }
}

/// The example cube with a circle of the given radius added to the history's sketch, centred
/// in its square, as a second profile loop.
fn cube_with_circle_profile(length: f64, profile: Value, radius: f64) -> Model {
    let mut model = cube_with_history(length, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let sketch = &mut wire["model"];
    sketch["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .push(entity(
            5,
            json!({"kind": "circle", "center": {"u": 5.0, "v": 5.0}, "radius": radius}),
        ));
    let loops = sketch["sketches"][0]["profiles"][0].clone();
    sketch["sketches"][0]["profiles"] =
        json!([loops, [{"entity": "f3d:model:sketch-entity#5", "reversed": false}]]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn extrude_outcome(model: &Model) -> (dutchman_core::FeatureOutcome, Report) {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (_, report) = convert(model, &writer);
    let extrude = report
        .outcome()
        .unwrap()
        .features
        .iter()
        .find(|f| f.kind == "extrude")
        .unwrap()
        .clone();
    (extrude, report)
}

fn profile_of(indices: &[usize]) -> Value {
    json!({"kind": "sketch_profiles", "value": {"sketch": SKETCH, "profiles": indices}})
}

#[test]
fn an_extrude_whose_region_differs_from_the_source_end_cap_is_blocked() {
    // The sweep covers a Ø4 circle; the model's end face is the whole 10 mm square.
    let model = cube_with_circle_profile(10.0, profile_of(&[1]), 2.0);
    let (extrude, report) = extrude_outcome(&model);
    assert_eq!(extrude.status, dutchman_core::FeatureStatus::Blocked);
    let reason = extrude.reason.unwrap();
    assert!(
        reason.starts_with("its profile region doesn't match the source (swept 12.5 mm²"),
        "{reason}"
    );
    assert!(reason.contains("the source 100.0 mm²)"), "{reason}");
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("history/profile-region-mismatch")));
    // A hole in the swept region leaves the end face smaller than the source's.
    let model = cube_with_circle_profile(10.0, profile_of(&[0, 1]), 2.0);
    let (extrude, _) = extrude_outcome(&model);
    assert_eq!(extrude.status, dutchman_core::FeatureStatus::Blocked);
    assert!(extrude.reason.unwrap().contains("swept 87.5 mm²"));
}

#[test]
fn an_extrude_whose_region_matches_the_source_end_cap_is_kept_and_proven() {
    let model = cube_with_circle_profile(10.0, profile_of(&[0]), 2.0);
    let (extrude, report) = extrude_outcome(&model);
    assert_eq!(extrude.status, dutchman_core::FeatureStatus::Mapped);
    assert!(!extrude.profile_unverified);
    assert!(!report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("history/profile-region-mismatch")));
}

#[test]
fn an_extrude_with_no_end_cap_in_the_model_is_kept_but_its_profile_is_unproven() {
    // The model has no face 5 mm up, so nothing says what this sweep covered.
    let model = cube_with_circle_profile(5.0, profile_of(&[1]), 2.0);
    let (extrude, _) = extrude_outcome(&model);
    assert_eq!(extrude.status, dutchman_core::FeatureStatus::Mapped);
    assert!(extrude.profile_unverified);
}

/// Recorded responses for the synthetic cube fixture above. No CLI or temporary tool
/// is used: the same JSON parser handles these and the native CLI's answers.
#[derive(Debug, Default)]
struct RecordedEvaluator {
    calls: std::sync::Mutex<Vec<String>>,
    different_history: bool,
    reject_curve_candidate: bool,
    warning: bool,
    require_untransformed: bool,
}

impl dutchman_solidcraft::SolidCraftEvaluator for RecordedEvaluator {
    fn evaluate(
        &self,
        design: &[u8],
        timeout: std::time::Duration,
        label: &str,
    ) -> Result<Value, String> {
        assert!(!timeout.is_zero());
        let mut archive = ZipArchive::new(Cursor::new(design)).unwrap();
        let document: Value =
            serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
        if self.require_untransformed && label != "opening" {
            // A verification-parameters document carries no occurrences to check.
            for occurrence in document["occurrences"].as_array().into_iter().flatten() {
                assert_eq!(
                    occurrence["transform"],
                    json!([
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 1.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0]
                    ]),
                    "{label}"
                );
            }
        }
        let rejected = self.reject_curve_candidate
            && document["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|feature| {
                    feature.get("suppressed").and_then(Value::as_bool) != Some(true)
                        && feature.pointer("/profiles/type").and_then(Value::as_str)
                            == Some("curves")
                });
        self.calls.lock().unwrap().push(label.to_owned());
        Ok(json!({
            "bodies": [{"volume_mm3": if label == "history" && self.different_history {900.0} else {1000.0},
                "area_mm2": 600.0, "bbox": {"min": [0, 0, 0], "max": [10, 10, 10]}}],
            "timeline": [{"name": "Original geometry", "error": if rejected { json!("Synthetic candidate refusal") } else { Value::Null },
                "warning": if self.warning { "1 face(s) could not be meshed" } else { "" }}],
            "total": {"faces": 6}
        }))
    }

    fn run(
        &self,
        design: &[u8],
        commands: &[Value],
        timeout: std::time::Duration,
        label: &str,
    ) -> Result<Vec<Value>, String> {
        assert!(!design.is_empty());
        assert!(!timeout.is_zero());
        self.calls.lock().unwrap().push(label.to_owned());
        commands
            .iter()
            .map(|command| {
                Ok(match command["command"].as_str().unwrap() {
                    "timeline.roll_to" => json!({"marker": command["params"]["feature"]}),
                    "document.inspect" => {
                        json!({"bodies": [{"name": "unit cube"}], "timeline": []})
                    }
                    "model.edges" => json!({"body": "unit cube", "edges": [{
                        "name": "front|left", "start": [0, 0, 0], "end": [0, 0, 10],
                        "mid": [0, 0, 5], "length": 10.0
                    }]}),
                    other => return Err(format!("Unexpected command: {other}")),
                })
            })
            .collect()
    }
}

#[test]
fn a_host_evaluator_proves_a_blend_and_verifies_the_fixture() {
    let host = std::sync::Arc::new(RecordedEvaluator::default());
    let writer = SolidCraftWriter::new()
        // A host must not depend on an installed CLI, even when this path is missing.
        .verification(Verification::With(PathBuf::from("/missing/solidcraft-cli")))
        .replay(Verification::With(PathBuf::from("/missing/solidcraft-cli")))
        .evaluator(host.clone())
        .target_version(Some(dutchman_solidcraft::SolidCraftVersion::default()));
    let model = input_blend("fillet", json!({"kind": "constant", "radius": 1.0}), false);
    let (document, report) = convert(&model, &writer);
    assert_eq!(document["features"][2]["type"], "fillet");
    assert_eq!(document["features"][2]["edges"], json!([[0.0, 0.0, 5.0]]));
    assert_eq!(report.outcome().unwrap().check, Check::Verified);
    assert_eq!(
        *host.calls.lock().unwrap(),
        [
            "bodies",
            "edges",
            "original",
            "history",
            "verification-parameters",
            "opening"
        ]
    );
}

#[test]
fn a_host_evaluator_skips_parameter_verification_for_history_without_parameters() {
    let host = std::sync::Arc::new(RecordedEvaluator::default());
    let writer = SolidCraftWriter::new().evaluator(host.clone());
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["parameters"] = json!([]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(&model, &writer);
    assert_eq!(report.outcome().unwrap().mode, Mode::History);
    assert_eq!(report.outcome().unwrap().check, Check::Verified);
    assert_eq!(
        *host.calls.lock().unwrap(),
        ["original", "history", "opening"]
    );
}

#[test]
fn a_host_evaluator_reports_geometry_warnings_after_replacing_a_differing_replay() {
    let host = std::sync::Arc::new(RecordedEvaluator {
        different_history: true,
        warning: true,
        ..RecordedEvaluator::default()
    });
    let writer = SolidCraftWriter::new()
        .evaluator(host.clone())
        .target_version(Some(dutchman_solidcraft::SolidCraftVersion::default()));
    let model = cube_with_history(10.0, first_profile(), None);
    let (_, report) = convert(&model, &writer);
    assert_eq!(report.outcome().unwrap().mode, Mode::Import);
    assert!(matches!(
        report.outcome().unwrap().check,
        Check::Flattened(_)
    ));
    assert_eq!(
        *host.calls.lock().unwrap(),
        [
            "original",
            "history",
            "verification-parameters",
            "import",
            "opening"
        ]
    );
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("could not be meshed")));
}

/// A closed square carrier with no stored profiles and an opaque source selection.
fn unresolved_square_selection(selected: &[u32]) -> Model {
    let ids: Vec<String> = selected
        .iter()
        .map(|id| format!("f3d:model:sketch-entity#{id}"))
        .collect();
    let profile = json!({"kind": "sketch_selection", "value": {"sketch": SKETCH,
        "selections": if ids.is_empty() {vec!["synthetic:native:selection#one".to_owned()]} else {ids}}});
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketches"][0]["profiles"] = json!([]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn an_inferred_native_profile_is_kept_only_when_the_body_check_confirms_it() {
    for selected in [&[][..], &[1, 2, 3, 4][..]] {
        let host = std::sync::Arc::new(RecordedEvaluator::default());
        let writer = SolidCraftWriter::new().evaluator(host.clone());
        let (document, report) = convert(&unresolved_square_selection(selected), &writer);
        let outcome = report.outcome().unwrap();
        assert_eq!(outcome.mode, Mode::History);
        assert_eq!(outcome.check, Check::Verified);
        assert_eq!(
            outcome.features[1].status,
            dutchman_core::FeatureStatus::Mapped
        );
        assert_eq!(
            document["features"][1]["profiles"]["type"],
            if selected.is_empty() { "all" } else { "curves" }
        );
        assert_eq!(
            report
                .losses()
                .iter()
                .filter(
                    |loss| loss.code.as_deref() == Some("history/profile-region-inferred")
                        && loss.detail.contains("check confirmed it")
                )
                .count(),
            1
        );
    }
}

#[test]
fn an_inferred_native_profile_is_refused_without_a_matching_body_check() {
    for writer in [
        SolidCraftWriter::new().verification(Verification::Off),
        SolidCraftWriter::new().evaluator(std::sync::Arc::new(RecordedEvaluator {
            different_history: true,
            ..RecordedEvaluator::default()
        })),
    ] {
        let (document, report) = convert(&unresolved_square_selection(&[1, 2, 3, 4]), &writer);
        assert_reference_sketches(&document, 1);
        assert_eq!(
            report.outcome().unwrap().features[1].status,
            dutchman_core::FeatureStatus::Blocked
        );
        assert!(report.outcome().unwrap().features[1]
            .reason
            .as_deref()
            .unwrap()
            .contains("could not be confirmed"));
        assert!(!report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("history/profile-region-inferred")));
    }
}

#[test]
fn an_open_selected_curve_candidate_can_fall_back_to_a_confirmed_carrier() {
    let host = std::sync::Arc::new(RecordedEvaluator {
        reject_curve_candidate: true,
        ..RecordedEvaluator::default()
    });
    let (document, report) = convert(
        &unresolved_square_selection(&[1, 2, 3]),
        &SolidCraftWriter::new().evaluator(host.clone()),
    );
    assert_eq!(report.outcome().unwrap().check, Check::Verified);
    assert_eq!(document["features"][1]["profiles"]["type"], "all");
    assert_eq!(
        host.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|call| call.as_str() == "history")
            .count(),
        3
    );
}

#[test]
fn an_inferred_partial_profile_needs_known_bodies_at_its_own_source_state() {
    for (center, mapped) in [
        (Some("cube:5,5,5"), true),
        (Some("cube:50,50,50"), false),
        (None, false),
    ] {
        let mut model = unresolved_square_selection(&[]);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        if let Some(center) = center {
            wire["model"]["features"][1]["source_properties"] =
                json!({"body_centers": center, "body_centers_exact": "cube"});
        }
        wire["model"]["features"].as_array_mut().unwrap().push(json!({"id": "synthetic:cube:feature#later", "ordinal": 2, "name": "Later", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "native", "value": {"kind": "Reference blocker", "parameters": {}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (_, report) = convert(
            &model,
            &SolidCraftWriter::new().evaluator(std::sync::Arc::new(RecordedEvaluator::default())),
        );
        assert_eq!(
            report.outcome().unwrap().features[1].status,
            if mapped {
                dutchman_core::FeatureStatus::Mapped
            } else {
                dutchman_core::FeatureStatus::Blocked
            },
            "{report:?}"
        );
        assert_eq!(
            report
                .losses()
                .iter()
                .any(|loss| loss.code.as_deref() == Some("history/profile-region-inferred")),
            mapped
        );
    }
}

/// The cube with its body hidden in the source (a Fusion body with its eye off), owned by the
/// design's component so it is a part of the design and not a consumed tool.
fn hidden_cube_with_history() -> Model {
    let model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    wire["geometry"]["bodies"][0]["visible"] = json!(false);
    wire["model"]["product_definitions"] = json!([{"id": "f3d:model:component#root", "kind": "part", "bodies": [body], "bom_properties": {}, "sketches": []}]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn a_body_the_source_hides_stays_hidden_when_the_history_rebuilds_it() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, _) = convert(&hidden_cube_with_history(), &writer);
    let extrude = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["type"] == "extrude")
        .unwrap();
    let name = extrude["body_names"][0].clone();
    assert_eq!(
        document["visibility"]["hidden_bodies"],
        json!([name]),
        "{document}"
    );

    // A shown source body is listed nowhere.
    let (shown, _) = convert(&cube_with_history(10.0, first_profile(), None), &writer);
    assert!(
        shown.pointer("/visibility/hidden_bodies").is_none(),
        "{shown}"
    );
}

/// The 10 mm square with a 4 mm circle centred in it, both taken as whole loops the way a
/// source lists the ring and the disc inside it, and the whole square besides.
fn square_and_disc_history() -> Model {
    let profile = json!({"kind": "sketch_regions", "value": {"sketch": SKETCH, "regions": [
        {"kind": "loops", "outer": 0, "holes": [1]},
        {"kind": "loops", "outer": 1, "holes": []},
        {"kind": "loops", "outer": 0, "holes": []},
    ]}});
    cube_with_circle_profile(10.0, profile, 2.0)
}

#[test]
fn a_loop_around_another_loop_the_selection_names_is_a_ring_and_a_disc() {
    let (document, report) = convert(
        &square_and_disc_history(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("2 of 2 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let profiles = document["features"][1]["profiles"]["profiles"]
        .as_array()
        .unwrap();
    // The square alone is not a region of the sketch: SolidCraft would refuse it.
    assert_eq!(profiles.len(), 2, "{profiles:?}");
    let holes: Vec<usize> = profiles
        .iter()
        .map(|profile| profile["holes"].as_array().unwrap().len())
        .collect();
    assert!(holes.contains(&0) && holes.contains(&1), "{profiles:?}");
}

#[test]
fn solidcraft_replays_a_loop_around_another_loop_the_selection_names() {
    let Some(cli) = solidcraft() else { return };
    let volume = volumes(&cli, &written(&square_and_disc_history()).0);
    assert!(!volume.is_empty());
    // The ring and the disc take the whole square: 10 x 10 x 10.
    let total: f64 = volume.iter().sum();
    assert!((total - 1000.0).abs() < 1.0, "{volume:?}");
}

#[test]
fn a_cut_before_any_feature_builds_a_body_is_refused_when_mapping() {
    let mut model = cube_with_history(10.0, json!({"kind": "sketch", "value": SKETCH}), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["definition"]["value"]["value"]["op"] = json!("cut");
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        !document["features"]
            .as_array()
            .map(|features| features.iter().any(|feature| feature["operation"] == "cut"))
            .unwrap_or(false),
        "{document}"
    );
    assert!(
        history_line(&report).contains("no earlier feature in the history builds one"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn design_only_history_is_refused_instead_of_silently_mapping_nothing() {
    let mut model = primitive_history(sphere());
    model.ir.model.features.clear();
    model.ir.design.features.push(
        serde_json::from_value(json!({
            "id": "test:design:feature#stored",
            "definition": {"operation": {"kind": "stored", "value": {
                "bodies": ["synthetic:cube:body#0"], "tessellations": [],
                "origin": {"kind": "base"}
            }}}
        }))
        .unwrap(),
    );
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(document["features"][0]["type"], "import");
    assert!(
        history_line(&report).contains("design-only history is not mapped yet"),
        "{report:?}"
    );
    assert!(
        history_line(&report).contains("ordered selections and evaluation states"),
        "{report:?}"
    );
}

#[test]
fn a_native_symmetric_extrude_keeps_its_literal_distance() {
    for namespace in ["f3d:", "inventor:"] {
        let mut model = cube_with_height_parameter(1.0);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["features"][1]["definition"]["value"]["value"]["extent"]["kind"] =
            json!("symmetric");
        model.ir =
            cadmpeg_ir::CadIr::from_json(&wire.to_string().replace("f3d:", namespace)).unwrap();
        let (doc, _) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert_eq!(doc["features"][1]["extent"]["direction"], "symmetric");
        assert_eq!(doc["features"][1]["extent"]["distance"], "10 mm");
    }
}

#[test]
fn native_parameters_do_not_change_a_fallback_without_mapped_sketches() {
    for namespace in ["f3d:", "inventor:"] {
        let mut model = cube_with_height_parameter(1.0);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["sketches"][0]["placement"] = json!({"kind":"unresolved"});
        wire["model"]["sketch_constraints"] = json!([]);
        model.ir =
            cadmpeg_ir::CadIr::from_json(&wire.to_string().replace("f3d:", namespace)).unwrap();
        let writer = SolidCraftWriter::new().verification(Verification::Off);
        let (with_parameters, _) = written_with(&model, &writer);
        wire["model"]["parameters"] = json!([]);
        model.ir =
            cadmpeg_ir::CadIr::from_json(&wire.to_string().replace("f3d:", namespace)).unwrap();
        let (without_parameters, _) = written_with(&model, &writer);
        assert_eq!(
            with_parameters, without_parameters,
            "{namespace}: fallback archive must remain byte-for-byte identical"
        );
        let (doc, _) = convert(&model, &writer);
        assert!(doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["type"] != "sketch"));
    }
}

/// Add neutral spatial relations to a synthetic non-planar path; no native file is used.
fn spatial_044_model() -> Model {
    let base = spatial_sweep_history(|entities, _| {
        entities.push(spatial_entity(
            4,
            json!({"kind": "point", "position": xyz([0.0, 0.0, 0.0])}),
        ));
        entities.push(spatial_entity(
            5,
            json!({"kind": "point", "position": xyz([0.0, 0.0, 30.0])}),
        ));
    });
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    model["parameters"].as_array_mut().unwrap().push(json!({"id": "f3d:model:parameter#44", "owner": "f3d:model:feature#3", "ordinal": 44, "name": "rise", "expression": "30 mm", "value": {"kind": "length", "value": 30.0}, "dependencies": [], "properties": {}}));
    model["spatial_sketch_constraints"] = json!([
        {"id": "f3d:model:sketch-constraint#441", "sketch": SPATIAL, "definition": {"kind": "line_length", "entity": spatial_id(1), "parameter": "f3d:model:parameter#44"}},
        {"id": "f3d:model:sketch-constraint#442", "sketch": SPATIAL, "definition": {"kind": "parallel_to_direction", "entity": spatial_id(1), "direction": xyz([0.0, 0.0, -1.0])}},
        {"id": "f3d:model:sketch-constraint#443", "sketch": SPATIAL, "definition": {"kind": "point_distance", "first": spatial_id(4), "second": spatial_id(5), "parameter": "f3d:model:parameter#44"}},
        {"id": "f3d:model:sketch-constraint#444", "sketch": SPATIAL, "definition": {"kind": "tangent", "first": spatial_id(1), "second": spatial_id(2)}},
    ]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

fn spatial_044_writer() -> SolidCraftWriter {
    SolidCraftWriter::new()
        .target_version(Some("0.44.0".parse().unwrap()))
        .verification(Verification::Off)
}

#[test]
fn spatial_044_constraints_keep_parameters_axis_and_endpoint_tangency() {
    let (document, report) = convert(&spatial_044_model(), &spatial_044_writer());
    let rail = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Rail sketch")
        .unwrap();
    let constraints = rail["sketch"]["wire_constraints"].as_array().unwrap();
    assert_eq!(rail["sketch"]["counters"]["q"], 2);
    assert_eq!(rail["sketch"]["counters"]["w"], 3);
    assert_eq!(rail["sketch"]["counters"]["k"], json!(constraints.len()));
    assert!(constraints
        .iter()
        .any(|c| c["type"] == "distance" && c["param"] == "rise" && c["value"] == 30.0));
    assert!(constraints
        .iter()
        .any(|c| c["type"] == "along_axis" && c["axis"] == "z"));
    assert!(constraints
        .iter()
        .any(|c| c["type"] == "tangent" && c["line_end"] == 1 && c["arc_end"] == 0));
    assert_eq!(constraints.len(), 4);
    assert!(!constraints.iter().any(|c| c["type"] == "coincident"));
    assert!(
        !report
            .losses()
            .iter()
            .any(|l| l.subject.starts_with("3D sketch") && l.detail.contains("not carried")),
        "{:?}",
        report.losses()
    );
}

#[test]
fn spatial_044_older_targets_keep_the_legacy_sketch_bytes() {
    let model = spatial_044_model();
    for version in ["0.18.2", "0.43.0", "0.43.9"] {
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new()
                .target_version(Some(version.parse().unwrap()))
                .verification(Verification::Off),
        );
        let rail = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == "Rail sketch")
            .unwrap();
        use sha2::Digest;
        let (bytes, _) = written_with(
            &model,
            &SolidCraftWriter::new()
                .target_version(Some(version.parse().unwrap()))
                .verification(Verification::Off),
        );
        let digest = |bytes: &[u8]| {
            sha2::Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        };
        println!("legacy_bytes {version} {}", digest(&bytes));
        let (profile_bytes, _) = written_with(
            &spatial_044_extrude(0.0),
            &SolidCraftWriter::new()
                .target_version(Some(version.parse().unwrap()))
                .verification(Verification::Off),
        );
        println!("legacy_bytes profile-{version} {}", digest(&profile_bytes));
        assert!(rail["sketch"].get("wire_constraints").is_none());
        assert_eq!(rail["sketch"]["counters"], json!({"q": 2, "w": 3}));
        assert!(report
            .losses()
            .iter()
            .any(|l| l.detail.starts_with("4 constraints not carried:")));
        // Changing neutral relations cannot alter the legacy geometry representation.
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["spatial_sketch_constraints"] = json!([]);
        let no_relations = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model.clone()
        };
        let (without, _) = convert(
            &no_relations,
            &SolidCraftWriter::new()
                .target_version(Some(version.parse().unwrap()))
                .verification(Verification::Off),
        );
        let other = without["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == "Rail sketch")
            .unwrap();
        assert_eq!(
            serde_json::to_vec(rail).unwrap(),
            serde_json::to_vec(other).unwrap()
        );
    }
}

/// A spatial sketch with a closed XY square and an unrelated out-of-plane leg.
fn spatial_044_extrude(lift: f64) -> Model {
    let base = spatial_sweep_history(|entities, _| {
        let corners = [
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [10.0, 10.0, lift],
            [0.0, 10.0, 0.0],
        ];
        *entities = (0..4)
            .map(|i| {
                spatial_entity(
                    i as u32 + 1,
                    json!({"kind": "line", "start": xyz(corners[i]), "end": xyz(corners[(i+1)%4])}),
                )
            })
            .collect();
        entities.push(spatial_entity(
            5,
            json!({"kind": "line", "start": xyz([0.0,0.0,0.0]), "end": xyz([0.0,0.0,8.0])}),
        ));
    });
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    wire["model"]["spatial_sketches"][0]["profiles"] = json!([{ "origin": xyz([0.0,0.0,0.0]), "normal": xyz([0.0,0.0,1.0]), "u_axis": xyz([1.0,0.0,0.0]), "boundary": (1..=4).map(|i| json!({"entity": spatial_id(i), "reversed": false})).collect::<Vec<_>>() }]);
    wire["model"]["spatial_sketch_constraints"] = json!([
        {"id":"f3d:model:sketch-constraint#4444", "sketch":SPATIAL, "definition":{"kind":"repeated_line_length","entities":(1..=4).map(spatial_id).collect::<Vec<_>>(),"parameter":"f3d:model:parameter#2"}},
        {"id":"f3d:model:sketch-constraint#4445", "sketch":SPATIAL, "definition":{"kind":"parallel_to_direction","entity":spatial_id(1),"direction":xyz([1.0,0.0,0.0])}},
        {"id":"f3d:model:sketch-constraint#4446", "sketch":SPATIAL, "definition":{"kind":"parallel_to_direction","entity":spatial_id(2),"direction":xyz([0.0,1.0,0.0])}},
        {"id":"f3d:model:sketch-constraint#4447", "sketch":SPATIAL, "definition":{"kind":"parallel_to_direction","entity":spatial_id(3),"direction":xyz([-1.0,0.0,0.0])}},
        {"id":"f3d:model:sketch-constraint#4448", "sketch":SPATIAL, "definition":{"kind":"parallel_to_direction","entity":spatial_id(4),"direction":xyz([0.0,-1.0,0.0])}},
    ]);
    wire["model"]["features"][2]["definition"]["value"] = json!({"kind":"extrude", "value": {"profile": {"kind": "spatial_sketch_profiles", "value": {"sketch": SPATIAL, "profiles": [0]}}, "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"}, "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 10.0}}}, "op": "new_body"}});
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

#[test]
fn spatial_044_extrudes_require_a_planar_loop_and_source_proof() {
    let (_, report) = convert(&spatial_044_extrude(1.0), &spatial_044_writer());
    assert!(
        history_line(&report).contains("3D sketch profile boundary is not planar"),
        "{}",
        history_line(&report)
    );
    let (_, report) = convert(&spatial_044_extrude(0.0), &spatial_044_writer());
    assert!(
        history_line(&report).contains("has not been confirmed against the source body"),
        "{}",
        history_line(&report)
    );
    let (_, report) = convert(
        &spatial_044_extrude(0.0),
        &SolidCraftWriter::new()
            .target_version(Some("0.43.0".parse().unwrap()))
            .verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("its sketch is not an earlier mapped sketch"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn spatial_044_extrudes_use_host_measurements_without_profile_inference() {
    for different_history in [false, true] {
        let host = std::sync::Arc::new(RecordedEvaluator {
            different_history,
            ..RecordedEvaluator::default()
        });
        let (_, report) = convert(
            &spatial_044_extrude(0.0),
            &SolidCraftWriter::new()
                .target_version(Some("0.44.0".parse().unwrap()))
                .evaluator(host.clone()),
        );
        let outcome = report.outcome().unwrap();
        let extrude = outcome
            .features
            .iter()
            .find(|f| f.kind == "extrude")
            .unwrap();
        assert!(extrude.inferred.is_none(), "{report:?}");
        if different_history {
            assert_eq!(extrude.status, dutchman_core::FeatureStatus::Blocked);
            assert!(extrude.reason.as_deref().unwrap().contains(
                "its planar 3D sketch profile has not been confirmed against the source body"
            ));
        } else {
            assert_eq!(extrude.status, dutchman_core::FeatureStatus::Mapped);
            assert!(!extrude.profile_unverified);
            assert!(report.losses().iter().any(|loss| {
                loss.severity == dutchman_core::Severity::Info
                    && loss.detail == "its planar 3D sketch profile is written as a separate planar sketch; the source body check confirmed it"
            }), "{report:?}");
        }
        assert!(!report.losses().iter().any(|loss| {
            matches!(
                loss.code.as_deref(),
                Some("history/profile-region-inferred" | "history/profile-region-measured")
            )
        }));
        assert!(host
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|label| label == "history"));
    }
}

fn spatial_044_cli() -> Option<PathBuf> {
    let cli = solidcraft()?;
    // detected() caps at the writer's newest default; read the CLI's actual release here.
    let version = dutchman_solidcraft::solidcraft_version(&Verification::With(cli.clone()))
        .and_then(|line| dutchman_solidcraft::solidcraft_minor(&line))
        .and_then(|minor| {
            format!("{minor}.0")
                .parse::<dutchman_solidcraft::SolidCraftVersion>()
                .ok()
        })?;
    if version < "0.44.0".parse().unwrap() {
        eprintln!("skipped: 3D sketch constraints require SolidCraft 0.44 or newer");
        return None;
    }
    Some(cli)
}

#[test]
fn spatial_044_constraints_and_extrude_replay_against_the_source_cube() {
    let Some(cli) = spatial_044_cli() else {
        return;
    };
    let writer = SolidCraftWriter::new()
        .target_version(Some("0.44.0".parse().unwrap()))
        .verification(Verification::With(cli.clone()));
    let model = spatial_044_extrude(0.0);
    let mut bytes = Vec::new();
    let mut report = Report::default();
    writer.write(&model, &mut bytes, &mut report).unwrap();
    assert_eq!(report.outcome().unwrap().mode, Mode::History, "{report:?}");
    assert!(
        matches!(report.outcome().unwrap().check, Check::Verified),
        "{report:?}"
    );
    assert!((volumes(&cli, &bytes)[0] - 1000.0).abs() < 1e-6);
    let grown_profile = with_param(&bytes, "side", "20 mm");
    assert!((volumes(&cli, &grown_profile)[0] - 4000.0).abs() < 0.01);
    let (path_bytes, _) = written_with(&spatial_044_model(), &spatial_044_writer());
    evaluated(&cli, &path_bytes);
    // A straight 3D path makes dimension edits measurable independently of spline fitting.
    let base = spatial_044_model();
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    wire["model"]["spatial_sketch_entities"]
        .as_array_mut()
        .unwrap()
        .retain(|e| {
            [spatial_id(1), spatial_id(4), spatial_id(5)]
                .contains(&e["id"].as_str().unwrap().to_owned())
        });
    wire["model"]["spatial_sketch_constraints"]
        .as_array_mut()
        .unwrap()
        .retain(|c| c["definition"]["kind"] != "tangent");
    wire["model"]["features"][2]["definition"]["value"]["value"]["path"]["value"]["curves"] =
        json!([spatial_id(1)]);
    let entities = wire["model"]["spatial_sketch_entities"]
        .as_array_mut()
        .unwrap();
    entities
        .iter_mut()
        .find(|e| e["id"] == spatial_id(1))
        .unwrap()["geometry"]["end"] = xyz([0.0, 0.0, 10.0]);
    entities
        .iter_mut()
        .find(|e| e["id"] == spatial_id(5))
        .unwrap()["geometry"]["position"] = xyz([0.0, 0.0, 10.0]);
    // These unused branches make the carrier non-planar without changing its selected path.
    entities.push(spatial_entity(
        6,
        json!({"kind":"line","start":xyz([0.0,0.0,10.0]),"end":xyz([8.0,0.0,10.0])}),
    ));
    entities.push(spatial_entity(
        7,
        json!({"kind":"line","start":xyz([0.0,0.0,10.0]),"end":xyz([0.0,8.0,10.0])}),
    ));
    let rise = wire["model"]["parameters"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|p| p["name"] == "rise")
        .unwrap();
    rise["expression"] = json!("10 mm");
    rise["value"]["value"] = json!(10.0);
    let straight = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let (bytes, proof) = written_with(&straight, &writer);
    assert_eq!(proof.outcome().unwrap().check, Check::Verified, "{proof:?}");
    assert!((volumes(&cli, &bytes)[0] - 1000.0).abs() < 0.01);
    let grown = with_param(&bytes, "rise", "40 mm");
    assert!((volumes(&cli, &grown)[0] - 4000.0).abs() < 0.01);
    // This path deliberately sweeps a shape different from the source cube.
    // The source check must retain the imported cube instead of declaring the path verified.
    let (_, report) = convert(&spatial_044_model(), &writer);
    assert_ne!(report.outcome().unwrap().mode, Mode::History, "{report:?}");
}

#[test]
fn spatial_044_failed_constraint_replay_retries_the_solved_geometry() {
    let Some(cli) = spatial_044_cli() else {
        return;
    };
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let spatial = serde_json::to_value(&spatial_044_model().ir).unwrap();
    for key in [
        "spatial_sketches",
        "spatial_sketch_entities",
        "spatial_sketch_constraints",
    ] {
        wire["model"][key] = spatial["model"][key].clone();
    }
    let mut feature = spatial["model"]["features"][1].clone();
    feature["ordinal"] = json!(2);
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(feature);
    let mut parameter = spatial["model"]["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "rise")
        .unwrap()
        .clone();
    parameter["expression"] = json!("-30 mm");
    parameter["value"]["value"] = json!(30.0);
    wire["model"]["parameters"]
        .as_array_mut()
        .unwrap()
        .push(parameter);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let (doc, report) = convert(
        &model,
        &SolidCraftWriter::new()
            .target_version(Some("0.44.0".parse().unwrap()))
            .verification(Verification::With(cli)),
    );
    assert_eq!(
        report.outcome().unwrap().mode,
        Mode::PartialHistory,
        "{report:?}"
    );
    let rail = doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Rail sketch")
        .unwrap();
    assert_eq!(rail["sketch"]["wire_constraints"], json!([]));
    assert_eq!(rail["sketch"]["wires_joined"], false);
    assert_eq!(rail["sketch"]["wires"].as_array().unwrap().len(), 5);
    assert!(
        report.outcome().unwrap().features.iter().any(|f| f
            .reason
            .as_ref()
            .is_some_and(|r| r.contains("not carried after replay failed"))),
        "{report:?}"
    );
    assert!(report.outcome().unwrap().features.iter().any(|f|f.kind=="spatial_sketch" && f.status==dutchman_core::FeatureStatus::Mapped),"{report:?}");
}

#[test]
fn spatial_044_independent_sketches_survive_an_earlier_blocker() {
    let base = spatial_044_model();
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    wire["model"]["features"][1]["ordinal"] = json!(2);
    wire["model"]["features"][2]["ordinal"] = json!(3);
    wire["model"]["features"].as_array_mut().unwrap().push(json!({"id":"f3d:model:feature#44", "ordinal":1, "name":"Earlier blocker", "suppressed":false,"definition":{"kind":"operation","value":{"kind":"native","value":{"kind":"Synthetic unsupported operation","parameters":{}}}},"dependencies":[],"source_properties":{},"source_content":[],"outputs":[]}));
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let (doc, report) = convert(&model, &spatial_044_writer());
    assert!(
        doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["name"] == "Rail sketch"
                && f["sketch"]["wire_constraints"]
                    .as_array()
                    .is_some_and(|c| !c.is_empty())),
        "{doc}"
    );
    assert!(report
        .outcome()
        .unwrap()
        .features
        .iter()
        .any(|f| f.name.as_deref() == Some("Rail sketch")
            && f.status == dutchman_core::FeatureStatus::Mapped));
}

/// The cube's sketch with a second 10 mm square beside the first, 20 mm along X, and an opaque
/// native pick of one of the two: the first two readings (the whole sketch) build two bodies.
fn two_square_selection() -> Model {
    let profile = json!({"kind": "sketch_selection", "value": {"sketch": SKETCH,
        "selections": ["synthetic:native:selection#one"]}});
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketch_constraints"] = json!([]);
    m["sketch_entities"] = json!([
        line(1, [0.0, 0.0], [10.0, 0.0]),
        line(2, [10.0, 0.0], [10.0, 10.0]),
        line(3, [10.0, 10.0], [0.0, 10.0]),
        line(4, [0.0, 10.0], [0.0, 0.0]),
        line(5, [20.0, 0.0], [30.0, 0.0]),
        line(6, [30.0, 0.0], [30.0, 10.0]),
        line(7, [30.0, 10.0], [20.0, 10.0]),
        line(8, [20.0, 10.0], [20.0, 0.0]),
    ]);
    let ring = |first: u32| -> Vec<Value> {
        (first..first + 4)
            .map(|key| json!({"entity": format!("f3d:model:sketch-entity#{key}"), "reversed": false}))
            .collect()
    };
    m["sketches"][0]["profiles"] = json!([ring(1), ring(5)]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

/// Measures a design by the profile its extrude sweeps, as SolidCraft would: the first square
/// is the source's cube, the second the same cube 20 mm along X, the whole sketch both. With
/// `tie`, either square measures as the cube. `faces` gives `inspect.compare` face counts:
/// the first square's replay has the source's six faces, the second's seven.
#[derive(Debug, Default)]
struct MeasuringEvaluator {
    calls: std::sync::Mutex<Vec<String>>,
    tie: bool,
    faces: bool,
    /// Lists the halves of a square split down the middle as its sketch's regions.
    halves: bool,
    unavailable_comparison: bool,
    missing_face_counts: bool,
    missing_measurement: Option<(&'static str, &'static str)>,
    missing_only_second: bool,
    concentric: bool,
}

impl MeasuringEvaluator {
    /// Which square the live extrude sweeps: `Some(true)` the first, `Some(false)` the
    /// second, `None` both or none (the original geometry alone).
    fn swept(design: &[u8]) -> Option<Option<bool>> {
        let mut archive = ZipArchive::new(Cursor::new(design)).unwrap();
        let document: Value =
            serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
        let extrude = document["features"].as_array().unwrap().iter().find(|f| {
            f["type"] == "extrude" && f.get("suppressed").and_then(Value::as_bool) != Some(true)
        })?;
        let text = extrude["profiles"].to_string();
        if extrude["profiles"]["type"] == "points" {
            // The left half named by its centre is the source's shape; the right half is not.
            return Some(Some(text.contains("[2.5,5.0]")));
        }
        Some(match (text.contains("\"l1\""), text.contains("\"l5\"")) {
            (true, false) => Some(true),
            (false, true) => Some(false),
            _ => None,
        })
    }

    fn body(min_x: f64) -> Value {
        json!({"volume_mm3": 1000.0, "area_mm2": 600.0, "faces": 6,
            "bbox": {"min": [min_x, 0, 0], "max": [min_x + 10.0, 10, 10]}})
    }
}

impl dutchman_solidcraft::SolidCraftEvaluator for MeasuringEvaluator {
    fn evaluate(
        &self,
        design: &[u8],
        _: std::time::Duration,
        label: &str,
    ) -> Result<Value, String> {
        self.calls.lock().unwrap().push(label.to_owned());
        let bodies = match Self::swept(design) {
            None => vec![Self::body(0.0)],
            Some(Some(true)) => vec![Self::body(0.0)],
            Some(Some(false)) if self.tie => vec![Self::body(0.0)],
            Some(Some(false)) => vec![Self::body(20.0)],
            Some(None) => vec![Self::body(0.0), Self::body(20.0)],
        };
        let mut result = json!({"bodies": bodies, "timeline": [], "total": {"faces": 6}});
        if let Some((_, field)) = self.missing_measurement.filter(|(side, _)| {
            *side == label
                && (!self.missing_only_second || Self::swept(design) == Some(Some(false)))
        }) {
            for body in result["bodies"].as_array_mut().unwrap() {
                match field {
                    "min" | "max" => {
                        body["bbox"].as_object_mut().unwrap().remove(field);
                    }
                    "coordinate" => {
                        body["bbox"]["min"].as_array_mut().unwrap().pop();
                    }
                    _ => {
                        body.as_object_mut().unwrap().remove(field);
                    }
                }
            }
        }
        Ok(result)
    }

    fn run(
        &self,
        design: &[u8],
        commands: &[Value],
        _: std::time::Duration,
        label: &str,
    ) -> Result<Vec<Value>, String> {
        self.calls.lock().unwrap().push(label.to_owned());
        if self.concentric && commands[0]["command"] == "sketch.inspect" {
            // An annulus and its inner disc have the same centroid but distinct identities.
            return Ok(vec![json!({"profiles": [
                {"index": 0, "area_mm2": 12.0, "centroid_sketch": [2.5, 5.0]},
                {"index": 1, "area_mm2": 12.0, "centroid_sketch": [2.5, 5.0]},
                {"index": 2, "area_mm2": 50.0, "centroid_sketch": [30.0, 5.0]},
            ]})]);
        }
        if self.halves && commands[0]["command"] == "sketch.inspect" {
            return Ok(vec![json!({"profiles": [
                {"index": 0, "area_mm2": 50.0, "centroid_sketch": [2.5, 5.0]},
                {"index": 1, "area_mm2": 50.0, "centroid_sketch": [7.5, 5.0]},
            ]})]);
        }
        if self.unavailable_comparison {
            return Err("the host has no such command".to_owned());
        }
        let first = Self::swept(design) == Some(Some(true));
        commands
            .iter()
            .map(|command| {
                Ok(match command["command"].as_str().unwrap() {
                    "timeline.suppress" => json!({"suppressed": false}),
                    "inspect.compare" => json!({
                        "a": {"faces": (!self.missing_face_counts).then_some(if self.faces && !first { 7 } else { 6 })},
                        "b": {"faces": (!self.missing_face_counts).then_some(6), "bbox": {"min": [0, 0, 0], "max": [10, 10, 10]}},
                        "deviation": {"max_mm": 0.0001},
                    }),
                    other => return Err(format!("Unexpected command: {other}")),
                })
            })
            .collect()
    }
}

#[test]
fn a_profile_region_is_inferred_when_only_one_candidate_measures_as_the_source() {
    let host = std::sync::Arc::new(MeasuringEvaluator {
        faces: true,
        ..MeasuringEvaluator::default()
    });
    let (document, report) = convert(
        &two_square_selection(),
        &SolidCraftWriter::new().evaluator(host.clone()),
    );
    let outcome = report.outcome().unwrap();
    assert_eq!(outcome.check, Check::Verified, "{report:?}");
    let extrude = &outcome.features[1];
    assert_eq!(extrude.status, dutchman_core::FeatureStatus::Mapped);
    let inferred = extrude.inferred.unwrap();
    assert_eq!(
        inferred.selection,
        dutchman_core::InferredSelection::ProfileRegion
    );
    assert_eq!(
        inferred.against,
        dutchman_core::InferredAgainst::OriginalShape
    );
    assert_eq!((inferred.candidates, inferred.matches), (2, 1));
    assert_eq!(inferred.deviation_nm, Some(100));
    let loops = document["features"][1]["profiles"].to_string();
    assert!(
        loops.contains("\"l1\"") && !loops.contains("\"l5\""),
        "{loops}"
    );
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.code.as_deref() == Some("history/profile-region-measured")));
}

#[test]
fn candidates_that_measure_alike_are_a_tie_unless_one_alone_has_the_source_faces() {
    for (faces, mapped) in [(false, false), (true, true)] {
        let host = std::sync::Arc::new(MeasuringEvaluator {
            tie: true,
            faces,
            ..MeasuringEvaluator::default()
        });
        let (document, report) = convert(
            &two_square_selection(),
            &SolidCraftWriter::new().evaluator(host),
        );
        let extrude = &report.outcome().unwrap().features[1];
        if mapped {
            assert_eq!(extrude.status, dutchman_core::FeatureStatus::Mapped);
            assert_eq!(extrude.inferred.unwrap().matches, 2);
            assert!(document["features"][1]["profiles"]
                .to_string()
                .contains("\"l1\""));
        } else {
            assert_eq!(extrude.status, dutchman_core::FeatureStatus::Blocked);
            assert!(
                extrude
                    .reason
                    .as_deref()
                    .unwrap()
                    .contains("2 of 2 candidate regions match the source equally"),
                "{extrude:?}"
            );
            assert!(extrude.inferred.is_none());
            assert_reference_sketches(&document, 1);
        }
    }
}

#[test]
fn measured_regions_are_not_tried_without_a_solidcraft_check() {
    let (_, report) = convert(
        &two_square_selection(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let extrude = &report.outcome().unwrap().features[1];
    assert_eq!(extrude.status, dutchman_core::FeatureStatus::Blocked);
    assert!(extrude.inferred.is_none());
    // The first readings' refusal stands: nothing measured, so nothing claims an inference.
    assert!(!extrude
        .reason
        .as_deref()
        .unwrap()
        .contains("could not be inferred"));
}

#[test]
fn solidcraft_measures_the_one_region_that_rebuilds_the_source() {
    let Some(cli) = solidcraft() else { return };
    let (_, report) = convert(
        &two_square_selection(),
        &SolidCraftWriter::new().verification(Verification::With(cli)),
    );
    let outcome = report.outcome().unwrap();
    assert_eq!(outcome.check, Check::Verified, "{report:?}");
    let inferred = outcome.features[1].inferred.unwrap();
    assert_eq!(inferred.matches, 1);
    assert!(
        inferred.deviation_nm.is_some_and(|nm| nm < 10_000),
        "{inferred:?}"
    );
}

/// The cube's square with a loose line down its middle, which cadmpeg's one loop does not
/// show: SolidCraft's own regions of the sketch are the candidates.
fn split_square_selection() -> Model {
    let profile = json!({"kind": "sketch_selection", "value": {"sketch": SKETCH,
        "selections": ["synthetic:native:selection#half"]}});
    let mut model = cube_with_history(10.0, profile, None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketch_constraints"] = json!([]);
    m["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .push(line(5, [5.0, -1.0], [5.0, 11.0]));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn solidcraft_region_centroids_are_refused_without_proven_interior_selections() {
    let host = std::sync::Arc::new(MeasuringEvaluator {
        halves: true,
        ..MeasuringEvaluator::default()
    });
    let (_, report) = convert(
        &split_square_selection(),
        &SolidCraftWriter::new().evaluator(host.clone()),
    );
    let extrude = &report.outcome().unwrap().features[1];
    assert_eq!(
        extrude.status,
        dutchman_core::FeatureStatus::Blocked,
        "{report:?}"
    );
    assert!(extrude.inferred.is_none());
    assert!(extrude
        .reason
        .as_deref()
        .unwrap()
        .contains("without proven interior selections"));
    assert!(host
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call == "sketch regions"));
    assert!(!host
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call == "deviation"));
}

#[test]
fn an_annulus_disc_and_remote_region_cannot_claim_uniqueness_from_shared_centroids() {
    let host = std::sync::Arc::new(MeasuringEvaluator {
        concentric: true,
        ..MeasuringEvaluator::default()
    });
    let mut model = two_square_selection();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketch_entities"] = json!([
        circle(1, [2.5, 5.0], (24.0 / std::f64::consts::PI).sqrt()),
        circle(2, [2.5, 5.0], (12.0 / std::f64::consts::PI).sqrt()),
        circle(3, [30.0, 5.0], (50.0 / std::f64::consts::PI).sqrt()),
        // A nonconstruction curve absent from the profile table forces discovery.
        line(4, [40.0, 0.0], [40.0, 10.0]),
    ]);
    let ring = |id| json!([{"entity": format!("f3d:model:sketch-entity#{id}"), "reversed": false}]);
    m["sketches"][0]["profiles"] = json!([ring(1), ring(2), ring(3)]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(&model, &SolidCraftWriter::new().evaluator(host.clone()));
    let extrude = &report.outcome().unwrap().features[1];
    assert_eq!(
        extrude.status,
        dutchman_core::FeatureStatus::Blocked,
        "{report:?}"
    );
    assert!(extrude.inferred.is_none());
    assert!(
        extrude
            .reason
            .as_deref()
            .unwrap()
            .contains("3 regions without proven interior selections"),
        "{extrude:?}"
    );
    assert!(host
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call == "sketch regions"));
    assert!(!host
        .calls
        .lock()
        .unwrap()
        .iter()
        .any(|call| call == "deviation"));
}

#[test]
fn measured_regions_require_every_body_measurement_on_both_sides() {
    for side in ["original", "history"] {
        for field in ["volume_mm3", "area_mm2", "bbox", "min", "max", "coordinate"] {
            let host = std::sync::Arc::new(MeasuringEvaluator {
                missing_measurement: Some((side, field)),
                ..MeasuringEvaluator::default()
            });
            let (_, report) = convert(
                &two_square_selection(),
                &SolidCraftWriter::new().evaluator(host),
            );
            let extrude = &report.outcome().unwrap().features[1];
            assert_eq!(
                extrude.status,
                dutchman_core::FeatureStatus::Blocked,
                "{side} {field}: {report:?}"
            );
            assert!(extrude.inferred.is_none(), "{side} {field}: {extrude:?}");
        }
    }
}

#[test]
fn an_incomplete_alternative_cannot_make_a_matching_region_look_unique() {
    let host = std::sync::Arc::new(MeasuringEvaluator {
        missing_measurement: Some(("history", "area_mm2")),
        missing_only_second: true,
        ..MeasuringEvaluator::default()
    });
    let (_, report) = convert(
        &two_square_selection(),
        &SolidCraftWriter::new().evaluator(host),
    );
    let extrude = &report.outcome().unwrap().features[1];
    assert_eq!(
        extrude.status,
        dutchman_core::FeatureStatus::Blocked,
        "{report:?}"
    );
    assert!(extrude.inferred.is_none());
    assert!(
        extrude
            .reason
            .as_deref()
            .unwrap()
            .contains("some candidate regions could not be measured"),
        "{extrude:?}"
    );
}

fn assert_output_links(document: &Value, report: &Report) {
    let features = document["features"].as_array().unwrap();
    for row in &report.outcome().unwrap().features {
        assert_eq!(
            row.output.method,
            if row.output.ids.is_empty() {
                dutchman_core::OutputMatch::NoOutput
            } else {
                dutchman_core::OutputMatch::Emitted
            }
        );
        for id in &row.output.ids {
            let feature = features.iter().find(|feature| feature["id"] == *id);
            assert!(feature.is_some(), "missing output {id} for {row:?}");
            let feature = feature.unwrap();
            assert_eq!(row.status, dutchman_core::FeatureStatus::Mapped);
            assert_eq!(row.output.method, dutchman_core::OutputMatch::Emitted);
            if row.kind == "sketch" {
                assert_eq!(feature["type"], "sketch");
            }
        }
    }
}

#[test]
fn output_links_name_saved_features_and_inspection_writes_nothing() {
    let model = cube_with_history(10.0, first_profile(), None);
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);
    let rows = &report.outcome().unwrap().features;
    assert_eq!(rows.len(), 2);
    for (row, feature) in rows.iter().zip(document["features"].as_array().unwrap()) {
        assert_eq!(row.output.ids, vec![feature["id"].as_u64().unwrap()]);
    }
    let plan = writer.plan(&model, &mut Report::default());
    assert!(plan.features.iter().all(|row| row.output.ids.is_empty()
        && row.output.method == dutchman_core::OutputMatch::NotWritten));
}

#[test]
fn fallback_links_only_the_saved_reference_sketch() {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // Both rows deliberately have the same name; links must follow emission.
    for feature in wire["model"]["features"].as_array_mut().unwrap() {
        feature["name"] = json!("Same");
    }
    wire["model"]["features"][1]["definition"]["value"]["value"]["profile"]["value"] =
        json!({"kind": "sketch_profiles", "value": {"sketch": SKETCH, "profiles": [99]}});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let rows = &report.outcome().unwrap().features;
    assert_eq!(report.outcome().unwrap().mode, Mode::Import);
    let sketch = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["type"] == "sketch")
        .unwrap();
    assert_eq!(rows[0].output.ids, vec![sketch["id"].as_u64().unwrap()]);
    assert!(rows[1].output.ids.is_empty());
    assert_eq!(rows[1].output.method, dutchman_core::OutputMatch::NoOutput);
}

#[test]
fn measured_regions_require_complete_surface_comparisons_when_checking_the_final_shape() {
    for missing_face_counts in [false, true] {
        let host = std::sync::Arc::new(MeasuringEvaluator {
            unavailable_comparison: !missing_face_counts,
            missing_face_counts,
            ..MeasuringEvaluator::default()
        });
        let (_, report) = convert(
            &two_square_selection(),
            &SolidCraftWriter::new().evaluator(host),
        );
        let extrude = &report.outcome().unwrap().features[1];
        assert_eq!(extrude.status, dutchman_core::FeatureStatus::Blocked);
        assert!(extrude.inferred.is_none());
    }
}

/// Two independent, overlapping synthetic cubes. Only the first is modified by the
/// final extrude. The saved topology includes that result, independently of the recipe.
fn scoped_extrude(operation: &str) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    for arena in [
        "points", "vertices", "curves", "edges", "coedges", "loops", "surfaces", "faces", "shells",
        "regions", "bodies",
    ] {
        let text = serde_json::to_string(&wire["geometry"][arena]).unwrap();
        let mut rows: Vec<Value> =
            serde_json::from_str(&text.replace("synthetic:cube:", "synthetic:other:")).unwrap();
        if arena == "bodies" {
            rows[0]["name"] = json!("Other cube");
        }
        wire["geometry"][arena].as_array_mut().unwrap().extend(rows);
    }
    // Rescale only the first body's point and surface coordinates to the expected
    // remaining half. Its lines carry endpoint geometry too; transform those carriers.
    if operation != "join" {
        fn half(value: &mut Value, cut: bool) {
            if let Value::Object(fields) = value {
                if fields.contains_key("x") && fields.contains_key("y") && fields.contains_key("z")
                {
                    // Directions stay unit length; only placed points are edited below.
                    if let Some(z) = fields.get_mut("z") {
                        *z = json!(z.as_f64().unwrap() * 0.5 + if cut { 5.0 } else { 0.0 });
                    }
                } else {
                    for (key, child) in fields {
                        if matches!(
                            key.as_str(),
                            "value" | "position" | "origin" | "start" | "end" | "center"
                        ) {
                            half(child, cut);
                        }
                    }
                }
            }
        }
        for arena in ["points", "curves", "surfaces"] {
            for row in wire["geometry"][arena].as_array_mut().unwrap() {
                if row["id"].as_str().unwrap().starts_with("synthetic:cube:") {
                    if arena == "points" {
                        half(&mut row["position"], operation == "cut");
                    } else {
                        half(&mut row["geometry"], operation == "cut");
                    }
                }
            }
        }
        for edge in wire["geometry"]["edges"].as_array_mut().unwrap() {
            if matches!(
                edge["id"].as_str().unwrap(),
                "synthetic:cube:edge#8"
                    | "synthetic:cube:edge#9"
                    | "synthetic:cube:edge#10"
                    | "synthetic:cube:edge#11"
            ) {
                edge["carrier"]["param_range"] = json!([0.0, 5.0]);
            }
        }
    }
    let mut second = wire["model"]["features"][1].clone();
    second["id"] = json!("synthetic:other:feature#1");
    second["ordinal"] = json!(2);
    second["name"] = json!("Other extrusion");
    second["outputs"] = json!(["synthetic:other:body#0"]);
    let mut scoped = second.clone();
    scoped["id"] = json!("synthetic:scope:feature#1");
    scoped["ordinal"] = json!(3);
    scoped["name"] = json!("Scoped extrusion");
    scoped["outputs"] = json!(["synthetic:cube:body#0"]);
    scoped["definition"]["value"]["value"]["op"] = json!(operation);
    scoped["definition"]["value"]["value"]["extent"]["side"]["termination"]["length"] =
        json!(if operation == "join" { 10.0 } else { 5.0 });
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .extend([second, scoped]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

/// A source join consumes the coincident second cube, leaving only the first identity.
/// A later independent cube makes the join intermediate, with an exact stored centre.
fn intermediate_coincident_join() -> Model {
    let mut model = scoped_extrude("join");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // The saved second cube belongs to the later extrusion. The consumed body's
    // identity is available only from its producer, not the final topology.
    wire["model"]["features"][2]["outputs"] = json!(["synthetic:consumed:body#0"]);
    wire["model"]["features"][3]["source_properties"] = json!({
        "body_centers": "cube:5,5,5", "body_centers_exact": "cube"
    });
    let mut later = wire["model"]["features"][2].clone();
    later["id"] = json!("synthetic:later:feature#1");
    later["ordinal"] = json!(4);
    later["name"] = json!("Later extrusion");
    later["outputs"] = json!(["synthetic:other:body#0"]);
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(later);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

/// Recorded cube evaluations preserve the count difference that the anonymous
/// intermediate centre comparison permits: a scoped join leaves two coincident cubes.
#[derive(Debug, Default)]
struct CoincidentJoinEvaluator {
    scoped_join_seen: std::sync::atomic::AtomicBool,
}

impl dutchman_solidcraft::SolidCraftEvaluator for CoincidentJoinEvaluator {
    fn evaluate(
        &self,
        design: &[u8],
        timeout: std::time::Duration,
        label: &str,
    ) -> Result<Value, String> {
        let mut archive = ZipArchive::new(Cursor::new(design)).unwrap();
        let document: Value =
            serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
        let mut count = 0;
        for feature in document["features"].as_array().unwrap() {
            if feature["suppressed"] == true {
                continue;
            }
            match feature["type"].as_str() {
                Some("import") => count += 2,
                Some("extrude") if feature["operation"] == "new_body" => count += 1,
                Some("extrude") if feature["name"] == "Scoped extrusion" => {
                    if feature.get("targets").is_some() {
                        self.scoped_join_seen
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                    } else {
                        count -= 1;
                    }
                }
                _ => {}
            }
        }
        let mut result = dutchman_solidcraft::SolidCraftEvaluator::evaluate(
            &RecordedEvaluator::default(),
            design,
            timeout,
            label,
        )?;
        let body = result["bodies"][0].clone();
        result["bodies"] = json!(vec![body; count]);
        Ok(result)
    }

    fn run(
        &self,
        _design: &[u8],
        _commands: &[Value],
        _timeout: std::time::Duration,
        _label: &str,
    ) -> Result<Vec<Value>, String> {
        Err("The coincident join fixture needs only body evaluations".to_owned())
    }
}

#[test]
fn intermediate_coincident_join_does_not_confirm_scope_from_centres() {
    let host = std::sync::Arc::new(CoincidentJoinEvaluator::default());
    let (document, report) = convert(
        &intermediate_coincident_join(),
        &SolidCraftWriter::new()
            .evaluator(host.clone())
            .target_version(Some("0.44.0".parse().unwrap())),
    );
    let join = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|feature| feature["name"] == "Scoped extrusion")
        .unwrap();
    assert!(join.get("targets").is_none(), "{report:?}");
    assert!(!host
        .scoped_join_seen
        .load(std::sync::atomic::Ordering::SeqCst));
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("history/body-scope-confirmed")),
        "{report:?}"
    );
    assert_eq!(
        report.outcome().unwrap().check,
        Check::Verified,
        "{report:?}"
    );
}

#[test]
fn scoped_extrudes_keep_older_target_bytes() {
    let cli = solidcraft();
    for operation in ["join", "cut", "intersect"] {
        let model = scoped_extrude(operation);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        // The previous mapper ignored modified-body outputs. Removing them is the
        // legacy mapping of precisely the same geometry and construction recipe.
        wire["model"]["features"][3]["outputs"] = json!([]);
        let legacy = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..model.clone()
        };
        for target in ["0.18.2", "0.25.1", "0.40.1", "0.43.0", "0.43.9"] {
            let writer = SolidCraftWriter::new()
                .verification(Verification::Off)
                .target_version(Some(target.parse().unwrap()));
            assert_eq!(
                written_with(&model, &writer).0,
                written_with(&legacy, &writer).0,
                "{target} {operation}"
            );
        }
        // Scope inference requires replay. Exercise that path too: verification off
        // alone would not catch an accidental admission of scoped targets before 0.44.
        if let Some(cli) = &cli {
            let writer = SolidCraftWriter::new()
                .verification(Verification::With(cli.clone()))
                .target_version(Some("0.43.0".parse().unwrap()));
            assert_eq!(
                written_with(&model, &writer).0,
                written_with(&legacy, &writer).0,
                "0.43.0 with source checks: {operation}"
            );
        }
    }
}

#[test]
fn scoped_extrudes_are_confirmed_against_source_bodies() {
    let Some(cli) = solidcraft() else { return };
    for operation in ["join", "cut", "intersect"] {
        let writer = SolidCraftWriter::new()
            .verification(Verification::With(cli.clone()))
            .target_version(Some("0.44.0".parse().unwrap()));
        let (document, report) = convert(&scoped_extrude(operation), &writer);
        let scoped = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == "Scoped extrusion")
            .unwrap();
        assert_eq!(
            scoped["targets"],
            json!(["unit cube"]),
            "{operation}: {report:?}"
        );
        assert!(
            report
                .losses()
                .iter()
                .any(|loss| loss.code.as_deref() == Some("history/body-scope-confirmed")),
            "{report:?}"
        );
        assert_eq!(
            report.outcome().unwrap().check,
            Check::Verified,
            "{report:?}"
        );
    }
}

/// Remove the synthetic tool from final geometry, optionally keeping its historical
/// topology under the producer's identity. No native files are used.
fn consumed_boolean_tool(history_producer: Option<&str>) -> Model {
    let mut model = combined_spheres("cut", false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let mut topology = serde_json::Map::new();
    for (arena, rows) in wire["geometry"].as_object_mut().unwrap() {
        if let Some(rows) = rows.as_array_mut() {
            let is_tool = |row: &Value| {
                row["id"]
                    .as_str()
                    .is_some_and(|id| id.starts_with("synthetic:tool:"))
            };
            let removed: Vec<_> = rows.iter().filter(|row| is_tool(row)).cloned().collect();
            if arena == "bodies" {
                topology.insert("body".to_owned(), removed[0].clone());
            } else if matches!(
                arena.as_str(),
                "regions"
                    | "shells"
                    | "faces"
                    | "loops"
                    | "coedges"
                    | "edges"
                    | "vertices"
                    | "surfaces"
                    | "curves"
                    | "pcurves"
                    | "points"
            ) {
                topology.insert(arena.clone(), json!(removed));
            }
            rows.retain(|row| !is_tool(row));
        }
    }
    if let Some(producer) = history_producer {
        wire["model"]["history_bodies"] = json!([{
            "id": "synthetic:history-body:tool#0",
            "produced_by": producer,
            "state": {"ordinal": 1, "identifier": 7},
            "topology": topology,
        }]);
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn proved_consumed_boolean_tools_keep_names_only_on_044() {
    let model = consumed_boolean_tool(Some("f3d:model:feature#3"));
    for target in ["0.18.2", "0.43.0", "0.44.0"] {
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new()
                .verification(Verification::Off)
                .target_version(Some(target.parse().unwrap())),
        );
        let features = document["features"].as_array().unwrap();
        let combine = features.iter().find(|f| f["type"] == "combine");
        if target == "0.44.0" {
            assert_eq!(
                combine.unwrap()["tools"],
                features[1]["body_names"],
                "{report:?}"
            );
            assert!(features[1]["body_names"]
                .as_array()
                .is_some_and(|names| names.len() == 1));
        } else {
            assert!(combine.is_none(), "{target}: {report:?}");
            assert!(
                history_line(&report)
                    .contains("it selects a body that an earlier feature has not named"),
                "{target}: {report:?}"
            );
        }
    }
}

#[test]
fn unproved_consumed_boolean_tools_remain_blocked_on_every_target() {
    // A body retained for a different producer cannot prove this output's identity.
    for proof in [
        "absent",
        "wrong producer",
        "ambiguous bodies",
        "multiple outputs",
    ] {
        let producer = match proof {
            "absent" => None,
            "wrong producer" => Some("f3d:model:feature#1"),
            _ => Some("f3d:model:feature#3"),
        };
        let mut model = consumed_boolean_tool(producer);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        if proof == "ambiguous bodies" {
            let mut other = wire["model"]["history_bodies"][0].clone();
            other["id"] = json!("synthetic:history-body:tool#1");
            wire["model"]["history_bodies"]
                .as_array_mut()
                .unwrap()
                .push(other);
        } else if proof == "multiple outputs" {
            wire["model"]["features"][1]["outputs"]
                .as_array_mut()
                .unwrap()
                .push(json!("synthetic:other:body#0"));
        }
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        for target in ["0.18.2", "0.43.0", "0.44.0"] {
            let (document, report) = convert(
                &model,
                &SolidCraftWriter::new()
                    .verification(Verification::Off)
                    .target_version(Some(target.parse().unwrap())),
            );
            let features = document["features"].as_array().unwrap();
            assert!(
                features[1].get("body_names").is_none(),
                "{target}: {report:?}"
            );
            assert!(
                features.iter().all(|f| f["type"] != "combine"),
                "{target}: {report:?}"
            );
            assert!(
                history_line(&report)
                    .contains("it selects a body that an earlier feature has not named"),
                "{target}: {report:?}"
            );
        }
    }
}

#[test]
fn consumed_tool_names_are_discarded_when_source_confirmation_fails() {
    let Some(cli) = solidcraft() else { return };
    // The synthetic final cube differs from the replayed sphere cut. Availability
    // of a verifier alone cannot admit the trial names or replace the blocker.
    let (document, report) = convert(
        &consumed_boolean_tool(None),
        &SolidCraftWriter::new()
            .verification(Verification::With(cli))
            .target_version(Some("0.44.0".parse().unwrap())),
    );
    let features = document["features"].as_array().unwrap();
    assert!(features[1].get("body_names").is_none(), "{report:?}");
    assert!(
        features.iter().all(|f| f["type"] != "combine"),
        "{report:?}"
    );
    assert!(
        history_line(&report).contains("it selects a body that an earlier feature has not named"),
        "{report:?}"
    );
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.detail == "The source body check confirmed the consumed tool bodies"),
        "{report:?}"
    );
}

#[test]
fn generated_combine_tools_require_a_single_declared_producer_result() {
    for (members, selected, expected) in [
        (json!([{"kind":"body", "id":"tool"}]), "tool", true),
        (json!([{"kind":"body", "id":"tool"}]), "unknown", false),
        (
            json!([{"kind":"body", "id":"tool"}, {"kind":"body", "id":"other"}]),
            "tool",
            false,
        ),
    ] {
        let mut model = combined_spheres("cut", false);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        wire["model"]["feature_result_topologies"] = json!([{
            "id":"synthetic:history-result:state#0", "output_of":"f3d:model:feature#3", "members":members
        }]);
        wire["model"]["features"][2]["definition"]["value"]["value"]["operands"]["tools"] = json!({
            "kind":"generated", "value":{"bodies":[{"feature":"f3d:model:feature#3", "local_id":selected}], "native":"opaque tool"}
        });
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        for target in ["0.43.0", "0.44.0"] {
            let (document, report) = convert(
                &model,
                &SolidCraftWriter::new()
                    .verification(Verification::Off)
                    .target_version(Some(target.parse().unwrap())),
            );
            let mapped = document["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["type"] == "combine");
            assert_eq!(
                mapped.is_some(),
                expected && target == "0.44.0",
                "{target} {selected}: {report:?}"
            );
            if let Some(mapped) = mapped {
                assert_eq!(mapped["tools"], json!(["Tool"]));
            }
        }
    }
}

#[test]
fn scoped_extrudes_keep_the_previous_mapping_without_source_proof() {
    let writer = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.44.0".parse().unwrap()));
    for operation in ["join", "cut", "intersect"] {
        let (document, report) = convert(&scoped_extrude(operation), &writer);
        let feature = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["name"] == "Scoped extrusion")
            .unwrap();
        assert!(feature.get("targets").is_none(), "{report:?}");
        assert!(!report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("history/body-scope-confirmed")));
    }
}

#[test]
fn scoped_extrude_cuts_can_select_several_bodies_and_leave_another_whole() {
    let Some(cli) = solidcraft() else { return };
    let mut model = scoped_extrude("cut");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for arena in [
        "points", "vertices", "curves", "edges", "coedges", "loops", "surfaces", "faces", "shells",
        "regions", "bodies",
    ] {
        let rows: Vec<Value> = wire["geometry"][arena]
            .as_array()
            .unwrap()
            .iter()
            .filter(|row| row["id"].as_str().unwrap().starts_with("synthetic:cube:"))
            .cloned()
            .collect();
        let text = serde_json::to_string(&rows).unwrap();
        let mut rows: Vec<Value> =
            serde_json::from_str(&text.replace("synthetic:cube:", "synthetic:third:")).unwrap();
        if arena == "bodies" {
            rows[0]["name"] = json!("Third cube");
        }
        wire["geometry"][arena].as_array_mut().unwrap().extend(rows);
    }
    let mut third = wire["model"]["features"][2].clone();
    third["id"] = json!("synthetic:third:feature#1");
    third["name"] = json!("Third extrusion");
    third["ordinal"] = json!(3);
    third["outputs"] = json!(["synthetic:third:body#0"]);
    wire["model"]["features"][3]["ordinal"] = json!(4);
    wire["model"]["features"][3]["outputs"] =
        json!(["synthetic:cube:body#0", "synthetic:third:body#0"]);
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .insert(3, third);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new()
            .verification(Verification::With(cli))
            .target_version(Some("0.44.0".parse().unwrap())),
    );
    let scoped = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["name"] == "Scoped extrusion")
        .unwrap();
    assert_eq!(
        scoped["targets"],
        json!(["unit cube", "Third cube"]),
        "{report:?}"
    );
    assert_eq!(
        report.outcome().unwrap().check,
        Check::Verified,
        "{report:?}"
    );
}

#[test]
fn scoped_extrudes_reject_targets_that_disagree_with_the_source() {
    let Some(cli) = solidcraft() else { return };
    let mut model = scoped_extrude("cut");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // Both names resolve, but the second body is whole in the saved source.
    wire["model"]["features"][3]["outputs"] =
        json!(["synthetic:cube:body#0", "synthetic:other:body#0"]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new()
            .verification(Verification::With(cli))
            .target_version(Some("0.44.0".parse().unwrap())),
    );
    assert_ne!(
        report.outcome().unwrap().check,
        Check::Verified,
        "{report:?}"
    );
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("history/body-scope-confirmed")),
        "{report:?}"
    );
}

#[test]
fn body_mirror_outputs_name_consumed_generated_boolean_tools() {
    let Some(cli) = solidcraft() else { return };
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // Independently construct the saved result: a box from -10 to +10 along X.
    // The recipe instead builds the original cube, reflects it, then joins the pair.
    fn widen(value: &mut Value) {
        if let Value::Object(fields) = value {
            if fields.contains_key("x") && fields.contains_key("y") && fields.contains_key("z") {
                let x = fields.get_mut("x").unwrap();
                *x = json!(x.as_f64().unwrap() * 2.0 - 10.0);
            } else {
                for (key, child) in fields {
                    if matches!(key.as_str(), "value" | "position" | "origin") {
                        widen(child);
                    }
                }
            }
        }
    }
    for arena in ["points", "curves", "surfaces"] {
        for row in wire["geometry"][arena].as_array_mut().unwrap() {
            if arena == "points" {
                widen(&mut row["position"]);
            } else {
                widen(&mut row["geometry"]);
            }
        }
    }
    for edge in wire["geometry"]["edges"].as_array_mut().unwrap() {
        if matches!(
            edge["id"].as_str().unwrap(),
            "synthetic:cube:edge#0"
                | "synthetic:cube:edge#2"
                | "synthetic:cube:edge#4"
                | "synthetic:cube:edge#6"
        ) {
            edge["carrier"]["param_range"] = json!([0.0, 20.0]);
        }
    }
    let mut mirror = body_mirror_definition();
    mirror["value"]["pattern"]["plane_origin"] = xyz([0.0, 0.0, 0.0]);
    wire["model"]["features"].as_array_mut().unwrap().extend([
        json!({"id":"f3d:model:feature#3", "ordinal":2, "name":"Reflected cube", "suppressed":false,
            "definition":{"kind":"operation", "value":mirror}, "outputs":["synthetic:mirror:body#0"],
            "dependencies":[], "source_properties":{}, "source_content":[]}),
        json!({"id":"f3d:model:feature#4", "ordinal":3, "name":"Join cubes", "suppressed":false,
            "definition":{"kind":"operation", "value":{"kind":"combine", "value":{
                "operands":{"target":cube_body(), "tools":{"kind":"generated", "value":{
                    "bodies":[{"feature":"f3d:model:feature#3", "local_id":"reflected"}], "native":"Synthetic mirror output"}}},
                "op":"join", "keep_tools":false, "destination":"target_component"}}},
            "outputs":["synthetic:cube:body#0"], "dependencies":[], "source_properties":{}, "source_content":[]}),
    ]);
    wire["model"]["feature_result_topologies"] = json!([{"id":"synthetic:history-result:state#0",
        "output_of":"f3d:model:feature#3", "members":[{"kind":"body", "id":"reflected"}]}]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new()
            .verification(Verification::With(cli))
            .target_version(Some("0.44.0".parse().unwrap())),
    );
    let features = document["features"].as_array().unwrap();
    let mirror = features
        .iter()
        .find(|feature| feature["type"] == "mirror")
        .unwrap();
    let combine = features
        .iter()
        .find(|feature| feature["type"] == "combine")
        .unwrap();
    assert_eq!(mirror["body_names"], combine["tools"], "{report:?}");
    assert_eq!(mirror["body_names"], json!(["Body 1"]));
    assert!(
        report
            .losses()
            .iter()
            .any(|loss| loss.severity == Severity::Info
                && loss.detail == "The source body check confirmed the consumed tool bodies"),
        "{report:?}"
    );

    assert_eq!(
        report.outcome().unwrap().check,
        Check::Verified,
        "{report:?}"
    );
}

/// Code-built clockwise rounded plate (32 × 20, radius 3) and two capsule holes
/// (radius 1, straight length 4), extruded 5 mm. The carrier cube is unused with
/// verification off; expected measurements below come from these source sketch dimensions.
fn clockwise_rounded_plate(flipped: bool) -> Model {
    use std::f64::consts::{FRAC_PI_2, PI};
    let mut model = cube_with_history(5.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let arc = |key, center: [f64; 2], radius, start, end| {
        entity(
            key,
            json!({"kind":"arc","center":{"u":center[0],"v":center[1]},
            "radius":radius,"start_angle":start,"end_angle":end}),
        )
    };
    let mut entities = vec![
        arc(1, [3.0, 3.0], 3.0, -FRAC_PI_2, -PI),
        line(2, [0.0, 3.0], [0.0, 17.0]),
        arc(3, [3.0, 17.0], 3.0, PI, FRAC_PI_2),
        line(4, [3.0, 20.0], [29.0, 20.0]),
        arc(5, [29.0, 17.0], 3.0, FRAC_PI_2, 0.0),
        line(6, [32.0, 17.0], [32.0, 3.0]),
        arc(7, [29.0, 3.0], 3.0, 0.0, -FRAC_PI_2),
        line(8, [29.0, 0.0], [3.0, 0.0]),
    ];
    let ring = |keys: Vec<u32>| {
        keys.into_iter()
            .map(|key| json!({"entity":format!("f3d:model:sketch-entity#{key}"),"reversed":false}))
            .collect::<Vec<_>>()
    };
    let mut profiles = vec![ring((1..=8).collect())];
    for (key, x) in [(9, 10.0), (13, 22.0)] {
        entities.extend([
            line(key, [x - 1.0, 8.0], [x - 1.0, 12.0]),
            arc(key + 1, [x, 12.0], 1.0, PI, 0.0),
            line(key + 2, [x + 1.0, 12.0], [x + 1.0, 8.0]),
            arc(key + 3, [x, 8.0], 1.0, 0.0, -PI),
        ]);
        profiles.push(ring((key..key + 4).collect()));
    }
    wire["model"]["sketch_entities"] = json!(entities);
    wire["model"]["sketch_constraints"] = json!([]);
    wire["model"]["sketches"][0]["profiles"] = json!(profiles);
    if flipped {
        wire["model"]["sketches"][0]["placement"]["frame"]["normal"]["z"] = json!(-1.0);
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn clockwise_rounded_corners_and_slot_arcs_keep_source_sweeps_and_midpoints() {
    use std::f64::consts::TAU;
    for flipped in [false, true] {
        let model = clockwise_rounded_plate(flipped);
        let source = serde_json::to_value(&model.ir).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("2 of 2 features rebuilt"),
            "{report:?}"
        );
        let sketch = &document["features"][0]["sketch"];
        let arcs: Vec<_> = sketch["curves"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|c| c["type"] == "arc")
            .collect();
        let sources: Vec<_> = source["model"]["sketch_entities"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["geometry"]["kind"] == "arc")
            .collect();
        assert_eq!(arcs.len(), 8);
        for (curve, source) in arcs.into_iter().zip(sources) {
            let geometry = &source["geometry"];
            let center = [
                geometry["center"]["u"].as_f64().unwrap(),
                geometry["center"]["v"].as_f64().unwrap(),
            ];
            let point = |key: &str| {
                let p = &sketch["points"][curve[key].as_u64().unwrap() as usize];
                [p["pos"][0].as_f64().unwrap(), p["pos"][1].as_f64().unwrap()]
            };
            let angle = |p: [f64; 2]| (p[1] - center[1]).atan2(p[0] - center[0]);
            let a = angle(point("a"));
            let sweep = (angle(point("b")) - a).rem_euclid(TAU);
            let start = geometry["start_angle"].as_f64().unwrap();
            let end = geometry["end_angle"].as_f64().unwrap();
            assert!((sweep - (end - start).abs()).abs() < 1e-9, "{curve}");
            let mid = a + sweep / 2.0;
            let source_mid = (start + end) / 2.0;
            assert!((mid.cos() - source_mid.cos()).abs() < 1e-9);
            assert!((mid.sin() - source_mid.sin()).abs() < 1e-9);
        }
    }
}

#[test]
fn clockwise_rounded_plate_replays_with_source_face_count_area_and_volume() {
    let Some(cli) = solidcraft() else { return };
    let pi = std::f64::consts::PI;
    let source_area = 32.0 * 20.0 - (4.0 - pi) * 9.0 - 2.0 * (8.0 + pi);
    let source_perimeter = 96.0 + 10.0 * pi;
    for flipped in [false, true] {
        let result = evaluated(&cli, &written(&clockwise_rounded_plate(flipped)).0);
        assert_eq!(result["bodies"].as_array().unwrap().len(), 1);
        assert_eq!(result["total"]["faces"], 18, "{result}");
        let body = &result["bodies"][0];
        for (key, expected) in [
            ("volume_mm3", source_area * 5.0),
            ("area_mm2", 2.0 * source_area + 5.0 * source_perimeter),
        ] {
            let actual = body[key].as_f64().unwrap();
            // Same relative tolerance used by the existing curved-profile replay tests.
            assert!(
                (actual - expected).abs() / expected < 1e-3,
                "{key}: {actual} vs {expected}"
            );
        }
    }
}

/// Synthetic complete spatial entries with no location sketch in the source.
fn spatial_hole_array(change: impl FnOnce(&mut Value)) -> Model {
    holed_cube(2.0, |hole| {
        directed_hole(hole);
        hole["placements"] = json!([
            {"kind": "directed", "position": {"x": 3.0, "y": 3.0, "z": 10.0},
                "direction": {"x": 0.0, "y": 0.0, "z": -2.0}},
            {"kind": "directed", "position": {"x": 7.0, "y": 7.0, "z": 10.0},
                "direction": {"x": 0.0, "y": 0.0, "z": -1.0}}
        ]);
        change(hole);
    })
}

fn spatial_hole_writer() -> SolidCraftWriter {
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.44.1".parse().unwrap()))
}

#[test]
fn complete_spatial_hole_arrays_get_an_editable_location_sketch() {
    let (document, report) = convert(&spatial_hole_array(|_| {}), &spatial_hole_writer());
    assert!(
        history_line(&report).starts_with("4 of 4 features rebuilt"),
        "{}",
        history_line(&report)
    );
    let features = document["features"].as_array().unwrap();
    let hole = features.iter().find(|f| f["type"] == "hole").unwrap();
    assert_eq!(hole["direction"], json!([0.0, 0.0, -1.0]));
    assert_eq!(hole["position"], json!([3.0, 3.0, 10.0]));
    let sketch = features
        .iter()
        .find(|f| f["id"] == hole["points"]["sketch"])
        .unwrap();
    let origin: Vec<f64> =
        serde_json::from_value(sketch["plane"]["plane"]["origin"].clone()).unwrap();
    let x: Vec<f64> = serde_json::from_value(sketch["plane"]["plane"]["x_dir"].clone()).unwrap();
    let y: Vec<f64> = serde_json::from_value(sketch["plane"]["plane"]["y_dir"].clone()).unwrap();
    let ids = hole["points"]["ids"].as_array().unwrap();
    assert_eq!(ids.len(), 2);
    for (id, expected) in ids.iter().zip([[3.0, 3.0, 10.0], [7.0, 7.0, 10.0]]) {
        let point = sketch["sketch"]["points"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == *id)
            .unwrap();
        let uv: Vec<f64> = serde_json::from_value(point["pos"].clone()).unwrap();
        for (axis, expected) in expected.into_iter().enumerate() {
            assert!((origin[axis] + uv[0] * x[axis] + uv[1] * y[axis] - expected).abs() < 1e-9);
        }
    }
}

#[test]
fn spatial_hole_arrays_refuse_unproven_entries_and_unsupported_placements() {
    let cases: Vec<(Model, &str)> = vec![
        (
            spatial_hole_array(|h| h["placements"][1]["direction"]["z"] = json!(1.0)),
            "different directions",
        ),
        (
            spatial_hole_array(|h| h["placements"][1]["position"]["z"] = json!(9.0)),
            "different entry planes",
        ),
        (
            spatial_hole_array(|h| {
                h["face"] = json!({"kind": "faces", "value": ["synthetic:cube:face#top"]});
                h["placements"][1]["position"]["x"] = json!(20.0);
            }),
            "does not drill into its entry face",
        ),
        (
            spatial_hole_array(
                |h| h["placements"][1] = json!({"kind": "axis", "origin": {"x": 7.0, "y": 7.0, "z": 10.0}, "axis": {"x": 0.0, "y": 0.0, "z": -1.0}}),
            ),
            "multiple spatial hole placements",
        ),
        (
            spatial_hole_array(|h| h["placements"] = json!([])),
            "empty or exceed",
        ),
        (
            spatial_hole_array(|h| {
                h["profile"] = json!({"kind": "sketch", "value": HOLES});
                h["profile_filter"] = json!("circles");
            }),
            "its placements are not its sketch locations",
        ),
    ];
    for (model, reason) in cases {
        let (document, report) = convert(&model, &spatial_hole_writer());
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
        assert!(!document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["type"] == "hole"));
    }
    let older = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.18.2".parse().unwrap()));
    let (_, report) = convert(&spatial_hole_array(|_| {}), &older);
    assert!(
        history_line(&report)
            .contains("multiple spatial hole locations need SolidCraft 0.44 patch 1"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn solidcraft_drills_complete_spatial_hole_arrays_without_a_source_sketch() {
    let Some(cli) = closed_blend_cli() else {
        return;
    };
    let (document, report) = convert(&spatial_hole_array(|_| {}), &spatial_hole_writer());
    assert!(
        history_line(&report).starts_with("4 of 4 features rebuilt"),
        "{}",
        history_line(&report)
    );
    assert!(document["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "hole"));
    let pi = std::f64::consts::PI;
    let side = spatial_hole_array(|hole| {
        for (placement, coordinate) in hole["placements"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .zip([3.0, 7.0])
        {
            placement["position"] = json!({"x": 10.0, "y": coordinate, "z": coordinate});
            placement["direction"] = json!({"x": -1.0, "y": 0.0, "z": 0.0});
        }
    });
    let blind = spatial_hole_array(|hole| {
        hole["extent"] = json!({"kind": "blind", "length": 4.0});
        hole["bottom"] = json!({"kind": "flat"});
        hole["face"] = json!({"kind": "faces", "value": ["synthetic:cube:face#top"]});
    });
    for (model, expected) in [
        (spatial_hole_array(|_| {}), 1000.0 - 20.0 * pi),
        (side, 1000.0 - 20.0 * pi),
        (blind, 1000.0 - 8.0 * pi),
    ] {
        let mut bytes = Vec::new();
        spatial_hole_writer()
            .write(&model, &mut bytes, &mut Report::default())
            .unwrap();
        let actual = volumes(&cli, &bytes);
        assert_eq!(actual.len(), 1);
        assert!((actual[0] - expected).abs() < 1e-6, "{actual:?}");
    }
}

#[test]
fn opaque_generated_edge_identities_are_supported_cadir_but_need_source_geometry() {
    for kind in ["fillet", "chamfer"] {
        let dimension = if kind == "fillet" {
            json!({"kind": "constant", "radius": 1.0})
        } else {
            json!({"kind": "distance", "distance": 1.0})
        };
        let model = input_blend(kind, dimension, false);
        for identity in ["3,8,12", "opaque generated edge", "n:sketch-entity:side"] {
            let mut wire = serde_json::to_value(&model.ir).unwrap();
            wire["model"]["features"][2]["definition"]["value"]["value"]["groups"][0]["edges"] = json!({"kind": "generated", "value": {
                "edges": [{"feature": "f3d:model:feature#2", "local_id": identity}], "native": "synthetic selection"}});
            let model = Model {
                ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
                ..model.clone()
            };
            let (_, report) = convert(&model, &spatial_hole_writer());
            let reason = "generated edge identity has no resolved source geometry, which is not supported yet";
            assert!(
                history_line(&report).contains(reason),
                "{}",
                history_line(&report)
            );
        }
    }
}

/// Ordered edges and their endpoints do not bind radius parameter zero to a
/// chain end. Neither carrier reversal nor missing endpoints may invent that binding.
#[test]
fn variable_fillet_without_radius_origin_keeps_source_geometry() {
    for (reverse, endpoints) in [(false, false), (true, false), (false, true), (true, true)] {
        let base = input_blend(
            "fillet",
            json!({"kind": "variable", "points": [
            {"parameter": 0.0, "radius": 0.5},
            {"parameter": 0.4, "radius": 1.5},
            {"parameter": 1.0, "radius": 1.0}]}),
            false,
        );
        let mut wire = serde_json::to_value(&base.ir).unwrap();
        let m = &mut wire["model"];
        m["features"][2]["definition"]["value"]["value"]["groups"][0]["edges"]["value"]["edges"] =
            json!(["near", "opposite"]);
        m["feature_input_topologies"][0]["faces"] =
            json!(["front", "left", "back", "right", "top"]);
        let samples = m["feature_input_topologies"][0]["edge_geometry"]
            .as_array_mut()
            .unwrap();
        // A connected, ordered open chain still has no radius-origin binding.
        samples[1]["faces"] = json!(["front", "top"]);
        samples[1]["interior"] = json!({"x": 5.0, "y": 0.0, "z": 10.0});
        samples[1]["tangent"] = json!({"x": 1.0, "y": 0.0, "z": 0.0});
        samples[1]["start"] = json!({"x": 0.0, "y": 0.0, "z": 10.0});
        samples[1]["end"] = json!({"x": 10.0, "y": 0.0, "z": 10.0});
        for sample in samples {
            if !endpoints {
                let row = sample.as_object_mut().unwrap();
                row.remove("start");
                row.remove("end");
            }
            if reverse {
                for axis in ["x", "y", "z"] {
                    sample["tangent"][axis] = json!(-sample["tangent"][axis].as_f64().unwrap());
                }
            }
        }
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..base
        };
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report)
                .contains("their radius stations are not bound to a source start vertex"),
            "{report:?}"
        );
        assert_partial(&document, &report, 1);
        assert!(!document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature["type"] == "fillet"));
    }
}

/// An opaque producer edge and ordered endpoint radii supply neither input
/// geometry nor a binding between a radius station and a spatial chain endpoint.
#[test]
fn variable_fillet_generated_edge_without_radius_origin_keeps_source_geometry() {
    for (start, end) in [(3.0, 1.0), (1.0, 3.0)] {
        let base = input_blend(
            "fillet",
            json!({"kind": "variable", "points": [
                {"parameter": 0.0, "radius": start},
                {"parameter": 1.0, "radius": end}]}),
            false,
        );
        let mut wire = serde_json::to_value(&base.ir).unwrap();
        wire["model"]["feature_input_topologies"] = json!([]);
        wire["model"]["features"][2]["definition"]["value"]["value"]["groups"][0]["edges"] = json!({"kind": "generated", "value": {
                "edges": [{"feature": "f3d:model:feature#2", "local_id": "synthetic-edge"}],
                "native": "synthetic selection"}});
        let model = Model {
            ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
            ..base
        };
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report)
                .contains("their radius stations are not bound to a source start vertex"),
            "{report:?}"
        );
        assert_partial(&document, &report, 1);
        assert!(!document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature["type"] == "fillet"));
    }
}

/// Invented annular blank with a curved side notch and a separate round cut
/// contour in the revolve sketch, then an axial cut and a historical fillet pick.
/// Only the rectangle is in the source profile table; both cut contours stay solid.
fn unresolved_revolve_before_trimmed_cuts(explicit_all: bool) -> Model {
    use std::f64::consts::{FRAC_PI_2, PI, TAU};
    let mut model = ring_history(json!({"kind":"one_sided",
        "termination":{"kind":"angle", "angle":TAU}}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["parameters"] = json!([]);
    m["sketch_entities"] = json!([
        line(1, [7.0, 0.0], [19.0, 0.0]),
        line(2, [19.0, 0.0], [19.0, -26.0]),
        line(3, [19.0, -26.0], [7.0, -26.0]),
        line(4, [7.0, -26.0], [7.0, 0.0]),
        line(5, [19.0, -10.0], [14.0, -10.0]),
        entity(
            6,
            json!({"kind":"arc", "center":{"u":14.0,"v":-13.0},
            "radius":3.0, "start_angle":FRAC_PI_2, "end_angle":FRAC_PI_2 + PI})
        ),
        line(7, [14.0, -16.0], [19.0, -16.0]),
        circle(8, [27.0, -13.0], 2.0),
    ]);
    m["features"][1]["outputs"] = json!(["synthetic:cube:body#0"]);
    m["features"][1]["definition"]["value"]["value"]["construction"]["value"]["profile"] =
        if explicit_all {
            whole_sketch()
        } else {
            json!({"kind":"sketch_selection", "value":{"sketch":SKETCH,
                "selections":["synthetic:native:selection#0"]}})
        };
    let boundary = |key: u32, range: [f64; 2], reversed| {
        json!({
        "entity":format!("f3d:model:sketch-entity#{key}"),
        "parameter_range":range, "reversed":reversed})
    };
    let profile = json!({"kind":"sketch_regions", "value":{"sketch":SKETCH,
        "regions":[{"kind":"trimmed", "outer_boundary":[
            boundary(5, [0.0, 1.0], false),
            boundary(6, [FRAC_PI_2, FRAC_PI_2 + PI], false),
            boundary(7, [0.0, 1.0], false),
            boundary(2, [10.0 / 26.0, 16.0 / 26.0], true)], "hole_boundaries":[]},
            {"kind":"trimmed", "outer_boundary":[boundary(8, [0.0, TAU], false)],
                "hole_boundaries":[]}]}});
    let cut = |id: &str, ordinal, profile: Value, direction: &str, extent: Value| {
        json!({
        "id":id, "ordinal":ordinal, "name":format!("Cut {ordinal}"), "suppressed":false,
        "dependencies":[], "outputs":["synthetic:cube:body#0"],
        "source_properties":{}, "source_content":[],
        "definition":{"kind":"operation", "value":{"kind":"extrude", "value":{
            "profile":{"kind":"planar", "value":profile}, "direction":{"kind":direction},
            "start":{"kind":"profile_plane"}, "extent":extent, "op":"cut"}}}})
    };
    m["features"].as_array_mut().unwrap().push(cut(
        "synthetic:shared:feature#cut",
        2,
        profile,
        "profile_normal",
        json!({"kind":"symmetric", "side":{"termination":{"kind":"through_all"}}}),
    ));
    let second = "synthetic:axial:sketch#0";
    let mut sketch = m["sketches"][0].clone();
    sketch["id"] = json!(second);
    sketch["placement"]["frame"] = json!({"origin":{"x":0,"y":0,"z":26},
        "normal":{"x":0,"y":0,"z":1}, "u_axis":{"x":1,"y":0,"z":0}});
    sketch["profiles"] = json!([(21..=24)
        .map(|key| json!({
        "entity":format!("f3d:model:sketch-entity#{key}"),"reversed":false}))
        .collect::<Vec<_>>()]);
    m["sketches"].as_array_mut().unwrap().push(sketch);
    for mut line in [
        line(21, [10.0, 8.0], [23.0, 8.0]),
        line(22, [23.0, 8.0], [23.0, 11.0]),
        line(23, [23.0, 11.0], [10.0, 11.0]),
        line(24, [10.0, 11.0], [10.0, 8.0]),
    ] {
        line["sketch"] = json!(second);
        m["sketch_entities"].as_array_mut().unwrap().push(line);
    }
    let mut sketch_feature = m["features"][0].clone();
    sketch_feature["id"] = json!("synthetic:axial:feature#sketch");
    sketch_feature["ordinal"] = json!(3);
    sketch_feature["definition"]["value"]["value"]["sketch"]["sketch"] = json!(second);
    m["features"].as_array_mut().unwrap().push(sketch_feature);
    m["features"].as_array_mut().unwrap().push(cut(
        "synthetic:axial:feature#cut",
        4,
        json!({"kind":"sketch","value":second}),
        "reversed_profile_normal",
        json!({"kind":"one_sided",
            "side":{"termination":{"kind":"through_all"}}}),
    ));
    m["features"].as_array_mut().unwrap().push(json!({
        "id":"synthetic:shared:feature#fillet", "ordinal":5, "name":"Corner round",
        "suppressed":false, "dependencies":[], "outputs":["synthetic:cube:body#0"],
        "source_properties":{}, "source_content":[], "definition":{"kind":"operation",
        "value":{"kind":"fillet","value":{"groups":[{"radius":{"kind":"constant","radius":0.4},
            "edges":{"kind":"historical","value":{"state":"synthetic:shared:input#0",
            "edges":["synthetic:shared:edge#0"],"native":"synthetic corner"}}}]}}}}));
    m["feature_input_topologies"] = json!([{"id":"synthetic:shared:input#0",
        "input_of":"synthetic:shared:feature#fillet", "bodies":[],"faces":[],"vertices":[],
        "edges":["synthetic:shared:edge#0"], "edge_geometry":[{"edge":"synthetic:shared:edge#0",
            "start":{"x":10,"y":8,"z":0},"end":{"x":10,"y":8,"z":26},
            "interior":{"x":10,"y":8,"z":13},"tangent":{"x":0,"y":0,"z":1},"faces":[]}]}]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn unresolved_revolve_does_not_take_extra_regions_needed_by_later_cuts_and_a_fillet() {
    let (document, report) = convert(
        &unresolved_revolve_before_trimmed_cuts(false),
        &SolidCraftWriter::new()
            .verification(Verification::Off)
            .replay(Verification::Off),
    );
    assert_eq!(
        report.outcome().unwrap().features[1].status,
        dutchman_core::FeatureStatus::Blocked,
        "{report:?}"
    );
    assert_reference_sketches(&document, 2);
    assert!(document["features"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["type"] != "revolve" && f["type"] != "fillet"));
}

#[test]
fn unresolved_revolve_synthetic_control_has_more_regions_than_its_stored_loop() {
    use dutchman_solidcraft::{CliEvaluator, SolidCraftEvaluator};
    let Some(cli) = solidcraft() else { return };
    // The control is written for the newest design format; older releases can't evaluate it.
    if dutchman_solidcraft::SolidCraftVersion::detected(&Verification::With(cli.clone()))
        < dutchman_solidcraft::SolidCraftVersion::V0_44_0
    {
        eprintln!("skipped: the control needs SolidCraft 0.44 or newer");
        return;
    }
    // An explicit all-regions control reproduces the old native-pick assumption.
    // The later cuts keep the notch curves solid in the shared sketch.
    let (bytes, report) = written_with(
        &unresolved_revolve_before_trimmed_cuts(true),
        &SolidCraftWriter::new()
            .verification(Verification::Off)
            .replay(Verification::Off),
    );
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let document: Value =
        serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
    let revolve = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "revolve")
        .unwrap_or_else(|| panic!("{report:?}"));
    let result = CliEvaluator::new(cli)
        .run(
            &bytes,
            &[
                json!({"command":"timeline.roll_to","params":{"feature":revolve["id"]}}),
                json!({"command":"document.inspect","params":{}}),
            ],
            std::time::Duration::from_secs(120),
            "synthetic shared sketch",
        )
        .unwrap();
    assert_eq!(
        result[1]["bodies"].as_array().unwrap().len(),
        2,
        "{result:?}"
    );
}

#[test]
fn unresolved_revolve_keeps_construction_and_reference_helpers() {
    let mut model = ring_history(json!({"kind":"one_sided",
        "termination":{"kind":"angle","angle":std::f64::consts::TAU}}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["features"][1]["definition"]["value"]["value"]["construction"]["value"]["profile"] = json!({"kind":"sketch_selection", "value":{"sketch":SKETCH,
            "selections":["synthetic:native:selection#0"]}});
    let mut helper = circle(5, [7.5, -5.0], 1.0);
    helper["construction"] = json!(true);
    m["sketch_entities"].as_array_mut().unwrap().extend([
        helper,
        entity(
            6,
            json!({"kind":"reference_line", "origin":{"u":0,"v":0},
            "direction":{"u":0,"v":1}}),
        ),
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(
        report.outcome().unwrap().features[1].status,
        dutchman_core::FeatureStatus::Mapped,
        "{report:?}"
    );
    assert_eq!(document["features"][1]["profiles"]["type"], "all");
}

/// Synthetic refusals follow the actual feature rows, including hidden history.
#[derive(Debug)]
struct FailedRebuild {
    feature_type: &'static str,
    original_unavailable: bool,
}

impl dutchman_solidcraft::SolidCraftEvaluator for FailedRebuild {
    fn evaluate(
        &self,
        design: &[u8],
        timeout: std::time::Duration,
        label: &str,
    ) -> Result<Value, String> {
        if label == "original" && self.original_unavailable {
            return Ok(json!({"evaluation_failed": "Synthetic source measurement refusal"}));
        }
        let mut result = dutchman_solidcraft::SolidCraftEvaluator::evaluate(
            &RecordedEvaluator::default(),
            design,
            timeout,
            label,
        )?;
        let mut archive = ZipArchive::new(Cursor::new(design)).unwrap();
        let document: Value =
            serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
        result["timeline"] = json!(document["features"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|feature| feature["type"] == self.feature_type && feature["suppressed"] != true)
            .map(
                |feature| json!({"id": feature["id"], "name": feature["name"],
                "error": "Synthetic rebuild refusal"})
            )
            .collect::<Vec<_>>());
        Ok(result)
    }

    fn run(
        &self,
        design: &[u8],
        commands: &[Value],
        timeout: std::time::Duration,
        label: &str,
    ) -> Result<Vec<Value>, String> {
        dutchman_solidcraft::SolidCraftEvaluator::run(
            &RecordedEvaluator::default(),
            design,
            commands,
            timeout,
            label,
        )
    }
}

#[test]
fn failed_rebuilds_keep_the_final_shape_and_name_the_feature_even_without_source_measurements() {
    for original_unavailable in [false, true] {
        let model = cube_with_history(10.0, first_profile(), None);
        let writer = SolidCraftWriter::new().evaluator(std::sync::Arc::new(FailedRebuild {
            feature_type: "extrude",
            original_unavailable,
        }));
        let (document, report) = convert(&model, &writer);
        let (source, _) = convert(
            &model,
            &SolidCraftWriter::new()
                .import_only(true)
                .verification(Verification::Off),
        );
        let source_blob = source["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|feature| feature["type"] == "import")
            .unwrap()["step_blob"]
            .clone();
        let features = document["features"].as_array().unwrap();
        assert!(features
            .iter()
            .any(|feature| feature["type"] == "import" && feature["step_blob"] == source_blob));
        assert!(
            features.iter().all(|feature| feature["type"] != "extrude"),
            "{document}"
        );
        assert!(
            features
                .iter()
                .any(|feature| feature["type"] == "import" && feature["suppressed"] == false),
            "{document}"
        );
        let blocked = report
            .outcome()
            .unwrap()
            .features
            .iter()
            .find(|feature| feature.kind == "extrude")
            .unwrap();
        assert_eq!(blocked.status, dutchman_core::FeatureStatus::Blocked);
        let reason = blocked.reason.as_ref().unwrap();
        assert!(reason.contains(blocked.name.as_ref().unwrap()), "{reason}");
        assert!(reason.contains("Synthetic rebuild refusal"), "{reason}");
    }
}

#[test]
fn a_refused_source_import_reports_missing_geometry_as_a_last_resort() {
    let model = cube_with_history(10.0, first_profile(), None);
    let host = std::sync::Arc::new(FailedRebuild {
        feature_type: "import",
        original_unavailable: false,
    });
    let writer = SolidCraftWriter::new()
        .import_only(true)
        .evaluator(host.clone());
    let (document, report) = convert(&model, &writer);
    assert!(
        document["features"]
            .as_array()
            .unwrap()
            .iter()
            .all(|feature| feature["type"] != "import"),
        "{document}"
    );
    assert!(
        report.losses().iter().any(
            |loss| loss.code.as_deref() == Some("geometry/import-failed")
                && loss.detail.contains("MISSING")
                && loss.detail.contains("Synthetic rebuild refusal")
                && loss.detail.contains(&loss.subject)
        ),
        "{report:?}"
    );
}

#[cfg(unix)]
#[test]
fn a_nonzero_evaluator_exit_without_named_failures_writes_an_unverified_design() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = tempfile::tempdir().unwrap();
    let cli = scratch.path().join("solidcraft-cli");
    std::fs::write(&cli, "#!/bin/sh\nif [ \"$1\" = --version ]; then echo 'solidcraft-cli 0.45.0'; exit 0; fi\nexit 1\n").unwrap();
    std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
    let writer = SolidCraftWriter::new().verification(Verification::With(cli));
    let (document, report) = convert(&cube_with_history(10.0, first_profile(), None), &writer);
    assert!(!document["features"].as_array().unwrap().is_empty());
    assert!(
        matches!(&report.outcome().unwrap().check,
        Check::Unverified(why) if why.contains("exited with")),
        "{report:?}"
    );
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("geometry/import-failed")),
        "{report:?}"
    );
}

#[test]
fn feature_profiles_take_the_complete_bound_sketch_for_extrudes_and_revolves() {
    let writer = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_45_0));
    let full = json!({"kind": "one_sided", "termination": {
        "kind": "angle", "angle": std::f64::consts::TAU}});
    let partial = json!({"kind": "one_sided", "termination": {
        "kind": "angle", "angle": std::f64::consts::FRAC_PI_2}});
    let mut no_profile_table = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&no_profile_table.ir).unwrap();
    wire["model"]["sketches"][0]["profiles"] = json!([]);
    no_profile_table.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    for (mut model, path) in [
        (
            cube_with_history(10.0, whole_sketch(), None),
            "/model/features/1/definition/value/value/profile/value",
        ),
        (
            no_profile_table,
            "/model/features/1/definition/value/value/profile/value",
        ),
        (
            ring_history(full),
            "/model/features/1/definition/value/value/construction/value/profile",
        ),
        (
            ring_history(partial),
            "/model/features/1/definition/value/value/construction/value/profile",
        ),
    ] {
        let (expected, expected_report) = convert(&model, &writer);
        assert_eq!(expected_report.outcome().unwrap().mode, Mode::History);
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        *wire.pointer_mut(path).unwrap() =
            json!({"kind": "feature", "value": "f3d:model:feature#1"});
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (actual, report) = convert(&model, &writer);
        assert_eq!(report.outcome().unwrap().mode, Mode::History, "{report:?}");
        assert_eq!(actual, expected);
    }
}

#[test]
fn feature_profiles_require_an_earlier_active_resolved_planar_sketch() {
    let writer = SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_45_0));
    for case in [
        "missing",
        "self",
        "suppressed",
        "later",
        "unresolved",
        "not_sketch",
    ] {
        let mut model = cube_with_history(
            10.0,
            json!({"kind": "feature", "value": "f3d:model:feature#1"}),
            None,
        );
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        match case {
            "missing" => {
                wire["model"]["features"][1]["definition"]["value"]["value"]["profile"]["value"]
                    ["value"] = json!("f3d:model:feature#999")
            }
            "self" => {
                wire["model"]["features"][1]["definition"]["value"]["value"]["profile"]["value"]
                    ["value"] = json!("f3d:model:feature#2")
            }
            "suppressed" => wire["model"]["features"][0]["suppressed"] = json!(true),
            "later" => wire["model"]["features"][0]["ordinal"] = json!(2),
            "unresolved" => {
                wire["model"]["features"][0]["definition"]["value"]["value"]["sketch"] =
                    json!({"kind": "unresolved"})
            }
            "not_sketch" => {
                wire["model"]["features"][0]["definition"]["value"] =
                    json!({"kind": "datum_principal_plane", "value": {"plane": "top"}})
            }
            _ => assert_eq!(case, "not_sketch", "unknown rejection case"),
        }
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(&model, &writer);
        assert_ne!(
            report.outcome().unwrap().mode,
            Mode::History,
            "{case}: {report:?}"
        );
        assert!(
            document["features"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["type"] != "extrude"),
            "{case}: {document}"
        );
        // The unresolved producer can itself stop the timeline first.
        if case != "unresolved" {
            assert!(
                report.losses().iter().any(|loss| {
                    loss.detail.contains(
                        "its feature profile has no earlier active resolved planar sketch mapping",
                    )
                }),
                "{case}: {report:?}"
            );
        }
    }
}

#[test]
fn feature_profiles_still_compare_the_selected_area_with_the_source_cap() {
    let mut model = cube_with_history(
        10.0,
        json!({"kind": "feature", "value": "f3d:model:feature#1"}),
        None,
    );
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for entity in wire["model"]["sketch_entities"].as_array_mut().unwrap() {
        for end in ["start", "end"] {
            let u = &mut entity["geometry"][end]["u"];
            *u = json!(u.as_f64().unwrap() * 2.0);
        }
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new()
            .verification(Verification::Off)
            .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_45_0)),
    );
    assert_ne!(report.outcome().unwrap().mode, Mode::History);
    assert!(
        report.losses().iter().any(|loss| loss.detail.contains(
            "its profile region doesn't match the source (swept 200.0 mm², the source 100.0 mm²)"
        )),
        "{report:?}"
    );
}

#[test]
fn feature_profiles_keep_original_geometry_for_targets_before_045() {
    let model = cube_with_history(
        10.0,
        json!({"kind": "feature", "value": "f3d:model:feature#1"}),
        None,
    );
    for target in ["0.18.2", "0.44.1"] {
        let writer = SolidCraftWriter::new()
            .verification(Verification::Off)
            .target_version(Some(target.parse().unwrap()));
        let (document, report) = convert(&model, &writer);
        assert_ne!(report.outcome().unwrap().mode, Mode::History);
        assert!(document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["type"] == "import"));
        assert!(
            report.losses().iter().any(|loss| loss
                .detail
                .contains("profiles from sketch features require SolidCraft 0.45 or newer")),
            "{target}: {report:?}"
        );
    }
}

#[test]
fn feature_profiles_do_not_treat_reference_axes_as_profile_geometry() {
    let mut model = cube_with_history(
        10.0,
        json!({"kind": "feature", "value": "f3d:model:feature#1"}),
        None,
    );
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketches"][0]["profiles"] = json!([]);
    wire["model"]["sketch_constraints"] = json!([]);
    wire["model"]["sketch_entities"] = json!([
        entity(
            1,
            json!({"kind": "reference_line", "origin": {"u": 0.0, "v": 0.0},
                         "direction": {"u": 1.0, "v": 0.0}})
        ),
        entity(
            2,
            json!({"kind": "reference_line", "origin": {"u": 0.0, "v": 0.0},
                         "direction": {"u": 0.0, "v": 1.0}})
        ),
        entity(
            3,
            json!({"kind": "point", "position": {"u": 0.0, "v": 0.0}})
        ),
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new()
            .verification(Verification::Off)
            .target_version(Some(dutchman_solidcraft::SolidCraftVersion::V0_45_0)),
    );
    assert_ne!(report.outcome().unwrap().mode, Mode::History);
    assert!(document["features"]
        .as_array()
        .unwrap()
        .iter()
        .all(|f| f["type"] != "extrude"));
    assert!(
        report.losses().iter().any(|loss| loss
            .detail
            .contains("its sketch feature has no resolved non-construction profile curves")),
        "{report:?}"
    );
}
