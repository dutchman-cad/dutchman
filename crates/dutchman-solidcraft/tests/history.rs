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
    json!({
        "id": format!("f3d:model:sketch-entity#{key}"),
        "sketch": SKETCH,
        "construction": false,
        "geometry": geometry,
    })
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
    let model = &mut wire["model"];
    model["bodies"][0]["color"] = json!({"r": 0.8, "g": 0.2, "b": 0.1, "a": 1.0});
    model["sketches"] = json!([{
        "id": SKETCH,
        "name": "Base",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [[
            {"entity": "f3d:model:sketch-entity#1"},
            {"entity": "f3d:model:sketch-entity#2"},
            {"entity": "f3d:model:sketch-entity#3"},
            {"entity": "f3d:model:sketch-entity#4"},
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
            json!({"kind": "native", "native_kind": "Fusion only", "entities": ["f3d:model:sketch-entity#3"]})
        ),
    ]);
    let extent = extent.unwrap_or_else(|| {
        json!({"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": length}}})
    });
    model["features"] = json!([
        {
            "id": "f3d:model:feature#1",
            "ordinal": 0,
            "name": "Base sketch",
            "suppressed": false,
            "definition": {"definition": "sketch", "sketch": {"space": "planar", "sketch": SKETCH}},
        },
        {
            "id": "f3d:model:feature#2",
            "ordinal": 1,
            "name": "Block",
            "suppressed": false,
            "dependencies": ["f3d:model:feature#1"],
            "outputs": ["synthetic:cube:body#0"],
            "definition": {
                "definition": "extrude",
                "profile": profile,
                "direction": {"kind": "profile_normal"},
                "start": {"kind": "profile_plane"},
                "extent": extent,
                "op": "new_body",
            },
        },
    ]);
    model["parameters"] = json!([
        {
            "id": "f3d:model:parameter#1",
            "owner": "f3d:model:feature#2",
            "name": "d1",
            "expression": format!("{length} mm"),
            "value": {"kind": "length", "value": length},
        },
        {
            "id": "f3d:model:parameter#2",
            "owner": "f3d:model:feature#1",
            "ordinal": 1,
            "name": "side",
            "expression": "10 mm",
            "value": {"kind": "length", "value": 10.0},
        },
    ]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "cube".to_owned(),
        source_file: Some("cube.f3d".to_owned()),
        assembly: None,
    }
}

const TOP: &str = "f3d:model:sketch#2";

