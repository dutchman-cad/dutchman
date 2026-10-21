//! Feature history written end to end, on cadmpeg's example 10 mm cube with a synthetic
//! sketch-and-extrude history added as CADIR rows.
//!
//! The tests that replay the history in SolidCraft run when `SOLIDCRAFT_CLI` names a
//! SolidCraft CLI (CI's round-trip job sets it) and are skipped otherwise. Set
//! `UPDATE_GOLDEN=1` to rewrite the golden file after an intended change.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use dutchman_core::{Model, Report, Writer};
use dutchman_solidcraft::{SolidCraftWriter, Verification, CLI_ENV};
use serde_json::{json, Value};
use zip::ZipArchive;

const SKETCH: &str = "f3d:model:sketch#1";

fn entity(key: u32, geometry: Value) -> Value {
    json!({
        "id": format!("f3d:model:sketch-entity#{key}"),
        "sketch": SKETCH,
        "construction": false,
        "geometry": geometry,
    })
}

fn line(key: u32, a: [f64; 2], b: [f64; 2]) -> Value {
    entity(
        key,
        json!({"kind": "line", "start": {"u": a[0], "v": a[1]}, "end": {"u": b[0], "v": b[1]}}),
    )
}

/// The example cube with a history: a 10 mm square on XY, extruded by `length` with
/// `extent` (`None`: one-sided blind) from `profile`.
fn cube_with_history(length: f64, profile: Value, extent: Option<Value>) -> Model {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut wire = serde_json::to_value(&ir).unwrap();
    let model = &mut wire["model"];
    model["bodies"][0]["color"] = json!({"r": 0.8, "g": 0.2, "b": 0.1, "a": 1.0});
    model["sketches"] = json!([{
        "id": SKETCH,
        "name": "Base",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [[
            {"entity": "f3d:model:sketch-entity#1"},
            {"entity": "f3d:model:sketch-entity#2"},
            {"entity": "f3d:model:sketch-entity#3"},
            {"entity": "f3d:model:sketch-entity#4"},
        ]],
    }]);
    model["sketch_entities"] = json!([
        line(1, [0.0, 0.0], [10.0, 0.0]),
        line(2, [10.0, 0.0], [10.0, 10.0]),
        line(3, [10.0, 10.0], [0.0, 10.0]),
        line(4, [0.0, 10.0], [0.0, 0.0]),
    ]);
    let constraint = |key: u32, definition: Value| {
        json!({"id": format!("f3d:model:sketch-constraint#{key}"), "sketch": SKETCH,
               "definition": definition})
    };
    model["sketch_constraints"] = json!([
        constraint(
            1,
            json!({"kind": "horizontal", "entity": "f3d:model:sketch-entity#1"})
        ),
        constraint(
            2,
            json!({"kind": "vertical", "entity": "f3d:model:sketch-entity#2"})
        ),
        constraint(
            3,
            json!({"kind": "perpendicular", "first": "f3d:model:sketch-entity#1",
                              "second": "f3d:model:sketch-entity#4"})
        ),
        constraint(
            4,
            json!({"kind": "equal", "first": "f3d:model:sketch-entity#1",
                              "second": "f3d:model:sketch-entity#2"})
        ),
        constraint(
            5,
            json!({"kind": "distance", "entities": ["f3d:model:sketch-entity#1"],
                              "parameter": "f3d:model:parameter#2"})
        ),
        constraint(
            7,
            json!({"kind": "horizontal", "entity": "f3d:model:sketch-entity#3"})
        ),
        constraint(
            8,
            json!({"kind": "vertical", "entity": "f3d:model:sketch-entity#4"})
        ),
        constraint(
            6,
            json!({"kind": "native", "native_kind": "Fusion only", "entities": ["f3d:model:sketch-entity#3"]})
        ),
    ]);
    let extent = extent.unwrap_or_else(|| {
        json!({"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": length}}})
    });
    model["features"] = json!([
        {
            "id": "f3d:model:feature#1",
            "ordinal": 0,
            "name": "Base sketch",
            "suppressed": false,
            "definition": {"definition": "sketch", "sketch": {"space": "planar", "sketch": SKETCH}},
        },
        {
            "id": "f3d:model:feature#2",
            "ordinal": 1,
            "name": "Block",
            "suppressed": false,
            "dependencies": ["f3d:model:feature#1"],
            "outputs": ["synthetic:cube:body#0"],
            "definition": {
                "definition": "extrude",
                "profile": profile,
                "direction": {"kind": "profile_normal"},
                "start": {"kind": "profile_plane"},
                "extent": extent,
                "op": "new_body",
            },
        },
    ]);
    model["parameters"] = json!([
        {
            "id": "f3d:model:parameter#1",
            "owner": "f3d:model:feature#2",
            "name": "d1",
            "expression": format!("{length} mm"),
            "value": {"kind": "length", "value": length},
        },
        {
            "id": "f3d:model:parameter#2",
            "owner": "f3d:model:feature#1",
            "ordinal": 1,
            "name": "side",
            "expression": "10 mm",
            "value": {"kind": "length", "value": 10.0},
        },
    ]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "cube".to_owned(),
        source_file: Some("cube.f3d".to_owned()),
        assembly: None,
    }
}

