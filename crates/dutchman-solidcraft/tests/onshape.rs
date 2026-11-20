//! Onshape uses the same history writer and source-geometry verifier as native files.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use dutchman_core::{Check, FeatureStatus, Format, Model, Report, Writer};
use dutchman_solidcraft::{SolidCraftEvaluator, SolidCraftVersion, SolidCraftWriter, Verification};
use serde_json::{json, Value};

fn model(name: &str) -> (Model, Report) {
    if name == "sketch-axis-revolve" {
        return synthetic_bundle_named("revolve", |response| {
            response["features"][0]["entities"]
                .as_array_mut()
                .unwrap()
                .push(
                    json!({"btType":"BTMSketchCurveSegment-155", "entityId":"sketch-axis",
                    "isConstruction":true, "startParam":0.0, "endParam":0.01,
                    "geometry":{"btType":"BTCurveGeometryLine-117", "dirY":1.0}}),
                );
            let axis = response["features"][1]["parameters"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|p| p["parameterId"] == "axis")
                .unwrap();
            axis["queries"] = json!([{"queryString": concat!(
                "query=qCompressed(1.0,\"%M5Sa$entityTypeS4$EDGESb$historyTypeS8$CREATION",
                "Sb$operationIdA1S12$Ring sketch.wireOpS9$queryTypeSd$SKETCH_ENTITY",
                "Se$sketchEntityIdSb$sketch-axis\",id);"
            )}]);
        });
    }
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
        ("revolve", vec!["sketch", "native"]),
        ("sketch-axis-revolve", vec!["sketch", "revolve"]),
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
        if matches!(name, "unsupported" | "revolve") {
            assert_eq!(
                outcome.features.last().unwrap().status,
                FeatureStatus::Blocked
            );
            assert!(report.losses().iter().any(|loss| loss.code.as_deref()
                == Some("onshape/history.not-supported")
                && loss.detail.contains(if name == "unsupported" {
                    "custom FeatureScript"
                } else {
                    "producer"
                })));
            assert_ne!(outcome.check, Check::Verified);
            assert!(doc["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["type"] == "import"));
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
fn custom_feature_without_a_proven_shape_blocks_later_topology_features() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/onshape-plate-fillet.onshape-bundle");
    let mut input = zip::ZipArchive::new(Cursor::new(std::fs::read(path).unwrap())).unwrap();
    let mut bundle = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..input.len() {
        let mut member = input.by_index(i).unwrap();
        let mut name = member.name().unwrap().into_owned();
        let mut data = Vec::new();
        member.read_to_end(&mut data).unwrap();
        if name.ends_with("features.json") {
            let mut response: Value = serde_json::from_slice(&data).unwrap();
            response["features"].as_array_mut().unwrap().insert(
                2,
                json!({"featureId":"custom", "name":"Custom shape",
                    "namespace":"synthetic", "featureType":"fixtureCustom", "parameters":[]}),
            );
            data = serde_json::to_vec(&response).unwrap();
        } else if name.ends_with("bodydetails/index.json") {
            data = serde_json::to_vec(&json!([3])).unwrap();
        } else if name.ends_with("bodydetails/2.json") {
            // The later fillet still has its own rollback selection geometry.
            name = name.replace("bodydetails/2.json", "bodydetails/3.json");
        }
        bundle
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        bundle.write_all(&data).unwrap();
    }
    let bytes = bundle.finish().unwrap().into_inner();
    let mut report = Report::default();
    let source = dutchman_core::read_bytes(
        Format::OnshapeBundle,
        Path::new("synthetic.onshape-bundle"),
        &bytes,
        &mut report,
    )
    .unwrap();
    // The custom feature has no proven shape, so the fillet's stored edge has no
    // proven producer and the reader keeps no input topology for it.
    assert!(source.ir.model.feature_input_topologies.is_empty());
    let mut output = Vec::new();
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .write(&source, &mut output, &mut report)
        .unwrap();
    let outcome = report.outcome().unwrap();
    assert_eq!(outcome.features[2].status, FeatureStatus::Blocked);
    assert_eq!(outcome.features[2].name.as_deref(), Some("Custom shape"));
    // The format-neutral writer gives native rows a generic reason; the reader's
    // named diagnostic must survive in the shared loss report.
    let loss = report
        .losses()
        .iter()
        .find(|loss| {
            loss.subject == "Custom shape"
                && loss.code.as_deref() == Some("onshape/history.not-supported")
        })
        .unwrap();
    assert!(loss
        .detail
        .contains("custom FeatureScript feature Custom shape"));
    assert!(loss
        .detail
        .contains("lacks complete per-feature body geometry"));
    assert_eq!(loss.severity, dutchman_core::Severity::Degraded);
    // With no proven input edge the reader keeps the fillet as a native step.
    assert_eq!(outcome.features[3].kind, "native");
    assert_eq!(outcome.features[3].status, FeatureStatus::Blocked);
    assert!(outcome.features[3]
        .reason
        .as_deref()
        .unwrap()
        .contains("after the first blocking feature"));
    assert_ne!(outcome.check, Check::Verified);
    let doc = document(&output);
    assert!(doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "import"));
    assert!(!doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "fillet"));
    let mut written = zip::ZipArchive::new(Cursor::new(output)).unwrap();
    let mut found_step = false;
    for i in 0..written.len() {
        let mut member = written.by_index(i).unwrap();
        if member.name().unwrap().ends_with(".step") {
            let mut step = String::new();
            member.read_to_string(&mut step).unwrap();
            assert_eq!(Some(step), source.source_step);
            found_step = true;
        }
    }
    assert!(found_step);
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
    let host = Arc::new(Measurements {
        step: source.source_step.clone().unwrap(),
        calls: Mutex::default(),
        mismatch: "",
    });
    let mut output = Vec::new();
    SolidCraftWriter::new()
        .evaluator(host)
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
    for name in [
        "plate-fillet",
        "revolve",
        "sketch-axis-revolve",
        "hole",
        "hole-forms",
        "variables",
    ] {
        let (model, mut report) = if name == "variables" {
            variable_model()
        } else {
            model(name)
        };
        let writer = SolidCraftWriter::new().verification(Verification::With(cli.clone()));
        let mut output = Vec::new();
        writer.write(&model, &mut output, &mut report).unwrap();
        if name == "revolve" {
            // This older fixture stores an axis edge but no body feature producing it.
            // Its STEP export remains usable; that edge cannot establish editable history.
            assert_ne!(report.outcome().unwrap().check, Check::Verified);
            assert!(report.losses().iter().any(|loss| loss.code.as_deref()
                == Some("onshape/history.not-supported")
                && loss.detail.contains("producer")));
            assert!(document(&output)["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["type"] == "import"));
            continue;
        }
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

/// Rebuild an owner-authored synthetic fixture in memory, changing REST responses only.
fn synthetic_bundle(edit: impl FnMut(&mut Value)) -> (Model, Report) {
    synthetic_bundle_named("plate-fillet", edit)
}

fn synthetic_bundle_named(name: &str, mut edit: impl FnMut(&mut Value)) -> (Model, Report) {
    use std::io::Write;
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../dutchman-core/tests/fixtures/onshape-{name}.onshape-bundle"
    ));
    let mut archive = zip::ZipArchive::new(Cursor::new(std::fs::read(path).unwrap())).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let name = member.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        if name.ends_with("features.json") {
            let mut value = serde_json::from_slice(&bytes).unwrap();
            edit(&mut value);
            bytes = serde_json::to_vec(&value).unwrap();
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
        Path::new("synthetic.onshape-bundle"),
        &bytes,
        &mut report,
    )
    .unwrap();
    (model, report)
}

fn write_synthetic(model: &Model, report: &mut Report) -> Value {
    write_synthetic_target(model, report, SolidCraftVersion::default())
}

fn write_synthetic_latest(model: &Model, report: &mut Report) -> Value {
    write_synthetic_target(model, report, SolidCraftVersion::V0_44_0)
}

fn write_synthetic_target(model: &Model, report: &mut Report, target: SolidCraftVersion) -> Value {
    let host = Arc::new(Measurements {
        step: model.source_step.clone().unwrap(),
        calls: Mutex::default(),
        mismatch: "",
    });
    let mut output = Vec::new();
    SolidCraftWriter::new()
        .target_version(Some(target))
        .evaluator(host)
        .write(model, &mut output, report)
        .unwrap();
    document(&output)
}

#[test]
fn onshape_ellipses_and_arcs_reach_the_shared_sketch_writer() {
    for span in [1.0, 4.0] {
        let (model, mut report) = synthetic_bundle(|value| {
            value["features"].as_array_mut().unwrap().truncate(2);
            let curves = value["features"][0]["entities"].as_array_mut().unwrap();
            let ellipse = json!({"btType":"BTMSketchCurve-4","entityId":"oval","isConstruction":true,
                "geometry":{"btType":"BTCurveGeometryEllipse-1189","xCenter":0.03,"yCenter":0.04,
                    "xDir":0.0,"yDir":1.0,"radius":0.006,"minorRadius":0.002}});
            curves.push(ellipse.clone());
            let mut arc = ellipse;
            arc["entityId"] = json!("oval-arc");
            arc["btType"] = json!("BTMSketchCurveSegment-155");
            arc["startParam"] = json!(0.0);
            arc["endParam"] = json!(span);
            curves.push(arc);
        });
        let doc = write_synthetic(&model, &mut report);
        let sketch = doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "sketch")
            .unwrap();
        let curves = sketch["sketch"]["curves"].as_array().unwrap();
        assert!(curves.iter().any(|c| c["type"] == "ellipse"), "{sketch}");
        assert_eq!(
            curves.iter().filter(|c| c["type"] == "conic").count(),
            if span < std::f64::consts::PI { 1 } else { 4 }
        );
        assert!(
            report
                .outcome()
                .unwrap()
                .features
                .iter()
                .all(|f| matches!(f.status, FeatureStatus::Mapped | FeatureStatus::NotNeeded)),
            "{:?}",
            report.losses()
        );
    }
}

fn inch_variables(value: &mut Value) {
    let rows = value["features"].as_array_mut().unwrap();
    rows.truncate(2);
    rows[0]["constraints"][1]["parameters"][1]["expression"] = json!("#stock * (20 / 25.4)");
    rows[1]["parameters"][4]["expression"] = json!("#reach / 12.7");
    rows[1]["parameters"]
        .as_array_mut()
        .unwrap()
        .push(json!({"parameterId":"symmetric","value":true}));
    for (id, name, expression) in [
        ("stock-variable", "stock", "1 inch"),
        ("reach-variable", "reach", "#stock * 1"),
    ] {
        let variable = json!({"featureId":id,"featureType":"assignVariable","parameters":[
            {"parameterId":"mode","value":"ASSIGNED"},{"parameterId":"variableType","value":"LENGTH"},
            {"parameterId":"name","value":name},{"parameterId":"lengthValue","expression":expression}]});
        let at = if name == "stock" { 0 } else { 1 };
        rows.insert(at, variable);
    }
}

#[test]
fn inch_variables_keep_expressions_units_and_drive_dimensions_and_symmetric_depths() {
    let (model, mut report) = synthetic_bundle(inch_variables);
    let doc = write_synthetic(&model, &mut report);
    let params = doc["params"].as_array().unwrap();
    let stock = params.iter().find(|p| p["name"] == "stock").unwrap();
    assert_eq!(stock["expr"], "1 in");
    assert_eq!(stock["unit"], "in");
    assert_eq!(stock["model"], false);
    let reach = params.iter().find(|p| p["name"] == "reach").unwrap();
    assert_eq!(reach["expr"], "(stock * 1)");
    assert_eq!(reach["unit"], "in");
    let features = doc["features"].as_array().unwrap();
    let sketch = features.iter().find(|f| f["type"] == "sketch").unwrap();
    let dimension = sketch["sketch"]["constraints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["type"] == "length")
        .unwrap();
    let name = dimension["param"].as_str().unwrap();
    assert_eq!(
        params.iter().find(|p| p["name"] == name).unwrap()["expr"],
        "(stock * (20 / 25.4))"
    );
    let extrude = features.iter().find(|f| f["type"] == "extrude").unwrap();
    let depth = extrude["extent"]["distance"].as_str().unwrap();
    assert_eq!(
        params.iter().find(|p| p["name"] == depth).unwrap()["expr"],
        "(reach / 12.7)"
    );
    assert_eq!(extrude["extent"]["direction"], "symmetric");
    assert_eq!(extrude["extent"]["convention"], "total_distance");
}

#[test]
fn design_parameters_survive_a_fallback_with_no_mapped_sketch() {
    let (model, mut report) = synthetic_bundle(|value| {
        inch_variables(value);
        value["features"][2]["entities"][0]["geometry"]["btType"] =
            json!("UnsupportedSyntheticCurve");
    });
    let doc = write_synthetic(&model, &mut report);
    assert_eq!(doc["params"].as_array().unwrap().len(), 2);
    assert!(doc["params"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["name"] == "reach" && p["expr"] == "(stock * 1)"));
    assert!(
        !report
            .losses()
            .iter()
            .any(|l| l.subject == "parameters" && l.detail.contains("non-parametric body")),
        "{:?}",
        report.losses()
    );
}

#[test]
fn unequal_chamfers_keep_a_named_loss_when_the_shared_writer_cannot_prove_them() {
    for form in ["TWO_OFFSETS", "DISTANCE_ANGLE"] {
        let (model, mut report) = synthetic_bundle(|value| {
            let feature = &mut value["features"][2];
            feature["featureType"] = json!("chamfer");
            feature["parameters"].as_array_mut().unwrap().extend([
                json!({"parameterId":"chamferType","value":form}),
                json!({"parameterId":"width","expression":"0.4 mm"}),
                json!({"parameterId":"width1","expression":"0.4 mm"}),
                json!({"parameterId":"width2","expression":"0.8 mm"}),
                json!({"parameterId":"angle","expression":"30 deg"}),
            ]);
        });
        let doc = write_synthetic(&model, &mut report);
        assert!(doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["type"] == "import"));
        assert!(
            report
                .outcome()
                .unwrap()
                .features
                .iter()
                .any(|f| f.kind == "chamfer"
                    && f.status == FeatureStatus::Blocked
                    && f.reason.as_deref().is_some_and(|r| !r.is_empty())),
            "{:?}",
            report.outcome()
        );
    }
}

#[test]
fn selected_line_regions_keep_their_extrude_beside_an_unused_ellipse() {
    let (model, mut report) = synthetic_bundle_named("default-plane-regions", |value| {
        value["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!({
            "btType":"BTMSketchCurve-4","entityId":"separate-oval","isConstruction":false,
            "geometry":{"btType":"BTCurveGeometryEllipse-1189","xCenter":0.04,"yCenter":0.04,
                "xDir":1.0,"yDir":0.0,"radius":0.003,"minorRadius":0.001}}));
    });
    let doc = write_synthetic(&model, &mut report);
    let sketch = doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "sketch")
        .unwrap();
    assert!(sketch["sketch"]["curves"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["type"] == "ellipse"));
    assert!(
        report
            .outcome()
            .unwrap()
            .features
            .iter()
            .all(|f| f.status == FeatureStatus::Mapped),
        "{:?}",
        report.outcome()
    );
}

