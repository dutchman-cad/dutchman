//! Optional real-kernel controls using only published synthetic fixtures from this repo.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use dutchman_core::{Model, Report};
use dutchman_solidcraft::{compare, Verification};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[test]
fn public_synthetic_controls_and_planted_assembly_placement() {
    let Some(cli) = std::env::var_os("SOLIDCRAFT_CLI").map(PathBuf::from) else {
        return;
    };
    let verification = Verification::With(cli);
    let dir = tempfile::tempdir().unwrap();
    let mut measured = 0;
    for name in [
        "generated_unit_cube.f3d",
        "synthetic_cube.ipt",
        "solidworks_cube.sldprt",
    ] {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures")
            .join(name);
        let source = dutchman_core::read(
            dutchman_core::Format::from_path(&path).unwrap(),
            &path,
            &mut Report::default(),
        )
        .unwrap();
        let step = dutchman_solidcraft::restate_step(&source, &mut Report::default()).unwrap();
        let path = dir.path().join("reference.step");
        std::fs::write(&path, step).unwrap();
        let reference =
            dutchman_core::read(dutchman_core::Format::Step, &path, &mut Report::default())
                .unwrap();
        let result = compare::models(&source, &reference, &verification, Duration::from_secs(30));
        assert_eq!(result["status"], "measured", "{name}: {result}");
        assert!(
            result["parts"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["looks_wrong"] == false && r["status"] == "measured"),
            "{name}: {result}"
        );
        measured += 1;
    }
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
    let part = dutchman_core::read(
        dutchman_core::Format::FusionArchive,
        &path,
        &mut Report::default(),
    )
    .unwrap();
    let pair = |x| {
        let at = |x| dutchman_core::Placement {
            name: None,
            part: 0,
            hidden: false,
            transform: [
                [1.0, 0.0, 0.0, x],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
        };
        Model {
            assembly: Some(dutchman_core::Assembly {
                own_part: None,
                parts: vec![part.clone()],
                placements: vec![at(0.0), at(x)],
                definitions: vec![],
                identities: vec![],
                joints: vec![],
                left_out: vec![],
                held: vec![],
                joint_notes: vec![],
                stored_placements: vec![],
            }),
            ..part.clone()
        }
    };
    let source = pair(20.0);
    let reference = pair(21.0);
    let result = compare::models(&source, &reference, &verification, Duration::from_secs(30));
    assert_eq!(result["status"], "measured", "{result}");
    assert_eq!(result["parts"][0]["looks_wrong"], true, "{result}");
    assert!(
        result["parts"][0]["differences"]["placement_mm"]
            .as_f64()
            .unwrap()
            > 0.9,
        "{result}"
    );
    let preview = dir.path().join("preview");
    assert!(compare::render_models(
        &source,
        &reference,
        &verification,
        Duration::from_secs(30),
        &preview
    ));
    assert!(preview.join("original.png").is_file());
    assert!(preview.join("converted.png").is_file());
    println!("Synthetic controls: {measured} measured, 0 flagged. Planted 1 mm assembly move: detected. Snapshot pair: produced.");
}
