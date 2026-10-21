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
