//! Synthetic CADIR histories. The example B-rep is deliberately unrelated to the final
//! treatment: edge picks must come from the feature's own input state and its replay.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use dutchman_core::{Model, Report, Writer};
use dutchman_solidcraft::{SolidCraftWriter, Verification, CLI_ENV};
use serde_json::{json, Value};

fn xyz(p: [f64; 3]) -> Value {
    json!({"x": p[0], "y": p[1], "z": p[2]})
}

fn feature(id: &str, ordinal: u64, definition: Value) -> Value {
    json!({"id": id, "ordinal": ordinal, "name": id, "suppressed": false,
        "definition": {"kind": "operation", "value": definition},
        "outputs": [], "dependencies": [], "source_properties": {}, "source_content": []})
}

fn model(wire: &Value) -> Model {
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "Synthetic edge selections".to_owned(),
        source_file: None,
        assembly: None,
        source_step: None,
        merge_bodies: false,
    }
}

fn sample(edge: &str, start: [f64; 3], mid: [f64; 3], end: [f64; 3], tangent: [f64; 3]) -> Value {
    json!({"edge": edge, "start": xyz(start), "interior": xyz(mid), "end": xyz(end),
        "supports": [], "tangent": xyz(tangent), "faces": []})
}

fn path_wire() -> Value {
    let mut wire = serde_json::to_value(cadmpeg_ir::examples::unit_cube().unwrap()).unwrap();
    let m = &mut wire["model"];
    let mut base = feature(
        "test:model:feature#1",
        0,
        json!({"kind": "block", "value": {
        "dimensions": [10.0,10.0,10.0], "placement": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0]], "op": "new_body"}}),
    );
    base["outputs"] = json!(["synthetic:cube:body#0"]);
    m["features"] = json!([
        base,
        feature(
            "test:model:feature#2",
            1,
            json!({"kind": "sketch", "value": {
            "sketch": {"kind": "planar", "sketch": "test:model:sketch#1"}}})
        ),
        feature(
            "test:model:feature#3",
            2,
            json!({"kind": "sweep", "value": {
            "shape": {"kind": "solid", "op": "new_body", "section": {"kind": "profile", "value": {
                "kind": "sketch_profiles", "value": {"sketch": "test:model:sketch#1", "profiles": [0]}}}, "sections": []},
            "path": {"kind": "historical_edges", "value": {"state": "test:model:feature-input#1", "edges": ["upright", "top"], "native": "synthetic path"}},
            "path_tangent": false, "linearize": false,
            "path_extent": {"along_fraction": 1.0, "against_fraction": 0.0}}})
        ),
    ]);
    m["sketches"] = json!([{"id": "test:model:sketch#1", "name": "Section", "placement": {"kind": "resolved", "frame": {
        "origin": xyz([0.0;3]), "normal": xyz([0.0,0.0,1.0]), "u_axis": xyz([1.0,0.0,0.0])}},
        "profiles": [[{"entity": "test:model:sketch-entity#1", "reversed": false}]]}]);
    m["sketch_entities"] = json!([{"id": "test:model:sketch-entity#1", "sketch": "test:model:sketch#1", "construction": false,
        "geometry": {"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 0.5}, "endpoint_refs": []}]);
    m["feature_input_topologies"] = json!([{"id": "test:model:feature-input#1", "input_of": "test:model:feature#3", "bodies": [], "faces": [], "face_owners": [], "vertices": [],
        "edges": ["upright", "top"], "edge_geometry": [
            sample("upright", [0.0;3], [0.0,0.0,5.0], [0.0,0.0,10.0], [0.0,0.0,1.0]),
            sample("top", [0.0,0.0,10.0], [5.0,0.0,10.0], [10.0,0.0,10.0], [1.0,0.0,0.0])]}]);
    wire
}

fn written(wire: &Value, writer: &SolidCraftWriter) -> (Vec<u8>, Value, Report) {
    written_model(&model(wire), writer)
}

fn written_model(model: &Model, writer: &SolidCraftWriter) -> (Vec<u8>, Value, Report) {
    let mut bytes = Vec::new();
    let mut report = Report::default();
    writer.write(model, &mut bytes, &mut report).unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let mut document = String::new();
    zip.by_name("document.json")
        .unwrap()
        .read_to_string(&mut document)
        .unwrap();
    (bytes, serde_json::from_str(&document).unwrap(), report)
}

/// Replay the editable copy on its own. These synthetic fixtures deliberately store
/// an unrelated final B-rep, so an unchecked history keeps that original visible.
fn editable_copy(bytes: &[u8]) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut document: Value =
        serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
    let hidden: Vec<_> = document["components"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|component| component["history_hidden"] == true)
        .map(|component| component["id"].clone())
        .collect();
    let features = document["features"].as_array_mut().unwrap();
    features.retain(|feature| hidden.contains(&feature["component"]));
    assert!(!features.is_empty());
    for feature in features {
        feature["component"] = json!(0);
        // The hidden copy's body-building features are written suppressed (#940); turn them on.
        feature["suppressed"] = json!(false);
    }
    document["components"] = json!([]);
    document["occurrences"] = json!([]);
    document["visibility"] = json!({});
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).unwrap();
        let name = entry.name().unwrap().into_owned();
        let mut contents = Vec::new();
        entry.read_to_end(&mut contents).unwrap();
        if name == "document.json" {
            contents = serde_json::to_vec(&document).unwrap();
        }
        output
            .start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&contents).unwrap();
    }
    output.finish().unwrap().into_inner()
}