/// The example cube built instead by a loft from its 10 mm base square to a square of side
/// `top` centred above it, 10 mm up; `change` edits the loft's definition.
fn loft_history(top: f64, change: impl FnOnce(&mut Value)) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    let mut sketches = model["sketches"].as_array().unwrap().clone();
    let (a, b) = (5.0 - top / 2.0, 5.0 + top / 2.0);
    let top_loop: Vec<Value> = [11, 12, 13, 14]
        .map(|k| json!({"entity": format!("f3d:model:sketch-entity#{k}")}))
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
    let mut definition = json!({
        "definition": "loft",
        "sections": [
            first_profile(),
            {"kind": "sketch_profiles", "value": {"sketch": TOP, "profiles": [0]}},
        ],
        "guidance": {"kind": "guides", "path": []},
        "op": "new_body",
    });
    change(&mut definition);
    model["features"] = json!([
        model["features"][0].clone(),
        {
            "id": "f3d:model:feature#3",
            "ordinal": 1,
            "name": "Top sketch",
            "suppressed": false,
            "definition": {"definition": "sketch", "sketch": {"space": "planar", "sketch": TOP}},
        },
        {
            "id": "f3d:model:feature#2",
            "ordinal": 2,
            "name": "Block",
            "suppressed": false,
            "dependencies": ["f3d:model:feature#1", "f3d:model:feature#3"],
            "outputs": ["synthetic:cube:body#0"],
            "definition": definition,
        },
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
    let mut definition = json!({
        "definition": "sweep",
        "shape": {"mode": "solid", "op": "new_body",
                  "section": {"kind": "profile", "value": first_profile()}},
        "path": {"kind": "sketch_curves",
                 "value": {"sketch": PATH, "curves": ["f3d:model:sketch-entity#21"]}},
        "path_tangent": false,
        "linearize": false,
        "path_extent": {"along_fraction": 1.0, "against_fraction": 0.0},
    });
    change(&mut definition);
    model["features"] = json!([
        model["features"][0].clone(),
        {
            "id": "f3d:model:feature#3",
            "ordinal": 1,
            "name": "Path sketch",
            "suppressed": false,
            "definition": {"definition": "sketch", "sketch": {"space": "planar", "sketch": PATH}},
        },
        {
            "id": "f3d:model:feature#2",
            "ordinal": 2,
            "name": "Block",
            "suppressed": false,
            "dependencies": ["f3d:model:feature#1", "f3d:model:feature#3"],
            "outputs": ["synthetic:cube:body#0"],
            "definition": definition,
        },
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
        history_line(&report).contains("3 of 3 features mapped"),
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
                    d["shape"]["mode"] = json!("surface");
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
            assert!(history_line(&report).contains("3 of 3 features mapped"));
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
    assert!(history_line(&report).contains("3 of 3 features mapped"));
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
        json!({
            "id": "f3d:model:feature#4", "ordinal": 2, "name": "Rail sketch",
            "suppressed": false,
            "definition": {"definition": "sketch", "sketch": {"space": "planar", "sketch": RAIL}},
        }),
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
        assert!(history_line(&report).contains("4 of 4 features mapped"));
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
        d["shape"]["mode"] = json!("surface");
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
            json!({"shape": {"mode": "surface", "section": {"kind": "unresolved", "value": null}}}),
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
            json!({"shape": {"mode": "solid", "op": "new_body",
                "section": {"kind": "profile", "value": first_profile()},
                "sections": [{"kind": "profile", "value": first_profile()}]}}),
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
            json!({"shape": {"mode": "solid", "op": "new_body",
                "section": {"kind": "generated", "value": {
                    "shape": "circular_region", "region": {"outer_radius": 2.0}}}}}),
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
        history_line(&report).contains("3 of 3 features mapped"),
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
            json!({"guidance": {"kind": "centerline", "path": {"kind": "native", "value": "n"}}}),
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
    wire["model"]["points"].as_array_mut().unwrap().push(json!({
        "id": "synthetic:apex:point#0",
        "position": {"x": 5.0, "y": 5.0, "z": 10.0},
    }));
    wire["model"]["vertices"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": "synthetic:apex:vertex#0", "point": "synthetic:apex:point#0",
        }));
    let index = if first { 0 } else { 1 };
    wire["model"]["features"][2]["definition"]["sections"][index] =
        json!({"kind": "vertex", "value": "synthetic:apex:vertex#0"});
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
            assert!(history_line(&report).contains("3 of 3 features mapped"));
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
            "regions": [{"region": "loops", "outer": 1, "holes": []}]},
        }));
    });
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketches"][0]["profiles"]
        .as_array_mut()
        .unwrap()
        .push(json!([
            {"entity": "f3d:model:sketch-entity#41"},
            {"entity": "f3d:model:sketch-entity#42"},
            {"entity": "f3d:model:sketch-entity#43"},
            {"entity": "f3d:model:sketch-entity#44"},
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
    assert!(history_line(&report).contains("3 of 3 features mapped"));
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
        definition["guidance"] = json!({"kind": "guides", "path": [
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
        json!({
            "id": "f3d:model:feature#4",
            "ordinal": 2,
            "name": "Rail sketch",
            "suppressed": false,
            "definition": {"definition": "sketch", "sketch": {"space": "planar", "sketch": RAIL}},
        }),
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
    m["features"][3]["definition"]["guidance"]["path"] = json!([{"kind": "sketch", "value": RAIL}]);
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
    assert!(history_line(&report).contains("4 of 4 features mapped"));

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
        definition["guidance"] = json!({"kind": "guides", "path": [
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
        history_line(&report).contains("4 of 4 features mapped"),
        "{}",
        history_line(&report)
    );

    // Three rails are more than SolidCraft takes.
    let three = loft_history(10.0, |definition| {
        let rail = json!({"kind": "sketch_curves", "value": {"sketch": TOP, "curves": ["f3d:model:sketch-entity#11"]}});
        definition["guidance"] =
            json!({"kind": "guides", "path": [rail.clone(), rail.clone(), rail]});
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
            history_line(&report).contains("3 of 3 features mapped"),
            "{}",
            history_line(&report)
        );
    }
    let railed = railed_loft(10.0);
    let mut wire = serde_json::to_value(&railed.ir).unwrap();
    for feature in wire["model"]["features"].as_array_mut().unwrap() {
        if feature["definition"]["definition"] == "loft" {
            feature["definition"]["ruled"] = json!(true);
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
    (serde_json::from_str(&text).unwrap(), report)
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

#[test]
fn a_sketch_and_extrude_history_is_written() {
    let model = cube_with_history(10.0, first_profile(), None);
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);

    assert_eq!(
        history_line(&report),
        "2 of 2 features mapped; unverified: verification turned off"
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

#[test]
fn import_only_never_writes_history() {
    let model = cube_with_history(10.0, first_profile(), None);
    let (document, report) = convert(&model, &SolidCraftWriter::new().import_only(true));
    assert_eq!(history_line(&report), "not written (--import-only)");
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
        "2 of 2 features mapped; verified against the original in SolidCraft"
    );
    assert_eq!(types(&document).len(), 3);
}

#[test]
fn a_history_that_replays_differently_is_flattened() {
    let Some(cli) = solidcraft() else { return };
    // The history says 4 mm; the geometry is the 10 mm cube.
    let model = cube_with_history(4.0, first_profile(), None);
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::With(cli)),
    );
    let line = history_line(&report);
    assert!(line.starts_with("flattened: replay differs"), "{line}");
    assert_eq!(types(&document), [("import".to_owned(), false)]);
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

/// SolidCraft's body volumes for a design.
fn volumes(cli: &Path, design: &[u8]) -> Vec<f64> {
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
    result["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|body| body["volume_mm3"].as_f64().unwrap())
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
    m["features"][1] = json!({
        "id": "f3d:model:feature#2",
        "ordinal": 1,
        "name": "Ring",
        "suppressed": false,
        "definition": {
            "definition": "revolve",
            "construction": {
                "state": "resolved",
                "profile": whole_sketch(),
                "axis": {
                    "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
                },
                "extent": extent,
            },
            "op": "new_body",
        },
    });
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn changed_ring(change: impl FnOnce(&mut Value)) -> Model {
    let mut model = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": std::f64::consts::TAU}}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    change(&mut wire["model"]["features"][1]["definition"]["construction"]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn written(model: &Model) -> (Vec<u8>, Report) {
    let mut report = Report::default();
    let mut bytes = Vec::new();
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .write(model, &mut bytes, &mut report)
        .unwrap();
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
    assert!(history_line(&report).starts_with("2 of 2 features mapped"));
    let revolve = &document["features"][1];
    assert_eq!(revolve["type"], "revolve");
    assert_eq!(revolve["angle"], "360 deg");
    assert_eq!(
        revolve["axis"],
        json!({"type": "line", "origin": [0.0, 0.0, 0.0], "dir": [0.0, 0.0, 1.0]})
    );
    assert_eq!(revolve["convention"], "sketch_normal");

    let half = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": std::f64::consts::PI}}));
    let (_, report) = convert(
        &half,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("which way it turns"),
        "{}",
        history_line(&report)
    );
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
        assert!(history_line(&report).starts_with("2 of 2 features mapped"));
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
                construction["axis"]["reference"] = reference;
            }
            construction["axis"]["origin"]["z"] = json!(3.0);
            construction["axis"]["direction"]["z"] = json!(-2.0);
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).starts_with("2 of 2 features mapped"));
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
        construction["axis"]["origin"]["x"] = json!(5.0);
        construction["axis"]["reference"] = json!({"kind": "sketch_curves", "value": {
            "sketch": SKETCH, "curves": ["f3d:model:sketch-entity#4"]}});
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features mapped"));
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
fn surface_revolves_short_of_a_full_turn_map() {
    let quarter = json!({"kind": "symmetric",
        "termination": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2}});
    let model = changed_ring(|construction| {
        construction["solid"] = json!(false);
        construction["face_maker_class"] = json!("Part::FaceMakerBullseye");
        construction["extent"] = quarter;
    });
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features mapped"));
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

    let model = changed_ring(|construction| construction["solid"] = json!(false));
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
        let model = changed_ring(|construction| construction["face_maker_class"] = json!(maker));
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
                construction[key] = value.clone();
            }
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("2 of 2 features mapped"),
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
            construction["state"] = json!("unresolved");
            construction["missing"] = json!(missing);
            construction.as_object_mut().unwrap().remove(missing);
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
    m["sketches"].as_array_mut().unwrap().push(json!({
        "id": HOLES,
        "name": "Holes",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 10.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
    }));
    m["sketch_entities"].as_array_mut().unwrap().push(json!({
        "id": "f3d:model:sketch-entity#9",
        "sketch": HOLES,
        "construction": false,
        "geometry": {"kind": "circle", "center": {"u": 5.0, "v": 5.0}, "radius": 2.0},
    }));
    let mut hole = json!({
        "definition": "hole",
        "profile": {"kind": "sketch", "value": HOLES},
        "profile_filter": "circles",
        "direction": {"x": 0.0, "y": 0.0, "z": -1.0},
        "shape": {"construction": {"construction": "form", "kind": {"kind": "simple"}},
                  "diameter": diameter},
        "extent": {"kind": "through_all"},
    });
    change(&mut hole);
    let features = m["features"].as_array_mut().unwrap();
    features.push(json!({
        "id": "f3d:model:feature#3",
        "ordinal": 2,
        "name": "Hole sketch",
        "suppressed": false,
        "definition": {"definition": "sketch", "sketch": {"space": "planar", "sketch": HOLES}},
    }));
    features.push(json!({
        "id": "f3d:model:feature#4",
        "ordinal": 3,
        "name": "Hole",
        "suppressed": false,
        "dependencies": ["f3d:model:feature#3", "f3d:model:feature#2"],
        "definition": hole,
    }));
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
        history_line(&report).starts_with("4 of 4 features mapped"),
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

#[test]
fn counterbores_and_countersinks_keep_their_entry_dimensions() {
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
            let model = holed_cube(4.0, |hole| {
                hole["shape"]["construction"]["kind"] = form.clone();
                if blind {
                    hole["extent"] = json!({"kind": "blind", "length": 6.0});
                    hole["bottom"] = json!({"kind": "flat"});
                }
            });
            let (document, report) = convert(
                &model,
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            assert!(
                history_line(&report).starts_with("4 of 4 features mapped"),
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
                hole["shape"]["construction"]["kind"] = form.clone();
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
    // 6 mm to its shoulder, with a 118° drill point below.
    let angle = 118_f64.to_radians();
    let blind = holed_cube(4.0, |hole| {
        hole["extent"] = json!({"kind": "blind", "length": 6.0});
        hole["bottom"] = json!({"kind": "angled", "included_angle": angle, "depth_to_tip": false});
    });
    let point = 2.0 / (angle / 2.0).tan();
    close(
        &volumes(&cli, &written(&blind).0),
        1000.0 - bore * 6.0 - bore * point / 3.0,
    );
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
            history_line(&report).starts_with("4 of 4 features mapped"),
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
        history_line(&report).starts_with("4 of 4 features mapped"),
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
        entities.push(json!({
            "id": "f3d:model:sketch-entity#10", "sketch": HOLES, "construction": false,
            "geometry": if point {
                json!({"kind": "point", "position": {"u": 3.0, "v": 3.0}})
            } else {
                json!({"kind": "circle", "center": {"u": 3.0, "v": 3.0}, "radius": 1.0})
            },
        }));
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("4 of 4 features mapped"),
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
            hole["shape"]["construction"]["kind"] = json!({"kind": "simple_drilled",
                "drill_point_angle": std::f64::consts::FRAC_PI_2});
            hole["extent"] = json!({"kind": "blind", "length": 6.0});
        }),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("4 of 4 features mapped"),
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
            hole["shape"]["construction"]["specification"] = cosmetic_hole_thread();
            if blind {
                hole["extent"] = json!({"kind": "blind", "length": 6.0});
            }
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("4 of 4 features mapped"),
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
            hole["shape"]["construction"]["specification"] = cosmetic_hole_thread();
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
    let thread_note = |change: Value| {
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
            hole["shape"]["construction"]["specification"] = specification;
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            history_line(&report).starts_with("4 of 4 features mapped"),
            "{change}: {}",
            history_line(&report)
        );
        let note = report
            .losses()
            .iter()
            .find(|loss| loss.detail.contains("thread"))
            .map(|loss| loss.detail.clone())
            .unwrap_or_default();
        (document["features"][3]["thread"].clone(), note)
    };
    let (label, note) = thread_note(json!({"hand": "left", "class": "6H",
        "depth": {"kind": "blind", "depth": 3.0}}));
    assert_eq!(label, "M6x1");
    assert_eq!(
        note,
        "its thread is kept as a cosmetic M6x1 label over the whole hole, without its \
         left-hand direction, its 6H tolerance class, and its 3 mm thread length"
    );
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
        hole["shape"]["construction"]["specification"] = specification;
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
            json!({"kind": "counterbore_drilled", "diameter": 8.0, "depth": 2.0, "drill_point_angle": 1.5}),
            "a counterbore with a drill point is not mapped yet",
        ),
        (
            json!({"kind": "counterdrill", "diameter": 8.0, "entry_diameter": 9.0, "depth": 2.0, "angle": 1.5}),
            "a counterdrill with a conical entry and cylindrical recess is not mapped yet",
        ),
    ] {
        let model = holed_cube(4.0, |hole| {
            hole["shape"]["construction"]["kind"] = form.clone()
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
                hole["shape"]["construction"]["kind"] = if kind == "counterbore" {
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
            let (_, report) = convert(
                &model,
                &SolidCraftWriter::new().verification(Verification::Off),
            );
            assert!(
                history_line(&report).contains("a counterbore or countersink with a drill point")
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
            json!({"shape": {"diameter": 4.0, "construction": {"construction": "native_thread",
                "major_diameter": 6.0, "thread_depth": 6.0, "pitch": 1.0,
                "drill_point_angle": 1.5}}}),
            "a hole with native thread geometry is not mapped yet",
        ),
        (
            json!({"shape": {"diameter": 4.0, "construction": {"construction": "form",
                "kind": {"kind": "simple"}}, "exit_kind": {"kind": "chamfer", "diameter": 6.0,
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
            json!({"shape": {"diameter": 4.0, "construction": {"construction": "form",
            "kind": {"kind": "simple"}, "specification": {"kind": "threaded", "standard": "ISO",
                "modeled": true, "cosmetic": false, "hand": "right", "depth": {"kind": "hole_depth"}}}}}),
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
        wire["model"]["features"][1]["definition"]["extent"]["side"]["termination"]["length"] =
            json!(8.0);
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
                hole["shape"]["construction"]["kind"] = json!({"kind": "simple_drilled",
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
    let start = json!({"kind": "start", "entity": "f3d:model:sketch-entity#1"});
    m["sketch_constraints"] = json!([
        constraint(
            1,
            json!({"kind": "coincident_loci",
            "loci": [start, {"kind": "start", "entity": "f3d:model:sketch-entity#2"}]})
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
    m["parameters"].as_array_mut().unwrap().push(json!({
        "id": "f3d:model:parameter#3",
        "owner": "f3d:model:feature#1",
        "ordinal": 2,
        "name": "theta",
        "expression": format!("{} deg", theta.to_degrees()),
        "value": {"kind": "angle", "value": theta},
    }));
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
            "regions": [{"region": "loops", "outer": 0}]}})
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
        assert!(history_line(&report).starts_with("2 of 2 features mapped"));
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
        assert!(curves.iter().all(|curve| curve["construction"] == false));
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

#[test]
fn overlapping_crossing_curves_keep_the_refusal() {
    let mut model = crossed_profile_history(false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketch_entities"]
        .as_array_mut()
        .unwrap()
        .push(line(11, [0.0, 0.0], [10.0, 10.0]));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(types(&document), [("import".to_owned(), false)]);
    assert!(history_line(&report).contains("a profile crossed by its own sketch's curves"));
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
    assert!(history_line(&report).starts_with("2 of 2 features mapped"));
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
        .map(|(i, _)| json!([{"entity": format!("f3d:model:sketch-entity#{}", i + 1)}]))
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
    assert!(history_line(&report).starts_with("2 of 2 features mapped"));
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
        assert_eq!(types(&document), [("import".to_owned(), false)]);
        assert!(history_line(&report).contains("a profile cadmpeg does not resolve"));
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

/// The shape cadmpeg 0.6's Inventor decoder gives an extrude: its profile as native profile
/// selections of the sketch, an explicit direction, a symmetric or through-next extent. In a
/// sketch of one profile the selection can only be that profile; multiple loops must
/// identify one material region by even-odd nesting, otherwise the history is refused.
#[test]
fn an_inventor_shaped_extrude_maps_only_when_its_sketch_has_one_profile() {
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
    assert_eq!(types(&document), [("import".to_owned(), false)]);
    assert_eq!(
        history_line(&report),
        "flattened: feature \"Block\": a profile cadmpeg does not resolve, in a sketch of 2 \
         profiles, is not mapped yet"
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
/// origin and `Second` twice, 30 mm and 60 mm along X.
fn assembly_of_histories() -> Model {
    let part = |name: &str| Model {
        name: name.to_owned(),
        source_file: Some(format!("{name}.ipt")),
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
        name: None,
        part,
        transform: at(x),
    };
    Model {
        name: "pair".to_owned(),
        source_file: Some("pair.iam".to_owned()),
        assembly: Some(dutchman_core::Assembly {
            joints: Vec::new(),
            left_out: Vec::new(),
            parts: vec![part("First"), part("Second")],
            placements: vec![placed(0, 0.0), placed(1, 30.0), placed(1, 60.0)],
        }),
        ..part("pair")
    }
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
            "First.ipt: 2 of 2 features mapped; unverified: verification turned off",
            "Second.ipt: 2 of 2 features mapped; unverified: verification turned off"
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
            wire["model"]["bodies"][0]["color"] = json!({"r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0});
        } else {
            for face in wire["model"]["faces"].as_array_mut().unwrap() {
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
        .push(json!({
            "id": "f3d:model:feature#3",
            "ordinal": 2,
            "name": "Copies",
            "suppressed": false,
            "definition": {
                "definition": "pattern",
                "seeds": [{"kind": "feature", "value": "f3d:model:feature#2"}],
                "pattern": layout,
            },
        }));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

/// Change a synthetic pattern's operands without passing through a native decoder.
fn pattern_seeds(mut model: Model, seeds: Value) -> Model {
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][2]["definition"]["seeds"] = seeds;
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
            patterned(json!({"kind": "linear",
                "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3})),
            json!([{"kind": "bodies", "value": selection}]),
        );
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(history_line(&report).starts_with("3 of 3 features mapped"));
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
        assert!(history_line(&report).starts_with("3 of 3 features mapped"));
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
        {"pattern": {"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0},
            "spacing": 20.0, "count": 3}},
        {"pattern": {"kind": "linear", "direction": {"x": 0.0, "y": 1.0, "z": 0.0},
            "spacing": 30.0, "count": 2}},
    ]}));
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("3 of 3 features mapped"));
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
    features.push(json!({"id": "f3d:model:feature#4", "ordinal": 3,
        "name": "Copies", "suppressed": false, "definition": {
            "definition": "pattern", "seeds": [{"kind": "feature", "value": "f3d:model:feature#2"}],
            "pattern": {"kind": "curve_driven", "path": path, "spacing": 20.0, "count": 3}}}));
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
        assert!(history_line(&report).starts_with("4 of 4 features mapped"));
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
            json!({"kind": "circular_angles", "axis_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0}, "angles": [0.0, std::f64::consts::TAU]}),
            "repeating the seed at a full turn",
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
            json!({"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0},
            "spacing": 20.0, "count": 10001}),
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
    let layout = json!({"kind": "linear",
        "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3});
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
            json!({"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0},
            "spacing": 20.0, "count": 3}),
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

    let too_many = patterned(json!({"kind": "linear",
        "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 101,
        "second": {"direction": {"x": 0.0, "y": 1.0, "z": 0.0}, "spacing": 20.0, "count": 100}}));
    let (_, report) = convert(
        &too_many,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).contains("more than 10000 instances"));
}

#[test]
fn a_feature_pattern_names_its_seed() {
    let model = patterned(json!({"kind": "linear",
        "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3}));
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features mapped"),
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
    let linear = patterned(json!({"kind": "linear",
        "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3}));
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
    assert!(history_line(&report).starts_with("3 of 3 features mapped"));
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
    for face in wire["model"]["faces"].as_array_mut().unwrap() {
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
            "poles": {"form": "polynomial", "points": [
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
        history_line(&report).starts_with("3 of 3 features mapped"),
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
    model["features"] = json!([{
        "id": "f3d:model:feature#2",
        "ordinal": 0,
        "name": "Ball",
        "suppressed": false,
        "outputs": ["synthetic:cube:body#0"],
        "definition": definition,
    }]);
    model["parameters"] = json!([]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

fn sphere() -> Value {
    json!({"definition": "sphere", "center": {"x": 5.0, "y": 5.0, "z": 5.0}, "radius": 5.0, "op": "new_body"})
}

fn torus(axis: [f64; 3]) -> Value {
    json!({"definition": "torus", "center": {"x": 0.0, "y": 0.0, "z": 2.0},
           "axis": {"x": axis[0], "y": axis[1], "z": axis[2]},
           "major_radius": 10.0, "minor_radius": 2.0, "op": "new_body"})
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
        history_line(&report).contains("1 of 1 features mapped"),
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

#[test]
fn primitives_solidcraft_cannot_place_flatten_the_history() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    for (definition, reason) in [
        (
            torus([1.0, 0.0, 0.0]),
            "a torus about an axis other than Z is not mapped yet",
        ),
        (
            json!({"definition": "block", "dimensions": [10.0, 10.0, 10.0],
                   "placement": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
                   "op": "new_body"}),
            "cadmpeg does not say whether a box's placement is its corner or its centre",
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
    for (definition, expected) in [
        (sphere(), 4.0 / 3.0 * pi * 125.0),
        (torus([0.0, 0.0, 1.0]), 2.0 * pi * pi * 10.0 * 4.0),
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
    json!({"definition": "coil",
           "construction": {
               "placement": {"kind": "explicit",
                   "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                   "axis": {"x": 0.0, "y": 0.0, "z": 1.0},
                   "radial": {"x": radial[0], "y": radial[1], "z": radial[2]}},
               "diameter": 20.0,
               "extent": extent,
               "section": {"kind": "circular", "diameter": 2.0},
               "section_placement": "center",
               "clockwise": clockwise,
               "taper": 0.0},
           "result": {"kind": "new_body"}})
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
        history_line(&report).contains("1 of 1 features mapped"),
        "{}",
        history_line(&report)
    );

    let mut other = coil(
        json!({"kind": "revolutions_height", "revolutions": 2.0, "height": 8.0}),
        [0.0, 1.0, 0.0],
        false,
    );
    other["construction"]["section"] = json!({"kind": "square", "size": 1.5});
    other["construction"]["section_placement"] = json!("inside");
    other["result"] = json!({"kind": "boolean", "operation": "join", "targets": {"kind": "bodies", "value": ["synthetic:cube:body#0"]}});
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
                definition["construction"]["placement"]["axis"] =
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
        let mut at = &mut definition;
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
        assert_eq!(types(&document), [("import".to_owned(), false)]);
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
                definition["construction"]["placement"]["axis"] =
                    json!({"x": axis[0], "y": axis[1], "z": axis[2]});
                let base = [3.0, -2.0, 5.0];
                definition["construction"]["placement"]["origin"] =
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
fn a_helix_surface_left_out_of_the_step_is_named_in_the_report() {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    // One of the cube's faces on a coil's swept surface, as Fusion stores a coil's.
    let surface = model["faces"][0]["surface"].clone();
    for row in model["surfaces"].as_array_mut().unwrap() {
        if row["id"] == surface {
            row["geometry"] = json!({"kind": "procedural", "construction": "synthetic:cube:procedural_surface#0"});
        }
    }
    model["procedural_surfaces"] = json!([{
        "id": "synthetic:cube:procedural_surface#0",
        "surface": surface,
        "definition": {"kind": "helix", "construction": {
            "angle_range": [-std::f64::consts::PI, std::f64::consts::PI],
            "dimension_range": [0.0, 250.0],
            "path": {"angle_range": [0.0, 25.0],
                     "center": {"x": 0.0, "y": 0.0, "z": 0.0},
                     "major": {"x": 10.0, "y": 0.0, "z": 0.0},
                     "minor": {"x": 0.0, "y": 10.0, "z": 0.0},
                     "pitch": {"x": 0.0, "y": 0.0, "z": 5.0},
                     "apex_factor": 0.0,
                     "axis": {"x": 0.0, "y": 0.0, "z": 1.0}},
            "profile": {"kind": "circle", "length": 0.0, "radius": 1.0}}},
        "record_bounds": [null, null, null, null],
    }]);
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    };
    let (_, report) = convert(&model, &SolidCraftWriter::new().import_only(true));
    let named = report
        .losses()
        .iter()
        .find(|loss| loss.subject == "geometry" && loss.detail.contains("helix surface"))
        .map(|loss| (loss.severity, loss.detail.clone()));
    assert_eq!(
        named,
        Some((
            Severity::Degraded,
            "a helix surface (the swept face of a coil or thread) has no form cadmpeg writes to \
             STEP and is left out: the solid it bounds opens without it, with no volume"
                .to_owned()
        ))
    );
}

/// The holed cube's history with its sketch and extrude replaced by a base feature holding
/// the cube's body as it is, as Fusion holds a body modelled directly or imported.
fn based_cube() -> Model {
    let mut model = holed_cube(4.0, |_| {});
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    let body = m["bodies"][0]["id"].clone();
    let features = m["features"].as_array_mut().unwrap();
    features.retain(|feature| feature["ordinal"].as_u64() >= Some(2));
    features.push(json!({
        "id": "f3d:model:feature#1",
        "ordinal": 0,
        "name": "Base Feature 1",
        "suppressed": false,
        "definition": {"definition": "base_feature",
            "bodies": {"kind": "resolved", "value": {"bodies": [body], "native": "f3d:base#1"}}},
    }));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_base_feature_is_imported_where_it_stands_in_the_timeline() {
    let (document, report) = convert(
        &based_cube(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features mapped"),
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
            ("import".to_owned(), true),
        ]
    );
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
        "1 of 1 features mapped; verified against the original in SolidCraft"
    );
}

/// The example cube's history with its extrude replaced by a sheet-metal base flange of the
/// same square, `thickness` thick on `side` of the sketch.
fn flanged_cube(thickness: f64, side: &str) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for feature in wire["model"]["features"].as_array_mut().unwrap() {
        if feature["definition"]["definition"] == "extrude" {
            feature["name"] = json!("BaseFlange 1");
            feature["definition"] = json!({
                "definition": "sheet_metal_base_flange",
                "profile": whole_sketch(),
                "thickness": thickness,
                "side": side,
            });
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
        history_line(&report).starts_with("2 of 2 features mapped"),
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
        "2 of 2 features mapped; verified against the original in SolidCraft"
    );
}

/// The example cube under a timeline of `features`, each `(name, definition)`.
fn cube_under(features: &[(&str, Value)]) -> Model {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut wire = serde_json::to_value(&ir).unwrap();
    wire["model"]["features"] = features
        .iter()
        .enumerate()
        .map(|(i, (name, definition))| {
            json!({"id": format!("inventor:model:feature#{i}"), "ordinal": i, "name": name,
                   "suppressed": false, "definition": definition})
        })
        .collect();
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "cube".to_owned(),
        source_file: Some("cube.ipt".to_owned()),
        assembly: None,
    }
}

#[test]
fn a_design_cadmpeg_decodes_no_feature_of_has_no_history_to_flatten() {
    // An Inventor part made of a base solid: its one feature is kept in its native form.
    let native = json!({"definition": "native", "kind": "InventorClass:0843c7c2"});
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
    // Beside a feature that does map, a native one still flattens the history.
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    if let Some(features) = wire["model"]["features"].as_array_mut() {
        features.push(
            json!({"id": "f3d:model:feature#9", "ordinal": 9, "name": "Base1",
                             "suppressed": false, "definition": native}),
        );
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let mut report = Report::default();
    let outcome = SolidCraftWriter::new().plan(&model, &mut report);
    assert!(
        matches!(outcome.check, Check::Flattened(_)),
        "{:?}",
        outcome.check
    );
}

/// A design of user parameters and no geometry, as a Fusion design other designs take their
/// parameters from: `W1` = 20 mm, `half` = W1 / 2 and an angle.
fn parameters_alone() -> Model {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut wire = serde_json::to_value(&ir).unwrap();
    if let Some(arenas) = wire["model"].as_object_mut() {
        for rows in arenas.values_mut() {
            if rows.is_array() {
                *rows = json!([]);
            }
        }
    }
    wire["model"]["parameters"] = json!([
        {"id": "f3d:model:parameter#1", "ordinal": 0, "name": "W1", "expression": "20 mm",
         "value": {"kind": "length", "value": 20.0}},
        {"id": "f3d:model:parameter#2", "ordinal": 1, "name": "half", "expression": "W1 / 2",
         "value": {"kind": "length", "value": 10.0},
         "dependencies": ["f3d:model:parameter#1"]},
        {"id": "f3d:model:parameter#3", "ordinal": 2, "name": "tilt", "expression": "15 deg",
         "value": {"kind": "angle", "value": 15.0_f64.to_radians()}},
    ]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "Parameters".to_owned(),
        source_file: Some("Parameters.f3d".to_owned()),
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
    change(&mut wire["model"]["features"][1]["definition"]);
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
            assert!(history_line(&report).starts_with("2 of 2 features mapped"));
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
        next["definition"]["start"] = json!({
            "kind": "from_face", "face": selection, "offset": offset
        });
        next["definition"]["extent"]["side"]["termination"]["length"] = json!(2.0);
    } else {
        next["definition"]["start"] = json!({"kind": "offset_profile_plane", "offset": 12.0});
        next["definition"]["direction"] = json!({"kind": "reversed_profile_normal"});
        next["definition"]["extent"]["side"]["termination"] = json!({
            "kind": "to_face", "face": selection, "offset": offset
        });
    }
    wire["model"]["features"].as_array_mut().unwrap().push(next);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
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
                history_line(&report).starts_with("3 of 3 features mapped"),
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
        assert!(history_line(&report).starts_with("3 of 3 features mapped"));
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
    assert!(history_line(&report).starts_with("2 of 2 features mapped"));
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
    assert!(history_line(&report).starts_with("2 of 2 features mapped"));
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
            "not resolved to model faces",
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
        let termination =
            &mut wire["model"]["features"][2]["definition"]["extent"]["side"]["termination"];
        termination["face"] = selection;
        model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert_eq!(types(&document), [("import".to_owned(), false)]);
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
fn a_two_sided_extrude_can_end_at_a_face_along_the_positive_normal() {
    let mut model = face_ended_extrude(false, 0.0, false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let definition = &mut wire["model"]["features"][2]["definition"];
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
    assert!(history_line(&report).starts_with("3 of 3 features mapped"));
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
    wire["model"]["features"][2]["definition"]["direction"] = json!({"kind": "profile_normal"});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).contains("not along the selected extrude side"));
}