const TOP: &str = "f3d:model:sketch#2";

/// The example cube built instead by a loft from its 10 mm base square to a square of side
/// `top` centred above it, 10 mm up; `change` edits the loft's definition.
fn loft_history(top: f64, change: impl FnOnce(&mut Value)) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    let mut sketches = model["sketches"].as_array().unwrap().clone();
    let (a, b) = (5.0 - top / 2.0, 5.0 + top / 2.0);
    let top_loop: Vec<Value> = [11, 12, 13, 14]
        .map(|k| json!({"entity": format!("f3d:model:sketch-entity#{k}")}))
        .to_vec();
    sketches.push(json!({
        "id": TOP,
        "name": "Top",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 10.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [top_loop],
    }));
    model["sketches"] = Value::Array(sketches);
    let top_line = |key: u32, from: [f64; 2], to: [f64; 2]| {
        let mut row = line(key, from, to);
        row["sketch"] = json!(TOP);
        row
    };
    let entities = model["sketch_entities"].as_array_mut().unwrap();
    entities.extend([
        top_line(11, [a, a], [b, a]),
        top_line(12, [b, a], [b, b]),
        top_line(13, [b, b], [a, b]),
        top_line(14, [a, b], [a, a]),
    ]);
    let mut definition = json!({
        "definition": "loft",
        "sections": [
            first_profile(),
            {"kind": "sketch_profiles", "value": {"sketch": TOP, "profiles": [0]}},
        ],
        "guidance": {"kind": "guides", "path": []},
        "op": "new_body",
    });
    change(&mut definition);
    model["features"] = json!([
        model["features"][0].clone(),
        {
            "id": "f3d:model:feature#3",
            "ordinal": 1,
            "name": "Top sketch",
            "suppressed": false,
            "definition": {"definition": "sketch", "sketch": {"space": "planar", "sketch": TOP}},
        },
        {
            "id": "f3d:model:feature#2",
            "ordinal": 2,
            "name": "Block",
            "suppressed": false,
            "dependencies": ["f3d:model:feature#1", "f3d:model:feature#3"],
            "outputs": ["synthetic:cube:body#0"],
            "definition": definition,
        },
    ]);
    // The extrude's length parameter has no feature to own it now.
    model["parameters"] = json!([model["parameters"][1].clone()]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

#[test]
fn a_loft_between_sketch_profiles_becomes_a_solidcraft_loft() {
    let (document, report) = convert(
        &loft_history(10.0, |_| {}),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = document["features"].as_array().unwrap();
    let id_of = |name: &str| features.iter().find(|f| f["name"] == name).unwrap()["id"].clone();
    let loft = features.iter().find(|f| f["type"] == "loft").unwrap();
    assert_eq!(loft["name"], "Block");
    assert_eq!(loft["operation"], "new_body");
    assert_eq!(loft["body_names"], json!(["unit cube"]));
    let sections = loft["sections"].as_array().unwrap();
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0]["sketch"], id_of("Base sketch"));
    assert_eq!(sections[1]["sketch"], id_of("Top sketch"));
    assert_eq!(sections[1]["profiles"]["type"], "curves");
    assert!(
        history_line(&report).contains("3 of 3 features mapped"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn lofts_solidcraft_would_build_differently_flatten_the_history() {
    for (change, reason) in [
        (json!({"ruled": true}), "ruled lofts are not mapped yet"),
        (json!({"closed": true}), "closed lofts are not mapped yet"),
        (
            json!({"max_degree": 3}),
            "lofts with a degree limit are not mapped yet",
        ),
        (
            json!({"guidance": {"kind": "centerline", "path": {"kind": "native", "value": "n"}}}),
            "lofts along guide rails or a centreline are not mapped yet",
        ),
        (
            json!({"sections": [first_profile(), {"kind": "point", "value": {"x": 5.0, "y": 5.0, "z": 10.0}}]}),
            "lofts that close to a point are not mapped yet",
        ),
        (
            json!({"sections": [first_profile(), first_profile()]}),
            "loft sections that share a sketch are not mapped yet",
        ),
    ] {
        let model = loft_history(10.0, |definition| {
            for (key, value) in change.as_object().unwrap() {
                definition[key] = value.clone();
            }
        });
        let (document, report) = convert(
            &model,
            &SolidCraftWriter::new().verification(Verification::Off),
        );
        assert!(
            document["features"]
                .as_array()
                .unwrap()
                .iter()
                .all(|f| f["type"] != "loft"),
            "{reason}"
        );
        assert!(
            history_line(&report).contains(reason),
            "{}",
            history_line(&report)
        );
    }
}

#[test]
fn a_loft_inside_an_assembly_keeps_its_own_sketches() {
    let mut model = assembly_of_histories();
    let assembly = model.assembly.as_mut().unwrap();
    for part in &mut assembly.parts {
        part.ir = loft_history(10.0, |_| {}).ir;
    }
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let features = document["features"].as_array().unwrap();
    let lofts: Vec<&Value> = features.iter().filter(|f| f["type"] == "loft").collect();
    assert_eq!(lofts.len(), 2);
    for loft in lofts {
        for section in loft["sections"].as_array().unwrap() {
            let sketch = features
                .iter()
                .find(|f| f["id"] == section["sketch"])
                .unwrap();
            assert_eq!(sketch["type"], "sketch");
            assert_eq!(sketch["component"], loft["component"]);
        }
    }
}

#[test]
fn solidcraft_lofts_between_the_sections() {
    let Some(cli) = solidcraft() else { return };
    // The cube again, then a frustum: 10/3 × (100 + 16 + 40) mm³.
    for (top, expected) in [(10.0, 1000.0), (4.0, 520.0)] {
        let volume = volumes(&cli, &written(&loft_history(top, |_| {})).0);
        assert_eq!(volume.len(), 1);
        assert!((volume[0] - expected).abs() < 1e-6, "{volume:?}");
    }
}

fn whole_sketch() -> Value {
    json!({"kind": "sketch", "value": SKETCH})
}

fn first_profile() -> Value {
    json!({"kind": "sketch_profiles", "value": {"sketch": SKETCH, "profiles": [0]}})
}

fn convert(model: &Model, writer: &SolidCraftWriter) -> (Value, Report) {
    let mut report = Report::default();
    let mut bytes = Vec::new();
    writer.write(model, &mut bytes, &mut report).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut text = String::new();
    archive
        .by_name("document.json")
        .unwrap()
        .read_to_string(&mut text)
        .unwrap();
    (serde_json::from_str(&text).unwrap(), report)
}

fn history_line(report: &Report) -> String {
    report
        .losses()
        .iter()
        .filter(|loss| loss.subject == "history")
        .map(|loss| loss.detail.clone())
        .collect::<Vec<_>>()
        .join("; ")
}

fn types(document: &Value) -> Vec<(String, bool)> {
    document["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["type"].as_str().unwrap().to_owned(),
                f["suppressed"].as_bool().unwrap(),
            )
        })
        .collect()
}

#[test]
fn a_sketch_and_extrude_history_is_written() {
    let model = cube_with_history(10.0, first_profile(), None);
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, report) = convert(&model, &writer);

    assert_eq!(
        history_line(&report),
        "2 of 2 features mapped; unverified: verification turned off"
    );
    assert_eq!(
        types(&document),
        [
            ("sketch".to_owned(), false),
            ("extrude".to_owned(), false),
            ("import".to_owned(), true)
        ]
    );

    let mut pretty = serde_json::to_string_pretty(&document).unwrap() + "\n";
    // The STEP blob's name depends on cadmpeg's STEP writer, not on the history.
    let blob = document["features"][2]["step_blob"].as_str().unwrap();
    pretty = pretty.replace(blob, "blobs/<sha256>.step");
    let golden =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/cube_history.document.json");
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&golden, &pretty).unwrap();
    }
    assert_eq!(pretty, std::fs::read_to_string(&golden).unwrap());
}