fn writer(cli: &Path) -> SolidCraftWriter {
    SolidCraftWriter::new()
        .target_version(Some("0.44.1".parse().unwrap()))
        .verification(Verification::Off)
        .replay(Verification::With(cli.to_path_buf()))
}

fn cli() -> Option<PathBuf> {
    let path = PathBuf::from(std::env::var_os(CLI_ENV)?);
    let version = std::process::Command::new(&path)
        .arg("--version")
        .output()
        .unwrap();
    let version = String::from_utf8(version.stdout).unwrap();
    let minimum = "0.44.1"
        .parse::<dutchman_solidcraft::SolidCraftVersion>()
        .unwrap();
    let release = version.split_whitespace().find_map(|word| {
        word.trim_start_matches('v')
            .parse::<dutchman_solidcraft::SolidCraftVersion>()
            .ok()
    });
    if release.is_none_or(|release| release < minimum) {
        eprintln!("skipped: synthetic edge replays require SolidCraft 0.44.1 or newer");
        return None;
    }
    Some(path)
}

fn measure(cli: &Path, input: &Path, output: &Path) -> Value {
    let result = std::process::Command::new(cli)
        .arg("eval")
        .arg(input)
        .arg("--json-out")
        .arg(output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap()
}

#[test]
fn topological_paths_keep_a_named_refusal_for_older_targets_and_missing_replays() {
    for (target, reason) in [
        ("0.18.2", "sweep paths of input edges need SolidCraft"),
        ("0.43.0", "sweep paths of input edges need SolidCraft"),
        (
            "0.44.1",
            "its input edges are found only on SolidCraft's replay",
        ),
    ] {
        let (_, document, report) = written(
            &path_wire(),
            &SolidCraftWriter::new()
                .target_version(Some(target.parse().unwrap()))
                .verification(Verification::Off),
        );
        assert!(!document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["type"] == "sweep"));
        assert!(
            report
                .losses()
                .iter()
                .any(|loss| loss.detail.contains(reason)),
            "{report:?}"
        );
    }
}

#[test]
fn topological_paths_refuse_disconnected_input_geometry_before_replay() {
    let mut wire = path_wire();
    let sample = &mut wire["model"]["feature_input_topologies"][0]["edge_geometry"][1];
    for key in ["start", "interior", "end"] {
        sample[key]["y"] = json!(2.0);
    }
    let (_, _, report) = written(
        &wire,
        &SolidCraftWriter::new()
            .target_version(Some("0.44.1".parse().unwrap()))
            .verification(Verification::Off),
    );
    assert!(
        report
            .losses()
            .iter()
            .any(|l| l.detail.contains("connected path in source order")),
        "{report:?}"
    );
}

