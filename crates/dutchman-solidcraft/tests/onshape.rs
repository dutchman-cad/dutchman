//! Onshape uses the same history writer and source-geometry verifier as native files.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dutchman_core::{Check, FeatureStatus, Format, Model, Report, Writer};
use dutchman_solidcraft::{
    solidcraft_version, SolidCraftEvaluator, SolidCraftVersion, SolidCraftWriter, Verification,
    VersionGate,
};
use serde_json::{json, Value};

fn model(name: &str) -> (Model, Report) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../dutchman-core/tests/fixtures/onshape-{name}.onshape-bundle"
    ));
    let mut report = Report::default();
    let model = dutchman_core::read(Format::OnshapeBundle, &path, &mut report).unwrap();
    (model, report)
}

/// The plate fixture with its sketch length and extrude depth written as Part Studio
/// variables (`#span`, and `#thick` defined from it), evaluating to the fixture's own values.
fn variable_model() -> (Model, Report) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/onshape-plate-fillet.onshape-bundle");
    let source = std::fs::read(path).unwrap();
    let mut archive = zip::ZipArchive::new(Cursor::new(source)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let mut name = member.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        // The two variables move every feature, and the body details captured at its
        // rollback position, two places down.
        if name.ends_with("bodydetails/index.json") {
            let indices: Vec<u64> = serde_json::from_slice(&bytes).unwrap();
            let moved: Vec<u64> = indices.iter().map(|i| i + 2).collect();
            bytes = serde_json::to_vec(&moved).unwrap();
        } else if let Some((folder, file)) = name.clone().rsplit_once("bodydetails/") {
            let index: u64 = file.strip_suffix(".json").unwrap().parse().unwrap();
            name = format!("{folder}bodydetails/{}.json", index + 2);
        }
        if name.ends_with("features.json") {
            let mut response: Value = serde_json::from_slice(&bytes).unwrap();
            let features = response["features"].as_array_mut().unwrap();
            for row in features[0]["constraints"].as_array_mut().unwrap() {
                for p in row["parameters"].as_array_mut().unwrap() {
                    if p["parameterId"] == "length" {
                        p["expression"] = json!("#span");
                    }
                }
            }
            for p in features[1]["parameters"].as_array_mut().unwrap() {
                if p["parameterId"] == "depth" {
                    p["expression"] = json!("#thick");
                }
            }
            for (at, (feature, name, expression)) in [
                ("span-variable", "span", "20 mm"),
                ("thick-variable", "thick", "#span / 10"),
            ]
            .into_iter()
            .enumerate()
            {
                features.insert(at, json!({"featureId": feature, "featureType": "assignVariable", "name": format!("Variable {name}"),
                    "parameters": [{"parameterId": "mode", "value": "ASSIGNED"}, {"parameterId": "variableType", "value": "LENGTH"},
                    {"parameterId": "name", "value": name}, {"parameterId": "lengthValue", "expression": expression}]}));
            }
            bytes = serde_json::to_vec(&response).unwrap();
        }
        output
            .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    let bytes = output.finish().unwrap().into_inner();
    let mut report = Report::default();
    let model = dutchman_core::read_bytes(
        Format::OnshapeBundle,
        Path::new("variables.onshape-bundle"),
        &bytes,
        &mut report,
    )
    .unwrap();
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
        (
            "hole-forms",
            vec!["sketch", "extrude", "sketch", "hole", "hole", "hole"],
        ),
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
        if name == "hole-forms" {
            let holes: Vec<&Value> = doc["features"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["type"] == "hole")
                .collect();
            assert_eq!(holes.len(), 3);
            assert_eq!(holes[0]["hole"]["type"], "counterbore");
            assert_eq!(holes[1]["hole"]["type"], "countersink");
            assert_eq!(holes[2]["hole"]["type"], "drilled");
            let tip: f64 = holes[2]["hole"]["tip_angle"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap();
            assert!((tip - 118.0).abs() < 1e-9);
            assert_eq!(holes[2]["thread"], "M4x0.7");
            assert_eq!(holes[2]["depth"], "6 mm");
            assert_eq!(holes[2]["thread_depth"], "5 mm");
        }
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
fn onshape_blends_blend_only_their_listed_edges() {
    // Onshape lists every edge a fillet blends (its tangent chain resolved, or propagation
    // off), so SolidCraft must not extend the pick along tangent edges of its own.
    let (source, mut report) = model("plate-fillet");
    let mut output = Vec::new();
    SolidCraftWriter::new()
        .target_version(Some(SolidCraftVersion::default()))
        .write(&source, &mut output, &mut report)
        .unwrap();
    let doc = document(&output);
    let fillet = doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "fillet")
        .unwrap();
    assert_eq!(fillet["tangent_chain"], json!(false), "{fillet}");
    // Before SolidCraft 0.40 every blend follows tangent chains: the fillet is a named loss.
    let (source, mut report) = model("plate-fillet");
    let mut output = Vec::new();
    SolidCraftWriter::new()
        .target_version(Some(SolidCraftVersion::V0_30_0))
        .write(&source, &mut output, &mut report)
        .unwrap();
    assert!(!document(&output)["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "fillet"));
    assert!(
        report
            .losses()
            .iter()
            .any(|l| l.detail.contains("needs SolidCraft 0.40 or newer")),
        "{:?}",
        report.losses()
    );
}

#[test]
fn real_solidcraft_replays_the_synthetic_bodies_against_the_step_exports() {
    let Some(cli) = std::env::var_os(dutchman_solidcraft::CLI_ENV).map(PathBuf::from) else {
        eprintln!("Set SOLIDCRAFT_CLI to replay Onshape fixture histories");
        return;
    };
    for name in ["plate-fillet", "revolve", "hole", "hole-forms", "variables"] {
        let (model, mut report) = if name == "variables" {
            variable_model()
        } else {
            model(name)
        };
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

#[test]
fn sketch_only_history_writes_without_an_import_or_step_blob() {
    let (mut model, mut report) = model("plate-fillet");
    model.source_step = None;
    model.ir.geometry = Default::default();
    model
        .ir
        .model
        .features
        .retain(|feature| feature.ordinal == 0);
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    writer.convertible(&model).unwrap();
    let mut bytes = Vec::new();
    writer.write(&model, &mut bytes, &mut report).unwrap();
    let doc = document(&bytes);
    assert_eq!(doc["features"].as_array().unwrap().len(), 1);
    assert_eq!(doc["features"][0]["type"], "sketch");
    assert!(matches!(
        report.outcome().unwrap().check,
        Check::NotApplicable(_)
    ));
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    for i in 0..zip.len() {
        assert!(!zip.by_index(i).unwrap().name().unwrap().ends_with(".step"));
    }
}

/// A sketch-only design has no source bodies: a replay must run without a feature error and
/// build no body, and one that builds a body or fails says so.
#[test]
fn sketch_only_designs_are_checked_by_a_clean_replay_without_bodies() {
    for (eval, expected) in [
        (json!({"bodies": [], "timeline": []}), "verified"),
        (
            json!({"bodies": [{"volume_mm3": 1.0}], "timeline": []}),
            "differs",
        ),
        (
            json!({"bodies": [], "timeline": [{"name": "Sketch", "error": "unsolved"}]}),
            "differs",
        ),
    ] {
        #[derive(Debug)]
        struct Fixed(Value);
        impl SolidCraftEvaluator for Fixed {
            fn evaluate(&self, _: &[u8], _: Duration, _: &str) -> Result<Value, String> {
                Ok(self.0.clone())
            }
            fn run(
                &self,
                _: &[u8],
                _: &[Value],
                _: Duration,
                _: &str,
            ) -> Result<Vec<Value>, String> {
                Err("not used".to_owned())
            }
        }
        let (mut model, mut report) = model("plate-fillet");
        model.source_step = None;
        model.ir.geometry = Default::default();
        model
            .ir
            .model
            .features
            .retain(|feature| feature.ordinal == 0);
        let writer = SolidCraftWriter::new().evaluator(Arc::new(Fixed(eval)));
        writer.write(&model, &mut Vec::new(), &mut report).unwrap();
        let check = &report.outcome().unwrap().check;
        match expected {
            "verified" => assert_eq!(check, &Check::Verified),
            _ => assert!(matches!(check, Check::Differs(_)), "{check:?}"),
        }
    }
}

#[test]
fn part_studio_variables_drive_the_sketch_dimension_and_extrude_depth() {
    let (model, mut report) = variable_model();
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
    assert!(
        outcome
            .features
            .iter()
            .all(|f| matches!(f.status, FeatureStatus::Mapped | FeatureStatus::NotNeeded)),
        "{:?}",
        report.losses()
    );
    assert_eq!(outcome.check, Check::Verified, "{:?}", report.losses());
    let doc = document(&output);
    let params = doc["params"].as_array().unwrap();
    let expr = |name: &str| {
        params
            .iter()
            .find(|p| p["name"] == name)
            .unwrap_or_else(|| panic!("no parameter {name}: {params:?}"))["expr"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(expr("span"), "20 mm");
    assert_eq!(expr("thick"), "(span / 10)");
    // The extrude distance and the sketch length name parameters that read the variables.
    let features = doc["features"].as_array().unwrap();
    let extrude = features.iter().find(|f| f["type"] == "extrude").unwrap();
    fn find<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
        match v {
            Value::Object(o) => o.get(key).or_else(|| o.values().find_map(|v| find(v, key))),
            Value::Array(a) => a.iter().find_map(|v| find(v, key)),
            _ => None,
        }
    }
    let depth = find(extrude, "distance").and_then(Value::as_str).unwrap();
    assert_eq!(expr(depth), "thick");
    let sketch = features.iter().find(|f| f["type"] == "sketch").unwrap();
    let driven: Vec<_> = sketch["sketch"]["constraints"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["param"].as_str())
        .map(expr)
        .collect();
    assert_eq!(driven, ["span"]);
}

#[test]
fn derived_design_writes_insert_design_links_and_keeps_boolean_editable() {
    for (target, linked) in [
        ("0.44.0", true),
        ("0.44.1", true),
        ("0.43.0", false),
        ("0.18.2", false),
    ] {
        let (model, mut report) = model("derived");
        let writer = SolidCraftWriter::new()
            .target_version(Some(target.parse().unwrap()))
            .verification(Verification::Off);
        let mut bytes = Vec::new();
        writer.write(&model, &mut bytes, &mut report).unwrap();
        let doc = document(&bytes);
        let combine = doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "combine");
        assert!(combine.is_some(), "{doc:#}\n{:?}", report.losses());
        let combine = combine.unwrap();
        assert_eq!(combine["target"], "Plate");
        assert_eq!(combine["tools"], json!(["Tool"]));
        assert_eq!(combine["operation"], "cut");
        let companions = writer.companion_files(&bytes).unwrap();
        assert_eq!(!companions.is_empty(), linked);
        assert_eq!(doc.get("links").is_some(), linked);
        assert_eq!(
            report
                .losses()
                .iter()
                .any(|loss| loss.code.as_deref() == Some("design/reference.frozen")),
            !linked
        );
        if linked {
            let link = doc["links"].as_object().unwrap().values().next().unwrap();
            assert_eq!(link["path"], companions[0].0);
            assert!(document(&companions[0].1)["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["type"] == "extrude"));
            assert_eq!(doc["components"].as_array().unwrap().len(), 1);
            assert_eq!(doc["occurrences"].as_array().unwrap().len(), 1);
        }
    }
}

#[test]
fn derived_companion_conversion_losses_are_reported_under_the_insert() {
    let (mut source, mut report) = model("derived");
    let mut wire = serde_json::to_value(&source.ir).unwrap();
    let mut reference: Value = serde_json::from_str(
        wire["model"]["features"][2]["source_properties"]["design_reference"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    reference["ir"]["model"]["features"][1]["definition"] = json!({
        "kind":"operation", "value":{"kind":"native", "value":{
            "kind":"Synthetic unsupported source operation", "parameters":{}
        }}
    });
    wire["model"]["features"][2]["source_properties"]["design_reference"] =
        json!(serde_json::to_string(&reference).unwrap());
    source.ir = serde_json::from_value(wire).unwrap();
    let mut bytes = Vec::new();
    SolidCraftWriter::new()
        .target_version(Some(SolidCraftVersion::V0_44_0))
        .verification(Verification::Off)
        .write(&source, &mut bytes, &mut report)
        .unwrap();
    assert!(document(&bytes)["links"].is_object());
    assert!(
        report
            .losses()
            .iter()
            .any(|loss| loss.subject.starts_with("Derived: ") && loss.detail.contains("native")),
        "{:?}",
        report.losses()
    );
}

#[test]
fn derived_file_conversion_publishes_companion_and_cli_replays_the_cut() {
    let dir = tempfile::tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/onshape-derived.onshape-bundle");
    let output = dir.path().join("result.sc");
    let writer = SolidCraftWriter::new()
        .target_version(Some("0.44.1".parse().unwrap()))
        .verification(Verification::Off);
    dutchman_core::convert(&input, &output, &writer).unwrap();
    let doc = document(&std::fs::read(&output).unwrap());
    let link = doc["links"].as_object().unwrap().values().next().unwrap();
    assert!(dir.path().join(link["path"].as_str().unwrap()).is_file());
    let second = dir.path().join("second.sc");
    dutchman_core::convert(&input, &second, &writer).unwrap();
    assert_eq!(
        document(&std::fs::read(&second).unwrap())["links"],
        doc["links"]
    );
    let companion = dir.path().join(link["path"].as_str().unwrap());
    let saved = std::fs::read(&companion).unwrap();
    std::fs::write(&companion, b"a locally edited companion").unwrap();
    let third = dir.path().join("third.sc");
    let error = dutchman_core::convert(&input, &third, &writer).unwrap_err();
    assert!(matches!(error, dutchman_core::Error::Encode(_)));
    assert!(error.to_string().contains("choose another output folder"));
    assert!(!third.exists());
    assert_eq!(
        std::fs::read(&companion).unwrap(),
        b"a locally edited companion"
    );
    std::fs::write(&companion, saved).unwrap();
    if let Ok(artifact) = std::env::var("DUTCHMAN_DERIVED_ARTIFACT") {
        let artifact = Path::new(&artifact);
        std::fs::create_dir_all(artifact).unwrap();
        for file in std::fs::read_dir(dir.path()).unwrap() {
            let file = file.unwrap();
            std::fs::copy(file.path(), artifact.join(file.file_name())).unwrap();
        }
    }
    let Ok(cli) = std::env::var("SOLIDCRAFT_CLI") else {
        return;
    };
    let version = solidcraft_version(&Verification::With(PathBuf::from(&cli)))
        .unwrap()
        .split_whitespace()
        .find_map(|word| word.parse::<SolidCraftVersion>().ok())
        .unwrap();
    if !VersionGate::LinkedDesign.supported(version) {
        return;
    }
    let evaluation = std::process::Command::new(&cli)
        .arg("eval")
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        evaluation.status.success(),
        "{}",
        String::from_utf8_lossy(&evaluation.stderr)
    );
    let measurements: Value = serde_json::from_slice(&evaluation.stdout).unwrap();
    assert_eq!(
        measurements["bodies"].as_array().unwrap().len(),
        1,
        "{measurements:#}"
    );
    assert!(
        (measurements["bodies"][0]["volume_mm3"].as_f64().unwrap() - 300.).abs() < 0.1,
        "{measurements:#}"
    );
    let replacement = dir.path().join("replacement.json");
    std::fs::write(&replacement,json!([{"command":"solid.box","params":{"length":10,"width":2,"height":4,"corner":[5,2,0],"body_name":"Tool"}}]).to_string()).unwrap();
    let replaced = std::process::Command::new(&cli)
        .arg("run")
        .arg(&replacement)
        .arg("--save")
        .arg(&companion)
        .arg("--quiet")
        .output()
        .unwrap();
    assert!(
        replaced.status.success(),
        "{}",
        String::from_utf8_lossy(&replaced.stderr)
    );
    let update = dir.path().join("update.json");
    std::fs::write(
        &update,
        json!([{"command":"component.update_links","params":{}}]).to_string(),
    )
    .unwrap();
    let refreshed = dir.path().join("refreshed.sc");
    let updated = std::process::Command::new(&cli)
        .arg("run")
        .arg(&update)
        .arg("--in")
        .arg(&output)
        .arg("--save")
        .arg(&refreshed)
        .arg("--results")
        .output()
        .unwrap();
    assert!(
        updated.status.success(),
        "{}",
        String::from_utf8_lossy(&updated.stderr)
    );
    let result: Value = serde_json::from_slice(&updated.stdout).unwrap();
    assert_eq!(result[0]["failed"], json!([]));
    assert_eq!(result[0]["updated"].as_array().unwrap().len(), 1);
    let evaluated = std::process::Command::new(&cli)
        .arg("eval")
        .arg(&refreshed)
        .output()
        .unwrap();
    assert!(
        evaluated.status.success(),
        "{}",
        String::from_utf8_lossy(&evaluated.stderr)
    );
    let measured: Value = serde_json::from_slice(&evaluated.stdout).unwrap();
    assert_eq!(measured["body_count"], 1);
    assert!(
        (measured["total"]["volume_mm3"].as_f64().unwrap() - 360.).abs() < 0.1,
        "{measured:#}"
    );
}
