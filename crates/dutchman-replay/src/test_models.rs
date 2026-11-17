// Synthetic constructors adapted from dutchman-solidcraft/tests/history.rs.
// Geometry originates in cadmpeg's public examples::unit_cube; histories are local test data.
use super::*;
const SKETCH: &str = "f3d:model:sketch#1";
const SPATIAL: &str = "f3d:model:spatial-sketch#1";
const PLATE: &str = "f3d:model:sketch#41";
pub(super) fn entity(key: u32, geometry: Value) -> Value {
    json!({"id": format!("f3d:model:sketch-entity#{key}"), "sketch": SKETCH, "construction": false, "geometry": geometry, "endpoint_refs": []})
}

pub(super) fn line(key: u32, a: [f64; 2], b: [f64; 2]) -> Value {
    entity(
        key,
        json!({"kind": "line", "start": {"u": a[0], "v": a[1]}, "end": {"u": b[0], "v": b[1]}}),
    )
}

pub(super) fn cube_with_history(length: f64, profile: Value, extent: Option<Value>) -> Model {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut wire = serde_json::to_value(&ir).unwrap();
    wire["geometry"]["bodies"][0]["color"] = json!({"r": 0.8, "g": 0.2, "b": 0.1, "a": 1.0});
    let model = &mut wire["model"];
    model["sketches"] = json!([{
        "id": SKETCH,
        "name": "Base",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [[
            {"entity": "f3d:model:sketch-entity#1", "reversed": false},
            {"entity": "f3d:model:sketch-entity#2", "reversed": false},
            {"entity": "f3d:model:sketch-entity#3", "reversed": false},
            {"entity": "f3d:model:sketch-entity#4", "reversed": false},
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
            json!({"kind": "native", "native_kind": "Fusion only", "entities": ["f3d:model:sketch-entity#3"], "native_properties": {}, "operands": []})
        ),
    ]);
    let extent = extent.unwrap_or_else(|| {
        json!({"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": length}}})
    });
    model["features"] = json!([
        {"id": "f3d:model:feature#1", "ordinal": 0, "name": "Base sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": SKETCH}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []},
        {"id": "f3d:model:feature#2", "ordinal": 1, "name": "Block", "suppressed": false, "dependencies": ["f3d:model:feature#1"], "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": {"kind": "extrude", "value": {"profile": {"kind":"planar", "value": profile}, "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"}, "extent": extent, "op": "new_body"}}}, "source_properties": {}, "source_content": []},
    ]);
    model["parameters"] = json!([
        {"id": "f3d:model:parameter#1", "owner": "f3d:model:feature#2", "name": "d1", "expression": format!("{length} mm"), "value": {"kind": "length", "value": length}, "ordinal": 0, "dependencies": [], "properties": {}},
        {"id": "f3d:model:parameter#2", "owner": "f3d:model:feature#1", "ordinal": 1, "name": "side", "expression": "10 mm", "value": {"kind": "length", "value": 10.0}, "dependencies": [], "properties": {}},
    ]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        fidelity: None,
        name: "cube".to_owned(),
        source_file: Some("cube.f3d".to_owned()),
        source_step: None,
        assembly: None,
    }
}

pub(super) fn xyz(p: [f64; 3]) -> Value {
    json!({"x": p[0], "y": p[1], "z": p[2]})
}

pub(super) fn spatial_entity(key: u32, geometry: Value) -> Value {
    json!({"id": format!("f3d:model:spatial-sketch-entity#{key}"), "sketch": SPATIAL, "construction": false, "geometry": geometry, "endpoint_refs": []})
}

pub(super) fn spatial_id(key: u32) -> String {
    format!("f3d:model:spatial-sketch-entity#{key}")
}

