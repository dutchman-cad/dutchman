#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use crate::*;
use dutchman_core::{Mode, Model, Report, Writer};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::PathBuf,
};

// Synthetic CADIR uses cadmpeg's public unit cube and locally authored history.
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
        "profiles": [[{"entity": "synthetic:test:sketch-entity#1", "reversed": false}]]
    }]);
    arena["sketch_entities"] = json!([{"id": "synthetic:test:sketch-entity#1", "sketch": "synthetic:test:sketch#1", "construction": false, "geometry": {"kind": "circle", "center": {"u": 5.0, "v": 0.0}, "radius": 2.0}, "endpoint_refs": []}]);
    arena["features"] = json!([
        {"id": "synthetic:test:feature#1", "ordinal": 0, "name": "Profile", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": "synthetic:test:sketch#1"}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []},
        {"id": "synthetic:test:feature#2", "ordinal": 1, "name": "Solid", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "extrude", "value": {"profile": {"kind":"planar", "value": {"kind": "sketch", "value": "synthetic:test:sketch#1"}}, "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"}, "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 5.0}}}, "op": "new_body"}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}
    ]);
    if kind == "revolve" {
        arena["features"][1]["definition"] = json!({"kind":"operation", "value": {"kind": "revolve", "value": {"op": "new_body", "construction": {"kind": "resolved", "value": {"profile": {"kind": "sketch", "value": "synthetic:test:sketch#1"}, "axis": {"origin": {"x": 0.0, "y": 0.0, "z": 0.0}, "direction": {"x": 0.0, "y": 1.0, "z": 0.0}}, "extent": {"kind": "one_sided", "termination": {"kind": "angle", "angle": std::f64::consts::TAU}}}}}}});
    }
    if kind == "pattern" {
        arena["features"].as_array_mut().unwrap().push(json!({"id": "synthetic:test:feature#3", "ordinal": 2, "name": "Copies", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "pattern", "value": {"seeds": [{"kind": "feature", "value": "synthetic:test:feature#2"}], "pattern": {"kind": "linear", "direction": {"x": 1.0, "y": 0.0, "z": 0.0}, "spacing": 20.0, "count": 3, "omitted_instances": [], "symmetric": false}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    }
    let model = Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "Synthetic".to_owned(),
        source_file: None,
        source_step: None,
        assembly: None,
    };
    if kind == "partial" {
        with_unmapped_suffix(model)
    } else {
        model
    }
}

fn with_unmapped_suffix(mut model: Model) -> Model {
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "synthetic:test:feature#3", "ordinal": 2, "name": "Unmapped", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "native", "value": {"kind": "Synthetic unsupported feature", "parameters": {}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

fn mapped_rows(kind: &str) -> (Model, dutchman_solidcraft::MappedHistory) {
    let model = model(kind);
    let mapped = dutchman_solidcraft::mapped_history(&model, &mut Report::default());
    (model, mapped)
}
fn check_rows(rows: &[Value]) {
    let mut plan = Plan::from_model(&model("pad"), &mut Report::default());
    plan.steps = rows
        .iter()
        .map(|r| row::from_row(r, &BTreeMap::new(), &[], rows).unwrap())
        .collect();
    for (step, row) in plan.steps.iter().zip(rows) {
        assert_eq!(&step.to_row(&plan).unwrap(), row, "{}", step.kind);
    }
    let json = serde_json::to_vec(&plan).unwrap();
    let restored: Plan = serde_json::from_slice(&json).unwrap();
    for (step, row) in restored.steps.iter().zip(rows) {
        assert_eq!(
            &step.to_row(&restored).unwrap(),
            row,
            "serialized {}",
            step.kind
        );
    }
}
#[test]
fn synthetic_plans_match_goldens_and_rows_round_trip() {
    for kind in ["pad", "revolve", "pattern", "partial"] {
        let (model, mapping) = mapped_rows(kind);
        if let Ok(rows) = &mapping.features {
            check_rows(rows);
        }
        check_rows(&mapping.sketches);
        let plan = Plan::from_mapped(&model, mapping);
        let bytes = format!("{}\n", serde_json::to_string_pretty(&plan).unwrap());
        let path = format!(
            "{}/tests/golden/{kind}.plan.json",
            env!("CARGO_MANIFEST_DIR")
        );
        if std::env::var("BLESS").as_deref() == Ok("1") {
            std::fs::write(&path, &bytes).unwrap();
            println!(
                "REPLAY_GOLDEN {kind} {}",
                serde_json::to_string(&bytes).unwrap()
            );
        }
        assert_eq!(bytes, std::fs::read_to_string(path).unwrap(), "{kind}");
        if kind == "partial" {
            assert_eq!(
                plan.steps
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>(),
                ["Profile", "Solid"]
            );
            assert_eq!(plan.stop.unwrap().feature, "Unmapped");
        }
    }
}
#[test]
fn plate_example_reserializes_exactly() {
    let path = format!(
        "{}/tests/golden/plate.plan.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read_to_string(&path).unwrap();
    let plan: Plan = serde_json::from_str(&bytes).unwrap();
    let written = format!("{}\n", serde_json::to_string_pretty(&plan).unwrap());
    if std::env::var("BLESS").as_deref() == Ok("1") {
        std::fs::write(&path, &written).unwrap();
        println!(
            "REPLAY_GOLDEN plate {}",
            serde_json::to_string(&written).unwrap()
        );
    }
    assert_eq!(written, std::fs::read_to_string(path).unwrap());
    assert_eq!(plan.steps.len(), 5);
    assert_eq!(
        plan.steps[4].fields.edges.as_ref().unwrap()[0].proof,
        "unique_on_solidcraft_replay"
    );
}
#[test]
fn cube_document_rows_round_trip() {
    let document: Value = serde_json::from_str(include_str!(
        "../../dutchman-solidcraft/tests/golden/cube_history.document.json"
    ))
    .unwrap();
    check_rows(document["features"].as_array().unwrap());
}
#[test]
fn quantities_preserve_text_and_resolve_only_supported_forms() {
    let params = BTreeMap::from([
        ("width".into(), "2 cm".into()),
        ("nested".into(), "-width".into()),
        ("cycle".into(), "cycle".into()),
    ]);
    for (expr, want) in [
        ("10", Some(10.0)),
        (" ( 1 mm ) ", Some(1.0)),
        ("2cm", Some(20.0)),
        ("1 m", Some(1000.0)),
        ("1 in", Some(25.4)),
        ("1 ft", Some(304.8)),
        ("90 deg", Some(90.0)),
        ("3.141592653589793 rad", Some(180.0)),
        ("width", Some(20.0)),
        ("-width", Some(-20.0)),
        ("nested", Some(-20.0)),
        ("(width) / (2)", Some(10.0)),
        ("cycle", None),
        ("1/0", None),
        ("1/2/3", None),
        ("2+2", None),
        ("1 yard", None),
        ("NaN", None),
        ("inf", None),
        ("1e999", None),
        ("missing", None),
    ] {
        let q = Quantity::from_row(expr, &params);
        assert_eq!(q.value, want, "{expr}");
        assert_eq!(q.expr, expr);
    }
    assert_eq!(
        Quantity::from_row("-width", &params).param.as_deref(),
        Some("width")
    );
    assert_eq!(Quantity::from_row("1 mm", &params).param, None);
    let deep: BTreeMap<_, _> = (0..66)
        .map(|i| {
            (
                format!("p{i}"),
                if i == 65 {
                    "1 mm".into()
                } else {
                    format!("p{}", i + 1)
                },
            )
        })
        .collect();
    assert_eq!(Quantity::from_row("p0", &deep).value, None);
}
fn raw(kind: &str, fields: Value) -> Value {
    let mut row = json!({"id":2,"name":"Feature","type":kind,"suppressed":false});
    row.as_object_mut()
        .unwrap()
        .extend(fields.as_object().unwrap().clone());
    row
}
#[test]
fn every_feature_shape_round_trips_with_unknown_fields() {
    let (_, mapping) = mapped_rows("pad");
    let sketch = mapping.sketches[0].clone();
    let mut rows = vec![sketch];
    // Supplemental rows deliberately carry an unknown option that no model emits.
    // Vocabulary: blend.rs:123; pattern.rs:141, :412; shell.rs:55;
    // loft.rs:217; sweep.rs:71-180; primitive.rs:182-199, :290, :353;
    // hole.rs:208; extrude.rs:106, :143. Real mapper rows are checked separately.
    let cases = [
        (
            "fillet",
            json!({"radius":"2 mm","style":{"type":"constant"},"edges":[[1.0,2.0,3.0]]}),
        ),
        (
            "chamfer",
            json!({"distance":"2 mm","edges":[[1.0,2.0,3.0]]}),
        ),
        (
            "mirror",
            json!({"features":["Profile"],"plane":{"type":"custom","plane":{"origin":[0.0,0.0,0.0],"x_dir":[1.0,0.0,0.0],"y_dir":[0.0,1.0,0.0]}}}),
        ),
        (
            "shell",
            json!({"faces":[[1.0,2.0,3.0]],"thickness":"1 mm","direction":"inside","body":"Solid","tangent_chain":false}),
        ),
        (
            "loft",
            json!({"sections":[{"sketch":1,"profiles":{"type":"all"}},{"sketch":1,"profiles":{"type":"whole_boundaries","profiles":[{"outer":["l1"],"holes":[]}]}}],"operation":"new_body"}),
        ),
        (
            "sweep",
            json!({"sketch":1,"profiles":{"type":"all"},"operation":"join","path_sketch":1,"path":["l1"],"twist":"20 deg"}),
        ),
        (
            "coil",
            json!({"base":[0.0,0.0,0.0],"axis":[0.0,0.0,1.0],"diameter":"10 mm","pitch":"2 mm","turns":"3","section":"circular","section_size":"1 mm","clockwise":true,"operation":"new_body"}),
        ),
        (
            "coil",
            json!({"sketch":1,"profiles":{"type":"all"},"base":[0.0,0.0,0.0],"axis":[0.0,0.0,1.0],"pitch":"(10 mm) / (2)","turns":"2","clockwise":false,"operation":"new_body"}),
        ),
        (
            "sphere",
            json!({"center":[0.0,0.0,0.0],"radius":"2 mm","operation":"new_body"}),
        ),
        (
            "torus",
            json!({"center":[0.0,0.0,0.0],"major":"5 mm","minor":"1 mm","operation":"join"}),
        ),
        (
            "unknown",
            json!({"something":{"expr":"a","value":12},"suppressed":true}),
        ),
    ];
    for (kind, fields) in cases {
        let mut r = raw(kind, fields);
        r["unhandled"] = json!({"a":[1,"x"]});
        rows.push(r);
    }
    for layout in [
        json!({"type":"rectangular","dir1":[1.0,0.0,0.0],"count1":"3","spacing1":"2 mm"}),
        json!({"type":"rectangular","dir1":[1.0,0.0,0.0],"count1":"3","spacing1":"2 mm","dir2":[0.0,1.0,0.0],"count2":"2","spacing2":"1 mm"}),
        json!({"type":"circular","origin":[0.0,0.0,0.0],"axis":[0.0,0.0,1.0],"count":"4","angle":"360 deg"}),
    ] {
        rows.push(raw(
            "pattern",
            json!({"features":["Profile"],"pattern":layout}),
        ));
    }
    for form in [
        json!({"type":"simple"}),
        json!({"type":"drilled","tip_angle":"118"}),
        json!({"type":"counterbore","cb_diameter":"8 mm","cb_depth":"2 mm","tip_angle":"180"}),
        json!({"type":"countersink","cs_diameter":"8 mm","cs_angle":"90"}),
    ] {
        for depth in [None, Some("5 mm")] {
            let mut r = raw(
                "hole",
                json!({"points":{"sketch":1,"ids":["p1"]},"hole":form,"diameter":"6 mm","direction":[0.0,0.0,-1.0],"position":[5.0,0.0,0.0]}),
            );
            if let Some(d) = depth {
                r["depth"] = json!(d);
            }
            rows.push(r);
        }
    }
    for extent in [
        json!({"direction":"positive","distance":"5 mm"}),
        json!({"direction":"negative","distance":"5 mm","distance2":"2 mm"}),
        json!({"direction":"symmetric","distance":"5 mm"}),
        json!({"direction":"positive","through_all":true}),
    ] {
        rows.push(raw("extrude",json!({"sketch":1,"profiles":{"type":"curves","loops":[["l1"]]},"extent":extent,"operation":"cut"})));
    }
    check_rows(&rows);
}
#[test]
fn bad_indices_stop_the_plan_and_unique_names_link_ordinals() {
    let (model, mut mapping) = mapped_rows("pad");
    mapping.features.as_mut().unwrap()[0]["sketch"]["curves"][0]["c"] = json!(999999);
    let plan = Plan::from_mapped(&model, mapping);
    assert!(plan.steps.is_empty());
    assert_eq!(
        plan.stop.unwrap().reason,
        "feature Profile: a sketch reference is out of range"
    );
    let (model, mapping) = mapped_rows("pad");
    let plan = Plan::from_mapped(&model, mapping);
    assert_eq!(plan.steps[0].source.ordinal, Some(0));
    assert_eq!(plan.steps[0].source.id, None);
}
struct TestEmitter {
    refuse: &'static str,
}
impl Emitter for TestEmitter {
    fn target(&self) -> &'static str {
        "test"
    }
    fn program(&self) -> &'static str {
        "Test"
    }
    fn format(&self) -> &'static str {
        "test-script/1"
    }
    fn supports(&self, step: &Step, _: &Plan) -> Result<(), String> {
        if step.kind == self.refuse || !step.extra.is_empty() {
            Err(format!(
                "feature {}: {} features are not supported yet in Test output",
                step.name, step.kind
            ))
        } else {
            Ok(())
        }
    }
    fn handled_options(&self, _kind: &str) -> &'static [&'static str] {
        &[]
    }
    fn lower(&self, plan: &Plan, upto: usize, _plan_sha256: &str) -> Result<Lowered, String> {
        let mut t = tsv::Tsv::new(self.target());
        t.push("PLAN_HASH", &[tsv::Arg::Text(_plan_sha256)])?;
        for step in plan.steps.iter().take(upto) {
            t.push("FEATURE", &[tsv::Arg::Text(&step.name)])?;
        }
        Ok(Lowered::new(t.finish()))
    }
    fn runtime(&self) -> (&'static str, &'static str) {
        ("fixed dispatcher\n", "Rebuild.txt")
    }
    fn instructions(&self, _: &Plan) -> String {
        "Run the script.\n".into()
    }
}
#[test]
fn emitter_refusal_keeps_only_the_supported_prefix() {
    let mut plan = Plan::from_model(&model("pad"), &mut Report::default());
    let mut revolve = plan.steps[1].clone();
    revolve.kind = "revolve".into();
    revolve.id = 3;
    revolve.name = "Turn".into();
    let mut pad = plan.steps[1].clone();
    pad.id = 4;
    pad.name = "Later pad".into();
    for s in [&revolve, &pad] {
        plan.outcomes.push(plan::Outcome {
            name: Some(s.name.clone()),
            kind: s.kind.clone(),
            status: OutcomeStatus::Mapped,
            reason: None,
        });
    }
    plan.steps.extend([revolve, pad]);
    assert_eq!(
        prefix::supported_prefix(&TestEmitter { refuse: "revolve" }, &mut plan),
        2
    );
    assert_eq!(plan.stop.unwrap().feature, "Turn");
    assert_eq!(plan.outcomes[3].status, "blocked");
}
#[test]
fn writer_packages_data_and_reports_complete_partial_and_empty_history() {
    for (kind, refuse, mode) in [
        ("pad", "", Mode::History),
        ("partial", "", Mode::PartialHistory),
        ("pad", "sketch", Mode::Import),
    ] {
        let writer = ScriptWriter::new(TestEmitter { refuse });
        assert!(writer.accepts_extension("ZIP"));
        let mut out = Vec::new();
        let mut report = Report::default();
        writer.write(&model(kind), &mut out, &mut report).unwrap();
        assert_eq!(report.outcome().unwrap().mode, mode);
        let mut zip = zip::ZipArchive::new(Cursor::new(out)).unwrap();
        assert_eq!(zip.len(), 5);
        for (index, name) in [
            "README.txt",
            "Rebuild.txt",
            "steps.tsv",
            "plan.json",
            "original.step",
        ]
        .iter()
        .enumerate()
        {
            assert_eq!(zip.by_index(index).unwrap().name().unwrap(), *name);
        }
        let mut json = String::new();
        zip.by_name("plan.json")
            .unwrap()
            .read_to_string(&mut json)
            .unwrap();
        assert!(json.ends_with('\n'));
        let _: Plan = serde_json::from_str(&json).unwrap();
        assert_eq!(package::sha256(json.as_bytes()).len(), 64);
    }
}
#[test]
fn tsv_escapes_names_and_writes_plain_numbers_with_limits() {
    use tsv::{Arg, Tsv};
    let hostile = "\t%0A'; Shell(\"x\")\n\rü\u{0001}";
    let mut t = Tsv::new("test");
    t.push(
        "DATA",
        &[
            Arg::Text(hostile),
            Arg::Num(0.04),
            Arg::Num(1e-12),
            Arg::Num(-0.0),
            Arg::Int(-2),
            Arg::Flag(true),
            Arg::None,
        ],
    )
    .unwrap();
    assert_eq!(t.as_str(),"dutchman-steps/1\ttest\r\n1\tDATA\t%09%250A'; Shell(\"x\")%0A%0D%C3%BC%01\t0.04\t0.000000000001\t0\t-2\t1\t-\r\n");
    assert_eq!(
        t.push("DATA", &[Arg::Num(f64::NAN)]).unwrap_err(),
        "a value is not a finite number"
    );
    assert!(t.push("DATA", &[Arg::Text(&"x".repeat(8000))]).is_err());
    for _ in 1..200_000 {
        t.push("X", &[]).unwrap();
    }
    assert!(t.push("X", &[]).is_err());
    let (_, mut mapping) = mapped_rows("pad");
    for row in mapping.features.as_mut().unwrap() {
        row["name"] = json!(hostile);
    }
    let plan = Plan::from_mapped(&model("pad"), mapping);
    let data = TestEmitter { refuse: "" }
        .lower(&plan, 2, &"0".repeat(64))
        .unwrap();
    assert!(data.tsv.contains("%09%250A"));
    assert!(!data.tsv.contains(hostile));
    for step in &plan.steps {
        assert_eq!(step.to_row(&plan).unwrap()["name"], hostile);
    }
}
#[test]
fn packages_are_deterministic_ordered_and_timestamped() {
    assert_eq!(
        package::sha256(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let files = [("a", b"first".as_slice()), ("b", b"second".as_slice())];
    let mut a = Vec::new();
    let mut b = Vec::new();
    package::write_package(&mut a, &files).unwrap();
    package::write_package(&mut b, &files).unwrap();
    assert_eq!(a, b);
    let mut zip = zip::ZipArchive::new(Cursor::new(a)).unwrap();
    let entry = zip.by_index(0).unwrap();
    let time = entry.last_modified().unwrap();
    assert_eq!(
        (
            time.year(),
            time.month(),
            time.day(),
            time.hour(),
            time.minute(),
            time.second()
        ),
        (1980, 1, 1, 0, 0, 0)
    );
    assert_eq!(entry.name().unwrap(), "a");
    assert_eq!(entry.compression(), zip::CompressionMethod::Deflated);
    assert!(entry.extra_data().is_none_or(|x| x.is_empty()));
    assert!(package::write_package(&mut Vec::new(), &[("../x", b"x")]).is_err());
}
fn good_result() -> Value {
    json!({"schema":"dutchman.script-result/1","kind":"rebuild","target":"test","program_version":"1","script_version":"dutchman 0.2.10","plan_sha256":"0".repeat(64),"verdict":"pass","check":"passed","steps_total":1,"steps_done":1,"failed":null,"measured":{"bodies":1,"volume_mm3":1.0,"area_mm2":6.0,"bbox_mm":[[0.0,0.0,0.0],[1.0,1.0,1.0]]},"original":null,"seconds":4})
}
#[test]
fn result_reader_checks_every_field_and_ignores_unknown_keys() {
    let good = good_result();
    assert!(ScriptResult::read(good.to_string().as_bytes()).is_ok());
    for field in good.as_object().unwrap().keys() {
        let mut bad = good.clone();
        bad[field] = json!([]);
        assert!(
            ScriptResult::read(bad.to_string().as_bytes()).is_err(),
            "{field}"
        );
        let mut missing = good.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(
            ScriptResult::read(missing.to_string().as_bytes()).is_err(),
            "missing {field}"
        );
    }
    for field in ["schema", "kind", "verdict", "check"] {
        let mut bad = good.clone();
        bad[field] = json!("unknown");
        assert!(ScriptResult::read(bad.to_string().as_bytes()).is_err());
    }
    let mut unknown = good.clone();
    unknown["added"] = json!({"anything":[1,2]});
    assert!(ScriptResult::read(unknown.to_string().as_bytes()).is_ok());
    let mut long = good.clone();
    long["target"] = json!("x".repeat(2001));
    assert!(ScriptResult::read(long.to_string().as_bytes()).is_err());
    assert!(ScriptResult::read(&[b' '; 65537]).is_err());
    assert!(ScriptResult::read(b"[]").is_err());
    for field in ["measured", "original"] {
        let mut bad = good.clone();
        bad[field] = json!({"bodies":1});
        assert!(ScriptResult::read(bad.to_string().as_bytes()).is_err());
    }
    let mut failed = good;
    failed["failed"] = json!({"line":22,"op":"EXTRUDE","step_kind":"extrude","message":"The feature could not be built"});
    assert!(ScriptResult::read(failed.to_string().as_bytes()).is_ok());
    failed["failed"]["line"] = json!("22");
    assert!(ScriptResult::read(failed.to_string().as_bytes()).is_err());
}

#[test]
fn sketch_curves_and_constraints_keep_stable_references() {
    let points = json!([{"id":"origin","pos":[0.0,0.0],"fixed":true},{"id":"p1","pos":[1.0,0.0],"fixed":false},{"id":"p2","pos":[0.0,1.0],"fixed":false},{"id":"p3","pos":[2.0,2.0],"fixed":false}]);
    let curves = json!([{"id":"l1","type":"line","a":1,"b":2,"construction":false},{"id":"c1","type":"circle","c":0,"r":2.0,"construction":false},{"id":"a1","type":"arc","a":1,"b":2,"c":0,"construction":true},{"id":"e1","type":"ellipse","c":0,"m":3,"r":1.0,"construction":false},{"id":"s1","type":"spline","pts":[0,1,2,3],"degree":3,"construction":false}]);
    let constraints = json!([{"id":"k1","type":"horizontal","l":0},{"id":"k2","type":"coincident","p":1,"q":0},{"id":"k3","type":"angle","a":0,"b":2,"value":std::f64::consts::FRAC_PI_2,"flip":true},{"id":"k4","type":"length","l":0,"value":2.0,"param":"width","driven":true},{"id":"k5","type":"point_on_curve","p":1,"c":1},{"id":"k6","type":"equal","a":1,"b":2}]);
    let row = raw(
        "sketch",
        json!({"plane":{"type":"origin","name":"XY"},"sketch":{"points":points,"curves":curves,"constraints":constraints}}),
    );
    check_rows(std::slice::from_ref(&row));
    let step = row::from_row(&row, &BTreeMap::new(), &[], &[]).unwrap();
    assert_eq!(
        step.fields.curves.as_ref().unwrap()[0].a.as_deref(),
        Some("p1")
    );
    assert_eq!(
        step.fields.constraints.as_ref().unwrap()[0].line.as_deref(),
        Some("l1")
    );
    assert_eq!(
        step.fields.constraints.as_ref().unwrap()[2]
            .value
            .as_ref()
            .unwrap()
            .value,
        Some(90.0)
    );
    let mut bad = row.clone();
    bad["sketch"]["constraints"][0]["l"] = json!(100);
    assert!(row::from_row(&bad, &BTreeMap::new(), &[], &[]).is_err());
    let mut bad = row;
    bad["sketch"]["curves"][4]["pts"][0] = json!(100);
    assert!(row::from_row(&bad, &BTreeMap::new(), &[], &[]).is_err());
}
#[test]
fn parameters_keep_source_text_kinds_and_evaluated_values() {
    let mut model = model("pad");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    wire["model"]["parameters"] = json!([
        {"id":"synthetic:test:parameter#1","ordinal":0,"name":"width","expression":"2 cm","value":{"kind":"length","value":20.0},"dependencies":[],"properties":{}},
        {"id":"synthetic:test:parameter#2","ordinal":1,"name":"turn","expression":"90 deg","value":{"kind":"angle","value":std::f64::consts::FRAC_PI_2},"dependencies":[],"properties":{}},
        {"id":"synthetic:test:parameter#3","ordinal":2,"name":"count","expression":"3","value":{"kind":"integer","value":3},"dependencies":[],"properties":{}}
    ]);
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let plan = Plan::from_model(&model, &mut Report::default());
    assert_eq!(
        plan.parameters
            .iter()
            .map(|p| (p.name.as_str(), p.kind.as_str(), p.value))
            .collect::<Vec<_>>(),
        [
            ("width", "length", Some(20.0)),
            ("turn", "angle", Some(90.0)),
            ("count", "number", Some(3.0))
        ]
    );
    assert_eq!(plan.parameters[0].expr, "2 cm");
    assert_eq!(plan.parameters[0].source.as_deref(), Some("width"));
}

#[test]
fn assemblies_are_refused_before_any_bytes_are_written() {
    let mut m = model("pad");
    m.assembly = Some(dutchman_core::Assembly {
        own_part: None,
        parts: vec![],
        definitions: vec![],
        identities: vec![],
        placements: vec![],
        joints: vec![],
        left_out: vec![],
        held: vec![],
        joint_notes: vec![],
        stored_placements: vec![],
    });
    let mut out = Vec::new();
    let error = ScriptWriter::new(TestEmitter { refuse: "" })
        .write(&m, &mut out, &mut Report::default())
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("assemblies are not supported yet in Test output; convert each part"));
    assert!(out.is_empty());
}
#[test]
fn ambiguous_source_names_have_no_source_link() {
    let (m, mut mapping) = mapped_rows("pad");
    let mut duplicate = mapping.outcomes[0].clone();
    duplicate.ordinal = Some(50);
    mapping.outcomes.push(duplicate);
    let plan = Plan::from_mapped(&m, mapping);
    assert_eq!(plan.steps[0].source, SourceLink::default());
}
#[test]
fn quantities_bound_grouping_and_keep_negated_parameter_links() {
    let params = BTreeMap::from([("width".into(), "2 mm".into())]);
    assert_eq!(
        Quantity::from_row("-(width)", &params),
        Quantity {
            value: Some(-2.0),
            expr: "-(width)".into(),
            param: Some("width".into())
        }
    );
    assert_eq!(Quantity::from_row("-(2 cm)", &params).value, Some(-20.0));
    let grouped = format!("{}1{}", "(".repeat(100), ")".repeat(100));
    assert_eq!(Quantity::from_row(&grouped, &params).value, None);
}
#[test]
fn pattern_counts_and_row_sizes_have_explicit_limits() {
    let (_, mapping) = mapped_rows("pad");
    let sketch = mapping.sketches[0].clone();
    let pattern = raw(
        "pattern",
        json!({"features":["Profile"],"pattern":{"type":"rectangular","dir1":[1.0,0.0,0.0],"count1":"10001","spacing1":"2 mm"}}),
    );
    assert!(
        row::from_row(&pattern, &BTreeMap::new(), &[], &[sketch, pattern.clone()])
            .unwrap_err()
            .contains("pattern count exceeds the script limit")
    );
    let huge = raw("unknown", json!({"text":"x".repeat(65537)}));
    assert!(row::from_row(&huge, &BTreeMap::new(), &[], &[]).is_ok());
}

#[test]
fn unknown_sketch_options_are_retained_as_data() {
    let (_, mapping) = mapped_rows("pad");
    let mut sketch = mapping.sketches[0].clone();
    sketch["sketch"]["annotation"] = json!({"expr":"text","value":17});
    sketch["sketch"]["points"][1]["display"] = json!({"expr":"text","value":18});
    sketch["sketch"]["curves"][0]["display"] = json!({"expr":"text","value":19});
    check_rows(&[sketch]);
}

#[test]
fn emitter_refusal_blocks_a_suffix_with_repeated_source_names() {
    let mut plan = Plan::from_model(&model("pad"), &mut Report::default());
    let mut later = plan.steps[1].clone();
    later.id = 3;
    later.name = "Solid 2".into();
    later.kind = "revolve".into();
    later.source = SourceLink::default();
    plan.steps.push(later);
    plan.outcomes.push(plan::Outcome {
        name: Some("Solid".into()),
        kind: "revolve".into(),
        status: OutcomeStatus::Mapped,
        reason: None,
    });
    assert_eq!(
        prefix::supported_prefix(&TestEmitter { refuse: "revolve" }, &mut plan),
        2
    );
    assert_eq!(plan.outcomes[1].status, "mapped");
    assert_eq!(plan.outcomes[2].status, "blocked");
}

#[test]
fn mapped_timeline_stops_before_a_refused_revolve() {
    let mut m = model("pad");
    let mut wire = serde_json::to_value(&m.ir).unwrap();
    let turn = serde_json::to_value(&model("revolve").ir).unwrap()["model"]["features"][1].clone();
    let mut turn = turn;
    turn["id"] = json!("synthetic:test:feature#3");
    turn["ordinal"] = json!(2);
    turn["name"] = json!("Turn");
    let mut later = wire["model"]["features"][1].clone();
    later["id"] = json!("synthetic:test:feature#4");
    later["ordinal"] = json!(3);
    later["name"] = json!("Later pad");
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .extend([turn, later]);
    m.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let mut plan = Plan::from_model(&m, &mut Report::default());
    assert_eq!(
        plan.steps
            .iter()
            .map(|s| s.kind.as_str())
            .collect::<Vec<_>>(),
        ["sketch", "extrude", "revolve", "extrude"]
    );
    assert_eq!(
        prefix::supported_prefix(&TestEmitter { refuse: "revolve" }, &mut plan),
        2
    );
    assert_eq!(plan.stop.unwrap().feature, "Turn");
    assert_eq!(plan.outcomes[3].status, "blocked");
}

#[test]
fn source_metadata_uses_the_format_word_and_explicit_nulls() {
    let mut m = model("pad");
    m.source_file = Some("Example.IPT".into());
    let plan = Plan::from_model(&m, &mut Report::default());
    assert_eq!(plan.source.format, "inventor_part");
    assert_eq!(plan.source.program_version, None);
    assert_eq!(plan.source.sha256, None);
    assert_eq!(plan.expected, None);
    m.source_file = None;
    m.ir.source = None;
    let plan = Plan::from_model(&m, &mut Report::default());
    assert_eq!(plan.source.format, "cadir");
}

#[test]
fn parameter_kind_recognizes_units_attached_to_a_number() {
    let mut m = model("pad");
    let mut wire = serde_json::to_value(&m.ir).unwrap();
    wire["model"]["parameters"] = json!([{"id":"synthetic:test:parameter#1","ordinal":0,"name":"width with spaces","expression":"2cm","dependencies":[],"properties":{}}]);
    m.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let plan = Plan::from_model(&m, &mut Report::default());
    assert_eq!(plan.parameters[0].name, "width_with_spaces");
    assert_eq!(plan.parameters[0].kind, "length");
    assert_eq!(plan.parameters[0].value, Some(20.0));
    assert_eq!(plan.parameters[0].source, None);
    assert_eq!(
        Quantity::from_row(&plan.parameters[0].expr, &BTreeMap::new()).value,
        Some(20.0)
    );
}

#[test]
fn unknown_keys_do_not_collide_with_typed_field_aliases() {
    let (_, mapping) = mapped_rows("pad");
    let mut rows = mapping.features.unwrap();
    rows[1]["profile"] = json!({"type":"curves","loops":[["unknown"]]});
    rows[1]["bodies"] = json!(["unhandled scope"]);
    rows[1]["plane"] = json!({"type":"custom","plane":{"origin":[1,2,3]}});
    rows[1]["sections"] = json!([{"profiles":{"type":"curves","loops":[["unknown"]]}}]);
    check_rows(&rows);
    let step = row::from_row(&rows[1], &BTreeMap::new(), &[], &rows).unwrap();
    assert_eq!(step.extra["profile"], rows[1]["profile"]);
    assert_eq!(step.extra["plane"], rows[1]["plane"]);
    assert!(TestEmitter { refuse: "" }
        .supports(
            &step,
            &Plan::from_model(&model("pad"), &mut Report::default())
        )
        .is_err());
}

#[test]
fn result_records_require_json_objects() {
    for field in ["measured", "original"] {
        let mut bad = good_result();
        bad[field] = json!([1, 1.0, 6.0, [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]]);
        assert!(ScriptResult::read(bad.to_string().as_bytes()).is_err());
    }
    let mut bad = good_result();
    bad["failed"] = json!([22, "EXTRUDE", "extrude", "The feature could not be built"]);
    assert!(ScriptResult::read(bad.to_string().as_bytes()).is_err());
}

#[test]
fn result_nesting_is_bounded_even_for_unknown_fields() {
    let mut nested = Value::Null;
    for _ in 0..65 {
        nested = json!({"next":nested});
    }
    let mut result = good_result();
    result["future"] = nested;
    assert!(ScriptResult::read(result.to_string().as_bytes()).is_err());
}

#[test]
fn revolve_axis_metadata_round_trips_without_changing_the_frame() {
    let (_, mapping) = mapped_rows("pad");
    let sketch = mapping.sketches[0].clone();
    let row = raw(
        "revolve",
        json!({"sketch":1,"profiles":{"type":"all"},"axis":{"type":"line","origin":[0.0,0.0,0.0],"dir":[0.0,1.0,0.0],"annotation":{"text":"kept"}},"angle":"270 deg","operation":"new_body","convention":"sketch_normal"}),
    );
    check_rows(&[sketch, row]);
}

#[test]
fn mapper_builders_round_trip_their_additional_shapes() {
    let (_, mapping) = mapped_rows("pad");
    let mut sketch = mapping.sketches[0].clone();
    // history/sketch.rs:411 (offset support), :899 (conic); constraints.rs:381, :850.
    sketch["plane"] =
        json!({"type":"offset","base":{"type":"origin","name":"XY"},"distance":"5 mm"});
    sketch["sketch"]["points"] = json!([
        {"id":"origin","pos":[0.0,0.0],"fixed":true},
        {"id":"p1","pos":[1.0,0.0],"fixed":false},
        {"id":"p2","pos":[0.0,1.0],"fixed":false}]);
    sketch["sketch"]["curves"] = json!([
        {"id":"l3","type":"line","a":1,"b":2,"construction":true},
        {"id":"q1","type":"conic","a":1,"b":2,"apex":0,"rho":0.5,"construction":false},
        {"id":"a1","type":"arc","a":1,"b":2,"c":0,"construction":false}]);
    sketch["sketch"]["constraints"] = json!([
        {"id":"k1","type":"pattern_point","p":1,"q":2,"center":0,"angle":std::f64::consts::FRAC_PI_2,"offset":[0.0,0.0],"directions":[null,null]},
        {"id":"k2","type":"arc_sweep","c":2,"value":std::f64::consts::FRAC_PI_2}]);
    let mut rows = vec![sketch];
    // history/revolve.rs:88, :266; profile.rs:747, :756.
    rows.push(raw("revolve", json!({"sketch":1,"profiles":{"type":"points","points":[[0.2,0.3]]},"axis":{"type":"sketch_line","curve":"l3"},"angle":"270 deg","operation":"new_body","convention":"sketch_normal","symmetric":true,"body_names":["Turn"]})));
    // history/extrude.rs:106, :143, :307-356, :427-438.
    for extent in [
        json!({"direction":"positive","distance":"5 mm","start_offset":"2 mm"}),
        json!({"direction":"negative","distance":"5 mm","taper":"3 deg"}),
        json!({"direction":"positive","distance":"0 mm","to":[1.0,2.0,3.0],"to_offset":"1 mm"}),
        json!({"direction":"positive","distance":"5 mm","from":[1.0,2.0,3.0],"start_offset":"1 mm"}),
        json!({"direction":"symmetric","distance":"5 mm","convention":"total_distance"}),
        json!({"direction":"positive","distance":"0 mm","through_all":true}),
    ] {
        rows.push(raw("extrude", json!({"sketch":1,"profiles":{"type":"all"},"extent":extent,"operation":"cut","body_names":["Cut"]})));
    }
    // history/pattern.rs:377, :442, :516, :604. Composites become staged rows or rectangular layouts.
    for layout in [
        json!({"type":"path","path_sketch":1,"path":["l3"],"count":"3","spacing":"2 mm"}),
        json!({"type":"rectangular","dir1":[1.0,0.0,0.0],"count1":"3","spacing1":"2 mm","dir2":[0.0,1.0,0.0],"count2":"2","spacing2":"3 mm"}),
        json!({"type":"circular","origin":[0.0,0.0,0.0],"axis":[0.0,0.0,1.0],"count":"3","angle":"180 deg"}),
    ] {
        rows.push(raw(
            "pattern",
            json!({"features":["Profile"],"pattern":layout,"bodies":["Solid"]}),
        ));
    }
    // history/loft.rs:175-191 (generated apex sketch and section), :217-233 (rails/surface).
    let mut apex = raw(
        "sketch",
        json!({"plane":{"type":"custom","plane":{"origin":[0.0,0.0,5.0],"x_dir":[1.0,0.0,0.0],"y_dir":[0.0,1.0,0.0]}},"sketch":{"points":[{"id":"origin","pos":[0.0,0.0],"fixed":true}],"curves":[],"constraints":[]}}),
    );
    apex["id"] = json!(3);
    apex["name"] = json!("Loft apex 3");
    rows.push(apex);
    rows.push(raw("loft",json!({"sections":[{"sketch":1,"profiles":{"type":"all"}},{"sketch":3,"point":"origin","profiles":{"type":"all"}}],"operation":"new_body","body_names":["Loft"]})));
    rows.push(raw("surface_loft",json!({"sections":[{"sketch":1,"profiles":{"type":"all"}},{"sketch":1,"profiles":{"type":"all"}}],"rails":[{"sketch":1,"curves":["l3"]}],"body_names":["Sheet"]})));
    // history/sweep.rs:71-180: orientation, scale, twist, rails and surface variants.
    for (key, value) in [
        ("orientation", json!("parallel")),
        ("orientation", json!({"direction":[0.0,0.0,1.0]})),
        ("scale", json!("2")),
        ("twist", json!("20 deg")),
        ("rails", json!([{"sketch":1,"curves":["l3"]}])),
    ] {
        let mut row = raw(
            "sweep",
            json!({"sketch":1,"profiles":{"type":"all"},"path_sketch":1,"path":["l3"],"operation":"join","body_names":["Solid"]}),
        );
        row[key] = value;
        rows.push(row);
    }
    rows.push(raw("surface_sweep",json!({"sketch":1,"profiles":{"type":"all"},"path_sketch":1,"path":["l3"],"body_names":["Sheet"]})));
    // history/primitive.rs:182-199, :290, :353, :377, :393-403.
    rows.push(raw("coil",json!({"base":[0.0,0.0,0.0],"axis":[0.0,0.0,1.0],"diameter":"10 mm","turns":"3","pitch":"2 mm","section":"square","section_size":"1 mm","start_angle":"30 deg","position":0.5,"clockwise":true,"operation":"new_body","body_names":["Spring"]})));
    rows.push(raw("box",json!({"corner":[0.0,0.0,0.0],"length":"3 mm","width":"2 mm","height":"1 mm","operation":"new_body"})));
    rows.push(raw("cylinder",json!({"base":[0.0,0.0,0.0],"axis":[0.0,0.0,1.0],"radius":"2 mm","height":"5 mm","operation":"new_body"})));
    rows.push(raw("cone",json!({"base":[0.0,0.0,0.0],"axis":[0.0,0.0,1.0],"radius":"2 mm","top_radius":"1 mm","height":"5 mm","operation":"new_body"})));
    check_rows(&rows);
    let step = row::from_row(&rows[0], &BTreeMap::new(), &[], &rows).unwrap();
    assert_eq!(
        step.fields.curves.as_ref().unwrap()[1].apex.as_deref(),
        Some("origin")
    );
    let constraints = step.fields.constraints.unwrap();
    assert_eq!(constraints[0].center.as_deref(), Some("origin"));
    let angle = constraints[1].value.as_ref().unwrap();
    assert_eq!(angle.value, Some(90.0));
    assert!(angle.expr.ends_with(" rad"));
}

#[test]
fn all_document_goldens_round_trip() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dutchman-solidcraft/tests/golden");
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.to_string_lossy().ends_with(".document.json") {
            let document: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            if let Some(rows) = document["features"].as_array() {
                check_rows(rows);
            }
        }
    }
}

#[test]
fn nested_options_cannot_bypass_adapter_support() {
    let (m, mut mapping) = mapped_rows("pad");
    mapping.features.as_mut().unwrap()[1]["extent"]["start_offset"] = json!("2 mm");
    let mut plan = Plan::from_mapped(&m, mapping);
    assert_eq!(
        prefix::supported_prefix(&TestEmitter { refuse: "" }, &mut plan),
        1
    );
    assert_eq!(
        plan.stop.as_ref().unwrap().reason,
        "feature Solid: option extent.start_offset is not supported yet"
    );
    for (path, value) in [
        ("form", json!({"type":"simple","unknown":1})),
        ("layout", json!({"type":"path","path_sketch":1})),
        ("sections", json!([{ "unknown":1}])),
        (
            "points",
            json!([{ "id":"p1", "at":[0.0,0.0], "fixed":false, "unknown":1 }]),
        ),
        ("curves", json!([{ "type":"line", "unknown":1 }])),
        ("constraints", json!([{ "type":"fix", "unknown":1 }])),
    ] {
        let mut step = plan.steps[1].clone();
        step.fields = serde_json::from_value(json!({path:value})).unwrap();
        assert!(
            prefix::check_support(&TestEmitter { refuse: "" }, &step, &plan)
                .unwrap_err()
                .contains(path)
        );
    }
}

#[test]
fn tsv_text_encoding_is_reversible_and_spreadsheet_safe() {
    fn decode(cell: &str) -> String {
        let mut bytes = Vec::new();
        let mut chars = cell.bytes();
        while let Some(b) = chars.next() {
            if b == b'%' {
                let hex = [chars.next().unwrap(), chars.next().unwrap()];
                bytes.push(u8::from_str_radix(std::str::from_utf8(&hex).unwrap(), 16).unwrap());
            } else {
                bytes.push(b);
            }
        }
        String::from_utf8(bytes).unwrap()
    }
    let mut corpus = vec![
        String::new(),
        "-".into(),
        "=+@-".into(),
        "日本語ü".into(),
        "\t\r\n%".into(),
    ];
    corpus.extend((0..=127u8).map(|b| char::from(b).to_string()));
    for a in &corpus {
        for b in &corpus {
            let text = format!("{a}{b}");
            let encoded = tsv::encode(&text);
            assert_eq!(decode(&encoded), text);
            assert!(!encoded.starts_with(['=', '+', '-', '@']));
            assert_ne!(encoded, "-");
        }
    }
    let mut t = tsv::Tsv::new("test");
    assert_eq!(
        t.push("bad", &[]).unwrap_err(),
        "the step operation needs an uppercase identifier"
    );
    t.push("DATA", &[tsv::Arg::Text("-"), tsv::Arg::None])
        .unwrap();
    assert!(t.as_str().ends_with("%2D\t-\r\n"));
}

#[test]
fn package_digest_is_available_to_lowering_and_the_dispatcher() {
    let mut out = Vec::new();
    ScriptWriter::new(TestEmitter { refuse: "" })
        .write(&model("pad"), &mut out, &mut Report::default())
        .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(out)).unwrap();
    let mut plan = Vec::new();
    zip.by_name("plan.json")
        .unwrap()
        .read_to_end(&mut plan)
        .unwrap();
    let mut tsv = String::new();
    zip.by_name("steps.tsv")
        .unwrap()
        .read_to_string(&mut tsv)
        .unwrap();
    assert!(tsv.starts_with(&format!(
        "dutchman-plan-sha256/1\t{}\r\n",
        package::sha256(&plan)
    )));
}

#[test]
fn reference_failures_have_outcome_reasons() {
    let (m, mut mapping) = mapped_rows("partial");
    mapping.sketches[0]["sketch"]["curves"][0]["c"] = json!(999);
    let mut plan = Plan::from_mapped(&m, mapping);
    assert!(plan.sketches.is_empty());
    assert_eq!(plan.outcomes[0].status, "blocked");
    assert_eq!(plan.outcomes[1].status, OutcomeStatus::Mapped);
    assert_eq!(
        prefix::supported_prefix(&TestEmitter { refuse: "" }, &mut plan),
        2
    );
    assert_eq!(plan.outcomes[1].status, OutcomeStatus::Mapped);
    assert!(plan.outcomes[0]
        .reason
        .as_ref()
        .unwrap()
        .starts_with("feature Profile:"));
}

#[test]
fn reverse_conversion_rejects_missing_ids_and_preserves_implicit_origin() {
    let mut plan = Plan::from_model(&model("pad"), &mut Report::default());
    plan.steps[0].fields.curves.as_mut().unwrap()[0].c = Some("missing".into());
    assert!(plan.steps[0]
        .to_row(&plan)
        .unwrap_err()
        .starts_with("feature Profile:"));
    let (_, mapping) = mapped_rows("pattern");
    let mut plan = Plan::from_mapped(&model("pattern"), mapping);
    plan.steps[2].fields.features = Some(vec![999]);
    assert!(plan.steps[2].to_row(&plan).is_err());
    let plan: Plan = serde_json::from_str(include_str!("../tests/golden/plate.plan.json")).unwrap();
    let row = plan.steps[0].to_row(&plan).unwrap();
    let mut step = row::from_row(&row, &BTreeMap::new(), &[], &[]).unwrap();
    step.source = plan.steps[0].source.clone();
    assert_eq!(step, plan.steps[0]);
}

#[test]
fn string_constraint_values_do_not_acquire_quotes() {
    let (_, mapping) = mapped_rows("pad");
    let mut row = mapping.sketches[0].clone();
    row["sketch"]["constraints"] = json!([{ "id":"k1","type":"radius","c":0,"value":"width" }]);
    let step = row::from_row(&row, &BTreeMap::new(), &[], &[]).unwrap();
    assert_eq!(
        step.fields.constraints.unwrap()[0]
            .value
            .as_ref()
            .unwrap()
            .expr,
        "width"
    );
    check_rows(&[row]);
}

#[test]
fn result_rejects_unsafe_text_bad_hashes_and_negative_measurements() {
    for text in [
        "\n", "\u{007f}", "\u{061c}", "\u{200e}", "\u{202e}", "\u{2069}",
    ] {
        let mut bad = good_result();
        bad["failed"] = json!({"line":1,"op":"X","step_kind":"X","message":text});
        assert!(ScriptResult::read(bad.to_string().as_bytes()).is_err());
    }
    for hash in ["a".repeat(63), "A".repeat(64), "g".repeat(64)] {
        let mut bad = good_result();
        bad["plan_sha256"] = json!(hash);
        assert!(ScriptResult::read(bad.to_string().as_bytes()).is_err());
    }
    for field in ["volume_mm3", "area_mm2"] {
        let mut bad = good_result();
        bad["measured"][field] = json!(-1);
        assert!(ScriptResult::read(bad.to_string().as_bytes()).is_err());
    }
    for (field, text) in [
        ("kind", "patch"),
        ("verdict", "refused"),
        ("check", "fingerprint"),
    ] {
        let mut bad = good_result();
        bad[field] = json!(text);
        assert!(ScriptResult::read(bad.to_string().as_bytes()).is_err());
    }
}

#[test]
fn quantity_work_is_bounded_and_signed_literals_and_parameters_resolve() {
    let mut params = BTreeMap::from([("x".into(), "5 mm".into())]);
    assert_eq!(Quantity::from_row("- 5 mm", &params).value, Some(-5.0));
    assert_eq!(
        Quantity::from_row("-(-x)", &params).param.as_deref(),
        Some("x")
    );
    assert_eq!(Quantity::from_row("-(-x)", &params).value, Some(5.0));
    for i in 0..40 {
        params.insert(
            format!("p{i}"),
            if i == 0 {
                "1".into()
            } else {
                format!("p{} / p{}", i - 1, i - 1)
            },
        );
    }
    assert_eq!(Quantity::from_row("p39", &params).value, None);
}

#[test]
fn renamed_parameters_use_written_names_and_resolve_links() {
    let mut m = model("pad");
    let mut wire = serde_json::to_value(&m.ir).unwrap();
    wire["model"]["parameters"] = json!([
        {"id":"synthetic:test:parameter#1","ordinal":0,"name":"D1@Sketch1","expression":"5 mm","value":{"kind":"length","value":5.0},"dependencies":[],"properties":{}},
        {"id":"synthetic:test:parameter#2","ordinal":1,"name":"has spaces","expression":"6 mm","value":{"kind":"length","value":6.0},"dependencies":[],"properties":{}},
        {"id":"synthetic:test:parameter#3","ordinal":2,"name":"mm","expression":"7","value":{"kind":"integer","value":7},"dependencies":[],"properties":{}},
        {"id":"synthetic:test:parameter#4","ordinal":3,"name":"1st","expression":"8 deg","value":{"kind":"angle","value":std::f64::consts::PI},"dependencies":[],"properties":{}}
    ]);
    m.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let mut mapping = dutchman_solidcraft::mapped_history(&m, &mut Report::default());
    assert_eq!(
        mapping
            .parameter_values
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        ["D1_Sketch1", "has_spaces", "p_1st", "p_mm"]
    );
    mapping.features.as_mut().unwrap()[1]["extent"]["distance"] = json!("D1_Sketch1");
    let plan = Plan::from_mapped(&m, mapping);
    assert!(plan.stop.is_none());
    for (name, value, kind) in [
        ("D1_Sketch1", 5.0, "length"),
        ("has_spaces", 6.0, "length"),
        ("p_mm", 7.0, "number"),
        ("p_1st", 180.0, "angle"),
    ] {
        let p = plan.parameters.iter().find(|p| p.name == name).unwrap();
        assert_eq!(
            (p.source.as_deref(), p.value, &*p.kind),
            (None, Some(value), kind)
        );
    }
    assert_eq!(
        plan.steps[1]
            .fields
            .extent
            .as_ref()
            .unwrap()
            .distance
            .as_ref()
            .unwrap()
            .param
            .as_deref(),
        Some("D1_Sketch1")
    );
}

#[test]
fn all_constraint_row_shapes_and_curve_options_round_trip() {
    // constraints.rs:185-230, :355-382, :504-516, :560-585, :656, :826, :850,
    // :944-974, :1183, :1210-1247; sketch.rs:899, :1614-1618.
    let (_, mapping) = mapped_rows("pad");
    let mut row = mapping.sketches[0].clone();
    row["sketch"]["points"] = json!([
        {"id":"origin","pos":[0.0,0.0],"fixed":true},
        {"id":"p1","pos":[1.0,0.0],"fixed":false},
        {"id":"p2","pos":[0.0,1.0],"fixed":false}]);
    row["sketch"]["curves"] = json!([
        {"id":"l1","type":"line","a":1,"b":2,"construction":false,"fixed":true},
        {"id":"l2","type":"line","a":0,"b":1,"construction":true},
        {"id":"a1","type":"arc","a":1,"b":2,"c":0,"construction":false}]);
    let mut constraints = Vec::new();
    for kind in ["horizontal", "vertical"] {
        constraints.push(json!({"type":kind,"l":0}));
    }
    for kind in [
        "parallel",
        "perpendicular",
        "tangent",
        "smooth",
        "equal",
        "concentric",
        "collinear",
        "pattern_radius",
    ] {
        constraints.push(json!({"type":kind,"a":0,"b":1}));
    }
    for kind in ["coincident", "horizontal_points", "vertical_points"] {
        constraints.push(json!({"type":kind,"p":1,"q":2}));
    }
    for kind in ["distance", "distance_x", "distance_y"] {
        constraints
            .push(json!({"type":kind,"p":1,"q":2,"value":1.0,"param":"width","driven":true}));
    }
    for kind in ["radius", "diameter", "arc_sweep"] {
        constraints.push(json!({"type":kind,"c":2,"value":1.0}));
    }
    constraints.extend([
        json!({"type":"fix","p":1}),
        json!({"type":"midpoint","p":1,"l":0}),
        json!({"type":"symmetric","p":1,"q":2,"l":0}),
        json!({"type":"point_on_curve","p":1,"c":0}),
        json!({"type":"length","l":0,"value":1.0}),
        json!({"type":"point_line_distance","p":1,"l":0,"value":1.0}),
        json!({"type":"line_angle","l":0,"value":1.0}),
        json!({"type":"angle","a":0,"b":1,"value":1.0,"flip":true}),
    ]);
    for (index, c) in constraints.iter_mut().enumerate() {
        c["id"] = json!(format!("k{}", index + 1));
    }
    row["sketch"]["constraints"] = json!(constraints);
    check_rows(&[row]);
    let sketch: Value = serde_json::from_str(include_str!(
        "../../dutchman-solidcraft/tests/golden/design_sketch_constraints.sketch.json"
    ))
    .unwrap();
    check_rows(&[sketch]);
}

#[test]
fn malformed_rows_name_the_feature_without_serde_diagnostics() {
    for fields in [
        json!({"axis":{}}),
        json!({"axis":{"curve":12}}),
        json!({"sketch":"wrong"}),
    ] {
        let r = raw("revolve", fields);
        let err = row::from_row(&r, &BTreeMap::new(), &[], &[]).unwrap_err();
        assert!(err.starts_with("feature Feature:"));
        assert!(!err.contains("serde"));
        assert!(!err.contains("variant"));
    }
    let mut r = raw("extrude", json!({}));
    r.as_object_mut().unwrap().remove("id");
    assert_eq!(
        row::from_row(&r, &BTreeMap::new(), &[], &[]).unwrap_err(),
        "feature Feature: a mapped row has no id"
    );
    let pattern = raw(
        "pattern",
        json!({"features":["Missing"],"pattern":{"type":"rectangular"}}),
    );
    let missing = json!({"name":"Missing","type":"extrude"});
    assert!(row::from_row(&pattern, &BTreeMap::new(), &[], &[missing])
        .unwrap_err()
        .contains("id is missing"));
}

struct FreeCadEmitter;
impl Emitter for FreeCadEmitter {
    fn target(&self) -> &'static str {
        "freecad"
    }
    fn program(&self) -> &'static str {
        "FreeCAD"
    }
    fn format(&self) -> &'static str {
        "test/1"
    }
    fn supports(&self, step: &Step, plan: &Plan) -> Result<(), String> {
        if plan
            .steps
            .iter()
            .take_while(|s| s.id != step.id)
            .any(|s| s.name == step.name)
        {
            return Err(format!(
                "feature {:?}: duplicate feature names are not supported yet",
                step.name
            ));
        }
        generate::history(&[step.to_row(plan)?]).map(|_| ())
    }
    fn handled_options(&self, _: &str) -> &'static [&'static str] {
        &[
            "convention",
            "orientation",
            "scale",
            "rails",
            "twist",
            "operation",
            "extent.start_offset",
            "extent.convention",
            "extent.from",
            "extent.to",
            "extent.to_offset",
            "extent.taper",
            "layout.path_sketch",
            "layout.path",
            "layout.spacing",
            "curves.rho",
            "curves.fixed",
            "constraints.angle",
            "constraints.offset",
            "constraints.directions",
            "thread",
            "position",
            "to",
            "style.type",
        ]
    }
    fn lower(&self, _: &Plan, _: usize, _: &str) -> Result<Lowered, String> {
        Ok(Lowered::new(String::new()))
    }
    fn runtime(&self) -> (&'static str, &'static str) {
        ("", "runtime.txt")
    }
    fn instructions(&self, _: &Plan) -> String {
        String::new()
    }
}