#[test]
fn topological_chain_replays_as_the_same_solidcraft_edge_path() {
    let Some(cli) = cli() else { return };
    for reverse_carrier in [false, true] {
        let mut wire = path_wire();
        if reverse_carrier {
            let sample = &mut wire["model"]["feature_input_topologies"][0]["edge_geometry"][1];
            let start = sample["start"].take();
            sample["start"] = sample["end"].take();
            sample["end"] = start;
            sample["tangent"] = xyz([-1.0, 0.0, 0.0]);
        }
        let (bytes, document, report) = written(&wire, &writer(&cli));
        let sweep = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "sweep")
            .unwrap_or_else(|| panic!("{report:?}"));
        assert_eq!(sweep["path_body"], "unit cube");
        assert_eq!(
            sweep["path_edges"],
            json!([[0.0, 0.0, 5.0], [5.0, 0.0, 10.0]])
        );
        assert_eq!(sweep["path_sketch"], 0);
        assert_eq!(sweep["path"], json!([]));
        let dir = tempfile::tempdir().unwrap();
        let actual = dir.path().join("actual.sc");
        std::fs::write(&actual, editable_copy(&bytes)).unwrap();
        let expected = dir.path().join("expected.json");
        std::fs::write(&expected, json!([
            {"command":"solid.box","params":{"length":"10 mm","width":"10 mm","height":"10 mm","body_name":"Base"}},
            {"command":"sketch.create","params":{"plane":"XY","name":"Section"}},
            {"command":"sketch.circle.center","params":{"center":[0,0],"radius":0.5}},
            {"command":"sketch.finish","params":{}},
            {"command":"solid.sweep","params":{"sketch":"Section","path_body":"Base","path_edges":[[0,0,5],[5,0,10]],"operation":"new"}}
        ]).to_string()).unwrap();
        let actual = measure(&cli, &actual, &dir.path().join("actual.json"));
        let expected = measure(&cli, &expected, &dir.path().join("expected-result.json"));
        assert_eq!(actual["bodies"].as_array().unwrap().len(), 2);
        assert_eq!(expected["bodies"].as_array().unwrap().len(), 2);
        let a = actual["total"]["volume_mm3"].as_f64().unwrap();
        let b = expected["total"]["volume_mm3"].as_f64().unwrap();
        assert!((a - b).abs() < 1e-5, "{actual}\n{expected}");
    }
}

#[derive(Debug)]
struct EdgeReplay(Vec<(String, Vec<Value>)>);

impl dutchman_solidcraft::SolidCraftEvaluator for EdgeReplay {
    fn evaluate(&self, _: &[u8], _: std::time::Duration, _: &str) -> Result<Value, String> {
        Err("this edge proof does not evaluate final geometry".to_owned())
    }

    fn run(
        &self,
        _: &[u8],
        commands: &[Value],
        _: std::time::Duration,
        _: &str,
    ) -> Result<Vec<Value>, String> {
        commands.iter().map(|command| match command["command"].as_str() {
            Some("timeline.roll_to") => Ok(json!({})),
            Some("document.inspect") => Ok(json!({"timeline":[],"bodies":self.0.iter().map(|(name,_)| json!({"name":name})).collect::<Vec<_>>()})),
            Some("model.edges") => {
                let name = command["params"]["body"].as_str().unwrap();
                let (_, edges) = self.0.iter().find(|(body,_)| body == name).unwrap();
                Ok(json!({"body":name,"edges":edges}))
            }
            _ => Err("unexpected replay command".to_owned()),
        }).collect()
    }
}

#[derive(Debug)]
struct ArcReplay {
    edges: EdgeReplay,
    analytic: bool,
}

impl dutchman_solidcraft::SolidCraftEvaluator for ArcReplay {
    fn evaluate(&self, _: &[u8], _: std::time::Duration, _: &str) -> Result<Value, String> {
        Err("this edge proof does not evaluate final geometry".to_owned())
    }

    fn run(
        &self,
        design: &[u8],
        commands: &[Value],
        timeout: std::time::Duration,
        label: &str,
    ) -> Result<Vec<Value>, String> {
        commands
            .iter()
            .map(|command| {
                if command["command"] == "inspect.measure" {
                    let length = 5.0 * std::f64::consts::FRAC_PI_2;
                    return Ok(json!({"items":[{"type":"edge","round":"arc",
                    "center":[0,0,10],"axis":[0,0,1],"radius":5,
                    "length_mm":if self.analytic { length } else { 7.8 },
                    "arc_length":length}]}));
                }
                self.edges
                    .run(design, std::slice::from_ref(command), timeout, label)
                    .map(|mut results| results.remove(0))
            })
            .collect()
    }
}