#[test]
fn a_symmetric_through_all_extent_is_mapped() {
    let extent = json!({"kind": "symmetric", "side": {"termination": {"kind": "through_all"}}});
    let model = cube_with_history(10.0, whole_sketch(), Some(extent));
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let (document, _) = convert(&model, &writer);
    let extrude = &document["features"][1];
    assert_eq!(extrude["profiles"], json!({"type": "all"}));
    assert_eq!(
        extrude["extent"],
        json!({"direction": "symmetric", "convention": "total_distance",
               "distance": "0 mm", "through_all": true})
    );
}

#[test]
fn import_only_never_writes_history() {
    let model = cube_with_history(10.0, first_profile(), None);
    let (document, report) = convert(&model, &SolidCraftWriter::new().import_only(true));
    assert_eq!(history_line(&report), "not written (--import-only)");
    assert_eq!(types(&document), [("import".to_owned(), false)]);
}

/// The SolidCraft CLI the replay tests use, if one is configured.
fn solidcraft() -> Option<PathBuf> {
    let cli = std::env::var_os(CLI_ENV).map(PathBuf::from);
    if cli.is_none() {
        eprintln!("skipped: set {CLI_ENV} to replay histories in SolidCraft");
    }
    cli
}

#[test]
#[ignore = "cadmpeg/cadmpeg#255: cadmpeg-ir 0.6.2's example cube has its face loops reversed, so \
            SolidCraft reads the original as 3.33 mm³; enable once a release with the fix is pinned"]