#[test]
fn prefix_matches_freecad_outcomes_reasons_modes_and_notes() {
    use dutchman_core::FeatureStatus;
    fn compare(m: &Model, mapping: dutchman_solidcraft::MappedHistory) {
        let mut expected = mapping.outcomes.clone();
        let full = mapping
            .features
            .as_ref()
            .is_ok_and(|rows| generate::history(rows).is_ok())
            && mapping
                .sketches
                .iter()
                .all(|row| generate::history(std::slice::from_ref(row)).is_ok());
        let mut reference_report = Report::default();
        let expected_mode = if full {
            Mode::History
        } else if let Some(p) = freecad_prefix::recover(
            m,
            mapping.features.clone().ok(),
            mapping.sketches.clone(),
            &mut expected,
        ) {
            p.write(&mut String::new(), &m.name, &mut reference_report)
                .unwrap();
            Mode::PartialHistory
        } else {
            Mode::Import
        };
        let mut plan = Plan::from_mapped(m, mapping);
        let upto = prefix::supported_prefix(&FreeCadEmitter, &mut plan);
        let actual: Vec<_> = plan
            .outcomes
            .iter()
            .map(|p| (p.status, p.reason.clone()))
            .collect();
        let wanted: Vec<_> = expected
            .iter()
            .map(|p| {
                (
                    match p.status {
                        FeatureStatus::Mapped => OutcomeStatus::Mapped,
                        FeatureStatus::Blocked => OutcomeStatus::Blocked,
                        FeatureStatus::NotNeeded => OutcomeStatus::NotNeeded,
                        FeatureStatus::LeftOut => OutcomeStatus::LeftOut,
                    },
                    p.reason.clone(),
                )
            })
            .collect();
        assert_eq!(actual, wanted);
        let mut report = Report::default();
        emitter::write_report(m, &plan, upto, expected, "FreeCAD", &mut report);
        assert_eq!(report.outcome().unwrap().mode, expected_mode);
        let notes = |r: &Report| {
            r.losses()
                .iter()
                .map(|n| (n.severity, n.subject.clone(), n.detail.clone()))
                .collect::<Vec<_>>()
        };
        assert_eq!(notes(&report), notes(&reference_report));
    }
    let m = model("partial");
    compare(
        &m,
        dutchman_solidcraft::mapped_history(&m, &mut Report::default()),
    );
    let m = model("pad");
    let mut mapping = dutchman_solidcraft::mapped_history(&m, &mut Report::default());
    mapping.features = Err("mapping stopped".into());
    compare(&m, mapping);
    let mut mapping = dutchman_solidcraft::mapped_history(&m, &mut Report::default());
    mapping.features.as_mut().unwrap()[1]["name"] = json!("Profile");
    compare(&m, mapping);
    let mut mapping = dutchman_solidcraft::mapped_history(&m, &mut Report::default());
    mapping.features.as_mut().unwrap()[1] = raw(
        "import",
        json!({"file":"Base.step","step":"","_step":"x".repeat(65_537)}),
    );
    mapping.outcomes[1].kind = "import".into();
    compare(&m, mapping);
    // Read the same row data embedded in every existing synthetic macro golden.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../dutchman-freecad/tests/golden");
    let mut macros = 0;
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("FCMacro") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let kind = path.file_stem().unwrap().to_str().unwrap();
        let source_model = model(kind);
        let mut mapped = dutchman_solidcraft::mapped_history(&source_model, &mut Report::default());
        let mut rows = Vec::new();
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("selected = partial_history(json.loads(") {
                let encoded = serde_json::Deserializer::from_str(data)
                    .into_iter::<String>()
                    .next()
                    .unwrap()
                    .unwrap();
                rows = serde_json::from_str(&encoded).unwrap();
            } else if let Some(data) = line.trim_start().strip_prefix("tip = rebuild(json.loads(") {
                let encoded = serde_json::Deserializer::from_str(data)
                    .into_iter::<String>()
                    .next()
                    .unwrap()
                    .unwrap();
                rows.push(serde_json::from_str(&encoded).unwrap());
            }
        }
        macros += 1;
        assert!(!rows.is_empty(), "{kind}");
        check_rows(&rows);
        mapped.features = Ok(rows);
        compare(&source_model, mapped);
    }
    assert_eq!(macros, 4);
    // A separately retained reference sketch refused by the same support implementation.
    let mut mapping = dutchman_solidcraft::mapped_history(&m, &mut Report::default());
    mapping.sketches[0]["operation"] = json!("intersect");
    mapping.features.as_mut().unwrap()[1]["operation"] = json!("intersect");
    compare(&m, mapping);
    // Unretained sketches stay mapped; suppressed and not-needed outcomes do not count.
    let mut mapping = dutchman_solidcraft::mapped_history(&m, &mut Report::default());
    mapping.features = Ok(vec![]);
    compare(&m, mapping);
    assert!(!std::hint::black_box(generate::PRELUDE).is_empty());
    assert!(generate::history_bodies(&[]).is_ok());
}

