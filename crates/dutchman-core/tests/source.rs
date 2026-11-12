#![cfg(feature = "source-reader")]
#![allow(clippy::unwrap_used)]

use dutchman_core::{source, Error, Format, PartPositions, Report, MAX_INPUT_BYTES};
use std::{cell::Cell, path::Path};

#[test]
fn oversized_input_is_refused_before_producing_bytes() {
    let produced = Cell::new(false);
    let name = Path::new("huge.f3d");
    let size = MAX_INPUT_BYTES + 1;
    let result = source::read(
        Format::FusionArchive,
        name,
        false,
        PartPositions::Smart,
        &|path, limit| {
            assert_eq!(limit, MAX_INPUT_BYTES);
            if size > limit {
                return Err(Error::TooLarge {
                    path: path.into(),
                    bytes: size,
                    limit,
                });
            }
            produced.set(true);
            Ok(Vec::new())
        },
        &|_, _| None,
        &mut Report::default(),
    );
    assert!(!produced.get());
    let error = result.unwrap_err();
    assert!(
        matches!(error, Error::TooLarge { bytes, limit, .. } if bytes == size && limit == MAX_INPUT_BYTES)
    );
    assert_eq!(
        error.to_string(),
        format!("huge.f3d: the file is {size} bytes, over Dutchman's {MAX_INPUT_BYTES}-byte limit")
    );
}

#[test]
fn oversized_referenced_part_is_refused_before_producing_bytes() {
    let refused = Cell::new(false);
    let produced = Cell::new(false);
    let mut report = Report::default();
    source::read(
        Format::SolidWorksAssembly,
        Path::new("assembly.sldasm"),
        false,
        PartPositions::Smart,
        &|path, limit| {
            assert_eq!(limit, MAX_INPUT_BYTES);
            if path == Path::new("assembly.sldasm") {
                return Ok(include_bytes!("fixtures/placed_cubes.sldasm").to_vec());
            }
            let size = MAX_INPUT_BYTES + 1;
            if size > limit {
                refused.set(true);
                return Err(Error::TooLarge {
                    path: path.into(),
                    bytes: size,
                    limit,
                });
            }
            produced.set(true);
            Ok(Vec::new())
        },
        &|_, _| Some("huge.sldprt".into()),
        &mut report,
    )
    .unwrap();
    assert!(refused.get());
    assert!(!produced.get());
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail.contains("over Dutchman's")));
}

#[test]
fn reports_the_requested_part_positions() {
    for positions in [
        PartPositions::Smart,
        PartPositions::LastShown,
        PartPositions::FirstPlaced,
    ] {
        let mut report = Report::default();
        source::read(
            Format::FusionArchive,
            Path::new("cube.f3d"),
            false,
            positions,
            &|_, _| Ok(include_bytes!("fixtures/generated_unit_cube.f3d").to_vec()),
            &|_, _| None,
            &mut report,
        )
        .unwrap();
        assert_eq!(report.part_positions().unwrap().0, positions);
    }
}