fn a_faithful_history_is_verified_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let model = cube_with_history(10.0, first_profile(), None);
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::With(cli)),
    );
    assert_eq!(
        history_line(&report),
        "2 of 2 features mapped; verified against the original in SolidCraft"
    );
    assert_eq!(types(&document).len(), 3);
}

#[test]
fn a_history_that_replays_differently_is_flattened() {
    let Some(cli) = solidcraft() else { return };
    // The history says 4 mm; the geometry is the 10 mm cube.
    let model = cube_with_history(4.0, first_profile(), None);
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::With(cli)),
    );
    let line = history_line(&report);
    assert!(line.starts_with("flattened: replay differs"), "{line}");
    assert_eq!(types(&document), [("import".to_owned(), false)]);
}

/// The design's container with parameter `name`'s expression changed, every other entry kept.
fn with_param(bytes: &[u8], name: &str, expr: &str) -> Vec<u8> {
    let mut archive = ZipArchive::new(Cursor::new(bytes.to_vec())).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let path = entry.name().unwrap().into_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if path == "document.json" {
            let mut document: Value = serde_json::from_slice(&data).unwrap();
            for param in document["params"].as_array_mut().unwrap() {
                if param["name"] == name {
                    param["expr"] = json!(expr);
                }
            }
            data = serde_json::to_vec(&document).unwrap();
        }
        out.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut out, &data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// SolidCraft's body volumes for a design.
fn volumes(cli: &Path, design: &[u8]) -> Vec<f64> {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("design.solidcraft");
    let output = dir.path().join("eval.json");
    std::fs::write(&input, design).unwrap();
    let status = std::process::Command::new(cli)
        .arg("eval")
        .arg(&input)
        .arg("--json-out")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let result: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    for entry in result["timeline"].as_array().unwrap() {
        assert!(entry["error"].is_null(), "{entry}");
    }
    result["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|body| body["volume_mm3"].as_f64().unwrap())
        .collect()
}

#[test]
fn a_converted_dimension_drives_the_geometry_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let model = cube_with_history(10.0, first_profile(), None);
    let mut report = Report::default();
    let mut bytes = Vec::new();
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .write(&model, &mut bytes, &mut report)
        .unwrap();

    assert_eq!(volumes(&cli, &bytes), [1000.0]);
    assert_eq!(
        volumes(&cli, &with_param(&bytes, "side", "20 mm")),
        [4000.0]
    );
}

/// The example cube's geometry with a revolve history: a 5 mm × 10 mm rectangle on XZ, 5 mm
/// to 10 mm out from Z, turned about the Z axis by `extent`.
fn ring_history(extent: Value) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketches"][0]["placement"]["frame"]["normal"] = json!({"x": 0.0, "y": 1.0, "z": 0.0});
    // On XZ, sketch v runs along -Z.
    m["sketch_entities"] = json!([
        line(1, [5.0, 0.0], [10.0, 0.0]),
        line(2, [10.0, 0.0], [10.0, -10.0]),
        line(3, [10.0, -10.0], [5.0, -10.0]),
        line(4, [5.0, -10.0], [5.0, 0.0]),
    ]);
    m["sketch_constraints"] = json!([]);
    m["features"][1] = json!({
        "id": "f3d:model:feature#2",
        "ordinal": 1,
        "name": "Ring",
        "suppressed": false,
        "definition": {
            "definition": "revolve",
            "construction": {
                "state": "resolved",
                "profile": whole_sketch(),
                "axis": {
                    "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
                },
                "extent": extent,
            },
            "op": "new_body",
        },
    });
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn written(model: &Model) -> (Vec<u8>, Report) {
    let mut report = Report::default();
    let mut bytes = Vec::new();
    SolidCraftWriter::new()
        .verification(Verification::Off)
        .write(model, &mut bytes, &mut report)
        .unwrap();
    (bytes, report)
}

#[test]
fn full_and_symmetric_revolves_are_written() {
    let full = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": std::f64::consts::TAU}}));
    let (document, report) = convert(
        &full,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(history_line(&report).starts_with("2 of 2 features mapped"));
    let revolve = &document["features"][1];
    assert_eq!(revolve["type"], "revolve");
    assert_eq!(revolve["angle"], "360 deg");
    assert_eq!(
        revolve["axis"],
        json!({"type": "line", "origin": [0.0, 0.0, 0.0], "dir": [0.0, 0.0, 1.0]})
    );
    assert_eq!(revolve["convention"], "sketch_normal");

    let half = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": std::f64::consts::PI}}));
    let (_, report) = convert(
        &half,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).contains("which way it turns"),
        "{}",
        history_line(&report)
    );
}

#[test]
fn revolves_rebuild_their_volume_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let ring = std::f64::consts::PI * (10.0_f64.powi(2) - 5.0_f64.powi(2)) * 10.0;
    let close = |got: &[f64], want: f64| {
        assert_eq!(got.len(), 1, "{got:?}");
        assert!((got[0] - want).abs() / want < 1e-3, "{got:?} vs {want}");
    };
    let full = ring_history(json!({"kind": "one_sided",
        "termination": {"kind": "angle", "angle": std::f64::consts::TAU}}));
    close(&volumes(&cli, &written(&full).0), ring);
    let quarter = ring_history(json!({"kind": "symmetric",
        "termination": {"kind": "angle", "angle": std::f64::consts::FRAC_PI_2}}));
    close(&volumes(&cli, &written(&quarter).0), ring / 4.0);
}