struct ExtraEmitter {
    duplicate: bool,
}
impl Emitter for ExtraEmitter {
    fn target(&self) -> &'static str {
        "test"
    }
    fn program(&self) -> &'static str {
        "Test"
    }
    fn format(&self) -> &'static str {
        "test/1"
    }
    fn supports(&self, _: &Step, _: &Plan) -> Result<(), String> {
        Ok(())
    }
    fn handled_options(&self, _: &str) -> &'static [&'static str] {
        &[]
    }
    fn lower(&self, _: &Plan, _: usize, digest: &str) -> Result<Lowered, String> {
        let mut lowered = Lowered::new(tsv::Tsv::new("test").finish());
        lowered.extra_files.push((
            if self.duplicate {
                "plan.json"
            } else {
                "features.json"
            }
            .into(),
            digest.as_bytes().to_vec(),
        ));
        Ok(lowered)
    }
    fn runtime(&self) -> (&'static str, &'static str) {
        ("fixed runtime", "runtime.txt")
    }
    fn instructions(&self, _: &Plan) -> String {
        "Run the script.".into()
    }
}
#[test]
fn lowered_extra_files_are_written_and_validated_with_the_package() {
    let mut out = Vec::new();
    ScriptWriter::new(ExtraEmitter { duplicate: false })
        .write(&model("pad"), &mut out, &mut Report::default())
        .unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(out)).unwrap();
    assert_eq!(zip.len(), 6);
    let mut plan = Vec::new();
    zip.by_name("plan.json")
        .unwrap()
        .read_to_end(&mut plan)
        .unwrap();
    let mut features = String::new();
    zip.by_name("features.json")
        .unwrap()
        .read_to_string(&mut features)
        .unwrap();
    assert_eq!(features, package::sha256(&plan));
    let mut out = Vec::new();
    assert!(ScriptWriter::new(ExtraEmitter { duplicate: true })
        .write(&model("pad"), &mut out, &mut Report::default())
        .is_err());
    assert!(out.is_empty());
    for name in ["../x", "dir/x", "dir\\x", "C:x", ".", "..", "bad\nname"] {
        assert!(package::write_package(&mut Vec::new(), &[(name, b"x")]).is_err());
    }
}

