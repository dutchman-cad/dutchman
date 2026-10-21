//! An assembly written as SolidCraft components and occurrences: the synthetic Inventor cube
//! placed twice.
//!
//! The SolidCraft check runs when `SOLIDCRAFT_CLI` names a SolidCraft CLI.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use dutchman_core::{read, Assembly, Format, Model, Placement, Report, Writer};
use dutchman_solidcraft::{SolidCraftWriter, Verification, CLI_ENV};
use serde_json::Value;
use zip::ZipArchive;

fn placed_twice() -> Model {
    let part = read(
        Format::InventorPart,
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures/synthetic_cube.ipt"),
        &mut Report::default(),
    )
    .unwrap();
    let at = |x: f64| {
        [
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]
    };
    Model {
        name: "pair".to_owned(),
        source_file: Some("pair.iam".to_owned()),
        assembly: Some(Assembly {
            joints: Vec::new(),
            parts: vec![part.clone()],
            placements: vec![
                Placement {
                    name: None,
                    part: 0,
                    transform: at(0.0),
                },
                Placement {
                    name: Some("Moved".to_owned()),
                    part: 0,
                    transform: at(30.0),
                },
            ],
        }),
        ..part
    }
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
fn an_assembly_becomes_components_and_occurrences() {
    let (bytes, report) = written(&placed_twice());
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let blobs = (0..archive.len())
        .filter(|i| {
            archive
                .by_index(*i)
                .unwrap()
                .name()
                .unwrap()
                .starts_with("blobs/")
        })
        .count();
    assert_eq!(blobs, 1, "one part, stored once");
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();

    assert_eq!(document["name"], "pair");
    assert_eq!(
        document["components"],
        serde_json::json!([{"id": 1, "name": "synthetic_cube", "parent": 0}])
    );
    let occurrences = document["occurrences"].as_array().unwrap();
    assert_eq!(occurrences.len(), 2);
    assert_eq!(occurrences[0]["name"], "synthetic_cube:1");
    assert_eq!(occurrences[1]["name"], "Moved");
    assert_eq!(occurrences[1]["component"], 1);
    // Column-major: the translation is the fourth column.
    assert_eq!(
        occurrences[1]["transform"][3],
        serde_json::json!([30.0, 0.0, 0.0, 1.0])
    );
    let import = &document["features"][0];
    assert_eq!(import["type"], "import");
    assert_eq!(import["component"], 1);
    assert_eq!(import["file"], "synthetic_cube.ipt");
    assert_eq!(document["next_id"], 5);

    let outcome = report.outcome().unwrap();
    assert_eq!(outcome.mode, dutchman_core::Mode::Import);
}

#[test]
fn solidcraft_places_each_occurrence() {
    let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) else {
        eprintln!("skipped: set {CLI_ENV} to open the assembly in SolidCraft");
        return;
    };
    let (bytes, _) = written(&placed_twice());
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("pair.solidcraft");
    let output = dir.path().join("eval.json");
    std::fs::write(&input, bytes).unwrap();
    let status = std::process::Command::new(cli)
        .arg("eval")
        .arg(&input)
        .arg("--json-out")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let result: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    let bodies = result["bodies"].as_array().unwrap();
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    let min_x = |body: &Value| body["bbox"]["min"][0].as_f64().unwrap();
    assert!(
        (min_x(&bodies[1]) - min_x(&bodies[0]) - 30.0).abs() < 1e-6,
        "{bodies:?}"
    );
}

/// The pair as a sub-assembly, placed twice in a top assembly beside one loose cube.
fn nested() -> Model {
    let pair = placed_twice();
    let cube = pair.assembly.as_ref().unwrap().parts[0].clone();
    let at = |x: f64, y: f64| [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, 0.0]];
    Model {
        name: "top".to_owned(),
        source_file: Some("top.iam".to_owned()),
        assembly: Some(Assembly {
            joints: Vec::new(),
            parts: vec![pair, cube.clone()],
            placements: vec![
                Placement {
                    name: None,
                    part: 0,
                    transform: at(0.0, 0.0),
                },
                Placement {
                    name: None,
                    part: 0,
                    transform: at(0.0, 50.0),
                },
                Placement {
                    name: None,
                    part: 1,
                    transform: at(100.0, 0.0),
                },
            ],
        }),
        ..cube
    }
}