#[test]
fn new_synthetic_sketches_and_inch_parameters_replay_against_the_source() {
    let Some(cli) = std::env::var_os(dutchman_solidcraft::CLI_ENV).map(PathBuf::from) else {
        eprintln!("Set SOLIDCRAFT_CLI to replay elliptical sketches and inch parameters");
        return;
    };
    for parameters in [false, true] {
        let (model, mut report) = synthetic_bundle_named("default-plane-regions", |value| {
            if parameters {
                value["features"][0]["constraints"] = json!([
                    {"entityId":"horizontal","constraintType":"HORIZONTAL","parameters":[
                        {"parameterId":"localFirst","value":"outline.side0"}]},
                    {"entityId":"length","constraintType":"LENGTH","parameters":[
                        {"parameterId":"localFirst","value":"outline.side0"},
                        {"parameterId":"length","expression":"20 mm"}]}
                ]);
                inch_variables(value);
                value["features"][3]["parameters"][4]["expression"] = json!("#reach / 5.08");
                // Keep this fixture's original 5 mm, one-sided extrusion.
                value["features"][3]["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|p| p["parameterId"] != "symmetric");
            } else {
                let curves = value["features"][0]["entities"].as_array_mut().unwrap();
                for span in [None, Some(1.0), Some(4.0)] {
                    let mut curve = json!({"btType":"BTMSketchCurve-4","entityId":format!("oval-{span:?}"),"isConstruction":true,
                        "geometry":{"btType":"BTCurveGeometryEllipse-1189","xCenter":0.03,"yCenter":0.04,
                            "xDir":0.0,"yDir":1.0,"radius":0.006,"minorRadius":0.002}});
                    if let Some(span) = span {
                        curve["btType"] = json!("BTMSketchCurveSegment-155");
                        curve["startParam"] = json!(0.0);
                        curve["endParam"] = json!(span);
                    }
                    curves.push(curve);
                }
            }
        });
        SolidCraftWriter::new()
            .verification(Verification::With(cli.clone()))
            .write(&model, &mut Vec::new(), &mut report)
            .unwrap();
        assert_eq!(
            report.outcome().unwrap().check,
            Check::Verified,
            "{:?}",
            report.losses()
        );
    }
}

// Independently authored neutral thin construction for existing writer checks.
// The Onshape reader refuses these records until source scope and sides resolve.
fn neutral_thin(mut model: Model, midplane: bool, flip: bool, second: f64) -> Model {
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let sketch = wire["model"]["sketches"][0]["id"].clone();
    let entities: Vec<Value> = wire["model"]["sketch_entities"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| {
            e["sketch"] == sketch && e["construction"] != true && e["geometry"]["kind"] != "point"
        })
        .map(|e| e["id"].clone())
        .collect();
    let feature = &mut wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|f| f["definition"]["value"]["kind"] == "extrude")
        .unwrap();
    feature["definition"]["value"]["value"]["profile"]["value"] =
        json!({"kind":"sketch_entities","value":{"sketch":sketch,"entities":entities}});
    let extrude = feature["definition"]["value"]["value"].clone();
    let literal = |v: f64| json!({"kind":"literal","value":v});
    let thickness = if midplane {
        json!({"kind":"symmetric","value":literal(2.0)})
    } else if second == 0.0 {
        json!({"kind":"one_side","value":{"value":literal(1.0),"side":{"kind":if flip {"inside"} else {"outside"}}}})
    } else {
        json!({"kind":"offsets","value":{"outside":literal(if flip {second} else {1.0}),"inside":literal(if flip {1.0} else {second})}})
    };
    let picks: Vec<Value> = entities
        .iter()
        .map(|e| json!({"target":{"kind":"sketch_entity","value":e}}))
        .collect();
    wire["design"]["features"] = json!([{"id":feature["id"],"definition":{"operation":{"kind":"extrude","value":{
        "profile":{"at":{"kind":"input"},"picks":{"kind":"named","value":picks}},
        "direction":{"kind":"profile_normal","reversed":{"kind":"literal","value":extrude["direction"]["kind"]=="reversed_profile_normal"}},
        "origin":{"kind":"profile_plane"},"span":{"kind":"between","start":{"kind":"at_origin"},"end":{"kind":"amount","value":literal(extrude["extent"]["side"]["termination"]["length"].as_f64().unwrap())}},
        "output":{"body":{"kind":"thin","thickness":thickness},"boolean":{"kind":"new_body"}},"native_options":{},"source_settings":{}}}}}]);
    model.ir = serde_json::from_value(wire).unwrap();
    model
}
fn thin_bundle(midplane: bool, flip: bool, second: f64) -> (Model, Report) {
    let (model, report) = synthetic_bundle(|value| {
        value["features"].as_array_mut().unwrap().truncate(2);
    });
    (neutral_thin(model, midplane, flip, second), report)
}

