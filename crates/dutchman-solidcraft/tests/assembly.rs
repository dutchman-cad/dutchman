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
        source_step: None,
        assembly: Some(Assembly {
            own_part: None,
            joints: Vec::new(),
            left_out: Vec::new(),
            held: Vec::new(),
            joint_notes: Vec::new(),
            stored_placements: Vec::new(),
            parts: vec![part.clone()],
            placements: vec![
                Placement {
                    hidden: false,
                    name: None,
                    part: 0,
                    transform: at(0.0),
                },
                Placement {
                    hidden: false,
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

fn document(bytes: &[u8]) -> Value {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap()
}

#[test]
fn folders_hold_each_top_designs_features_including_nested_parts() {
    let model = nested();
    let (bytes, report) = written(&model);
    let doc = document(&bytes);
    let groups = doc["timeline_groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["name"], doc["components"][0]["name"]);
    assert_eq!(groups[1]["name"], doc["components"][2]["name"]);
    assert_eq!(groups[0]["features"], serde_json::json!([3]));
    assert_eq!(groups[1]["features"], serde_json::json!([7]));
    assert!(groups.iter().all(|group| group["parent"].is_null()));
    assert_eq!(doc["next_id"], 13);
    assert_eq!(groups[0]["id"], 11);
    assert_eq!(groups[1]["id"], 12);
    let notes: Vec<_> = report
        .losses()
        .iter()
        .filter(|note| note.subject == "timeline folders")
        .collect();
    assert_eq!(notes.len(), 2);
    assert!(notes
        .iter()
        .all(|note| note.severity == dutchman_core::Severity::Info));
    assert_eq!(notes[0].detail, "\"pair\": 1 feature(s) grouped");
    if let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) {
        assert_eq!(evaluate(&cli, &bytes).len(), 5);
    }
}

#[test]
fn a_single_part_or_one_top_design_has_no_folders() {
    let model = placed_twice();
    let mut own = with_sketches();
    own.assembly.as_mut().unwrap().own_part = Some(0);
    for model in [
        &model,
        &model.assembly.as_ref().unwrap().parts[0],
        &with_components(true),
        &own,
    ] {
        let (bytes, report) = written(model);
        assert!(document(&bytes).get("timeline_groups").is_none());
        assert!(!report
            .losses()
            .iter()
            .any(|note| note.subject == "timeline folders"));
    }
}

#[test]
fn root_features_get_a_unique_folder_and_joints_stay_out() {
    let mut model = with_sketches();
    model.name = "Block".to_owned();
    let assembly = model.assembly.as_mut().unwrap();
    assembly.joints.push(dutchman_core::Joint {
        a: vec![0],
        b: vec![1],
        frame_a: dutchman_core::placement::IDENTITY,
        frame_b: dutchman_core::placement::IDENTITY,
        suppressed: false,
        motion: dutchman_core::Motion::Rigid,
        flip: true,
    });
    let (bytes, _) = written(&model);
    let doc = document(&bytes);
    let groups = doc["timeline_groups"].as_array().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0]["name"], "Block");
    assert_eq!(groups[1]["name"], "Block (2)");
    let grouped: Vec<_> = groups
        .iter()
        .flat_map(|g| g["features"].as_array().unwrap())
        .collect();
    let features: Vec<_> = doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| &f["id"])
        .collect();
    assert_eq!(grouped, features, "all features stay in timeline order");
    assert!(!grouped.contains(&&doc["assembly"]["joints"][0]["id"]));
    assert_eq!(groups[1]["features"], doc["visibility"]["hidden_sketches"]);
    if let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) {
        assert_eq!(evaluate(&cli, &bytes).len(), 2);
    }
}

#[test]
fn each_parts_history_is_reported() {
    // The cube with a mapped sphere history beside the cube with none.
    let mut model = placed_twice();
    let assembly = model.assembly.as_mut().unwrap();
    let mut part = assembly.parts[0].clone();
    part.name = "Ball".to_owned();
    let mut wire = serde_json::to_value(&part.ir).unwrap();
    wire["model"]["features"] = serde_json::json!([{
        "id": "f3d:model:feature#1", "ordinal": 0, "name": "Ball", "suppressed": false,
        "outputs": [wire["model"]["bodies"][0]["id"]],
        "definition": {"definition": "sphere", "center": {"x": 5.0, "y": 5.0, "z": 5.0},
                       "radius": 5.0, "op": "new_body"},
    }]);
    part.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    assembly.parts.push(part);
    assembly.placements[1].part = 1;
    let (_, report) = written(&model);
    let parts = report.parts();
    assert_eq!(parts.len(), 2);
    assert_eq!(parts[0].name, "synthetic_cube");
    assert!(matches!(
        parts[0].check,
        dutchman_core::Check::NotApplicable(_)
    ));
    assert_eq!((parts[0].mapped, parts[0].total), (0, 0));
    assert_eq!(parts[1].name, "Ball");
    assert!(matches!(
        parts[1].check,
        dutchman_core::Check::Unverified(_)
    ));
    assert_eq!((parts[1].mapped, parts[1].total), (1, 1));
}

