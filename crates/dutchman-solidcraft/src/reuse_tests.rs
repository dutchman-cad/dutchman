//! Compare final checks with the same checks run without evaluation reuse.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use super::*;
use serde_json::{json, Value};
use std::io::{Cursor, Read};
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct Counting {
    calls: Mutex<Vec<Vec<u8>>>,
    repair: bool,
    sketches: bool,
    refuse_import: bool,
}

impl SolidCraftEvaluator for Counting {
    fn evaluate(&self, design: &[u8], _: Duration, _: &str) -> Result<Value, String> {
        self.calls.lock().unwrap().push(design.to_vec());
        let mut archive = zip::ZipArchive::new(Cursor::new(design)).unwrap();
        let doc: Value =
            serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
        let mut timeline = Vec::new();
        if self.refuse_import {
            if let Some(feature) = doc["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|feature| feature["type"] == "import")
            {
                timeline.push(json!({"id": feature["id"], "name": feature["name"],
                    "error": "Synthetic refusal"}));
            }
        }
        if self.repair {
            for feature in doc["features"].as_array().unwrap() {
                if let Some(blob) = feature["step_blob"].as_str() {
                    let mut step = String::new();
                    archive
                        .by_name(blob)
                        .unwrap()
                        .read_to_string(&mut step)
                        .unwrap();
                    if step.contains("#4=ORIENTED_EDGE('',*,*,#1,.T.)") {
                        timeline.push(json!({"id": feature["id"], "name": feature["name"],
                            "error": "a sheet touching itself along an edge"}));
                    }
                }
            }
        }
        timeline
            .push(json!({"name": "Synthetic check", "warning": "Synthetic measurement warning"}));
        Ok(
            json!({"bodies": if self.sketches {json!([])} else {json!([{"volume_mm3": 1.0}])},
            "timeline": timeline, "warnings": ["Synthetic measurement warning"]}),
        )
    }

    fn run(&self, _: &[u8], _: &[Value], _: Duration, _: &str) -> Result<Vec<Value>, String> {
        Err("No commands expected".to_owned())
    }
}

fn part() -> Model {
    Model {
        ir: cadmpeg_ir::examples::unit_cube().unwrap(),
        fidelity: None,
        name: "Synthetic cube".to_owned(),
        source_file: None,
        assembly: None,
        source_step: None,
    }
}

// Report is not Serialize. Compare JSON projections of the lines and checks these
// paths consume, plus the complete Debug representation so no other field can drift.
fn report_json(report: &Report) -> Value {
    json!({
        "losses": report.losses().iter().map(|loss| json!({
            "level": format!("{:?}", loss.severity), "code": loss.code,
            "subject": loss.subject, "message": loss.detail,
            "stage": loss.stage.id(), "upstream": loss.upstream(),
        })).collect::<Vec<_>>(),
        "verification_checks": report.verification_checks(),
        "verification_tolerances": report.verification_tolerances(),
        "complete_report": format!("{report:?}"),
    })
}

fn compare(model: &Model, import_only: bool, repair: bool, sketches: bool, expected: usize) {
    let mut reports = Vec::new();
    let mut outputs = Vec::new();
    for reuse in [false, true] {
        let host = Arc::new(Counting {
            repair,
            sketches,
            ..Counting::default()
        });
        let mut writer = SolidCraftWriter::new()
            .import_only(import_only)
            .target_version(Some(SolidCraftVersion::default()))
            .evaluator(host.clone());
        writer.reuse_evaluations = reuse;
        let mut bytes = Vec::new();
        let mut report = Report::default();
        writer.write(model, &mut bytes, &mut report).unwrap();
        if !sketches && !repair {
            assert!(report
                .losses()
                .iter()
                .any(|loss| loss.detail.contains("Synthetic measurement warning")));
        }
        let calls = host.calls.lock().unwrap();
        if reuse {
            assert_eq!(calls.len(), expected);
            if repair {
                assert_ne!(calls[1], calls[2]);
            }
        } else if repair {
            assert_eq!(calls.len(), expected);
        } else {
            assert_eq!(calls.len(), 2);
        }
        outputs.push(bytes);
        reports.push(report_json(&report));
    }
    assert_eq!(reports[0], reports[1]);
    assert_eq!(outputs[0], outputs[1]);
}

fn assembly() -> Model {
    let part = part();
    let mut model = part.clone();
    model.assembly = Some(dutchman_core::Assembly {
        own_part: None,
        parts: vec![part],
        placements: vec![dutchman_core::Placement {
            unproven: false,
            hidden: false,
            name: None,
            part: 0,
            transform: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        }],
        joints: Vec::new(),
        left_out: Vec::new(),
        held: Vec::new(),
        joint_notes: Vec::new(),
        stored_placements: Vec::new(),
        identities: Vec::new(),
        definitions: Vec::new(),
    });
    model
}

#[test]
fn assembly_checks_evaluate_identical_bytes_once() {
    compare(&assembly(), true, false, false, 1);
}

#[test]
fn assembly_repair_evaluates_changed_bytes_again() {
    let model = assembly();
    let mut reports = Vec::new();
    let mut outputs = Vec::new();
    for reuse in [false, true] {
        let host = Arc::new(Counting {
            refuse_import: true,
            ..Counting::default()
        });
        let mut writer = SolidCraftWriter::new()
            .import_only(true)
            .target_version(Some(SolidCraftVersion::default()))
            .evaluator(host.clone());
        writer.reuse_evaluations = reuse;
        let mut report = Report::default();
        let mut bytes = Vec::new();
        writer.write(&model, &mut bytes, &mut report).unwrap();
        let calls = host.calls.lock().unwrap();
        assert_eq!(calls.len(), if reuse { 2 } else { 3 });
        assert_ne!(calls[0], calls[1]);
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("geometry/import-failed")));
        reports.push(report_json(&report));
        outputs.push(bytes);
    }
    assert_eq!(reports[0], reports[1]);
    assert_eq!(outputs[0], outputs[1]);
}

