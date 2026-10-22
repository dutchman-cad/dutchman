#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use dutchman_core::{FeatureStatus, Mode, Model, Report, Writer};
use serde_json::{json, Value};

use super::{generate, FreeCadWriter};

// Synthetic CADIR, following dutchman-solidcraft/tests/history.rs. The B-rep is
// cadmpeg's generated cube; the history is authored here to test script text only.
fn model(kind: &str) -> Model {
    let mut wire = serde_json::to_value(cadmpeg_ir::examples::unit_cube().unwrap()).unwrap();
    let arena = &mut wire["model"];
    arena["sketches"] = json!([{
        "id": "synthetic:test:sketch#1",
        "name": "Profile",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}
        }},
        "profiles": [[{"entity": "synthetic:test:sketch-entity#1"}]]
    }]);
    arena["sketch_entities"] = json!([{
        "id": "synthetic:test:sketch-entity#1",
        "sketch": "synthetic:test:sketch#1",
        "construction": false,
        "geometry": {"kind": "circle", "center": {"u": 5.0, "v": 0.0}, "radius": 2.0}
    }]);
    arena["features"] = json!([
        {"id": "synthetic:test:feature#1", "ordinal": 0, "name": "Profile", "suppressed": false,
         "definition": {"definition": "sketch", "sketch": {"space": "planar", "sketch": "synthetic:test:sketch#1"}}},
        {"id": "synthetic:test:feature#2", "ordinal": 1, "name": "Solid", "suppressed": false,
         "definition": {"definition": "extrude", "profile": {"kind": "sketch", "value": "synthetic:test:sketch#1"},
             "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"},
             "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 10.0}}},
             "op": "new_body"}}
    ]);
    if kind == "revolve" {
        arena["features"][1]["definition"] = json!({
            "definition": "revolve", "op": "new_body",
            "construction": {"state": "resolved",
                "profile": {"kind": "sketch", "value": "synthetic:test:sketch#1"},
                "axis": {"origin": {"x": 0.0, "y": 0.0, "z": 0.0}, "direction": {"x": 0.0, "y": 1.0, "z": 0.0}},
                "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": std::f64::consts::TAU}}
            }
        });
    }
    if kind == "pattern" {
        arena["features"].as_array_mut().unwrap().push(json!({
            "id": "synthetic:test:feature#3", "ordinal": 2, "name": "Copies", "suppressed": false,
            "definition": {"definition": "pattern", "seeds": [{"kind": "feature", "value": "synthetic:test:feature#2"}],
                "pattern": {"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3}}
        }));
    }
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "Synthetic".to_owned(),
        source_file: None,
        assembly: None,
    }
}

fn written(model: &Model, writer: FreeCadWriter) -> (String, Report) {
    let mut bytes = Vec::new();
    let mut report = Report::default();
    writer.write(model, &mut bytes, &mut report).unwrap();
    (String::from_utf8(bytes).unwrap(), report)
}

#[test]
fn synthetic_histories_match_macro_goldens() {
    for (kind, golden) in [
        ("pad", include_str!("../tests/golden/pad.FCMacro")),
        ("revolve", include_str!("../tests/golden/revolve.FCMacro")),
        ("pattern", include_str!("../tests/golden/pattern.FCMacro")),
    ] {
        let (script, report) = written(&model(kind), FreeCadWriter::new());
        assert_eq!(report.outcome().unwrap().mode, Mode::History);
        // STEP has encoder metadata unrelated to macro generation. Replace only its
        // single escaped literal; compare every other byte of the executable macro.
        let normalized: String = script
            .lines()
            .map(|line| {
                if line.starts_with("original = original_step(") {
                    "original = original_step(\"Synthetic\", \"STEP payload\")\n".to_owned()
                } else {
                    format!("{line}\n")
                }
            })
            .collect();
        assert_eq!(normalized, golden, "{kind}");
    }
}

#[test]
fn hostile_names_are_data_only() {
    let name = "O'Brien \"quoted\" \\ folder\nnext\rline\t雪 🛠\0\u{2028}";
    let literal = generate::quoted(name).unwrap();
    assert_eq!(serde_json::from_str::<String>(&literal).unwrap(), name);
    assert!(!literal.contains('\n'));
    let mut model = model("pad");
    model.name = name.to_owned();
    let (script, _) = written(&model, FreeCadWriter::new());
    assert!(script.contains(&format!("body.Label = {literal}\n")));
    assert!(!script.contains("exec("));
    assert!(!script.contains("eval("));
    assert!(script.contains("os.unlink(path)"));
}