#[test]
fn topological_body_arcs_use_the_shared_exact_circle_measurements() {
    let diagonal = 5.0 / 2.0_f64.sqrt();
    let mut wire = path_wire();
    wire["model"]["features"][2]["definition"]["value"]["value"]["path"]["value"]["edges"] =
        json!(["arc"]);
    let state = &mut wire["model"]["feature_input_topologies"][0];
    state["edges"] = json!(["arc"]);
    state["edge_geometry"] = json!([sample(
        "arc",
        [5.0, 0.0, 10.0],
        [diagonal, diagonal, 10.0],
        [0.0, 5.0, 10.0],
        [
            -std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
            0.0
        ]
    )]);
    for analytic in [false, true] {
        let writer = SolidCraftWriter::new()
            .target_version(Some("0.44.1".parse().unwrap()))
            .verification(Verification::Off)
            .replay(Verification::Auto)
            .evaluator(std::sync::Arc::new(ArcReplay {
                edges: EdgeReplay(vec![(
                    "unit cube".to_owned(),
                    vec![json!({
                        "index":0,"name":"arc","start":[5,0,10],
                        "mid":[diagonal,diagonal,10],"end":[0,5,10],"length":7.8
                    })],
                )]),
                analytic,
            }));
        let (_, document, report) = written(&wire, &writer);
        let sweep = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "sweep");
        if analytic {
            assert_eq!(sweep.unwrap()["path_body"], "unit cube", "{report:?}");
        } else {
            assert!(sweep.is_none(), "{document}");
            assert!(
                report.losses().iter().any(|loss| loss
                    .detail
                    .contains("no proven rebuilt circular arc measurement")),
                "{report:?}"
            );
        }
    }
}

