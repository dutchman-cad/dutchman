//! A native file converted end to end, checked against SolidCraft's design container layout
//! and a golden copy of the design document.
//!
//! Set `UPDATE_GOLDEN=1` to rewrite the golden file after an intended change.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use dutchman_core::convert;
use dutchman_solidcraft::{SolidCraftWriter, Verification};
use serde_json::Value;
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipArchive};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures")
        .join(name)
}

fn golden(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(name)
}

fn entry(archive: &mut ZipArchive<Cursor<Vec<u8>>>, name: &str) -> (Vec<u8>, CompressionMethod) {
    let mut file = archive.by_name(name).unwrap();
    let method = file.compression();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    (bytes, method)
}

#[test]
fn fusion_unit_cube_becomes_a_solidcraft_import() {
    converts_to_an_import("generated_unit_cube.f3d");
}

#[test]
fn inventor_unit_cube_becomes_a_solidcraft_import() {
    converts_to_an_import("synthetic_cube.ipt");
}

#[test]
fn solidworks_unit_cube_becomes_a_solidcraft_import() {
    converts_to_an_import("solidworks_cube.sldprt");
}

/// Converts fixture `file`, which has no history, and checks the result is SolidCraft's
/// import container with a document matching `tests/golden/<stem>.document.json`.
fn converts_to_an_import(file: &str) {
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem);
    let dir = std::env::temp_dir().join(format!("dutchman-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let output = dir.join(format!("{stem}.solidcraft"));

    let report = convert(
        &fixture(file),
        &output,
        &SolidCraftWriter::new().verification(Verification::Off),
    )
    .unwrap();
    for loss in report.losses() {
        println!("{loss}");
    }

    let bytes = std::fs::read(&output).unwrap();
    assert!(bytes.starts_with(b"PK\x03\x04"), "a zip container");
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();

    // The layout SolidCraft writes: the STEP payload, then the document, then the stamp.
    let names: Vec<String> = (0..archive.len())
        .map(|index| {
            archive
                .by_index(index)
                .unwrap()
                .name()
                .unwrap()
                .into_owned()
        })
        .collect();
    assert_eq!(names.len(), 3, "{names:?}");
    let blob = names.first().unwrap().clone();
    assert_eq!(names.get(1..).unwrap(), ["document.json", "container.json"]);

    let (container, method) = entry(&mut archive, "container.json");
    assert_eq!(method, CompressionMethod::Stored);
    assert_eq!(container, br#"{"format":"solidcraft-container/1"}"#);

    let (step, method) = entry(&mut archive, &blob);
    assert_eq!(method, CompressionMethod::Stored);
    let digest: String = Sha256::digest(&step)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert_eq!(blob, format!("blobs/{digest}.step"), "content-addressed");
    let step = String::from_utf8(step).unwrap();
    assert!(step.starts_with("ISO-10303-21;"), "STEP text");
    assert!(step.contains("AUTOMOTIVE_DESIGN"), "AP214");
    assert!(step.contains("MANIFOLD_SOLID_BREP"), "the cube is a solid");

    let (document, method) = entry(&mut archive, "document.json");
    assert_eq!(method, CompressionMethod::Deflated);
    let document: Value = serde_json::from_slice(&document).unwrap();
    let import = document["features"].get(0).unwrap();
    assert_eq!(import["type"], "import");
    assert_eq!(import["step"], "");
    assert_eq!(import["step_blob"], blob.as_str());
    assert_eq!(import["file"], file);

    let pretty = serde_json::to_string_pretty(&document).unwrap() + "\n";
    let golden = golden(&format!("{stem}.document.json"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &pretty).unwrap();
    }
    let expected = std::fs::read_to_string(&golden).unwrap();
    assert_eq!(
        pretty,
        expected,
        "document.json differs from {}",
        golden.display()
    );
}