pub(super) fn spatial_sweep_history(change: impl FnOnce(&mut Vec<Value>, &mut Value)) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    model["spatial_sketches"] = json!([{"id": SPATIAL, "name": "Rail", "profiles": []}]);
    let mut entities = vec![
        spatial_entity(
            1,
            json!({"kind": "line", "start": xyz([0.0, 0.0, 0.0]), "end": xyz([0.0, 0.0, 30.0])}),
        ),
        spatial_entity(
            2,
            json!({"kind": "arc", "center": xyz([20.0, 0.0, 30.0]), "normal": xyz([0.0, 1.0, 0.0]),
                   "reference_direction": xyz([-1.0, 0.0, 0.0]), "radius": 20.0,
                   "start_angle": 0.0, "end_angle": std::f64::consts::FRAC_PI_2}),
        ),
        spatial_entity(
            3,
            json!({"kind": "nurbs", "curve": {
                "degree": 3,
                "knots": [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
                "poles": {"kind": "polynomial", "points": [
                    xyz([20.0, 0.0, 50.0]), xyz([30.0, 0.0, 50.0]),
                    xyz([40.0, 5.0, 52.0]), xyz([50.0, 10.0, 60.0])]},
                "periodic": false}}),
        ),
    ];
    let mut definition = json!({"kind": "sweep", "value": {"shape": {"kind": "solid", "op": "new_body", "section": {"kind": "profile", "value": first_profile()}, "sections": []}, "path": {"kind": "spatial_sketch_curves",
                 "value": {"sketch": SPATIAL, "curves": [spatial_id(1), spatial_id(2), spatial_id(3)]}}, "path_tangent": false, "linearize": false, "path_extent": {"along_fraction": 1.0, "against_fraction": 0.0}}});
    change(&mut entities, &mut definition["value"]);
    model["spatial_sketch_entities"] = Value::Array(entities);
    model["features"] = json!([
        model["features"][0].clone(),
        {"id": "f3d:model:feature#3", "ordinal": 1, "name": "Rail sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "spatial_sketch", "value": {"sketch": SPATIAL}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []},
        {"id": "f3d:model:feature#2", "ordinal": 2, "name": "Block", "suppressed": false, "dependencies": ["f3d:model:feature#1", "f3d:model:feature#3"], "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": definition}, "source_properties": {}, "source_content": []},
    ]);
    model["parameters"] = json!([model["parameters"][1].clone()]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

pub(super) fn whole_sketch() -> Value {
    json!({"kind": "sketch", "value": SKETCH})
}

pub(super) fn first_profile() -> Value {
    json!({"kind": "sketch_profiles", "value": {"sketch": SKETCH, "profiles": [0]}})
}

pub(super) fn primitive_history(definition: Value) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["model"];
    model["features"] = json!([{"id": "f3d:model:feature#2", "ordinal": 0, "name": "Ball", "suppressed": false, "outputs": ["synthetic:cube:body#0"], "definition": {"kind": "operation", "value": definition}, "dependencies": [], "source_properties": {}, "source_content": []}]);
    model["parameters"] = json!([]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

pub(super) fn sphere() -> Value {
    json!({"kind": "sphere", "value": {"center": {"x": 5.0, "y": 5.0, "z": 5.0}, "radius": 5.0, "op": "new_body"}})
}

pub(super) fn flanged_cube(thickness: f64, side: &str) -> Model {
    let mut model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    for feature in wire["model"]["features"].as_array_mut().unwrap() {
        if feature["definition"]["value"]["kind"] == "extrude" {
            feature["name"] = json!("BaseFlange 1");
            feature["definition"] = json!({"kind":"operation", "value": {"kind": "sheet_metal_base_flange", "value": {"profile": whole_sketch(), "thickness": thickness, "side": side}}});
        }
    }
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

pub(super) fn input_edge_flange(back: bool, position: &str, angle: f64, radius: f64) -> Model {
    let mut model = flanged_cube(1.0, "forward");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let state = "test:sheet:feature-input#wall";
    let edges = [
        "test:sheet:historical-edge#front",
        "test:sheet:historical-edge#back",
    ];
    let faces = [
        "test:sheet:historical-face#top",
        "test:sheet:historical-face#rim",
    ];
    let point = |x, y, z| json!({"x": x, "y": y, "z": z});
    let geometry = [0, 1].map(|i| {
        json!({
            "edge": edges[i], "interior": point(5.0, 10.0 * i as f64, 1.0),
            "start": point(0.0, 10.0 * i as f64, 1.0),
            "end": point(10.0, 10.0 * i as f64, 1.0),
            "tangent": point(1.0, 0.0, 0.0), "faces": faces,
        })
    });
    wire["model"]["feature_input_topologies"] = json!([{"id": state, "input_of": "f3d:model:feature#3", "edges": edges, "faces": faces, "edge_geometry": geometry, "bodies": [], "vertices": []}]);
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Sheet wall", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sheet_metal_edge_flange", "value": {"edges": {"kind": "historical", "value": {
                    "state": state, "edges": [edges[usize::from(back)]], "native": "synthetic edge",
                }}, "height": {"kind": "distance", "value": 30.0}, "angle": angle, "height_datum": "outer_faces", "bend_position": position, "width": {"kind": "full_edge"}, "bend_radius": radius}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

pub(super) fn attached_plate(near: f64, radius: f64, extra: bool) -> Model {
    let mut model = flanged_cube(1.0, "forward");
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let m = &mut wire["model"];
    let point = |x, y, z| json!({"x": x, "y": y, "z": z});
    let edges = [
        "test:sheet:historical-edge#front",
        "test:sheet:historical-edge#back",
    ];
    let faces = [
        "test:sheet:historical-face#top",
        "test:sheet:historical-face#rim",
    ];
    let geometry = [0, 1].map(|i| {
        json!({
            "edge": edges[i], "interior": point(5.0, 10.0 * i as f64, 1.0),
            "start": point(0.0, 10.0 * i as f64, 1.0),
            "end": point(10.0, 10.0 * i as f64, 1.0),
            "tangent": point(1.0, 0.0, 0.0), "faces": faces,
        })
    });
    let state = |name: &str, owner: &str| json!({"id": format!("test:sheet:feature-input#{name}"), "input_of": owner, "edges": edges, "faces": faces, "edge_geometry": geometry, "bodies": [], "vertices": []});
    m["feature_input_topologies"] = json!([
        state("plate", "f3d:model:feature#4"),
        state("bend", "f3d:model:feature#5"),
    ]);
    let attachment = |name: &str| {
        json!({"kind": "historical", "value": {
            "state": format!("test:sheet:feature-input#{name}"),
            "edges": [edges[0]], "native": "synthetic edge",
        }})
    };
    m["sketches"].as_array_mut().unwrap().push(json!({
        "id": PLATE, "name": "Plate",
        "placement": {"kind": "resolved", "frame": {
            "origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "normal": {"x": 0.0, "y": -1.0, "z": 0.0},
            "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0},
        }},
        "profiles": [],
    }));
    let (lo, hi) = (near, 20.0);
    let mut lines = vec![
        line(41, [0.0, lo], [10.0, lo]),
        line(42, [10.0, lo], [10.0, hi]),
        line(43, [10.0, hi], [0.0, hi]),
        line(44, [0.0, hi], [0.0, lo]),
    ];
    if extra {
        lines.push(line(45, [0.0, lo], [5.0, hi]));
    }
    for mut l in lines {
        l["sketch"] = json!(PLATE);
        m["sketch_entities"].as_array_mut().unwrap().push(l);
    }
    let features = m["features"].as_array_mut().unwrap();
    features.push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Plate sketch", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": PLATE}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    features.push(json!({"id": "f3d:model:feature#4", "ordinal": 3, "name": "Plate", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sheet_metal_attached_plate", "value": {"profile": {"kind": "sketch", "value": PLATE}, "thickness": 1.0, "side": "forward", "attachment": attachment("plate")}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    features.push(json!({"id": "f3d:model:feature#5", "ordinal": 4, "name": "Bend", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sheet_metal_bend", "value": {"plate": "f3d:model:feature#4", "attachment": attachment("bend"), "inside_radius": radius, "relief": {"form": "style_default", "width": 1.0, "depth": 0.5,
                       "minimum_remnant": 0.0, "seam_gap": 0.0}, "transition": {"form": "style_default", "radius": 0.0}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}));
    model.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
    model
}

pub(super) fn cube_body() -> Value {
    json!({"kind": "bodies", "value": ["synthetic:cube:body#0"]})
}

pub(super) fn combined_spheres(operation: &str, keep_tools: bool) -> Model {
    let base = primitive_history(sphere());
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["geometry"];
    // A second, independent synthetic topology, with its own arena identities.
    for arena in [
        "points", "vertices", "curves", "edges", "coedges", "loops", "surfaces", "faces", "shells",
        "regions", "bodies",
    ] {
        let text = serde_json::to_string(&model[arena]).unwrap();
        let mut rows: Vec<Value> =
            serde_json::from_str(&text.replace("synthetic:cube:", "synthetic:tool:")).unwrap();
        if arena == "bodies" {
            rows[0]["name"] = json!("Tool");
        }
        model[arena].as_array_mut().unwrap().extend(rows);
    }
    let model = &mut wire["model"];
    let mut sphere = sphere();
    sphere["value"]["center"]["x"] = json!(10.0);
    model["features"].as_array_mut().unwrap().extend([
        json!({"id": "f3d:model:feature#3", "ordinal": 1, "name": "Tool ball", "suppressed": false, "outputs": ["synthetic:tool:body#0"], "definition": {"kind": "operation", "value": sphere}, "dependencies": [], "source_properties": {}, "source_content": []}),
        json!({"id": "f3d:model:feature#4", "ordinal": 2, "name": "Boolean", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "combine", "value": {"operands": {"target": cube_body(), "tools": {"kind": "bodies", "value": ["synthetic:tool:body#0"]}}, "op": operation, "keep_tools": keep_tools, "destination": "target_component"}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
    ]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

pub(super) fn split_cube(x: f64, shifted: bool, after: Option<Value>) -> Model {
    let base = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&base.ir).unwrap();
    let model = &mut wire["geometry"];
    let mut outputs = vec![json!("synthetic:cube:body#0")];
    if shifted {
        for arena in [
            "points", "vertices", "curves", "edges", "coedges", "loops", "surfaces", "faces",
            "shells", "regions", "bodies",
        ] {
            let text = serde_json::to_string(&model[arena]).unwrap();
            let mut rows: Vec<Value> =
                serde_json::from_str(&text.replace("synthetic:cube:", "synthetic:piece:")).unwrap();
            if arena == "points" {
                for row in &mut rows {
                    let moved = row["position"]["x"].as_f64().unwrap() + 10.0;
                    row["position"]["x"] = json!(moved);
                }
            }
            model[arena].as_array_mut().unwrap().extend(rows);
        }
        outputs.insert(0, json!("synthetic:piece:body#0"));
    }
    let frame = json!({"origin": {"x": x, "y": 0.0, "z": 0.0},
        "normal": {"x": 1.0, "y": 0.0, "z": 0.0}, "u_axis": {"x": 0.0, "y": 1.0, "z": 0.0}});
    let model = &mut wire["model"];
    let features = model["features"].as_array_mut().unwrap();
    features.extend([
        json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Cutting plane", "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "datum_plane", "value": {"frame": frame}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
        json!({"id": "f3d:model:feature#4", "ordinal": 3, "name": "Split", "suppressed": false, "outputs": outputs, "definition": {"kind": "operation", "value": {"kind": "split_body", "value": {"targets": cube_body(), "tool": {"kind": "plane", "value": {"plane": "f3d:model:feature#3"}}}}}, "dependencies": [], "source_properties": {}, "source_content": []}),
    ]);
    if let Some(definition) = after {
        features.push(
            json!({"id": "f3d:model:feature#5", "ordinal": 4, "name": "After", "suppressed": false, "definition": {"kind": "operation", "value": definition}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}),
        );
    }
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..base
    }
}

pub(super) fn face_extrude(up: bool, change: impl FnOnce(&mut Value)) -> Model {
    let model = cube_with_history(10.0, first_profile(), None);
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let z = if up { 10.0 } else { 0.0 };
    let point = |x: f64, y: f64| json!({"x": x, "y": y, "z": z});
    let state = "test:face:feature-input#3";
    let face = "test:face:historical-face#target";
    let side = |k: usize| format!("test:face:historical-face#side{k}");
    let corners = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
    let geometry: Vec<Value> = (0..4)
        .map(|k| {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let tangent = [(b.0 - a.0) / 10.0, (b.1 - a.1) / 10.0];
            json!({
                "edge": format!("test:face:historical-edge#{k}"),
                "interior": point((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0),
                "start": point(a.0, a.1), "end": point(b.0, b.1),
                "tangent": {"x": tangent[0], "y": tangent[1], "z": 0.0},
                "faces": [face, side(k)],
            })
        })
        .collect();
    let edges: Vec<String> = (0..4)
        .map(|k| format!("test:face:historical-edge#{k}"))
        .collect();
    let mut faces: Vec<String> = (0..4).map(side).collect();
    faces.push(face.to_owned());
    wire["model"]["feature_input_topologies"] = json!([{"id": state, "input_of": "f3d:model:feature#3", "edges": edges, "faces": faces, "edge_geometry": geometry, "bodies": [], "vertices": []}]);
    let mut definition = json!({"kind": "extrude", "value": {"profile": {"kind":"planar", "value": {"kind": "historical_faces", "value": {
            "state": state, "faces": [face], "native": ["synthetic profile group"],
        }}}, "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"}, "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 5.0}}}, "op": "join"}});
    let mut payload = definition["value"].take();
    change(&mut payload);
    if payload.get("kind").is_some() {
        definition = payload;
    } else {
        definition["value"] = payload;
    }
    wire["model"]["features"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id": "f3d:model:feature#3", "ordinal": 2, "name": "Face extrude", "suppressed": false, "dependencies": ["f3d:model:feature#2"], "definition": {"kind": "operation", "value": definition}, "source_properties": {}, "source_content": [], "outputs": []}));
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

pub(super) fn face_delete(selection: impl FnOnce(&str, &str) -> Value, heal: bool) -> Model {
    face_extrude(true, |definition| {
        *definition = json!({"kind": "delete_face", "value": {"faces": selection("test:face:feature-input#3", "test:face:historical-face#target"), "heal": heal}});
    })
}

pub(super) fn face_move(motion: Value) -> Model {
    face_extrude(true, |definition| {
        *definition = json!({"kind": "move_face", "value": {"faces": {"kind": "historical", "value": {
                "state": "test:face:feature-input#3",
                "faces": ["test:face:historical-face#target"], "native": "synthetic selection"}}, "motion": motion}});
    })
}