#[test]
fn import_only_and_unmapped_features_keep_step_and_report_each_feature() {
    let model = model("pad");
    let (script, report) = written(&model, FreeCadWriter::new().import_only(true));
    assert_eq!(report.outcome().unwrap().mode, Mode::Import);
    assert!(report
        .outcome()
        .unwrap()
        .features
        .iter()
        .all(|f| f.status == FeatureStatus::Blocked && f.reason.is_some()));
    assert!(script.contains("ISO-10303-21"));
    assert!(!script.contains("tip = rebuild(json.loads("));

    let mut model = model;
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"][1]["definition"] =
        json!({"definition": "native", "kind": "Synthetic unsupported feature"});
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (_, report) = written(&model, FreeCadWriter::new());
    assert_eq!(report.outcome().unwrap().features.len(), 2);
    assert!(report
        .outcome()
        .unwrap()
        .features
        .iter()
        .all(|f| f.reason.is_some()));
}

#[test]
fn fixed_float_data_round_trips_without_turning_names_into_code() {
    let value = json!({"small": 1e-30, "large": 1e30, "name": "\"; raise RuntimeError('name') #"});
    let code = generate::data(&value).unwrap();
    let outer = code
        .strip_prefix("json.loads(")
        .unwrap()
        .strip_suffix(')')
        .unwrap();
    let inner: String = serde_json::from_str(outer).unwrap();
    let round_trip: Value = serde_json::from_str(&inner).unwrap();
    assert_eq!(round_trip, value);
    assert!(inner.contains("e-30"));
}

#[test]
fn huge_pattern_counts_are_reported_before_macro_execution() {
    let rows = [
        json!({"type": "pattern", "name": "Too many", "pattern": {"type": "rectangular", "count1": "10001"}}),
    ];
    assert!(generate::history(&rows)
        .unwrap_err()
        .contains("count exceeds"));
}

#[test]
fn sketch_constraints_keep_their_evaluated_dimensions() {
    let mut model = model("pad");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["parameters"] = json!([{
        "id": "synthetic:test:parameter#1", "name": "Radius", "expression": "2 mm",
        "owner": "synthetic:test:feature#1", "value": {"kind": "length", "value": 2.0}
    }]);
    wire["model"]["sketch_constraints"] = json!([{
        "id": "synthetic:test:sketch-constraint#1", "sketch": "synthetic:test:sketch#1",
        "definition": {"kind": "radius", "entity": "synthetic:test:sketch-entity#1", "parameter": "synthetic:test:parameter#1"}
    }]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let mapped = dutchman_solidcraft::mapped_history(&model, &mut Report::default());
    let rows = mapped.features.unwrap();
    assert_eq!(rows[0]["sketch"]["constraints"][0]["type"], "radius");
    assert_eq!(rows[0]["sketch"]["constraints"][0]["value"], 2.0);
    let (script, report) = written(&model, FreeCadWriter::new());
    assert_eq!(report.outcome().unwrap().mode, Mode::History);
    assert!(script.contains("parameter_table(json.loads("));
}

#[test]
fn assembly_occurrences_keep_nested_placements_and_independent_bodies() {
    let part = model("pad");
    let placement = dutchman_core::Placement {
        name: Some("Placed part".to_owned()),
        part: 0,
        transform: [
            [1.0, 0.0, 0.0, 12.0],
            [0.0, 1.0, 0.0, -3.0],
            [0.0, 0.0, 1.0, 0.0],
        ],
    };
    let assembly = Model {
        assembly: Some(dutchman_core::Assembly {
            parts: vec![part.clone()],
            placements: vec![placement.clone(), placement],
            joints: Vec::new(),
            left_out: Vec::new(),
        }),
        ..part
    };
    let (script, report) = written(&assembly, FreeCadWriter::new());
    assert_eq!(script.matches("parents.append(container(").count(), 2);
    assert_eq!(script.matches("body.Label = \"Synthetic\"").count(), 2);
    assert_eq!(script.matches("parents.pop()").count(), 2);
    assert_eq!(report.outcome().unwrap().features.len(), 4);
}
