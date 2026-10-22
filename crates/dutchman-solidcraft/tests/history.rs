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
fn sweeps_solidcraft_would_build_differently_flatten_the_history() {
    for (change, reason) in [
        (
            json!({"shape": {"mode": "surface", "section": {"kind": "unresolved", "value": null}}}),
            "surface sweeps are not mapped yet",
        ),
        (
            json!({"path_extent": {"along_fraction": 0.5, "against_fraction": 0.0}}),
            "sweeps along part of their path are not mapped yet",
        ),
        (json!({"taper": 0.1}), "tapered sweeps are not mapped yet"),
        (json!({"scale": 2.0}), "scaled sweeps are not mapped yet"),
        (
            json!({"orientation": {"kind": "binormal", "direction": {"x": 0.0, "y": 0.0, "z": 1.0}}}),
            "sweeps oriented by binormal are not mapped yet",
        ),
        (
            json!({"transition": "round_corner"}),
            "sweeps with corner transitions are not mapped yet",
        ),
        (
            json!({"path": {"kind": "native", "value": "n"}}),
            "its path is not a set of sketch curves",
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
                .all(|f| f["type"] != "sweep"),
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
        (json!({"ruled": true}), "ruled lofts are not mapped yet"),
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
        (json!({"solid": false}), "surface lofts are not mapped yet"),
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

/// The shape cadmpeg 0.6's Inventor decoder gives an extrude: its profile as native profile
/// selections of the sketch, an explicit direction, a symmetric or through-next extent. In a
/// sketch of one profile the selection can only be that profile; in a sketch of more, the
/// history cannot be replayed until cadmpeg resolves the selection, and the design is written
/// as geometry with the reason reported.
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
            "a tapered coil is not mapped yet",
        ),
        (
            with(
                &["construction", "extent"],
                json!({"kind": "spiral", "revolutions": 3.0, "radial_pitch": 2.0}),
            ),
            "a flat spiral coil is not mapped yet",
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
            "a coil of triangular section is not mapped yet",
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