#[test]
fn topological_paths_prove_every_edge_and_refuse_ambiguous_or_split_bodies() {
    let a = json!({"name":"A|B","start":[0,0,0],"mid":[0,0,5],"end":[0,0,10],"length":10});
    let b = json!({"name":"B|C","start":[0,0,10],"mid":[5,0,10],"end":[10,0,10],"length":10});
    for wrong in 0..5 {
        let mut bodies = vec![("unit cube".to_owned(), vec![a.clone(), b.clone()])];
        let reason = match wrong {
            0 => None,
            1 => {
                bodies[0].1[1]["end"][0] = json!(12.0);
                bodies[0].1[1]["length"] = json!(12.0);
                Some("other ends")
            }
            2 => {
                bodies[0].1[1]["length"] = json!(11.0);
                Some("not the same kind")
            }
            3 => {
                bodies.push(("Twin".to_owned(), vec![a.clone(), b.clone()]));
                Some("more than one rebuilt edge")
            }
            _ => {
                bodies[0].1.pop();
                bodies.push(("Other body".to_owned(), vec![b.clone()]));
                Some("crosses several rebuilt bodies")
            }
        };
        let writer = SolidCraftWriter::new()
            .target_version(Some("0.44.1".parse().unwrap()))
            .verification(Verification::Off)
            .replay(Verification::Auto)
            .evaluator(std::sync::Arc::new(EdgeReplay(bodies)));
        let (_, document, report) = written(&path_wire(), &writer);
        let sweep = document["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["type"] == "sweep");
        if let Some(reason) = reason {
            assert!(sweep.is_none(), "{document}");
            assert!(
                report.losses().iter().any(|l| l.detail.contains(reason)),
                "{report:?}"
            );
        } else {
            assert_eq!(sweep.unwrap()["path_body"], "unit cube");
        }
    }
}

#[test]
fn topological_closed_sweep_paths_keep_the_traversal_refusal() {
    let mut wire = path_wire();
    wire["model"]["features"][2]["definition"]["value"]["value"]["path"]["value"]["edges"] =
        json!(["ring"]);
    let state = &mut wire["model"]["feature_input_topologies"][0];
    state["edges"] = json!(["ring"]);
    state["edge_geometry"] = json!([sample(
        "ring",
        [5.0, 0.0, 0.0],
        [-5.0, 0.0, 0.0],
        [5.0, 0.0, 0.0],
        [0.0, -1.0, 0.0]
    )]);
    let (_, _, report) = written(
        &wire,
        &SolidCraftWriter::new()
            .target_version(Some("0.44.1".parse().unwrap()))
            .verification(Verification::Off),
    );
    assert!(
        report
            .losses()
            .iter()
            .any(|l| l.detail.contains("proven start and traversal direction")),
        "{report:?}"
    );
}

fn sketch_wire_path(arc: bool) -> Value {
    let mut wire = path_wire();
    let m = &mut wire["model"];
    let mut path_feature = feature(
        "test:model:feature#4",
        2,
        json!({"kind":"sketch","value":{"sketch":{"kind":"planar","sketch":"test:model:sketch#2"}}}),
    );
    path_feature["outputs"] = json!(["test:wire:body#1"]);
    m["features"][2]["ordinal"] = json!(3);
    m["features"][2]["dependencies"] = json!(["test:model:feature#4"]);
    m["features"]
        .as_array_mut()
        .unwrap()
        .insert(2, path_feature);
    m["sketches"].as_array_mut().unwrap().push(json!({
        "id":"test:model:sketch#2","name":"Spine","profiles":[],
        "placement":{"kind":"resolved","frame":{"origin":xyz([0.0;3]),
            "normal":xyz([0.0,1.0,0.0]),"u_axis":xyz([1.0,0.0,0.0])}}}));
    m["feature_input_topologies"][0]["bodies"] = json!(["test:wire:body#1"]);
    if arc {
        m["sketch_entities"].as_array_mut().unwrap().push(json!({
            "id":"test:model:sketch-entity#2","sketch":"test:model:sketch#2","construction":false,"endpoint_refs":[],
            "geometry":{"kind":"arc","center":{"u":10.0,"v":0.0},"radius":10.0,
                "start_angle":std::f64::consts::PI,"end_angle":3.0*std::f64::consts::FRAC_PI_2}}));
        m["features"][3]["definition"]["value"]["value"]["path"]["value"]["edges"] =
            json!(["bend"]);
        m["feature_input_topologies"][0]["edges"] = json!(["bend"]);
        let half = std::f64::consts::FRAC_1_SQRT_2;
        m["feature_input_topologies"][0]["edge_geometry"] = json!([sample(
            "bend",
            [0.0; 3],
            [10.0 - 10.0 * half, 0.0, 10.0 * half],
            [10.0, 0.0, 10.0],
            [half, 0.0, half]
        )]);
    } else {
        for (id, a, b) in [
            (2, [0.0, 0.0], [0.0, -10.0]),
            (3, [0.0, -10.0], [10.0, -10.0]),
        ] {
            m["sketch_entities"].as_array_mut().unwrap().push(json!({
                "id":format!("test:model:sketch-entity#{id}"),"sketch":"test:model:sketch#2","construction":false,"endpoint_refs":[],
                "geometry":{"kind":"line","start":{"u":a[0],"v":a[1]},"end":{"u":b[0],"v":b[1]}}}));
        }
    }
    wire
}

#[test]
fn topological_sketch_wires_resolve_only_to_their_own_exact_curves() {
    for arc in [false, true] {
        let wire = sketch_wire_path(arc);
        let w = SolidCraftWriter::new()
            .target_version(Some("0.44.1".parse().unwrap()))
            .verification(Verification::Off);
        let (_, document, report) = written(&wire, &w);
        let features = document["features"].as_array().unwrap();
        let sweep = features
            .iter()
            .find(|f| f["type"] == "sweep")
            .unwrap_or_else(|| panic!("{report:?}"));
        let sketch = features
            .iter()
            .find(|f| f["name"] == "test:model:feature#4")
            .unwrap();
        assert_eq!(sweep["path_sketch"], sketch["id"]);
        assert_eq!(
            sweep["path"].as_array().unwrap().len(),
            if arc { 1 } else { 2 }
        );
        assert!(sweep.get("path_body").is_none());
        for wrong in 0..3 {
            let mut bad = wire.clone();
            match wrong {
                0 => {
                    bad["model"]["feature_input_topologies"][0]["bodies"] =
                        json!(["test:other:body#1"])
                }
                1 => bad["model"]["features"][3]["dependencies"] = json!([]),
                _ => {
                    bad["model"]["feature_input_topologies"][0]["edge_geometry"][0]["end"]["x"] =
                        json!(20.0)
                }
            }
            let (_, document, _) = written(&bad, &w);
            assert!(!document["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["type"] == "sweep"));
        }
    }
}

#[test]
fn topological_sketch_arc_replays_as_a_quarter_torus() {
    let Some(cli) = cli() else { return };
    let (bytes, document, report) = written(&sketch_wire_path(true), &writer(&cli));
    assert!(
        document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f["type"] == "sweep"),
        "{report:?}"
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("arc.sc");
    std::fs::write(&path, editable_copy(&bytes)).unwrap();
    let actual = measure(&cli, &path, &dir.path().join("arc.json"));
    assert_eq!(actual["bodies"].as_array().unwrap().len(), 2, "{actual}");
    let expected = 1000.0 + std::f64::consts::PI * 0.25 * 10.0 * std::f64::consts::FRAC_PI_2;
    assert!(
        (actual["total"]["volume_mm3"].as_f64().unwrap() - expected).abs() < 0.05,
        "{actual}"
    );
}

#[derive(Debug)]
struct UnsoundOriginal;

impl dutchman_solidcraft::SolidCraftEvaluator for UnsoundOriginal {
    fn evaluate(&self, _: &[u8], _: std::time::Duration, label: &str) -> Result<Value, String> {
        let original = label == "original";
        let x = if original { 10_000.0 } else { 0.0 };
        Ok(json!({"timeline":[],"bodies":[{
            "name":"unit cube","volume_mm3":if original { Value::Null } else { json!(1000.0) },
            "area_mm2":600.0,"closed":!original,
            "bbox":{"min":[x,0.0,0.0],"max":[x+10.0,10.0,10.0]},
            "center_of_mass":if original { Value::Null } else { json!([5.0,5.0,5.0]) }
        }]}))
    }

    fn run(
        &self,
        _: &[u8],
        commands: &[Value],
        _: std::time::Duration,
        _: &str,
    ) -> Result<Vec<Value>, String> {
        commands.iter().map(|command| match command["command"].as_str() {
            Some("timeline.roll_to") => Ok(json!({})),
            Some("document.inspect") => Ok(json!({"timeline":[],"bodies":[{"name":"unit cube"}]})),
            Some("model.edges") => Ok(json!({"body":"unit cube","edges":[{
                "index":0,"name":"rim","start":[5,0,10],"mid":[-5,0,10],"end":[5,0,10],
                "length":31.365175761189757}]})),
            Some("inspect.measure") => Ok(json!({"items":[{"type":"edge","round":"circle",
                "center":[0,0,10],"axis":[0,0,1],"radius":5,"length_mm":std::f64::consts::TAU*5.0}]})),
            _ => Err("unexpected replay command".to_owned()),
        }).collect()
    }
}

#[test]
fn topological_histories_keep_the_original_position_when_the_source_cannot_be_compared() {
    {
        let mut wire = sketch_wire_path(true);
        wire["geometry"]["bodies"][0]["transform"] = json!([
            [1.0, 0.0, 0.0, 10_000.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ]);
        let part = model(&wire);
        for assembly in [false, true] {
            let mut source = part.clone();
            if assembly {
                source.assembly = Some(dutchman_core::Assembly {
                    own_part: None,
                    parts: vec![part.clone()],
                    definitions: Vec::new(),
                    identities: Vec::new(),
                    placements: vec![dutchman_core::Placement {
                        unproven: false,
                        hidden: false,
                        name: None,
                        part: 0,
                        transform: [
                            [1.0, 0.0, 0.0, 0.0],
                            [0.0, 1.0, 0.0, 0.0],
                            [0.0, 0.0, 1.0, 0.0],
                        ],
                    }],
                    joints: Vec::new(),
                    left_out: Vec::new(),
                    held: Vec::new(),
                    joint_notes: Vec::new(),
                    stored_placements: Vec::new(),
                });
            }
            let writer = SolidCraftWriter::new()
                .target_version(Some("0.44.1".parse().unwrap()))
                .evaluator(std::sync::Arc::new(UnsoundOriginal));
            let (_, document, report) = written_model(&source, &writer);
            let hidden: Vec<_> = document["components"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|component| component["history_hidden"] == true)
                .map(|component| component["id"].clone())
                .collect();
            let kind = "sweep";
            let feature = document["features"]
                .as_array()
                .unwrap()
                .iter()
                .find(|feature| feature["type"] == kind)
                .unwrap();
            assert!(
                hidden.contains(&feature["component"]),
                "{document}\n{report:?}"
            );
            assert!(document["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|feature| feature["type"] == "import"
                    && feature["suppressed"] == false
                    && !hidden.contains(&feature["component"])));
            assert!(
                report
                    .losses()
                    .iter()
                    .any(|loss| loss.detail.contains("kept hidden beside the original")),
                "{report:?}"
            );
        }
    }
    // Ordinary histories keep their existing verification behavior.
    for verification in [Verification::Auto, Verification::Off] {
        let mut wire = sketch_wire_path(true);
        wire["model"]["features"]
            .as_array_mut()
            .unwrap()
            .truncate(1);
        wire["model"]["sketches"] = json!([]);
        wire["model"]["sketch_entities"] = json!([]);
        wire["model"]["feature_input_topologies"] = json!([]);
        let writer = SolidCraftWriter::new()
            .target_version(Some("0.44.1".parse().unwrap()))
            .verification(verification)
            .evaluator(std::sync::Arc::new(UnsoundOriginal));
        let (_, document, report) = written(&wire, &writer);
        assert!(
            document.get("components").is_none(),
            "{document}\n{report:?}"
        );
        assert_eq!(report.outcome().unwrap().mode, dutchman_core::Mode::History);
    }
}

#[test]
fn unchecked_topological_histories_keep_the_original_visible() {
    {
        let mut wire = sketch_wire_path(true);
        wire["geometry"]["bodies"][0]["transform"] = json!([
            [1.0, 0.0, 0.0, 10_000.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0]
        ]);
        let writer = SolidCraftWriter::new()
            .target_version(Some("0.44.1".parse().unwrap()))
            .verification(Verification::Off)
            .replay(Verification::Auto)
            .evaluator(std::sync::Arc::new(UnsoundOriginal));
        let (_, document, report) = written(&wire, &writer);
        assert_eq!(
            report.outcome().unwrap().mode,
            dutchman_core::Mode::PartialHistory
        );
        assert!(document["components"]
            .as_array()
            .unwrap()
            .iter()
            .any(|component| component["history_hidden"] == true));
        assert!(document["features"]
            .as_array()
            .unwrap()
            .iter()
            .any(|feature| feature["type"] == "import" && feature["suppressed"] == false));
    }
}

#[test]
fn topological_closed_chains_refuse_unproven_traversal_with_twist_and_scale() {
    let semicircles = vec![
        sample(
            "first",
            [5.0, 0.0, 0.0],
            [0.0, 5.0, 0.0],
            [-5.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
        ),
        sample(
            "second",
            [-5.0, 0.0, 0.0],
            [0.0, -5.0, 0.0],
            [5.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
        ),
    ];
    let polygon = vec![
        sample(
            "first",
            [0.0; 3],
            [5.0, 0.0, 0.0],
            [10.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
        ),
        sample(
            "second",
            [10.0, 0.0, 0.0],
            [10.0, 5.0, 0.0],
            [10.0, 10.0, 0.0],
            [0.0, 1.0, 0.0],
        ),
        sample(
            "third",
            [10.0, 10.0, 0.0],
            [5.0, 10.0, 0.0],
            [0.0, 10.0, 0.0],
            [-1.0, 0.0, 0.0],
        ),
        sample(
            "fourth",
            [0.0, 10.0, 0.0],
            [0.0, 5.0, 0.0],
            [0.0; 3],
            [0.0, -1.0, 0.0],
        ),
    ];
    for geometry in [semicircles, polygon] {
        for controls in [
            json!({}),
            json!({"twist":0.5}),
            json!({"scale":1.5}),
            json!({"twist":0.5,"scale":1.5}),
        ] {
            let mut wire = path_wire();
            let ids: Vec<_> = geometry.iter().map(|edge| edge["edge"].clone()).collect();
            let definition = &mut wire["model"]["features"][2]["definition"]["value"]["value"];
            definition["path"]["value"]["edges"] = json!(ids);
            definition
                .as_object_mut()
                .unwrap()
                .extend(controls.as_object().unwrap().clone());
            let state = &mut wire["model"]["feature_input_topologies"][0];
            state["edges"] = json!(ids);
            state["edge_geometry"] = json!(geometry);
            let (_, document, report) = written(
                &wire,
                &SolidCraftWriter::new()
                    .target_version(Some("0.44.1".parse().unwrap()))
                    .verification(Verification::Off),
            );
            assert!(!document["features"]
                .as_array()
                .unwrap()
                .iter()
                .any(|f| f["type"] == "sweep"));
            assert!(
                report
                    .losses()
                    .iter()
                    .any(|loss| loss.detail.contains("proven start and traversal direction")),
                "{report:?}"
            );
        }
    }
}