/// The example cube's geometry with a right-triangle history: a fixed 40 mm horizontal base,
/// a vertical side, and the hypotenuse at `theta` from the base (shown as 36.87°), extruded by
/// 10 mm.
fn triangle_history() -> Model {
    let mut model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    m["sketches"][0]["profiles"] = json!([]);
    m["sketch_entities"] = json!([
        line(1, [0.0, 0.0], [40.0, 0.0]),
        line(2, [0.0, 0.0], [40.0, 30.0]),
        line(3, [40.0, 0.0], [40.0, 30.0]),
    ]);
    let constraint = |key: u32, definition: Value| {
        json!({"id": format!("f3d:model:sketch-constraint#{key}"), "sketch": SKETCH,
               "definition": definition})
    };
    let start = json!({"kind": "start", "entity": "f3d:model:sketch-entity#1"});
    m["sketch_constraints"] = json!([
        constraint(
            1,
            json!({"kind": "coincident_loci",
            "loci": [start, {"kind": "start", "entity": "f3d:model:sketch-entity#2"}]})
        ),
        constraint(
            2,
            json!({"kind": "horizontal", "entity": "f3d:model:sketch-entity#1"})
        ),
        constraint(
            3,
            json!({"kind": "vertical", "entity": "f3d:model:sketch-entity#3"})
        ),
        constraint(
            4,
            json!({"kind": "distance", "entities": ["f3d:model:sketch-entity#1"],
                              "parameter": "f3d:model:parameter#2"})
        ),
        constraint(
            5,
            json!({"kind": "angle", "first": "f3d:model:sketch-entity#1",
                              "second": "f3d:model:sketch-entity#2",
                              "parameter": "f3d:model:parameter#3"})
        ),
    ]);
    m["parameters"][1]["expression"] = json!("40 mm");
    m["parameters"][1]["value"] = json!({"kind": "length", "value": 40.0});
    let theta = 30.0_f64.atan2(40.0);
    m["parameters"].as_array_mut().unwrap().push(json!({
        "id": "f3d:model:parameter#3",
        "owner": "f3d:model:feature#1",
        "ordinal": 2,
        "name": "theta",
        "expression": format!("{} deg", theta.to_degrees()),
        "value": {"kind": "angle", "value": theta},
    }));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_converted_angle_drives_the_geometry_in_solidcraft() {
    let Some(cli) = solidcraft() else { return };
    let (bytes, report) = written(&triangle_history());
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("constraints not carried")),
        "{:?}",
        report.losses()
    );
    let close = |got: Vec<f64>, want: f64| {
        assert_eq!(got.len(), 1, "{got:?}");
        assert!((got[0] - want).abs() < 1e-6, "{got:?} vs {want}");
    };
    close(volumes(&cli, &bytes), 6000.0);
    close(
        volumes(&cli, &with_param(&bytes, "theta", "45 deg")),
        8000.0,
    );
}

