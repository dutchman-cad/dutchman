use dutchman_core::{Format, Report};
use dutchman_solidcraft::SolidCraftWriter;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn a_deadline_discards_planning_without_writing_a_design() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/freecad-1.1.4-A01/P01a.FCStd");
    let model =
        dutchman_core::read(Format::FreeCadDocument, &path, &mut Report::default()).unwrap();
    let writer = SolidCraftWriter::new();
    let mut report = Report::default();
    assert!(writer
        .plan_until(&model, &mut report, Instant::now())
        .is_none());
    let ordinary = writer.plan(&model, &mut Report::default());
    let bounded = writer
        .plan_until(
            &model,
            &mut report,
            Instant::now() + Duration::from_secs(30),
        )
        .unwrap();
    assert_eq!(ordinary, bounded);
    assert!(!bounded.features.is_empty());
    for feature in &bounded.features {
        assert!(feature.output.ids.is_empty());
        assert_eq!(
            feature.output.method,
            dutchman_core::OutputMatch::NotWritten
        );
    }
}

#[test]
fn components_without_exportable_geometry_have_no_planned_history() {
    use dutchman_core::{Assembly, Mode, Placement, Writer};
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
    let cube = dutchman_core::read(Format::FusionArchive, &path, &mut Report::default()).unwrap();
    for no_bodies in [true, false] {
        let mut empty = cube.clone();
        empty.name = "Empty component".to_owned();
        empty.assembly = None;
        empty.source_file = Some("empty-component".to_owned());
        if no_bodies {
            empty.ir.geometry.bodies.clear();
        } else {
            empty.ir.geometry.faces.clear();
        }
        empty.ir.model.features = serde_json::from_value(serde_json::json!([{
            "id": "f3d:model:feature#1", "ordinal": 0, "name": "History",
            "suppressed": false, "dependencies": [], "source_properties": {},
            "source_content": [], "outputs": [],
            "definition": {"kind": "operation", "value": {
                "kind": "tree_node", "value": {"role": "history", "children": {"children": []}}
            }}
        }]))
        .unwrap();
        let mut model = cube.clone();
        model.ir.geometry.bodies.clear();
        model.assembly = Some(Assembly {
            own_part: None,
            parts: vec![cube.clone(), empty],
            definitions: Vec::new(),
            identities: vec![Some("cube".to_owned()), Some("empty".to_owned())],
            placements: (0..2)
                .map(|part| Placement {
                    unproven: false,
                    name: None,
                    part,
                    transform: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
                    hidden: false,
                })
                .collect(),
            joints: Vec::new(),
            left_out: Vec::new(),
            held: Vec::new(),
            joint_notes: Vec::new(),
            stored_placements: Vec::new(),
        });
        let writer = SolidCraftWriter::new()
            .threads(1)
            .verification(dutchman_solidcraft::Verification::Off)
            .target_version(Some(Default::default()));
        let planned = writer.plan(&model, &mut Report::default());
        let mut report = Report::default();
        writer.write(&model, &mut Vec::new(), &mut report).unwrap();
        assert_eq!(planned.mode, Mode::Import);
        assert_eq!(planned.mode, report.outcome().unwrap().mode);
        assert!(planned.features.iter().all(|feature| {
            feature.output.ids.is_empty()
                && feature.output.method == dutchman_core::OutputMatch::NotWritten
        }));
        assert!(planned
            .features
            .iter()
            .all(|feature| feature.kind != "tree_node"));
    }
}