#[test]
fn parameter_links_must_resolve_even_on_retained_reference_sketches() {
    let (m, mut mapping) = mapped_rows("pad");
    mapping.sketches[0]["sketch"]["constraints"] =
        json!([{ "id":"k1","type":"radius","c":0,"value":2.0,"param":"missing"}]);
    let plan = Plan::from_mapped(&m, mapping);
    assert!(plan.sketches.is_empty());
    assert_eq!(plan.outcomes[0].status, OutcomeStatus::Blocked);
    assert_eq!(
        plan.outcomes[0].reason.as_deref(),
        Some("feature Profile: a parameter reference was not found")
    );
}

#[test]
fn surface_revolve_spline_reference_line_and_hole_options_round_trip() {
    let (_, mapping) = mapped_rows("pad");
    let mut sketch = mapping.sketches[0].clone();
    // sketch.rs:847 (reference line), :940 (spline control flag), :790 (fixed curve).
    sketch["sketch"]["curves"].as_array_mut().unwrap().extend([
        json!({"id":"l1","type":"line","a":0,"b":1,"construction":true,"infinite":true,"fixed":true}),
        json!({"id":"s1","type":"spline","pts":[0,1,0],"control":true,"degree":2,"construction":false})]);
    // revolve.rs:88-112: surface variants omit operation and convention.
    let surface = raw(
        "surface_revolve",
        json!({"sketch":1,"profiles":{"type":"curves","loops":[["l1"]]},"axis":{"type":"sketch_line","curve":"l1"},"angle":"90 deg","symmetric":true,"body_names":["Surface"]}),
    );
    let mut rows = vec![sketch, surface];
    // hole.rs:205-226: point picks, depth, entry form, cosmetic thread and to-face termination.
    for form in [
        json!({"type":"simple"}),
        json!({"type":"drilled","tip_angle":"118"}),
        json!({"type":"counterbore","cb_diameter":"8 mm","cb_depth":"2 mm","tip_angle":"118"}),
        json!({"type":"countersink","cs_diameter":"8 mm","cs_angle":"90","tip_angle":"118"}),
    ] {
        let mut hole = raw(
            "hole",
            json!({"hole":form,"diameter":"5 mm","direction":[0.0,0.0,1.0],"position":[0.0,0.0,0.0],"depth":"6 mm","thread":"M6x1","to":[0.0,0.0,6.0]}),
        );
        rows.push(hole.clone());
        hole["points"] = json!({"sketch":1,"ids":["p1"]});
        rows.push(hole);
    }
    check_rows(&rows);
}