/// The shape cadmpeg 0.6's Inventor decoder gives an extrude: its profile as native profile
/// selections of the sketch, an explicit direction, a symmetric or through-next extent. Until
/// cadmpeg resolves those selections into sketch profiles the history cannot be replayed, and
/// the design is written as geometry with the reason reported.
#[test]
fn an_inventor_shaped_extrude_flattens_with_its_reason() {
    let inventor_profile = json!({"kind": "sketch_selection",
        "value": {"sketch": SKETCH, "selections": ["inventor:native:profile-selection#1"]}});
    let model = cube_with_history(
        10.0,
        inventor_profile,
        Some(json!({"kind": "symmetric",
            "side": {"termination": {"kind": "blind", "length": 10.0}}})),
    );
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert_eq!(types(&document), [("import".to_owned(), false)]);
    assert_eq!(
        history_line(&report),
        "flattened: feature \"Block\": sketch_selection profiles are not mapped yet"
    );
    let outcome = report.outcome().unwrap();
    let statuses: Vec<_> = outcome
        .features
        .iter()
        .map(|f| (f.kind.as_str(), f.status))
        .collect();
    assert_eq!(
        statuses,
        [
            ("sketch", dutchman_core::FeatureStatus::Mapped),
            ("extrude", dutchman_core::FeatureStatus::Blocked)
        ]
    );
}

/// Two parts with the cube history (and so the same parameter names), `First` placed at the
/// origin and `Second` twice, 30 mm and 60 mm along X.
fn assembly_of_histories() -> Model {
    let part = |name: &str| Model {
        name: name.to_owned(),
        source_file: Some(format!("{name}.ipt")),
        ..cube_with_history(10.0, first_profile(), None)
    };
    let at = |x: f64| {
        [
            [1.0, 0.0, 0.0, x],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ]
    };
    let placed = |part: usize, x: f64| dutchman_core::Placement {
        name: None,
        part,
        transform: at(x),
    };
    Model {
        name: "pair".to_owned(),
        source_file: Some("pair.iam".to_owned()),
        assembly: Some(dutchman_core::Assembly {
            parts: vec![part("First"), part("Second")],
            placements: vec![placed(0, 0.0), placed(1, 30.0), placed(1, 60.0)],
        }),
        ..part("pair")
    }
}