#[test]
fn typed_thin_outputs_write_exact_curves_sides_and_thicknesses() {
    for (midplane, flip, second, side, thickness, thickness2) in [
        (false, false, 0.0, Some("outside"), "1 mm", None),
        (false, true, 0.0, Some("inside"), "1 mm", None),
        (true, false, 0.0, None, "2 mm", None),
        (false, false, 2.0, Some("two_sides"), "2 mm", Some("1 mm")),
    ] {
        let (model, mut report) = thin_bundle(midplane, flip, second);
        let doc = write_synthetic_latest(&model, &mut report);
        let extrude = doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "extrude")
            .unwrap();
        assert_eq!(extrude["thin"]["side"].as_str(), side);
        assert_eq!(extrude["thin"]["thickness"], thickness);
        assert_eq!(extrude["thin"]["thickness2"].as_str(), thickness2);
        assert_eq!(extrude["thin"]["curves"].as_array().unwrap().len(), 4);
        assert!(
            report
                .outcome()
                .unwrap()
                .features
                .iter()
                .all(|f| f.status == FeatureStatus::Mapped),
            "{:?}",
            report.outcome()
        );
    }
}

#[test]
fn thin_outputs_refuse_conflicting_projections_and_preserve_older_targets() {
    let mut report = Report::default();
    // A mismatched profile in an otherwise well-formed typed record is not
    // allowed to supply output semantics to a different model projection.
    let (model, _) = thin_bundle(false, false, 0.0);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["design"]["features"][0]["definition"]["operation"]["value"]["profile"]["picks"] =
        json!({"kind":"named","value":[]});
    let model = Model {
        ir: serde_json::from_value(wire).unwrap(),
        ..model
    };
    let doc = write_synthetic_latest(&model, &mut report);
    assert!(!doc["features"]
        .as_array()
        .unwrap()
        .iter()
        .any(|f| f["type"] == "extrude"));
    assert!(report.outcome().unwrap().features.iter().any(|f| f
        .reason
        .as_deref()
        .is_some_and(|r| r.contains("projection disagree"))));
    let (model, mut report) = thin_bundle(false, false, 0.0);
    let mut output = Vec::new();
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .target_version(Some("0.18.2".parse().unwrap()))
        .write(&model, &mut output, &mut report)
        .unwrap();
    assert!(report.outcome().unwrap().features.iter().any(|f| f
        .reason
        .as_deref()
        .is_some_and(|r| r.contains("thin extrudes need SolidCraft"))));
}