#[test]
fn folders_keep_mapped_history_and_its_suppressed_original() {
    let mut model = placed_twice();
    let assembly = model.assembly.as_mut().unwrap();
    let mut part = assembly.parts[0].clone();
    let mut wire = serde_json::to_value(&part.ir).unwrap();
    wire["model"]["features"] = serde_json::json!([{
        "id": "f3d:model:feature#1", "ordinal": 0, "name": "Ball", "suppressed": false,
        "outputs": [wire["model"]["bodies"][0]["id"]],
        "definition": {"definition": "sphere", "center": {"x": 5.0, "y": 5.0, "z": 5.0},
                       "radius": 5.0, "op": "new_body"},
    }]);
    part.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    assembly.parts.push(part);
    assembly.placements[1].part = 1;
    let (bytes, _) = written(&model);
    let doc = document(&bytes);
    let features = doc["features"].as_array().unwrap();
    assert_eq!(features.len(), 3);
    assert_eq!(features[0]["type"], "import");
    assert_eq!(features[1]["type"], "sphere");
    assert_eq!(features[1]["name"], "Ball");
    assert_eq!(features[2]["name"], "Original geometry (imported)");
    assert_eq!(features[2]["suppressed"], true);
    assert_eq!(doc["timeline_groups"][0]["name"], "synthetic_cube");
    assert_eq!(doc["timeline_groups"][1]["name"], "synthetic_cube (2)");
    assert_eq!(
        doc["timeline_groups"][1]["features"],
        serde_json::json!([features[1]["id"], features[2]["id"]])
    );
    if let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) {
        assert_eq!(evaluate(&cli, &bytes).len(), 2);
    }
}

#[test]
fn root_bodies_and_sketches_share_a_folder_only_when_contiguous() {
    for own_first in [false, true] {
        let mut model = with_sketches();
        let assembly = model.assembly.as_mut().unwrap();
        let mut own = assembly.parts[0].clone();
        own.name = model.name.clone();
        let own_index = if own_first { 0 } else { 1 };
        assembly.parts.insert(own_index, own);
        assembly.own_part = Some(own_index);
        for placement in &mut assembly.placements {
            placement.part = 1 - own_index;
        }
        assembly.placements.push(Placement {
            hidden: false,
            name: None,
            part: own_index,
            transform: dutchman_core::placement::IDENTITY,
        });
        let (bytes, _) = written(&model);
        let doc = document(&bytes);
        let groups = doc["timeline_groups"].as_array().unwrap();
        assert_eq!(groups.len(), if own_first { 3 } else { 2 });
        if own_first {
            assert_eq!(groups[0]["name"], "blocks");
            assert_eq!(groups[2]["name"], "blocks (2)");
            assert_eq!(groups[0]["features"].as_array().unwrap().len(), 1);
            assert_eq!(groups[2]["features"], doc["visibility"]["hidden_sketches"]);
        } else {
            assert_eq!(groups[1]["name"], "blocks");
            assert_eq!(groups[1]["features"].as_array().unwrap().len(), 2);
        }
        let ids: Vec<_> = groups
            .iter()
            .flat_map(|g| g["features"].as_array().unwrap())
            .collect();
        let timeline: Vec<_> = doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| &f["id"])
            .collect();
        assert_eq!(ids, timeline);
        if let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) {
            assert_eq!(evaluate(&cli, &bytes).len(), 3);
        }
    }
}

#[test]
fn a_part_with_no_bodies_is_an_empty_component_its_joints_still_join() {
    let mut model = placed_twice();
    if let Some(assembly) = &mut model.assembly {
        let mut empty = assembly.parts[0].clone();
        let mut wire = serde_json::to_value(&empty.ir).unwrap();
        // Every arena emptied: a component with nothing in it.
        for arena in wire["model"].as_object_mut().unwrap().values_mut() {
            if arena.is_array() {
                *arena = serde_json::json!([]);
            }
        }
        empty.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        empty.name = "Spacer".to_owned();
        assembly.parts.push(empty);
        assembly.placements[1].part = 1;
        assembly.joints.push(dutchman_core::Joint {
            a: vec![0],
            b: vec![1],
            frame_a: dutchman_core::placement::IDENTITY,
            frame_b: dutchman_core::placement::IDENTITY,
            suppressed: false,
            motion: dutchman_core::Motion::Rigid,
            flip: true,
        });
    }
    let (bytes, report) = written(&model);
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let names: Vec<&str> = document["components"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["name"].as_str())
        .collect();
    assert!(names.contains(&"Spacer"), "{names:?}");
    assert_eq!(document["occurrences"].as_array().unwrap().len(), 2);
    // Only the cube imports geometry; the joint holds both occurrences.
    let imports = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["type"] == "import")
        .count();
    assert_eq!(imports, 1, "{}", document["features"]);
    assert_eq!(document["assembly"]["joints"].as_array().unwrap().len(), 1);
    assert!(!report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("left out")));
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail == "Spacer: placed as an empty component: it has no bodies"));
}