#[test]
fn datum_builders_keep_nonbuilding_outcomes_without_rows() {
    let mut m = model("pad");
    let mut wire = serde_json::to_value(&m.ir).unwrap();
    let frame = json!({"origin":{"x":0.0,"y":0.0,"z":0.0},"normal":{"x":0.0,"y":0.0,"z":1.0},"u_axis":{"x":1.0,"y":0.0,"z":0.0}});
    // datum.rs:34-68: datum builders record supports and emit no timeline row.
    for (index, (kind, value)) in [
        ("datum_principal_plane", json!({"plane":"top"})),
        ("datum_plane", json!({"frame":frame})),
        (
            "datum_offset_plane",
            json!({"distance":3.0,"reference":{"kind":"resolved_plane","frame":frame}}),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        wire["model"]["features"].as_array_mut().unwrap().push(json!({"id":format!("synthetic:test:feature#{}",index+3),"ordinal":index+2,"name":format!("Datum {index}"),"suppressed":false,"definition":{"kind":"operation","value":{"kind":kind,"value":value}},"dependencies":[],"source_properties":{},"source_content":[],"outputs":[]}));
    }
    m.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let plan = Plan::from_model(&m, &mut Report::default());
    assert!(plan.stop.is_none());
    assert_eq!(plan.steps.len(), 2);
    assert!(plan
        .outcomes
        .iter()
        .skip(2)
        .all(|o| o.status == OutcomeStatus::NotNeeded));
}

#[test]
fn reference_failure_names_the_correct_outcome_with_repeated_source_names() {
    let (m, mut mapping) = mapped_rows("pad");
    let mut duplicate = mapping.outcomes[0].clone();
    duplicate.ordinal = Some(99);
    mapping.outcomes.push(duplicate);
    let mut sketch = mapping.sketches[0].clone();
    sketch["id"] = json!(99);
    sketch["name"] = json!("Profile 2");
    sketch["sketch"]["curves"][0]["c"] = json!(999);
    mapping.sketches.push(sketch);
    let plan = Plan::from_mapped(&m, mapping);
    assert_eq!(plan.outcomes[0].status, OutcomeStatus::Mapped);
    assert_eq!(plan.outcomes[2].status, OutcomeStatus::Blocked);
    assert_eq!(
        plan.outcomes[2].reason.as_deref(),
        Some("feature Profile 2: a sketch reference is out of range")
    );
}

#[test]
fn ordered_composite_pattern_rows_preserve_feature_seed_references() {
    let (_, mapping) = mapped_rows("pad");
    let sketch = mapping.sketches[0].clone();
    // pattern.rs:184-236: each stage includes the original seed and all prior stage names.
    let mut first = raw(
        "pattern",
        json!({"features":["Profile"],"pattern":{"type":"rectangular","dir1":[1.0,0.0,0.0],"count1":"2","spacing1":"5 mm"}}),
    );
    first["name"] = json!("Copies stage 1");
    let mut second = raw(
        "mirror",
        json!({"features":["Profile","Copies stage 1"],"plane":{"type":"custom","plane":{"origin":[0.0,0.0,0.0],"x_dir":[1.0,0.0,0.0],"y_dir":[0.0,1.0,0.0]}}}),
    );
    second["id"] = json!(3);
    second["name"] = json!("Copies");
    check_rows(&[sketch, first, second]);
}

#[test]
fn duplicate_parameter_names_keep_written_identity_without_guessing_source() {
    let mut m = model("pad");
    let mut wire = serde_json::to_value(&m.ir).unwrap();
    wire["model"]["parameters"] = json!([
        {"id":"synthetic:test:parameter#1","ordinal":0,"name":"width","expression":"5 mm","value":{"kind":"length","value":5.0},"dependencies":[],"properties":{}},
        {"id":"synthetic:test:parameter#2","ordinal":1,"name":"width","expression":"6 mm","value":{"kind":"length","value":6.0},"dependencies":[],"properties":{}}
    ]);
    m.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let mapping = dutchman_solidcraft::mapped_history(&m, &mut Report::default());
    let plan = Plan::from_mapped(&m, mapping);
    assert_eq!(
        plan.parameters
            .iter()
            .map(|p| (&*p.name, p.source.as_deref(), p.value))
            .collect::<Vec<_>>(),
        [("width", None, Some(5.0)), ("width_2", None, Some(6.0))]
    );
}

#[test]
fn large_import_text_reaches_the_emitter_refusal() {
    // history/base.rs:149-157 embeds the restated STEP as a single string.
    let row = raw(
        "import",
        json!({"file":"Base.step","step":"","_step":"x".repeat(65_537)}),
    );
    check_rows(std::slice::from_ref(&row));
    let mut plan = Plan::from_model(&model("pad"), &mut Report::default());
    plan.steps =
        vec![row::from_row(&row, &BTreeMap::new(), &[], std::slice::from_ref(&row)).unwrap()];
    assert_eq!(prefix::supported_prefix(&FreeCadEmitter, &mut plan), 0);
    assert!(plan
        .stop
        .unwrap()
        .reason
        .contains("import features are not supported yet in FreeCAD output"));
}

#[test]
fn duplicate_names_are_the_emitters_policy() {
    let mut plan = Plan::from_model(&model("pad"), &mut Report::default());
    plan.steps[1].name = plan.steps[0].name.clone();
    plan.steps[1].extra.clear();
    assert_eq!(
        prefix::supported_prefix(&TestEmitter { refuse: "none" }, &mut plan),
        2
    );
    assert!(plan.stop.is_none());
    assert_eq!(prefix::supported_prefix(&FreeCadEmitter, &mut plan), 1);
    assert!(plan
        .stop
        .unwrap()
        .reason
        .contains("duplicate feature names"));
}

#[test]
fn numeric_constraint_text_remains_text() {
    let (_, mapping) = mapped_rows("pad");
    for value in [
        json!("5"),
        json!("1.25"),
        json!(" 5 "),
        json!("1e3"),
        json!("-0"),
        json!("true"),
        json!("null"),
        json!(5.0),
    ] {
        let mut sketch = mapping.sketches[0].clone();
        sketch["sketch"]["constraints"] =
            json!([{"id":"k1","type":"diameter","c":0,"value":value}]);
        check_rows(&[sketch]);
    }
}

#[test]
fn typed_records_refuse_unknown_keys() {
    let (_, mapping) = mapped_rows("pad");
    let sketch = mapping.sketches[0].clone();
    let rejects = |row: Value| {
        let err =
            row::from_row(&row, &BTreeMap::new(), &[], &[sketch.clone(), row.clone()]).unwrap_err();
        assert_eq!(
            err,
            format!(
                "feature {}: its mapped fields are not supported yet",
                row["name"].as_str().unwrap()
            )
        );
    };
    for plane in [
        json!({"type":"origin","name":"XY","unknown":true}),
        json!({"type":"offset","base":{"type":"origin","name":"XY"},"distance":"1 mm","unknown":true}),
        json!({"type":"custom","plane":{"origin":[0,0,0],"x_dir":[1,0,0],"y_dir":[0,1,0],"unknown":true}}),
        json!({"type":"custom","plane":{"origin":[0,0,0],"x_dir":[1,0,0],"y_dir":[0,1,0]},"unknown":true}),
    ] {
        let mut row = sketch.clone();
        row["plane"] = plane;
        rejects(row);
    }
    for profile in [
        json!({"type":"all","unknown":true}),
        json!({"type":"points","points":[[0,0]],"unknown":true}),
        json!({"type":"curves","loops":[["c1"]],"unknown":true}),
        json!({"type":"whole_boundaries","profiles":[],"unknown":true}),
        json!({"type":"whole_boundaries","profiles":[{"outer":["c1"],"holes":[],"unknown":true}]}),
    ] {
        rejects(raw(
            "extrude",
            json!({"sketch":1,"profiles":profile,"operation":"new_body"}),
        ));
    }
    rejects(raw(
        "revolve",
        json!({"axis":{"type":"line","origin":[0,0,0],"dir":[0,1,0],"curve":"wrong variant"}}),
    ));
    // EdgeRef and FaceRef are also public records used by plan readers.
    assert!(
        serde_json::from_value::<EdgeRef>(json!({"at":[0,0,0],"proof":"test","unknown":true}))
            .is_err()
    );
    assert!(serde_json::from_value::<FaceRef>(
        json!({"at":[0,0,0],"normal":[0,0,1],"proof":"test","unknown":true})
    )
    .is_err());
    assert!(serde_json::from_value::<Axis>(
        json!({"type":"line","origin":[0,0,0],"dir":[0,1,0],"unknown":true})
    )
    .is_err());
}

#[test]
fn mapper_emits_a_sketch_line_revolve_axis() {
    let mut m = model("revolve");
    let mut wire = serde_json::to_value(&m.ir).unwrap();
    wire["model"]["sketch_entities"].as_array_mut().unwrap().push(json!({
        "id":"synthetic:test:sketch-entity#2", "sketch":"synthetic:test:sketch#1", "construction":true,
        "geometry":{"kind":"line","start":{"u":0.0,"v":0.0},"end":{"u":0.0,"v":10.0}}, "endpoint_refs":[]
    }));
    wire["model"]["features"][1]["definition"]["value"]["value"]["construction"]["value"]["axis"]
        ["reference"] = json!({"kind":"sketch_curves","value":{"sketch":"synthetic:test:sketch#1","curves":["synthetic:test:sketch-entity#2"]}});
    m.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let mapping = dutchman_solidcraft::mapped_history(&m, &mut Report::default());
    let rows = mapping.features.as_ref().unwrap();
    assert_eq!(rows[1]["axis"]["type"], "sketch_line");
    check_rows(rows);
    let plan = Plan::from_mapped(&m, mapping);
    assert!(matches!(
        plan.steps[1].fields.axis,
        Some(Axis::SketchLine { .. })
    ));
    assert!(!plan.steps[1].extra.contains_key("axis"));
}

#[path = "test_models.rs"]
mod test_models;

#[test]
fn additional_builders_emit_rows_that_round_trip() {
    use test_models::*;
    let selection = |state: &str, face: &str| json!({"kind":"historical","value":{"state":state,"faces":[face],"native":"synthetic selection"}});
    let split = split_cube(10.0, true, None);
    let mirrored = split_cube(
        10.0,
        true,
        Some(
            json!({"kind":"pattern","value":{"seeds":[{"kind":"bodies","value":{"kind":"bodies","value":["synthetic:cube:body#0"]}}],"pattern":{"kind":"mirror","plane_origin":{"x":20.0,"y":0.0,"z":0.0},"plane_normal":{"x":1.0,"y":0.0,"z":0.0}}}}),
        ),
    );
    let cases = [
        (flanged_cube(1.0, "reverse"), "sheet_base"),
        (
            input_edge_flange(false, "inside", std::f64::consts::FRAC_PI_2, 2.0),
            "sheet_flange",
        ),
        (attached_plate(3.0, 2.0, false), "sheet_flange"),
        (face_delete(selection, true), "delete_face"),
        (
            face_move(json!({"kind":"offset","distance":2.5})),
            "offset_face",
        ),
        (combined_spheres("cut", false), "combine"),
        (split, "split"),
        (mirrored, "mirror"),
        (spatial_sweep_history(|_, _| {}), "sketch"),
    ];
    for (model, kind) in cases {
        let mapping = dutchman_solidcraft::mapped_history(&model, &mut Report::default());
        let rows = mapping.features.as_ref().unwrap();
        assert!(rows.iter().any(|r| r["type"] == kind), "{kind}: {rows:?}");
        check_rows(rows);
        check_rows(&mapping.sketches);
    }
    // base.rs:149; whole stored cube, with no later operations on its final geometry.
    let mut m = model("pad");
    let mut wire = serde_json::to_value(&m.ir).unwrap();
    let mut base = wire["model"]["features"][1].clone();
    base["ordinal"] = json!(0);
    base["definition"] = json!({"kind":"operation","value":{"kind":"base_feature","value":{"bodies":{"kind":"resolved","value":{"bodies":["synthetic:cube:body#0"],"native":"synthetic base"}}}}});
    wire["model"]["features"] = json!([base]);
    m.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let mapping = dutchman_solidcraft::mapped_history(&m, &mut Report::default());
    let rows = mapping.features.as_ref().unwrap();
    assert_eq!(rows[0]["type"], "import");
    check_rows(rows);
}

#[test]
fn mapper_emits_cosmetic_thread_rows() {
    // Synthetic cylindrical band follows history/thread.rs:182-200's local topology.
    let mut m = model("pad");
    let mut wire = serde_json::to_value(&m.ir).unwrap();
    let g = &mut wire["geometry"];
    let face = g["faces"][0]["id"].clone();
    let surface = g["surfaces"][0]["id"].clone();
    g["faces"][0]["surface"] = surface;
    g["faces"][0]["loops"]["loops"] = json!([g["loops"][0]["id"], g["loops"][1]["id"]]);
    g["surfaces"][0]["geometry"] = json!({"kind":"solved","value":{"kind":"cylinder","origin":{"x":0,"y":0,"z":0},"axis":{"x":0,"y":0,"z":1},"ref_direction":{"x":1,"y":0,"z":0},"radius":1.5}});
    for i in 0..2 {
        g["loops"][i]["boundary"]["coedges"] = json!([g["coedges"][i]["id"]]);
        g["coedges"][i]["edge"] = g["edges"][i]["id"].clone();
        g["edges"][i]["carrier"]["curve"] = g["curves"][i]["id"].clone();
        g["edges"][i]["carrier"]["param_range"] = json!([0, std::f64::consts::TAU]);
        g["curves"][i]["geometry"] = json!({"kind":"solved","value":{"kind":"circle","center":{"x":0,"y":0,"z":8*i},"axis":{"x":0,"y":0,"z":1},"radius":1.5,"ref_direction":{"x":1,"y":0,"z":0}}});
    }
    let mut feature = wire["model"]["features"][1].clone();
    feature["ordinal"] = json!(2);
    feature["id"] = json!("synthetic:test:feature#3");
    feature["name"] = json!("Thread");
    feature["definition"] = json!({"kind":"operation","value":{"kind":"cosmetic_thread","value":{"modeled":false,"face":{"kind":"faces","value":[face]},"diameter":3.0,"pitch":0.5,"extent":{"kind":"blind","length":6.0}}}});
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(feature);
    m.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    let mapping = dutchman_solidcraft::mapped_history(&m, &mut Report::default());
    let rows = mapping.features.as_ref().unwrap();
    assert_eq!(rows[2]["type"], "thread");
    assert_eq!(rows[2]["designation"], "M3x0.5");
    check_rows(rows);
}
