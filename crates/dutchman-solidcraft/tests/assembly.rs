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