#[test]
fn an_assembly_that_places_no_parts_is_written_as_an_empty_design() {
    let mut model = placed_twice();
    if let Some(assembly) = &mut model.assembly {
        assembly.parts.clear();
        assembly.placements.clear();
    }
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    assert_eq!(writer.convertible(&model), Ok(()));
    let (bytes, report) = written(&model);
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(document["features"], serde_json::json!([]));
    assert_eq!(
        report.outcome().map(|outcome| outcome.check.clone()),
        Some(dutchman_core::Check::NotApplicable(
            "the assembly places no parts".to_owned()
        ))
    );
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
        source_step: None,
        assembly: Some(Assembly {
            own_part: None,
            joints: Vec::new(),
            left_out: Vec::new(),
            held: Vec::new(),
            joint_notes: Vec::new(),
            stored_placements: Vec::new(),
            parts: vec![pair, cube.clone()],
            placements: vec![
                Placement {
                    hidden: false,
                    name: None,
                    part: 0,
                    transform: at(0.0, 0.0),
                },
                Placement {
                    hidden: false,
                    name: None,
                    part: 0,
                    transform: at(0.0, 50.0),
                },
                Placement {
                    hidden: false,
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

/// [`with_components`], with two sketches of the design's own: a 20 mm line on XY dimensioned
/// by parameter `span`, and one whose plane is not resolved; and parameter `spare`, which no
/// sketch uses.
fn with_sketches() -> Model {
    let model = with_components(false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let sketch = |key: u32, placement: Value| {
        serde_json::json!({"id": format!("f3d:model:sketch#{key}"), "name": format!("Layout {key}"),
                           "visible": false, "placement": placement})
    };
    wire["model"]["sketches"] = serde_json::json!([
        sketch(
            1,
            serde_json::json!({"kind": "resolved", "frame": {
                "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
            }})
        ),
        sketch(2, serde_json::json!({"kind": "unresolved"})),
    ]);
    wire["model"]["sketch_entities"] = serde_json::json!([{
        "id": "f3d:model:sketch-entity#1",
        "sketch": "f3d:model:sketch#1",
        "construction": false,
        "geometry": {"kind": "line", "start": {"u": 0.0, "v": 0.0}, "end": {"u": 20.0, "v": 0.0}},
    }]);
    wire["model"]["sketch_constraints"] = serde_json::json!([{
        "id": "f3d:model:sketch-constraint#1",
        "sketch": "f3d:model:sketch#1",
        "definition": {"kind": "distance", "entities": ["f3d:model:sketch-entity#1"],
                       "parameter": "f3d:model:parameter#1"},
    }]);
    let parameter = |key: u32, name: &str, value: f64| {
        serde_json::json!({"id": format!("f3d:model:parameter#{key}"), "name": name,
                           "expression": format!("{value} mm"),
                           "value": {"kind": "length", "value": value}})
    };
    wire["model"]["parameters"] =
        serde_json::json!([parameter(1, "span", 20.0), parameter(2, "spare", 5.0)]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn an_assemblys_own_sketches_are_kept_hidden_in_the_design() {
    let (bytes, report) = written(&with_sketches());
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let sketches: Vec<&Value> = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|feature| feature["type"] == "sketch")
        .collect();
    assert_eq!(sketches.len(), 1, "the unresolved one is left out");
    let sketch = sketches[0];
    assert_eq!(sketch["name"], "Layout 1");
    assert_eq!(sketch["component"], 0, "the design's own, not a part's");
    assert_eq!(sketch["sketch"]["curves"].as_array().unwrap().len(), 1);
    assert_eq!(
        document["visibility"]["hidden_sketches"],
        serde_json::json!([sketch["id"]])
    );
    assert!(sketch["id"].as_u64() < document["next_id"].as_u64());
    let params: Vec<&str> = document["params"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|param| param["name"].as_str())
        .collect();
    assert_eq!(params, ["span"], "only the parameter the sketch uses");
    assert!(report.losses().iter().any(|loss| loss.subject == "sketches"
        && loss.detail == "kept 1 sketch in the design itself, for reference; 1 left out"));
}

#[test]
fn solidcraft_solves_an_assemblys_own_sketch_and_keeps_it_hidden() {
    let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) else {
        eprintln!("skipped: set {CLI_ENV} to check the sketch in SolidCraft");
        return;
    };
    let (bytes, _) = written(&with_sketches());
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("design.solidcraft");
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
    let sketch = &result["sketches"][0];
    assert_eq!(sketch["name"], "Layout 1");
    assert_eq!(sketch["status"], "solved");
    assert_eq!(sketch["visible"], false);
    assert_eq!(result["bodies"].as_array().unwrap().len(), 2);
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
    // Each connector frame is in its own part, placed so the joints hold the parts where they
    // are stored, the second facing the first (half a turn about x): A's origin seen from B
    // (30 mm on, a quarter turn round) is 30 mm along B's +Y, turned back; the root's origin
    // seen from Top (at x = 100) is 100 mm back along X.
    let frame = |key: Option<u32>| match key {
        Some(1) => serde_json::json!([
            [0.0, -1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 30.0],
            [0.0, 0.0, -1.0, 0.0]
        ]),
        Some(9) => serde_json::json!([
            [1.0, 0.0, 0.0, -100.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0]
        ]),
        _ => serde_json::json!([
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ]),
    };
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
        serde_json::json!({"operand": operand, "frame": frame(key), "detached": false})
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
        .all(|j| j["kind"] == "rigid" && j["flip"] == false));
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
    // As built: each joint's two frames, as SolidCraft places them in the world, meet face to
    // face: the same origin, opposite z axes.
    let joints = results[0]["joints"].as_array().unwrap();
    assert_eq!(joints.len(), 2, "{results}");
    for joint in joints {
        assert_eq!(joint["type"], "rigid");
        for (key, sign) in [("origin", 1.0), ("z", -1.0)] {
            let a = joint["frame_a"][key].as_array().unwrap();
            let b = joint["frame_b"][key].as_array().unwrap();
            for (a, b) in a.iter().zip(b) {
                assert!(
                    (a.as_f64().unwrap() - sign * b.as_f64().unwrap()).abs() < 1e-9,
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

/// The two blocks at the root (A at x 0–10, B at x 30–40), A fixed to the design, and B joined
/// to A by `kind` about a vertical axis through (20, 0, 0), each connector frame given in its
/// own part, the second facing the first.
fn hinged(kind: Value) -> Model {
    let mut model = with_components(false);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    // The hinge at (20, 0, 0): 20 mm along A (at the origin), 10 mm back from B (at x = 30).
    let at = |x: f64| {
        serde_json::json!([
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ])
    };
    // Half a turn about x, as the second frame faces the first.
    let facing = |x: f64| {
        serde_json::json!([
            [1.0, 0.0, 0.0, x],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0]
        ])
    };
    let side = |key: Option<u32>, at: Value| {
        let operand = match key {
            None => {
                serde_json::json!({"container": {"container": "root"}, "object": "root:origin"})
            }
            Some(key) => serde_json::json!({
                "container": {"container": "occurrence", "occurrence": format!("f3d:model:occurrence#{key}")},
                "object": key.to_string(),
            }),
        };
        serde_json::json!({"operand": operand, "frame": at, "detached": false})
    };
    let joint = |key: u32, kind: Value, a: Value, b: Value| {
        serde_json::json!({
            "id": format!("f3d:model:joint#{key}"),
            "operands": {"arity": "pair", "kind": kind, "connectors": [a, b]},
            "suppressed": false,
        })
    };
    wire["model"]["assembly_joints"] = serde_json::json!([
        joint(
            1,
            serde_json::json!({"kind": "fixed"}),
            side(None, at(0.0)),
            side(Some(0), facing(0.0))
        ),
        joint(
            2,
            kind,
            side(Some(0), at(20.0)),
            side(Some(1), facing(-10.0))
        ),
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model.assembly = dutchman_core::local_assembly(&model).unwrap();
    model
}

#[test]
fn a_held_placement_is_written_grounded() {
    let mut model = placed_twice();
    if let Some(assembly) = &mut model.assembly {
        assembly.held = vec![vec![1]];
    }
    let (bytes, _) = written(&model);
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let grounded: Vec<&Value> = document["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|occurrence| occurrence["grounded"] == true)
        .map(|occurrence| &occurrence["name"])
        .collect();
    assert_eq!(grounded, [&serde_json::json!("Moved")]);
}

#[test]
fn joints_tied_to_nothing_ground_one_part_and_keep_their_frame_sense() {
    // The hinge alone, without the joint holding its first part to the design, and with its
    // frames aligned: its fixed side is grounded, and it is written flipped.
    let mut model = hinged(serde_json::json!({"kind": "revolute"}));
    if let Some(assembly) = &mut model.assembly {
        assembly.joints.remove(0);
        assembly.joints[0].flip = true;
    }
    let (bytes, report) = written(&model);
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let hinge = &document["assembly"]["joints"][0];
    assert_eq!(hinge["flip"], true);
    let grounded: Vec<&Value> = document["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|occurrence| occurrence["grounded"] == true)
        .map(|occurrence| &occurrence["id"])
        .collect();
    assert_eq!(grounded, [&hinge["b"]["occurrence"]]);
    assert!(
        report.losses().iter().any(|loss| loss.subject == "joints"
            && loss.detail.contains("is grounded, inferred")
            && loss.detail.contains("\"Revolute 1\"")),
        "{report:?}"
    );

    // Held to the design, nothing is grounded: the design holds the parts.
    let (bytes, _) = written(&hinged(serde_json::json!({"kind": "revolute"})));
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    assert!(document["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .all(|occurrence| occurrence["grounded"] == false));
}

#[test]
fn revolute_and_slider_joints_are_written_with_their_motion() {
    let (bytes, report) = written(&hinged(serde_json::json!({"kind": "revolute",
        "angular_limits": {"bounds": "range", "minimum": -1.0, "maximum": 2.0}})));
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let hinge = &document["assembly"]["joints"][1];
    assert_eq!(hinge["kind"], "revolute");
    assert_eq!(hinge["name"], "Revolute 1");
    assert_eq!(hinge["values"], serde_json::json!([0.0]));
    assert_eq!(hinge["limits"], serde_json::json!([[-1.0, 2.0]]));
    // A's frame is at the hinge in A's coordinates (A is at the origin); B's in B's, 30 mm on.
    assert_eq!(
        hinge["a"]["snap"]["origin"],
        serde_json::json!([20.0, 0.0, 0.0])
    );
    assert_eq!(
        hinge["b"]["snap"]["origin"],
        serde_json::json!([-10.0, 0.0, 0.0])
    );
    assert!(report.losses().iter().any(|loss| loss.detail
        == "2 joint(s) written as built (1 revolute, 1 rigid), holding their parts as placed"));
    let kinds: Vec<(&str, u64)> = report
        .joints()
        .iter()
        .map(|(kind, n)| (kind.as_str(), *n))
        .collect();
    assert_eq!(kinds, [("revolute", 1), ("rigid", 1)]);
}

/// A grounded base and a hinge, both turned a quarter turn about Z and translated.
fn grounded_hinge() -> Model {
    let mut model = hinged(serde_json::json!({"kind": "revolute"}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let connector = wire["model"]["assembly_joints"][0]["operands"]["connectors"][1].clone();
    wire["model"]["assembly_joints"][0]["operands"] = serde_json::json!({
        "arity": "grounded", "connector": connector,
    });
    for (index, y) in [5.0, 35.0].into_iter().enumerate() {
        wire["model"]["occurrences"][index]["transform"] = serde_json::json!([
            [0.0, -1.0, 0.0, 15.0],
            [1.0, 0.0, 0.0, y],
            [0.0, 0.0, 1.0, 7.0],
        ]);
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model.assembly = dutchman_core::local_assembly(&model).unwrap();
    model
}

#[test]
fn a_grounded_joint_fixes_its_occurrence_and_keeps_the_revolute_joint() {
    let model = grounded_hinge();
    let assembly = model.assembly.as_ref().unwrap();
    assert!(assembly.left_out.is_empty(), "{:?}", assembly.left_out);
    assert_eq!(assembly.joints.len(), 2);
    let ground = &assembly.joints[0];
    assert!(ground.a.is_empty());
    assert_eq!(ground.b, [0]);
    assert_eq!(ground.motion, dutchman_core::Motion::Rigid);
    assert_eq!(ground.frame_a, assembly.placements[0].transform);

    let (bytes, report) = written(&model);
    assert!(
        report
            .losses()
            .iter()
            .all(|loss| loss.severity != dutchman_core::Severity::Dropped),
        "{report:?}"
    );
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    let document: Value = serde_json::from_str(&text).unwrap();
    let occurrences = document["occurrences"].as_array().unwrap();
    assert_eq!(occurrences.len(), 2);
    assert_eq!(
        occurrences[0]["transform"],
        serde_json::json!([
            [0.0, 1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [15.0, 5.0, 7.0, 1.0],
        ])
    );
    let joints = document["assembly"]["joints"].as_array().unwrap();
    assert_eq!(joints.len(), 2);
    assert_eq!(joints[0]["kind"], "rigid");
    assert_eq!(joints[0]["a"]["occurrence"], 0);
    assert_eq!(joints[0]["b"]["occurrence"], occurrences[0]["id"]);
    assert_eq!(joints[1]["kind"], "revolute");
    assert_eq!(joints[1]["a"]["occurrence"], occurrences[0]["id"]);
    assert_eq!(joints[1]["b"]["occurrence"], occurrences[1]["id"]);
    assert_eq!(report.joints().get("rigid"), Some(&1));
    assert_eq!(report.joints().get("revolute"), Some(&1));
}

#[test]
fn solidcraft_keeps_the_grounded_base_fixed_when_driving_its_hinge() {
    let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) else {
        eprintln!("skipped: set {CLI_ENV} to drive the joints in SolidCraft");
        return;
    };
    let (bytes, _) = written(&grounded_hinge());
    assert_eq!(
        driven(&cli, &bytes, "Revolute 1", 90.0),
        [[-5.0, 15.0, 7.0, 5.0, 25.0], [5.0, 5.0, 7.0, 15.0, 15.0]]
    );
}

/// Drives the joint named `joint` to `value` (degrees, or mm for a slider) in SolidCraft and
/// returns each body's bounding box after: min x, y, z, max x, y.
fn driven(cli: &Path, design: &[u8], joint: &str, value: f64) -> Vec<[f64; 5]> {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("design.solidcraft");
    let moved = dir.path().join("moved.solidcraft");
    let script = dir.path().join("drive.json");
    std::fs::write(&input, design).unwrap();
    std::fs::write(
        &script,
        serde_json::json!([{"command": "joint.drive", "params": {"joint": joint, "value": value}}])
            .to_string(),
    )
    .unwrap();
    let status = std::process::Command::new(cli)
        .arg("run")
        .arg(&script)
        .arg("--in")
        .arg(&input)
        .arg("--save")
        .arg(&moved)
        .status()
        .unwrap();
    assert!(status.success());
    let mut boxes: Vec<[f64; 5]> = evaluate(cli, &std::fs::read(&moved).unwrap())
        .iter()
        .map(|b| {
            let c = |k: &str, i: usize| (b["bbox"][k][i].as_f64().unwrap() * 1e6).round() / 1e6;
            [
                c("min", 0),
                c("min", 1),
                c("min", 2),
                c("max", 0),
                c("max", 1),
            ]
        })
        .collect();
    boxes.sort_by(|a, b| a.partial_cmp(b).unwrap());
    boxes
}

#[test]
fn solidcraft_moves_the_part_when_the_joint_is_driven() {
    let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) else {
        eprintln!("skipped: set {CLI_ENV} to drive the joints in SolidCraft");
        return;
    };
    // A quarter turn about the hinge at (20, 0): B swings from x 30–40, y 0–10 to x 10–20,
    // y 10–20; A, fixed to the design, stays at the origin.
    let revolute = written(&hinged(serde_json::json!({"kind": "revolute"}))).0;
    assert_eq!(
        driven(&cli, &revolute, "Revolute 1", 90.0),
        [[0.0, 0.0, 0.0, 10.0, 10.0], [10.0, 10.0, 0.0, 20.0, 20.0]]
    );
    // A slider along the hinge's z: B rises 5 mm and stays over x 30–40.
    let slider = written(&hinged(serde_json::json!({"kind": "slider"}))).0;
    assert_eq!(
        driven(&cli, &slider, "Slider 1", 5.0),
        [[0.0, 0.0, 0.0, 10.0, 10.0], [30.0, 0.0, 5.0, 40.0, 10.0]]
    );
}

#[test]
fn a_joined_part_is_placed_where_its_joint_puts_it_not_where_it_was_stored() {
    // Store B 40 mm further on than its joint puts it, as an unsolved placement would be.
    let mut model = hinged(serde_json::json!({"kind": "revolute"}));
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["occurrences"][1]["transform"][0][3] = serde_json::json!(70.0);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model.assembly = dutchman_core::local_assembly(&model).unwrap();
    let placed = &model.assembly.as_ref().unwrap().placements;
    assert_eq!(placed[1].transform[0][3], 30.0, "{placed:?}");

    let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) else {
        eprintln!("skipped: set {CLI_ENV} to open the design in SolidCraft");
        return;
    };
    let (bytes, _) = written(&model);
    let mut starts: Vec<f64> = evaluate(&cli, &bytes)
        .iter()
        .map(|b| b["bbox"]["min"][0].as_f64().unwrap())
        .collect();
    starts.sort_by(f64::total_cmp);
    assert_eq!(starts, [0.0, 30.0]);
}

fn document_of(model: &Model) -> Value {
    let (bytes, _) = written(model);
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    serde_json::from_str(&text).unwrap()
}

#[test]
fn a_placement_the_source_hides_is_written_hidden() {
    // The second cube is hidden in the source: it is still placed, and hidden.
    let mut model = placed_twice();
    if let Some(assembly) = &mut model.assembly {
        assembly.placements[1].hidden = true;
    }
    let document = document_of(&model);
    let moved = document["occurrences"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["name"] == "Moved")
        .unwrap()["id"]
        .as_u64()
        .unwrap();
    assert_eq!(document["occurrences"].as_array().unwrap().len(), 2);
    assert_eq!(
        document["visibility"]["hidden_occurrences"],
        serde_json::json!([[moved]])
    );

    // Inside a sub-assembly placed twice, it is hidden in each placement of it.
    let mut outer = placed_twice();
    if let Some(assembly) = &mut outer.assembly {
        let mut inner = model.clone();
        inner.name = "pair".to_owned();
        assembly.parts = vec![inner];
    }
    let document = document_of(&outer);
    let hidden = document["visibility"]["hidden_occurrences"]
        .as_array()
        .unwrap();
    assert_eq!(hidden.len(), 2, "{hidden:?}");
    assert!(hidden
        .iter()
        .all(|path| path.as_array().unwrap().len() == 2));
    assert!(placed_twice_shows_nothing_hidden());
}

fn placed_twice_shows_nothing_hidden() -> bool {
    document_of(&placed_twice())
        .get("visibility")
        .and_then(|v| v.get("hidden_occurrences"))
        .is_none()
}

#[test]
fn flattened_parts_keep_helper_info_once_per_part() {
    let mut model = placed_twice();
    let part = &mut model.assembly.as_mut().unwrap().parts[0];
    let mut wire = serde_json::to_value(&part.ir).unwrap();
    wire["model"]["features"] = serde_json::json!([
        {"id": "test:model:feature#helper", "ordinal": 0, "name": "Unused point",
            "suppressed": false, "definition": {"definition": "datum_point",
                "position": {"x": 0.0, "y": 0.0, "z": 0.0}}},
        {"id": "test:model:feature#later", "ordinal": 1, "name": "Later",
            "suppressed": false, "definition": {"definition": "native", "kind": "Mystery"}}
    ]);
    part.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = written(&model);
    let notes: Vec<_> = report
        .losses()
        .iter()
        .filter(|loss| loss.detail == "helper not converted: nothing uses it")
        .collect();
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].severity, dutchman_core::Severity::Info);
    assert!(notes[0].subject.contains("Unused point"));
}

#[test]
fn freecad_external_body_links_replay_at_the_recipe_volumes_and_bounds() {
    if !Format::FreeCadDocument.can_convert() {
        return;
    }
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/freecad-1.1.4-A01/A01a.FCStd");
    let model = read(Format::FreeCadDocument, &input, &mut Report::default()).unwrap();
    let (bytes, _) = written(&model);
    let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) else {
        return;
    };
    let mut bodies = evaluate(&cli, &bytes);
    assert_eq!(bodies.len(), 2);
    bodies.sort_by(|a, b| {
        a["bbox"]["min"][0]
            .as_f64()
            .unwrap()
            .total_cmp(&b["bbox"]["min"][0].as_f64().unwrap())
    });
    // CC0 recipe A01a, FreeCAD 1.1.4 manifest.json (0.01 mm, 1e-4 relative volume).
    for (body, volume, low, high) in [
        (&bodies[0], 6000.0, [-10.0, -15.0, 0.0], [10.0, 15.0, 10.0]),
        (
            &bodies[1],
            62831.8530718,
            [40.0, -30.0, 0.0],
            [100.0, 30.0, 40.0],
        ),
    ] {
        let actual = body["volume_mm3"].as_f64().unwrap();
        assert!((actual - volume).abs() <= volume * 1e-4, "{body}");
        for (axis, (low, high)) in low.into_iter().zip(high).enumerate() {
            assert!(
                (body["bbox"]["min"][axis].as_f64().unwrap() - low).abs() <= 0.01,
                "{body}"
            );
            assert!(
                (body["bbox"]["max"][axis].as_f64().unwrap() - high).abs() <= 0.01,
                "{body}"
            );
        }
    }
}

#[test]
fn sketch_and_joint_choices_leave_only_geometry_at_stored_placements() {
    let mut model = with_sketches();
    let assembly = model.assembly.as_mut().unwrap();
    assembly.stored_placements = assembly.placements.iter().map(|p| p.transform).collect();
    let stored = assembly.placements[1].transform;
    assembly.placements[1].transform[0][3] += 123.0;
    assembly.joints.push(dutchman_core::Joint {
        a: vec![0],
        b: vec![1],
        frame_a: dutchman_core::placement::IDENTITY,
        frame_b: dutchman_core::placement::IDENTITY,
        suppressed: false,
        motion: dutchman_core::Motion::Rigid,
        flip: true,
    });
    for history in [false, true] {
        let mut bytes = Vec::new();
        let mut report = Report::default();
        SolidCraftWriter::new()
            .import_only(!history)
            .sketches(false)
            .joints(false)
            .verification(Verification::Off)
            .write(&model, &mut bytes, &mut report)
            .unwrap();
        let doc = document(&bytes);
        assert!(doc.get("assembly").is_none());
        assert!(doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .all(|f| f["type"] != "sketch"));
        assert_eq!(doc["occurrences"][1]["transform"][3][0], stored[0][3]);
        assert_eq!(
            report
                .losses()
                .iter()
                .filter(
                    |loss| loss.subject == "joints" && loss.detail.contains("left out by choice")
                )
                .count(),
            1
        );
        assert!(!report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("for reference")));
    }
    let (bytes, _) = written(&model);
    let doc = document(&bytes);
    assert!(!doc["assembly"]["joints"].as_array().unwrap().is_empty());
    assert!(doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "sketch"));
}

#[test]
fn reference_sketch_choice_applies_to_single_parts_and_component_parts() {
    let mut part = with_sketches();
    part.assembly = None;
    for assembly in [false, true] {
        let model = if assembly {
            let mut model = placed_twice();
            model.assembly.as_mut().unwrap().parts = vec![part.clone()];
            model
        } else {
            part.clone()
        };
        for history in [false, true] {
            for sketches in [false, true] {
                let mut bytes = Vec::new();
                let mut report = Report::default();
                SolidCraftWriter::new()
                    .import_only(!history)
                    .sketches(sketches)
                    .verification(Verification::Off)
                    .write(&model, &mut bytes, &mut report)
                    .unwrap();
                let doc = document(&bytes);
                let kept = doc["features"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|f| f["type"] == "sketch");
                assert_eq!(kept, sketches, "assembly={assembly}, history={history}");
            }
        }
    }
}

#[test]
fn parallel_parts_keep_the_complete_container_and_report_in_serial_order() {
    let mut models = vec![
        nested(),
        with_components(true),
        with_sketches(),
        jointed(),
        mixed_histories(),
    ];
    if Format::FreeCadDocument.can_convert() {
        for file in ["product_assembly.FCStd", "freecad-1.1.4-A01/A01a.FCStd"] {
            models.push(
                read(
                    Format::FreeCadDocument,
                    &Path::new(env!("CARGO_MANIFEST_DIR"))
                        .join("../dutchman-core/tests/fixtures")
                        .join(file),
                    &mut Report::default(),
                )
                .unwrap(),
            );
        }
    }
    for model in models {
        for import_only in [false, true] {
            for sketches in [false, true] {
                let writer = SolidCraftWriter::new()
                    .verification(Verification::Off)
                    .import_only(import_only)
                    .sketches(sketches);
                let mut serial = Vec::new();
                let mut serial_report = Report::default();
                writer
                    .clone()
                    .threads(1)
                    .write(&model, &mut serial, &mut serial_report)
                    .unwrap();
                let mut parallel = Vec::new();
                let mut parallel_report = Report::default();
                writer
                    .threads(4)
                    .write(&model, &mut parallel, &mut parallel_report)
                    .unwrap();
                assert_eq!(serial, parallel, "{}", model.name);
                parallel_report.set_threads(1, 1);
                assert_eq!(
                    format!("{serial_report:?}"),
                    format!("{parallel_report:?}"),
                    "{}",
                    model.name
                );
            }
        }
    }
}

fn mixed_histories() -> Model {
    let mut model = placed_twice();
    let assembly = model.assembly.as_mut().unwrap();
    let mut part = assembly.parts[0].clone();
    part.name = "Repeated name".to_owned();
    let mut wire = serde_json::to_value(&part.ir).unwrap();
    let sphere = serde_json::json!({
        "id": "test:model:feature#sphere", "ordinal": 0, "name": "Ball", "suppressed": false,
        "outputs": [wire["model"]["bodies"][0]["id"]],
        "definition": {"definition": "sphere", "center": {"x": 5.0, "y": 5.0, "z": 5.0},
                       "radius": 5.0, "op": "new_body"},
    });
    wire["model"]["features"] = serde_json::json!([sphere]);
    part.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    assembly.parts.push(part.clone());
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::json!({
            "id": "test:model:feature#later", "ordinal": 1, "name": "Later", "suppressed": false,
            "definition": {"definition": "native", "kind": "Mystery"},
        }));
    part.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    assembly.parts.push(part.clone());
    // A restatement failure must retry serially and keep its Dropped note at this position.
    part.ir.model.faces.clear();
    assembly.parts.push(part.clone());
    part.ir.model.bodies.clear();
    assembly.parts.push(part);
    assembly.placements = assembly
        .parts
        .iter()
        .enumerate()
        .map(|(index, _)| Placement {
            hidden: false,
            name: None,
            part: index,
            transform: dutchman_core::placement::IDENTITY,
        })
        .collect();
    model
}

#[test]
fn parallel_replay_checks_keep_partial_histories_and_reports_identical() {
    let Some(cli) = std::env::var_os(CLI_ENV).map(PathBuf::from) else {
        return;
    };
    let model = mixed_histories();
    let writer = SolidCraftWriter::new().verification(Verification::With(cli));
    let mut serial = Vec::new();
    let mut serial_report = Report::default();
    writer
        .clone()
        .threads(1)
        .write(&model, &mut serial, &mut serial_report)
        .unwrap();
    let mut parallel = Vec::new();
    let mut parallel_report = Report::default();
    writer
        .threads(4)
        .write(&model, &mut parallel, &mut parallel_report)
        .unwrap();
    assert_eq!(serial, parallel);
    parallel_report.set_threads(1, 1);
    assert_eq!(format!("{serial_report:?}"), format!("{parallel_report:?}"));
}