#[test]
fn a_sub_assembly_becomes_a_component_with_its_parts_beneath_it() {
    let (bytes, _) = written(&nested());
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let components: Vec<(String, u64)> = document["components"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["name"].as_str().unwrap().to_owned(),
                c["parent"].as_u64().unwrap(),
            )
        })
        .collect();
    let pair = document["components"][0]["id"].as_u64().unwrap();
    assert_eq!(
        components,
        [
            ("pair".to_owned(), 0),
            ("synthetic_cube".to_owned(), pair),
            ("synthetic_cube (2)".to_owned(), 0)
        ]
    );
    let occurrences = document["occurrences"].as_array().unwrap();
    assert_eq!(occurrences.len(), 5);
    let inside_pair = occurrences.iter().filter(|o| o["parent"] == pair).count();
    assert_eq!(inside_pair, 2);
    assert_eq!(document["features"].as_array().unwrap().len(), 2);
}

#[test]
fn solidcraft_composes_nested_placements() {
    let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) else {
        eprintln!("skipped: set {CLI_ENV} to open the assembly in SolidCraft");
        return;
    };
    let (bytes, _) = written(&nested());
    let bodies = evaluate(&cli, &bytes);
    let mut corners: Vec<(i64, i64)> = bodies
        .iter()
        .map(|b| {
            let min = &b["bbox"]["min"];
            let max = &b["bbox"]["max"];
            // Corner-independent: the box's centre, rounded to whole millimetres.
            let centre = |axis: usize| {
                ((min[axis].as_f64().unwrap() + max[axis].as_f64().unwrap()) / 2.0).round() as i64
            };
            (centre(0), centre(1))
        })
        .collect();
    corners.sort_unstable();
    let base = corners[0];
    let offsets: Vec<(i64, i64)> = corners
        .iter()
        .map(|(x, y)| (x - base.0, y - base.1))
        .collect();
    assert_eq!(
        offsets,
        [(0, 0), (0, 50), (30, 0), (30, 50), (100, 0)],
        "{bodies:?}"
    );
}

fn evaluate(cli: &Path, design: &[u8]) -> Vec<Value> {
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
    result["bodies"].as_array().unwrap().clone()
}

/// A design (CADIR built here from the synthetic cube) whose component owns the cube and is
/// placed at the origin and 30 mm along X, as a Fusion design with components would be. With
/// `nested`, both placements are inside an occurrence of the design's root component placed
/// 100 mm along X, as cadmpeg places the components a Fusion root component contains.
fn with_components(nested: bool) -> Model {
    let part = placed_twice().assembly.unwrap().parts[0].clone();
    let mut wire = serde_json::to_value(&part.ir).unwrap();
    let body = wire["model"]["bodies"][0]["id"].clone();
    wire["model"]["product_definitions"] = serde_json::json!([
        {"id": "f3d:model:component#block", "kind": "part", "label": "Block", "bodies": [body]},
        {"id": "f3d:model:component#root", "kind": "part", "label": "Top", "bodies": []},
    ]);
    let placed = |key: u32, definition: &str, x: f64, parent: Value| {
        serde_json::json!({
            "id": format!("f3d:model:occurrence#{key}"),
            "prototype": {"scope": "local", "definition": format!("f3d:model:component#{definition}")},
            "parent": parent,
            "ordinal": key,
            "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
            "scale": [1.0, 1.0, 1.0],
        })
    };
    let root = serde_json::json!({"kind": "root"});
    wire["model"]["occurrences"] = if nested {
        let inside =
            serde_json::json!({"kind": "occurrence", "occurrence": "f3d:model:occurrence#9"});
        serde_json::json!([
            placed(9, "root", 100.0, root),
            placed(0, "block", 0.0, inside.clone()),
            placed(1, "block", 30.0, inside),
        ])
    } else {
        serde_json::json!([
            placed(0, "block", 0.0, root.clone()),
            placed(1, "block", 30.0, root)
        ])
    };
    let mut model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        name: "blocks".to_owned(),
        ..part
    };
    model.assembly = dutchman_core::local_assembly(&model).unwrap();
    model
}