#[test]
fn imported_parts_evaluate_identical_bytes_once() {
    compare(&part(), true, false, false, 1);
    // A part with no editable history also reaches the geometry-only fallback.
    compare(&part(), false, false, false, 1);
}

#[test]
fn sketch_only_parts_evaluate_identical_bytes_once() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/onshape-plate-fillet.onshape-bundle");
    let mut model = dutchman_core::read(
        dutchman_core::Format::OnshapeBundle,
        &path,
        &mut Report::default(),
    )
    .unwrap();
    model.source_step = None;
    model.ir.geometry = Default::default();
    model
        .ir
        .model
        .features
        .retain(|feature| feature.ordinal == 0);
    compare(&model, false, false, true, 1);
}

#[test]
fn repaired_part_evaluates_changed_bytes_again() {
    let mut model = part();
    model.source_step = Some("ISO-10303-21;DATA;#1=EDGE_CURVE('',#10,#11,#12,.T.);#2=ORIENTED_EDGE('',*,*,#1,.T.);#3=ORIENTED_EDGE('',*,*,#1,.F.);#4=ORIENTED_EDGE('',*,*,#1,.T.);ENDSEC;END-ISO-10303-21;".to_owned());
    compare(&model, true, true, false, 3);
    // The import check sees the refusal too, so it cannot supply a successful
    // opening result: one import attempt, then the original and repaired archives.
}

#[test]
fn opening_repair_then_assembly_geometry_evaluates_only_twice() {
    let source = "ISO-10303-21;DATA;#1=EDGE_CURVE('',#10,#11,#12,.T.);#2=ORIENTED_EDGE('',*,*,#1,.T.);#3=ORIENTED_EDGE('',*,*,#1,.F.);#4=ORIENTED_EDGE('',*,*,#1,.T.);ENDSEC;END-ISO-10303-21;";
    let container = crate::container::Container::new(source.as_bytes()).unwrap();
    let blob = container.step_blob().to_owned();
    let bytes = container.finish(&json!({"features":[{"id":1,"type":"import","name":"Sheet","component":10,"step_blob":blob}],"occurrences":[{"id":11,"component":10,"parent":0}]})).unwrap();
    let mut reports = Vec::new();
    for reuse in [false, true] {
        let host = Arc::new(Counting {
            repair: true,
            ..Counting::default()
        });
        let mut writer = SolidCraftWriter::new().evaluator(host.clone());
        writer.reuse_evaluations = reuse;
        let evaluator = writer.final_evaluator();
        let mut report = Report::default();
        let repaired = writer
            .check_opening(bytes.clone(), &mut report, evaluator.as_ref())
            .unwrap();
        assert_ne!(bytes, repaired);
        writer.check_geometry(
            &repaired,
            verify::Expected::default(),
            &mut report,
            None,
            "Synthetic assembly",
            evaluator.as_ref(),
        );
        let calls = host.calls.lock().unwrap();
        assert_eq!(calls.len(), if reuse { 2 } else { 3 });
        assert_ne!(calls[0], calls[1]);
        reports.push(report_json(&report));
    }
    assert_eq!(reports[0], reports[1]);
}
