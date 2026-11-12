//! Onshape uses the same history writer and source-geometry verifier as native files.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dutchman_core::{Check, FeatureStatus, Format, Model, Report, Writer};
use dutchman_solidcraft::{SolidCraftEvaluator, SolidCraftVersion, SolidCraftWriter, Verification};
use serde_json::{json, Value};

fn model(name: &str) -> (Model, Report) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../dutchman-core/tests/fixtures/onshape-{name}.onshape-bundle"
    ));
    let mut report = Report::default();
    let model = dutchman_core::read(Format::OnshapeBundle, &path, &mut report).unwrap();
    (model, report)
}

fn document(bytes: &[u8]) -> Value {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut data = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut data)
        .unwrap();
    serde_json::from_str(&data).unwrap()
}

/// Transport responses exercise the shared verifier even when no kernel is installed.
/// These tests assert reference routing and deliberate volume/position/box mismatches;
/// the separate CLI test below performs the actual geometric replay.
#[derive(Debug)]
struct Measurements {
    step: String,
    calls: Mutex<Vec<String>>,
    mismatch: &'static str,
}

impl SolidCraftEvaluator for Measurements {
    fn evaluate(&self, design: &[u8], _: Duration, label: &str) -> Result<Value, String> {
        self.calls.lock().unwrap().push(label.to_owned());
        if label == "original" {
            let mut archive = zip::ZipArchive::new(Cursor::new(design)).unwrap();
            let mut found = false;
            for i in 0..archive.len() {
                let mut entry = archive.by_index(i).unwrap();
                if entry.name().unwrap().ends_with(".step") {
                    let mut data = String::new();
                    entry.read_to_string(&mut data).unwrap();
                    assert_eq!(data, self.step);
                    found = true;
                }
            }
            assert!(found, "the verifier must receive export.step");
        }
        let shifted = label == "history" && self.mismatch == "position";
        let resized = label == "history" && self.mismatch == "box";
        Ok(
            json!({"bodies": [{"volume_mm3": if label == "history" && self.mismatch == "volume" { 300.0 } else { 400.0 },
            "area_mm2": 520.0, "bbox": {"min": if shifted { [30,0,0] } else { [0,0,0] },
                "max": if shifted { [50,10,2] } else if resized { [21,10,2] } else { [20,10,2] }}}],
            "timeline": [], "total": {"faces": 6}}),
        )
    }

    fn run(
        &self,
        _: &[u8],
        commands: &[Value],
        _: Duration,
        label: &str,
    ) -> Result<Vec<Value>, String> {
        self.calls.lock().unwrap().push(label.to_owned());
        commands.iter().map(|command| Ok(match command["command"].as_str().unwrap() {
            "timeline.roll_to" => json!({"marker": command["params"]["feature"]}),
            "document.inspect" => json!({"bodies": [{"name": "Plate"}], "timeline": []}),
            "model.edges" => json!({"body": "Plate", "edges": [{"name": "upright", "start": [0,0,0], "end": [0,0,2], "mid": [0,0,1], "length": 2.0}]}),
            other => return Err(format!("Unexpected command {other}")),
        })).collect()
    }
}

#[test]
fn each_bundle_converts_with_expected_history_rows_and_losses() {
    for (name, kinds) in [
        ("plate-fillet", vec!["sketch", "extrude", "fillet"]),
        ("revolve", vec!["sketch", "revolve"]),
        ("hole", vec!["sketch", "extrude", "sketch", "hole"]),
        ("unsupported", vec!["sketch", "extrude", "native"]),
    ] {
        let (model, mut report) = model(name);
        let host = Arc::new(Measurements {
            step: model.source_step.clone().unwrap(),
            calls: Mutex::default(),
            mismatch: "",
        });
        let writer = SolidCraftWriter::new()
            .evaluator(host)
            .target_version(Some(SolidCraftVersion::default()));
        let mut output = Vec::new();
        writer.write(&model, &mut output, &mut report).unwrap();
        let outcome = report.outcome().unwrap();
        assert_eq!(
            outcome
                .features
                .iter()
                .map(|f| f.kind.as_str())
                .collect::<Vec<_>>(),
            kinds,
            "{name}: {:?}",
            report.losses()
        );
        let doc = document(&output);
        assert!(doc["features"].is_array());
        if name == "unsupported" {
            assert_eq!(
                outcome.features.last().unwrap().status,
                FeatureStatus::Blocked
            );
            assert!(report.losses().iter().any(|loss| loss.code.as_deref()
                == Some("onshape/history.not-supported")
                && loss.detail.contains("custom FeatureScript")));
            assert_ne!(outcome.check, Check::Verified);
        } else {
            assert!(
                outcome
                    .features
                    .iter()
                    .all(|f| f.status == FeatureStatus::Mapped),
                "{name}: {:?}",
                report.losses()
            );
            assert_eq!(
                outcome.check,
                Check::Verified,
                "{name}: {:?}",
                report.losses()
            );
        }
    }
}

#[test]
fn export_step_is_the_reference_and_body_mismatches_preserve_it() {
    for mismatch in ["", "position", "box", "volume"] {
        let (model, mut report) = model("plate-fillet");
        let host = Arc::new(Measurements {
            step: model.source_step.clone().unwrap(),
            calls: Mutex::default(),
            mismatch,
        });
        let writer = SolidCraftWriter::new()
            .evaluator(host.clone())
            .target_version(Some(SolidCraftVersion::default()));
        let mut output = Vec::new();
        writer.write(&model, &mut output, &mut report).unwrap();
        let calls = host.calls.lock().unwrap();
        assert!(calls.iter().any(|s| s == "original"));
        assert!(calls.iter().any(|s| s == "history"));
        assert_eq!(
            report.outcome().unwrap().check == Check::Verified,
            mismatch.is_empty(),
            "{mismatch}: {:?}",
            report.losses()
        );
        assert!(document(&output)["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["type"] == "import"));
    }
}

#[test]
fn real_solidcraft_replays_the_synthetic_bodies_against_the_step_exports() {
    let Some(cli) = std::env::var_os(dutchman_solidcraft::CLI_ENV).map(PathBuf::from) else {
        eprintln!("Set SOLIDCRAFT_CLI to replay Onshape fixture histories");
        return;
    };
    for name in ["plate-fillet", "revolve", "hole"] {
        let (model, mut report) = model(name);
        let writer = SolidCraftWriter::new().verification(Verification::With(cli.clone()));
        writer.write(&model, &mut Vec::new(), &mut report).unwrap();
        assert_eq!(
            report.outcome().unwrap().check,
            Check::Verified,
            "{name}: {:?}",
            report.losses()
        );
    }
}