#[test]
fn a_design_with_components_becomes_components_and_occurrences() {
    let (bytes, _) = written(&with_components(false));
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(document["components"][0]["name"], "Block");
    assert_eq!(document["occurrences"].as_array().unwrap().len(), 2);
}

#[test]
fn solidcraft_places_each_component_occurrence() {
    let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) else {
        eprintln!("skipped: set {CLI_ENV} to open the design in SolidCraft");
        return;
    };
    for nested in [false, true] {
        let (bytes, _) = written(&with_components(nested));
        let bodies = evaluate(&cli, &bytes);
        assert_eq!(bodies.len(), 2, "{bodies:?}");
        let mut min_x: Vec<f64> = bodies
            .iter()
            .map(|body| body["bbox"]["min"][0].as_f64().unwrap())
            .collect();
        min_x.sort_by(f64::total_cmp);
        assert!((min_x[1] - min_x[0] - 30.0).abs() < 1e-6, "{bodies:?}");
        if nested {
            // The cube's own minimum, moved by the outer occurrence.
            let (flat, _) = written(&with_components(false));
            let flat_min = evaluate(&cli, &flat)
                .iter()
                .map(|body| body["bbox"]["min"][0].as_f64().unwrap())
                .fold(f64::INFINITY, f64::min);
            assert!((min_x[0] - flat_min - 100.0).abs() < 1e-6, "{bodies:?}");
        }
    }
}

#[test]
fn components_inside_a_component_stay_beneath_it() {
    let (bytes, _) = written(&with_components(true));
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let components = document["components"].as_array().unwrap();
    let id_of = |name: &str| {
        components
            .iter()
            .find(|c| c["name"] == name)
            .unwrap_or_else(|| panic!("{name} in {components:?}"))["id"]
            .clone()
    };
    let (top, block) = (id_of("Top"), id_of("Block"));
    assert_eq!(
        components.iter().find(|c| c["id"] == block).unwrap()["parent"],
        top
    );
    let occurrences = document["occurrences"].as_array().unwrap();
    assert_eq!(occurrences.len(), 3);
    let inside: Vec<&Value> = occurrences.iter().filter(|o| o["parent"] == top).collect();
    assert_eq!(inside.len(), 2);
    assert!(inside.iter().all(|o| o["component"] == block));
}