#[test]
fn thin_circle_walls_replay_with_the_stated_side_and_volume() {
    let Some(cli) = std::env::var_os(dutchman_solidcraft::CLI_ENV).map(PathBuf::from) else {
        eprintln!("Set SOLIDCRAFT_CLI to replay thin circle walls");
        return;
    };
    let evaluator = dutchman_solidcraft::CliEvaluator::new(cli);
    for (midplane, flip, second, area) in [
        (false, false, 0.0, 11.0),
        (false, true, 0.0, 9.0),
        (true, false, 0.0, 20.0),
        (false, false, 2.0, 27.0),
    ] {
        let (model, mut report) = synthetic_bundle(|value| {
            value["features"].as_array_mut().unwrap().truncate(2);
            value["features"][0]["constraints"] = json!([]);
            value["features"][0]["entities"] = json!([{"btType":"BTMSketchCurve-4","entityId":"ring",
                "geometry":{"btType":"BTCurveGeometryCircle-115","radius":0.005,"xDir":1.0}}]);
        });
        let model = neutral_thin(model, midplane, flip, second);
        let host = Arc::new(Measurements {
            step: model.source_step.clone().unwrap(),
            calls: Mutex::default(),
            mismatch: "",
        });
        let mut output = Vec::new();
        SolidCraftWriter::new()
            .target_version(Some(SolidCraftVersion::V0_44_0))
            .evaluator(host)
            .write(&model, &mut output, &mut report)
            .unwrap();
        // The transport double above exercises archive writing; the kernel's
        // independent measurement must match the analytic annulus, not that double.
        let measured = evaluator
            .evaluate(&output, Duration::from_secs(120), "thin-circle")
            .unwrap();
        let bodies = measured["bodies"].as_array().unwrap();
        assert_eq!(bodies.len(), 1, "{measured}");
        let volume = bodies[0]["volume_mm3"].as_f64().unwrap();
        assert!(
            (volume - area * std::f64::consts::PI * 2.0).abs() < 1e-7,
            "{measured}"
        );
    }
}

