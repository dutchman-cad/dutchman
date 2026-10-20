//! Decoding native files through cadmpeg, on synthetic fixtures only.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};

use dutchman_core::{read, Error, Format, Report};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dutchman-decode-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

#[test]
fn decodes_a_fusion_unit_cube() {
    let mut report = Report::default();
    let model = read(
        Format::FusionArchive,
        &fixture("generated_unit_cube.f3d"),
        &mut report,
    )
    .unwrap();

    assert_eq!(model.name, "generated_unit_cube");
    assert_eq!(model.ir.model.bodies.len(), 1);
    assert_eq!(model.ir.model.faces.len(), 6);
    assert!(model.fidelity.is_some());
}

#[test]
fn a_file_without_geometry_is_an_error() {
    let mut report = Report::default();
    let err = read(
        Format::FusionArchive,
        &fixture("container_metadata_only.f3d"),
        &mut report,
    )
    .unwrap_err();
    assert!(
        err.to_string().contains("no geometry could be read"),
        "{err}"
    );
}

#[test]
fn hostile_inputs_are_errors_not_panics() {
    let cube = std::fs::read(fixture("generated_unit_cube.f3d")).unwrap();
    let mut flipped = cube.clone();
    for byte in flipped.iter_mut().skip(64).step_by(97) {
        *byte ^= 0x5a;
    }
    let cases: [(&str, &[u8]); 5] = [
        ("empty.f3d", b""),
        ("text.f3d", b"ISO-10303-21;\nHEADER;\nENDSEC;\n"),
        ("noise.f3d", &[0xff; 4096]),
        ("truncated.f3d", cube.get(..cube.len() / 2).unwrap()),
        ("flipped.f3d", &flipped),
    ];
    for (name, bytes) in cases {
        let path = scratch(name, bytes);
        let mut report = Report::default();
        let result = read(Format::FusionArchive, &path, &mut report);
        assert!(
            matches!(result, Err(Error::Decode(_))),
            "{name}: {result:?}"
        );
    }
}

#[test]
fn formats_without_a_decoder_are_not_supported_yet() {
    let path = scratch("part.ipt", b"anything");
    let mut report = Report::default();
    let err = read(Format::InventorPart, &path, &mut report).unwrap_err();
    assert!(matches!(
        err,
        Error::NotSupportedYet {
            format: Format::InventorPart
        }
    ));
}