/// Two blocks inside the root component's occurrence (placed 100 mm along X), the second 30 mm
/// further and turned a quarter turn about Z, held by a fixed joint, and the root occurrence
/// fixed to the design itself.
fn jointed() -> Model {
    let mut model = with_components(true);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["occurrences"][2]["transform"] = serde_json::json!([
        [0.0, -1.0, 0.0, 30.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0]
    ]);
    let identity = serde_json::json!([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0]
    ]);
    let side = |key: Option<u32>| {
        let operand = match key {
            None => {
                serde_json::json!({"container": {"container": "root"}, "object": "root:origin"})
            }
            Some(key) => serde_json::json!({
                "container": {"container": "occurrence", "occurrence": format!("f3d:model:occurrence#{key}")},
                "object": key.to_string(),
            }),
        };
        serde_json::json!({"operand": operand, "frame": identity, "detached": false})
    };
    let joint = |key: u32, a: Value, b: Value| {
        serde_json::json!({
            "id": format!("f3d:model:joint#{key}"),
            "operands": {"arity": "pair", "kind": {"kind": "fixed"}, "connectors": [a, b]},
            "suppressed": false,
        })
    };
    wire["model"]["assembly_joints"] = serde_json::json!([
        joint(1, side(Some(0)), side(Some(1))),
        joint(2, side(None), side(Some(9))),
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model.assembly = dutchman_core::local_assembly(&model).unwrap();
    model
}

#[test]
fn fixed_joints_become_rigid_joints_that_hold_the_parts_as_placed() {
    let (bytes, report) = written(&jointed());
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let joints = document["assembly"]["joints"].as_array().unwrap();
    assert_eq!(joints.len(), 2);
    assert!(joints
        .iter()
        .all(|j| j["kind"] == "rigid" && j["flip"] == true));
    // The blocks: A's frame is its own origin; B's is the same place seen from B, which sits
    // 30 mm further along X, turned a quarter turn: A's origin is 30 mm along B's +Y.
    let blocks = &joints[0];
    assert_eq!(
        blocks["a"]["snap"]["origin"],
        serde_json::json!([0.0, 0.0, 0.0])
    );
    let origin: Vec<f64> = blocks["b"]["snap"]["origin"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect();
    assert!(
        (origin[0]).abs() < 1e-9 && (origin[1] - 30.0).abs() < 1e-9 && origin[2].abs() < 1e-9,
        "{origin:?}"
    );
    // The root occurrence, fixed to the design itself (occurrence 0).
    assert_eq!(joints[1]["a"]["occurrence"], 0);
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.subject == "joints" && loss.detail.starts_with("2 joint(s)")));
}

#[test]
fn solidcraft_reads_each_joint_as_built_and_solves_without_moving_anything() {
    let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) else {
        eprintln!("skipped: set {CLI_ENV} to open the design in SolidCraft");
        return;
    };
    let (bytes, _) = written(&jointed());
    let before = evaluate(&cli, &bytes);

    let dir = tempfile::tempdir().unwrap();
    let design = dir.path().join("design.solidcraft");
    let solved = dir.path().join("solved.solidcraft");
    let script = dir.path().join("solve.json");
    let results = dir.path().join("results.json");
    std::fs::write(&design, &bytes).unwrap();
    std::fs::write(
        &script,
        r#"[{"command":"joint.list","params":{}},{"command":"joint.solve","params":{}}]"#,
    )
    .unwrap();
    let status = std::process::Command::new(&cli)
        .arg("run")
        .arg(&script)
        .arg("--in")
        .arg(&design)
        .arg("--save")
        .arg(&solved)
        .arg("--results")
        .arg("--json-out")
        .arg(&results)
        .status()
        .unwrap();
    assert!(status.success());
    let results: Value = serde_json::from_str(&std::fs::read_to_string(results).unwrap()).unwrap();
    // As built: each joint's two frames, as SolidCraft places them in the world, coincide.
    let joints = results[0]["joints"].as_array().unwrap();
    assert_eq!(joints.len(), 2, "{results}");
    for joint in joints {
        assert_eq!(joint["type"], "rigid");
        for key in ["origin", "z"] {
            let a = joint["frame_a"][key].as_array().unwrap();
            let b = joint["frame_b"][key].as_array().unwrap();
            for (a, b) in a.iter().zip(b) {
                assert!(
                    (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9,
                    "{}: {key} {a} vs {b}",
                    joint["name"]
                );
            }
        }
    }
    let solve = &results[1];
    assert_eq!(solve["conflicts"], serde_json::json!([]), "{solve}");
    assert_eq!(solve["errors"], serde_json::json!([]), "{solve}");

    let after = evaluate(&cli, &std::fs::read(&solved).unwrap());
    let boxes = |bodies: &[Value]| {
        let mut boxes: Vec<String> = bodies.iter().map(|b| b["bbox"].to_string()).collect();
        boxes.sort();
        boxes
    };
    assert_eq!(boxes(&before), boxes(&after));
    assert_eq!(before.len(), 2);
}