#[test]
fn parts_keep_their_histories_inside_an_assembly() {
    let (document, report) = convert(
        &assembly_of_histories(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let components = document["components"].as_array().unwrap();
    let second = components.iter().find(|c| c["name"] == "Second").unwrap()["id"].clone();
    let in_second: Vec<_> = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["component"] == second)
        .map(|f| {
            (
                f["type"].as_str().unwrap().to_owned(),
                f["suppressed"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        in_second,
        [
            ("sketch".to_owned(), false),
            ("extrude".to_owned(), false),
            ("import".to_owned(), true)
        ]
    );
    // Feature ids are unique, and each extrude still points at its own sketch.
    let features = document["features"].as_array().unwrap();
    let mut ids: Vec<u64> = features.iter().map(|f| f["id"].as_u64().unwrap()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), features.len());
    for extrude in features.iter().filter(|f| f["type"] == "extrude") {
        let sketch = features
            .iter()
            .find(|f| f["id"] == extrude["sketch"])
            .unwrap();
        assert_eq!(sketch["component"], extrude["component"]);
    }
    // The second part's parameters are renamed, and its dimension follows.
    let names: Vec<&str> = document["params"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["d1", "side", "Second_d1", "Second_side"]);
    let second_sketch = features
        .iter()
        .find(|f| f["type"] == "sketch" && f["component"] == second)
        .unwrap();
    let linked: Vec<&str> = second_sketch["sketch"]["constraints"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["param"].as_str())
        .collect();
    assert_eq!(linked, ["Second_side"]);
    let lines: Vec<String> = report
        .losses()
        .iter()
        .filter(|loss| loss.subject == "history")
        .map(|loss| loss.detail.clone())
        .collect();
    assert_eq!(
        lines,
        [
            "First.ipt: 2 of 2 features mapped; unverified: verification turned off",
            "Second.ipt: 2 of 2 features mapped; unverified: verification turned off"
        ]
    );
    assert_eq!(report.outcome().unwrap().mode, dutchman_core::Mode::History);
}

/// [`assembly_of_histories`] with `First`'s cube green and `Second`'s top face blue.
fn assembly_with_face_colours() -> Model {
    let mut model = assembly_of_histories();
    let assembly = model.assembly.as_mut().unwrap();
    for (part, recolour) in assembly.parts.iter_mut().zip([true, false]) {
        let mut wire = serde_json::to_value(&part.ir).unwrap();
        if recolour {
            wire["model"]["bodies"][0]["color"] = json!({"r": 0.0, "g": 1.0, "b": 0.0, "a": 1.0});
        } else {
            for face in wire["model"]["faces"].as_array_mut().unwrap() {
                if face["id"] == "synthetic:cube:face#top" {
                    face["color"] = json!({"r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0});
                }
            }
        }
        part.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    }
    model
}

#[test]
fn parts_inside_an_assembly_keep_their_own_body_and_face_colours() {
    let (document, report) = convert(
        &assembly_with_face_colours(),
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    // Both parts name their body "unit cube"; the second's is renamed so each keeps its look.
    let bodies = &document["appearances"]["bodies"];
    assert_eq!(bodies["unit cube"]["color"], json!([0, 255, 0]));
    assert_eq!(bodies["unit cube (Second)"]["color"], json!([204, 51, 26]));
    let second = document["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["name"] == "Second")
        .unwrap()["id"]
        .clone();
    let extrude = document["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["type"] == "extrude" && f["component"] == second)
        .unwrap();
    assert_eq!(extrude["body_names"], json!(["unit cube (Second)"]));
    // The face look is on the second part's body, at a point in that part's own frame: on its
    // top face, not moved to where it is placed.
    let faces = document["appearances"]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 1);
    assert_eq!(faces[0]["body"], "unit cube (Second)");
    assert_eq!(faces[0]["look"]["color"], json!([0, 0, 255]));
    let p: Vec<f64> = faces[0]["point"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_f64().unwrap())
        .collect();
    assert!(
        (p[2] - 10.0).abs() < 1e-9 && (0.0..10.0).contains(&p[0]) && (0.0..10.0).contains(&p[1]),
        "{p:?}"
    );
    assert!(
        !report
            .losses()
            .iter()
            .any(|loss| loss.subject == "appearance"),
        "{:?}",
        report.losses()
    );
}

#[test]
fn solidcraft_rebuilds_each_parts_history_in_the_assembly() {
    let Some(cli) = solidcraft() else { return };
    let (bytes, _) = written(&assembly_of_histories());
    assert_eq!(volumes(&cli, &bytes), [1000.0, 1000.0, 1000.0]);
    // The second part's own `side` drives only its placements.
    let mut grown = volumes(&cli, &with_param(&bytes, "Second_side", "20 mm"));
    grown.sort_by(f64::total_cmp);
    assert_eq!(grown, [1000.0, 4000.0, 4000.0]);
}

#[test]
fn inspecting_an_assembly_maps_each_parts_history() {
    let mut report = Report::default();
    let outcome = SolidCraftWriter::new().plan(&assembly_of_histories(), &mut report);
    assert_eq!(outcome.mode, dutchman_core::Mode::History);
    let names: Vec<String> = outcome
        .features
        .iter()
        .map(|f| f.name.clone().unwrap_or_default())
        .collect();
    assert_eq!(
        names,
        [
            "First.ipt: Base sketch",
            "First.ipt: Block",
            "Second.ipt: Base sketch",
            "Second.ipt: Block"
        ]
    );
}
/// The cube history followed by a pattern of its extrude, laid out by `layout`.
fn patterned(layout: Value) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({
            "id": "f3d:model:feature#3",
            "ordinal": 2,
            "name": "Copies",
            "suppressed": false,
            "definition": {
                "definition": "pattern",
                "seeds": [{"kind": "feature", "value": "f3d:model:feature#2"}],
                "pattern": layout,
            },
        }));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

#[test]
fn a_feature_pattern_names_its_seed() {
    let model = patterned(json!({"kind": "linear",
        "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3}));
    let (document, report) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    assert!(
        history_line(&report).starts_with("3 of 3 features mapped"),
        "{}",
        history_line(&report)
    );
    let pattern = &document["features"][2];
    assert_eq!(pattern["type"], "pattern");
    assert_eq!(pattern["features"], json!(["Block"]));
}

#[test]
fn solidcraft_replays_feature_patterns() {
    let Some(cli) = solidcraft() else { return };
    let linear = patterned(json!({"kind": "linear",
        "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3}));
    let mut copies = volumes(&cli, &written(&linear).0);
    copies.sort_by(f64::total_cmp);
    assert_eq!(copies.iter().sum::<f64>(), 3000.0, "{copies:?}");
    // Spacing is between instances: the third copy spans 40 to 50 mm.
    let mut starts: Vec<f64> = bounds(&cli, &written(&linear).0)
        .iter()
        .map(|(min, _)| min[0])
        .collect();
    starts.sort_by(f64::total_cmp);
    assert_eq!(starts, [0.0, 20.0, 40.0]);

    let around = patterned(json!({"kind": "circular",
        "axis_origin": {"x": -20.0, "y": 0.0, "z": 0.0},
        "axis_dir": {"x": 0.0, "y": 0.0, "z": 1.0},
        "angle": std::f64::consts::TAU, "count": 4}));
    let copies = volumes(&cli, &written(&around).0);
    assert!(
        (copies.iter().sum::<f64>() - 4000.0).abs() < 1e-6,
        "{copies:?}"
    );
}

/// Each body's bounding box (min, max) as SolidCraft evaluates the design.
fn bounds(cli: &Path, design: &[u8]) -> Vec<([f64; 3], [f64; 3])> {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("design.solidcraft");
    let output = dir.path().join("eval.json");
    std::fs::write(&input, design).unwrap();
    let status = std::process::Command::new(cli)
        .arg("eval")
        .arg(&input)
        .arg("--json-out")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let result: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    let corner = |v: &Value| [0, 1, 2].map(|i| v[i].as_f64().unwrap());
    result["bodies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| (corner(&b["bbox"]["min"]), corner(&b["bbox"]["max"])))
        .collect()
}

#[test]
fn a_face_colour_becomes_a_face_look_on_that_face() {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for face in wire["model"]["faces"].as_array_mut().unwrap() {
        if face["id"] == "synthetic:cube:face#top" {
            face["color"] = json!({"r": 0.0, "g": 0.0, "b": 1.0, "a": 1.0});
        }
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let (document, _) = convert(
        &model,
        &SolidCraftWriter::new().verification(Verification::Off),
    );
    let faces = document["appearances"]["faces"].as_array().unwrap();
    assert_eq!(faces.len(), 1, "only the face that differs from its body");
    let look = &faces[0];
    assert_eq!(look["body"], "unit cube");
    assert_eq!(look["look"]["color"], json!([0, 0, 255]));
    let p: Vec<f64> = look["point"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_f64().unwrap())
        .collect();
    assert!(
        (p[2] - 10.0).abs() < 1e-9 && (0.0..10.0).contains(&p[0]) && (0.0..10.0).contains(&p[1]),
        "{p:?}"
    );

    if let Some(cli) = solidcraft() {
        assert_eq!(volumes(&cli, &written(&model).0), [1000.0]);
    }
}

#[test]
fn solidcraft_extrudes_a_spline_profile() {
    let Some(cli) = solidcraft() else { return };
    let mut model = cube_with_history(10.0, whole_sketch(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["sketches"][0]["profiles"] = json!([]);
    wire["model"]["sketch_constraints"] = json!([]);
    wire["model"]["sketch_entities"] = json!([
        line(1, [0.0, 0.0], [30.0, 0.0]),
        entity(
            2,
            json!({"kind": "nurbs", "curve": {"degree": 2, "periodic": false,
            "knots": [0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            "poles": {"form": "polynomial", "points": [
                {"u": 0.0, "v": 0.0}, {"u": 15.0, "v": 30.0}, {"u": 30.0, "v": 0.0}]}}})
        ),
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    // Under a quadratic arch: base × control height / 3 = 300 mm², extruded 10 mm.
    let got = volumes(&cli, &written(&model).0);
    assert_eq!(got.len(), 1, "{got:?}");
    assert!((got[0] - 3000.0).abs() / 3000.0 < 1e-3, "{got:?}");
}

#[test]
fn solidcraft_colours_the_face_of_each_parts_own_body() {
    let Some(cli) = solidcraft() else { return };
    let dir = tempfile::tempdir().unwrap();
    let design = dir.path().join("pair.solidcraft");
    std::fs::write(&design, written(&assembly_with_face_colours()).0).unwrap();
    let script = dir.path().join("looks.json");
    let list = |body: &str| json!({"command": "appearance.list", "params": {"body": body}});
    std::fs::write(
        &script,
        json!([list("unit cube (Second)"), list("unit cube")]).to_string(),
    )
    .unwrap();
    let output = dir.path().join("looks.out.json");
    let status = std::process::Command::new(cli)
        .arg("run")
        .arg(&script)
        .arg("--in")
        .arg(&design)
        .arg("--results")
        .arg("--json-out")
        .arg(&output)
        .status()
        .unwrap();
    assert!(status.success());
    let results: Value = serde_json::from_str(&std::fs::read_to_string(output).unwrap()).unwrap();
    let face_colours = |result: &Value| -> Vec<String> {
        result["body"]["faces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|face| face["appearance"]["color"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(face_colours(&results[0]), ["#0000ff"], "{results}");
    assert!(face_colours(&results[1]).is_empty(), "{results}");
}