#[path = "../../dutchman-core/tests/fixtures/onshape_regions.rs"]
mod region_fixture;

#[test]
fn an_open_divider_is_refused_before_writing_even_without_verification() {
    for divider in ["line", "arc", "coincident"] {
        for verification_off in [false, true] {
            let (model, mut report) =
                synthetic_bundle_named("default-plane-regions", |value| match divider {
                    "arc" => region_fixture::arc_divider(value),
                    "coincident" => region_fixture::coincident_dividers(value),
                    _ => region_fixture::frame(value, true),
                });
            assert!(report
                .losses()
                .iter()
                .any(|l| l.detail.contains("open divider")));
            let mut output = Vec::new();
            let mut writer =
                SolidCraftWriter::new().target_version(Some(SolidCraftVersion::V0_44_0));
            if verification_off {
                writer = writer.verification(Verification::Off);
            } else {
                writer = writer.evaluator(Arc::new(Measurements {
                    step: model.source_step.clone().unwrap(),
                    calls: Mutex::default(),
                    mismatch: "",
                }));
            }
            writer.write(&model, &mut output, &mut report).unwrap();
            let doc = document(&output);
            assert!(doc["features"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["type"] != "extrude"));
            assert!(doc["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["type"] == "import"));
            assert_eq!(
                report.outcome().unwrap().features[1].status,
                FeatureStatus::Blocked
            );
        }
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
fn unresolved_onshape_thin_walls_keep_final_geometry_with_verification_off() {
    for midplane in [false, true] {
        for flip in [false, true] {
            let (model, mut report) = synthetic_bundle(|value| {
                value["features"].as_array_mut().unwrap().truncate(2);
                let params = value["features"][1]["parameters"].as_array_mut().unwrap();
                params
                    .iter_mut()
                    .find(|p| p["parameterId"] == "bodyType")
                    .unwrap()["value"] = json!("THIN");
                params.extend([
                    json!({"parameterId":"midplane","value":midplane}),
                    json!({"parameterId":"flipWall","value":flip}),
                ]);
            });
            assert!(model.ir.design.features.is_empty());
            assert!(report
                .losses()
                .iter()
                .any(|l| l.code.as_deref() == Some("onshape/extrude.thin-not-supported")));
            let doc = write_synthetic_latest(&model, &mut report);
            assert!(!doc["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["type"] == "extrude"));
            assert!(doc["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["type"] == "import"));
        }
    }
}

#[test]
fn stored_thin_controls_replay_the_directed_source_circle() {
    let Some(cli) = std::env::var_os(dutchman_solidcraft::CLI_ENV).map(PathBuf::from) else {
        eprintln!("Set SOLIDCRAFT_CLI to replay stored thin controls");
        return;
    };
    let evaluator = dutchman_solidcraft::CliEvaluator::new(cli);
    for clockwise in [false, true] {
        for flip in [false, true] {
            let (model, mut report) = synthetic_bundle(|response| {
                response["features"].as_array_mut().unwrap().truncate(2);
                response["features"][0]["constraints"] = json!([]);
                response["features"][0]["entities"] = json!([{
                    "btType":"BTMSketchCurve-4","entityId":"ring",
                    "geometry":{"btType":"BTCurveGeometryCircle-115","radius":0.005,
                    "xDir":1.0,"clockwise":clockwise}}]);
                let native = response["features"][0]["featureId"].as_str().unwrap();
                let string = |s: &str| format!("S{:x}${s}", s.len());
                let fields = [
                    ("entityType", string("EDGE")),
                    ("historyType", string("CREATION")),
                    ("queryType", string("SKETCH_ENTITY")),
                    (
                        "operationId",
                        format!("A1{}", string(&format!("{native}.wireOp"))),
                    ),
                    ("sketchEntityId", string("ring")),
                ];
                let literal = format!(
                    "%M5{}",
                    fields
                        .into_iter()
                        .map(|(key, value)| string(key) + &value)
                        .collect::<String>()
                );
                let query = format!(
                    "query=qCompressed(1.0,{},id);",
                    serde_json::to_string(&literal).unwrap()
                );
                let feature = &mut response["features"][1];
                let parameters = feature["parameters"].as_array_mut().unwrap();
                parameters
                    .iter_mut()
                    .find(|p| p["parameterId"] == "bodyType")
                    .unwrap()["value"] = json!("THIN");
                parameters
                    .iter_mut()
                    .find(|p| p["parameterId"] == "entities")
                    .unwrap()["parameterId"] = json!("wallShape");
                parameters
                    .iter_mut()
                    .find(|p| p["parameterId"] == "wallShape")
                    .unwrap()["queries"] = json!([{"queryString":query}]);
                parameters.extend([
                    json!({"parameterId":"domain","value":"MODEL"}),
                    json!({"parameterId":"midplane","value":false}),
                    json!({"parameterId":"flipWall","value":flip}),
                    json!({"parameterId":"oppositeDirection","value":false}),
                    json!({"parameterId":"thickness1","expression":"1 mm"}),
                    json!({"parameterId":"thickness2","expression":"0 mm"}),
                ]);
            });
            // The design scope retains the sketch's native timeline row beside the
            // typed thin extrusion; the model history supplies its decoded sketch.
            assert_eq!(model.ir.design.features.len(), 2, "{:?}", report.losses());
            let design = serde_json::to_value(&model.ir.design.features).unwrap();
            assert_eq!(design[0]["definition"]["operation"]["kind"], "native");
            assert_eq!(design[1]["definition"]["operation"]["kind"], "extrude");
            assert_eq!(
                model
                    .ir
                    .design
                    .features
                    .iter()
                    .map(|feature| &feature.id)
                    .collect::<Vec<_>>(),
                model
                    .ir
                    .model
                    .features
                    .iter()
                    .map(|feature| &feature.id)
                    .collect::<Vec<_>>()
            );
            let host = Arc::new(Measurements {
                step: model.source_step.clone().unwrap(),
                calls: Mutex::default(),
                mismatch: "",
            });
            let mut output = Vec::new();
            SolidCraftWriter::new()
                .target_version(Some(SolidCraftVersion::V0_44_0))
                .evaluator(host)
                .write(&model, &mut output, &mut report)
                .unwrap();
            let measured = evaluator
                .evaluate(&output, Duration::from_secs(120), "stored-thin-circle")
                .unwrap();
            let bodies = measured["bodies"].as_array().unwrap();
            assert_eq!(bodies.len(), 1, "{measured}");
            let volume = bodies[0]["volume_mm3"].as_f64().unwrap();
            // A 5 mm source radius, 1 mm stored directed offset and 2 mm depth.
            let annulus = if flip ^ clockwise {
                25.0 - 16.0
            } else {
                36.0 - 25.0
            };
            assert!(
                (volume - annulus * std::f64::consts::PI * 2.0).abs() < 1e-7,
                "clockwise={clockwise}, flip={flip}: {measured}"
            );
        }
    }
}
