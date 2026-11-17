#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
use std::io::{Cursor, Read, Write};

use serde_json::{json, Value};

use super::*;

const FIXTURE: &[u8] =
    include_bytes!("../../tests/fixtures/onshape-default-plane-regions.onshape-bundle");

fn changed(edit: impl FnMut(&mut Value)) -> Vec<u8> {
    changed_fixture(FIXTURE, edit)
}

fn changed_fixture(fixture: &[u8], mut edit: impl FnMut(&mut Value)) -> Vec<u8> {
    let mut archive = zip::ZipArchive::new(Cursor::new(fixture)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let name = member.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        if name.ends_with("features.json") {
            let mut response = serde_json::from_slice(&bytes).unwrap();
            edit(&mut response);
            bytes = serde_json::to_vec(&response).unwrap();
        }
        output
            .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output.finish().unwrap().into_inner()
}

fn read(bytes: &[u8]) -> (Value, Report) {
    let mut report = Report::default();
    let model = crate::read_bytes(
        Format::OnshapeBundle,
        std::path::Path::new("synthetic.onshape-bundle"),
        bytes,
        &mut report,
    )
    .unwrap();
    let mut wire = serde_json::to_value(model.ir.model).unwrap();
    wire["features"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|f| f["ordinal"].as_u64());
    (wire, report)
}

// A deliberately simple literal encoder, independent of the fixture generator's
// dictionary/reference encoder. Only synthetic records pass through this helper.
fn compact(value: &Value) -> String {
    match value {
        Value::String(s) => format!("S{:x}${s}", s.len()),
        Value::Number(n) => format!("D{:x}", n.as_i64().unwrap()),
        Value::Bool(b) => if *b { "T" } else { "F" }.to_owned(),
        Value::Array(a) => format!(
            "A{:x}{}",
            a.len(),
            a.iter().map(compact).collect::<String>()
        ),
        Value::Object(o) => format!(
            "M{:x}{}",
            o.len(),
            o.iter()
                .map(|(k, v)| compact(&json!(k)) + &compact(v))
                .collect::<String>()
        ),
        _ => panic!("unsupported synthetic compact value"),
    }
}

fn replace_query(query: &mut Value, value: &Value) {
    query["queryString"] = json!(format!(
        "query=qCompressed(1.0,{},id);",
        serde_json::to_string(&format!("%{}", compact(value))).unwrap()
    ));
}

fn selected(response: &mut Value) -> &mut Value {
    &mut response["features"][1]["parameters"][2]["queries"][0]
}

fn kinds(wire: &Value) -> Vec<&str> {
    wire["features"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["definition"]["value"]["kind"].as_str().unwrap())
        .collect()
}

#[test]
fn real_shaped_synthetic_bundle_resolves_plane_and_selected_region() {
    let (wire, report) = read(FIXTURE);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    assert!(
        report.losses().iter().all(|loss| !loss
            .code
            .as_deref()
            .unwrap_or("")
            .starts_with("onshape/")),
        "{:?}",
        report.losses()
    );
    assert_eq!(
        wire["sketches"][0]["placement"]["frame"]["normal"],
        json!({"x":0.0,"y":0.0,"z":1.0})
    );
    let profile = &wire["features"][1]["definition"]["value"]["value"]["profile"]["value"];
    assert_eq!(profile["kind"], "sketch_regions");
    assert_eq!(
        profile["value"]["regions"][0]["outer_boundary"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        wire["features"][1]["definition"]["value"]["value"]["extent"]["side"]["termination"]
            ["length"],
        5.0
    );
}

#[test]
fn compressed_default_planes_resolve_their_own_full_frames() {
    for (plane, normal, u) in [
        ("Top", [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ("Front", [0.0, -1.0, 0.0], [1.0, 0.0, 0.0]),
        ("Right", [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ] {
        let bytes = changed(|response| {
            let query = &mut response["features"][0]["parameters"][0]["queries"][0];
            let mut value = query::resolved(query).unwrap();
            value["operationId"] = json!([format!("{plane}.planeOp")]);
            replace_query(query, &value);
        });
        let (wire, report) = read(&bytes);
        assert_eq!(
            kinds(&wire),
            ["sketch", "extrude"],
            "{plane}: {:?}",
            report.losses()
        );
        let frame = &wire["sketches"][0]["placement"]["frame"];
        assert_eq!(frame["origin"], topology::point([0.0; 3]));
        assert_eq!(frame["normal"], topology::point(normal));
        assert_eq!(frame["u_axis"], topology::point(u));
    }
}

#[test]
fn several_regions_keep_their_boundaries_and_never_expand_to_the_whole_sketch() {
    let bytes = changed(|response| {
        let mut second = response["features"][0]["entities"].as_array().unwrap()[..4].to_vec();
        for entity in &mut second {
            for field in ["entityId", "startPointId", "endPointId"] {
                entity[field] = json!(format!("second.{}", entity[field].as_str().unwrap()));
            }
            entity["geometry"]["pntX"] = json!(entity["geometry"]["pntX"].as_f64().unwrap() + 0.03);
        }
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .extend(second);
        let first = selected(response);
        let mut value = query::resolved(first).unwrap();
        for boundary in value["disambiguationData"][0]["entities"]
            .as_array_mut()
            .unwrap()
        {
            let entity = &mut boundary[0]["derivedFrom"]["sketchEntityId"];
            *entity = json!(format!("second.{}", entity.as_str().unwrap()));
        }
        let mut query = first.clone();
        query["deterministicIds"] = json!(["second-region"]);
        replace_query(&mut query, &value);
        response["features"][1]["parameters"][2]["queries"]
            .as_array_mut()
            .unwrap()
            .push(query);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let profile = &wire["features"][1]["definition"]["value"]["value"]["profile"]["value"];
    assert_eq!(profile["kind"], "sketch_regions");
    assert_eq!(profile["value"]["regions"].as_array().unwrap().len(), 2);
}

#[test]
fn ambiguous_region_queries_get_specific_losses_and_keep_the_sketch() {
    for (case, reason) in [
        ("source", "another sketch"),
        ("entity", "no solved source curve"),
        ("repeat", "repeats a source curve"),
        ("ids", "one boundary description"),
        ("disambiguation", "unique boundary"),
        ("side", "resolved side"),
    ] {
        let bytes = changed(|response| {
            let query = selected(response);
            let mut value = query::resolved(query).unwrap();
            let boundaries = &mut value["disambiguationData"][0]["entities"];
            match case {
                "source" => {
                    boundaries[0][0]["derivedFrom"]["operationId"] = json!(["different.wireOp"])
                }
                "entity" => boundaries[0][0]["derivedFrom"]["sketchEntityId"] = json!("absent"),
                "repeat" => {
                    let repeated = boundaries[0].clone();
                    boundaries.as_array_mut().unwrap().push(repeated);
                }
                "ids" => query["deterministicIds"] = json!(["one", "two"]),
                "disambiguation" => value["disambiguationData"] = json!([]),
                "side" => boundaries[0][1] = json!(0),
                _ => panic!("unknown test case"),
            }
            replace_query(query, &value);
        });
        let (wire, report) = read(&bytes);
        assert_eq!(kinds(&wire), ["sketch", "native"]);
        assert!(
            report
                .losses()
                .iter()
                .any(|loss| loss.detail.contains(reason)),
            "{case}: {:?}",
            report.losses()
        );
    }
}

#[test]
fn compressed_queries_are_bounded_and_truncation_never_panics() {
    let mut query = Value::Null;
    changed(|response| query = selected(response).clone());
    let original = query["queryString"].as_str().unwrap().to_owned();
    assert!(query::resolved(&query).is_ok());
    for n in 0..original.trim_end_matches(';').len() {
        query["queryString"] = json!(&original[..n]);
        assert!(query::resolved(&query).is_err());
    }
    for encoded in [
        "%Rffff",
        "%Affff",
        "%M1S1$kR1",
        "%M2S1$kD1R0D2",
        "&100001$eA==",
    ] {
        query["queryString"] = json!(format!(
            "query=qCompressed(1.0,{},id);",
            serde_json::to_string(encoded).unwrap()
        ));
        assert!(query::resolved(&query).is_err(), "{encoded}");
    }
}

#[test]
fn a_selected_region_excludes_other_closed_geometry_of_its_sketch() {
    let bytes = changed(|response| {
        let mut other = response["features"][0]["entities"].as_array().unwrap()[..4].to_vec();
        for entity in &mut other {
            for field in ["entityId", "startPointId", "endPointId"] {
                entity[field] = json!(format!("unused.{}", entity[field].as_str().unwrap()));
            }
            entity["geometry"]["pntX"] = json!(entity["geometry"]["pntX"].as_f64().unwrap() + 0.03);
        }
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .extend(other);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let profile = &wire["features"][1]["definition"]["value"]["value"]["profile"]["value"];
    let boundary = profile["value"]["regions"][0]["outer_boundary"]
        .as_array()
        .unwrap();
    assert_eq!(boundary.len(), 4);
    assert!(boundary
        .iter()
        .all(|b| !b["entity"].as_str().unwrap().contains("unused")));
}

#[test]
fn a_region_can_keep_an_outer_circle_and_an_inner_hole() {
    let bytes = changed(|response| {
        response["features"][0]["entities"] = json!([
            {"btType":"BTMSketchCurve-4", "entityId":"outer", "geometry":{"btType":"BTCurveGeometryCircle-115", "xCenter":0.01, "yCenter":0.01, "radius":0.01}},
            {"btType":"BTMSketchCurve-4", "entityId":"inner", "geometry":{"btType":"BTCurveGeometryCircle-115", "xCenter":0.01, "yCenter":0.01, "radius":0.005}}
        ]);
        let query = selected(response);
        let mut value = query::resolved(query).unwrap();
        let mut boundary = value["disambiguationData"][0]["entities"][0].clone();
        boundary[0]["derivedFrom"]["sketchEntityId"] = json!("outer");
        let mut hole = boundary.clone();
        hole[0]["derivedFrom"]["sketchEntityId"] = json!("inner");
        value["disambiguationData"][0]["entities"] = json!([hole, boundary]);
        replace_query(query, &value);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let region = &wire["features"][1]["definition"]["value"]["value"]["profile"]["value"]["value"]
        ["regions"][0];
    assert!(region["outer_boundary"][0]["entity"]
        .as_str()
        .unwrap()
        .ends_with("outer"));
    assert_eq!(region["hole_boundaries"].as_array().unwrap().len(), 1);
}

#[test]
fn compressed_reference_expansion_has_a_budget_and_a_depth_limit() {
    // The literal itself stays small while its backward references would make
    // the neutral JSON exponentially large without accounting for each clone.
    let mut literal = "%A29S1$x".to_owned();
    for i in 0..40 {
        literal.push_str(&format!("A2R{i:x}R{i:x}"));
    }
    let mut query = json!({});
    query["queryString"] = json!(format!(
        "query=qCompressed(1.0,{},id);",
        serde_json::to_string(&literal).unwrap()
    ));
    assert!(query::resolved(&query)
        .unwrap_err()
        .contains("reference limit"));
    let mut literal = "%A47S1$x".to_owned();
    for i in 0..70 {
        // Each completed array wraps the preceding cached value, keeping the
        // encoded nesting shallow while the referenced tree grows deeper.
        literal.push_str(&format!("A1R{i:x}"));
    }
    query["queryString"] = json!(format!(
        "query=qCompressed(1.0,{},id);",
        serde_json::to_string(&literal).unwrap()
    ));
    assert!(query::resolved(&query)
        .unwrap_err()
        .contains("nesting limit"));
}

#[test]
fn adjacent_regions_keep_the_shared_edge_in_each_directed_boundary() {
    let bytes = changed(|response| {
        let line = |name: &str, x, y, dx, dy| {
            json!({
                "btType":"BTMSketchCurveSegment-155", "entityId":name,
                "startParam":0.0, "endParam":0.01,
                "geometry":{"btType":"BTCurveGeometryLine-117", "pntX":x, "pntY":y, "dirX":dx, "dirY":dy}
            })
        };
        response["features"][0]["entities"] = json!([
            line("bottom-left", 0.0, 0.0, 1.0, 0.0),
            line("bottom-right", 0.01, 0.0, 1.0, 0.0),
            line("right", 0.02, 0.0, 0.0, 1.0),
            line("top-right", 0.02, 0.01, -1.0, 0.0),
            line("top-left", 0.01, 0.01, -1.0, 0.0),
            line("left", 0.0, 0.01, 0.0, -1.0),
            line("divider", 0.01, 0.0, 0.0, 1.0)
        ]);
        let original = selected(response).clone();
        let resolved = query::resolved(&original).unwrap();
        let edge = resolved["disambiguationData"][0]["entities"][0].clone();
        let mut queries = Vec::new();
        for (i, names) in [
            ["bottom-left", "divider", "top-left", "left"],
            ["bottom-right", "right", "top-right", "divider"],
        ]
        .iter()
        .enumerate()
        {
            let mut value = resolved.clone();
            value["disambiguationData"][0]["entities"] = Value::Array(
                names
                    .iter()
                    .map(|name| {
                        let mut edge = edge.clone();
                        edge[0]["derivedFrom"]["sketchEntityId"] = json!(name);
                        edge
                    })
                    .collect(),
            );
            let mut query = original.clone();
            query["deterministicIds"] = json!([format!("region-{i}")]);
            replace_query(&mut query, &value);
            queries.push(query);
        }
        response["features"][1]["parameters"][2]["queries"] = json!(queries);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let regions = wire["features"][1]["definition"]["value"]["value"]["profile"]["value"]["value"]
        ["regions"]
        .as_array()
        .unwrap();
    assert_eq!(regions.len(), 2);
    for region in regions {
        let boundary = region["outer_boundary"].as_array().unwrap();
        assert_eq!(boundary.len(), 4);
        assert_eq!(
            boundary
                .iter()
                .filter(|b| b["entity"].as_str().unwrap().ends_with("divider"))
                .count(),
            1
        );
    }
}

#[test]
fn two_sketch_dimensions_get_distinct_ordinals_within_their_feature() {
    let bytes = changed(|response| {
        let dimension = |id: &str, line: &str, expression: &str| {
            json!({"btType": "BTMSketchConstraint-2", "entityId": id, "constraintType": "LENGTH",
                "parameters": [
                    {"btType": "BTMParameterString-149", "parameterId": "localFirst", "value": line},
                    {"btType": "BTMParameterQuantity-147", "parameterId": "length", "expression": expression}]})
        };
        response["features"][0]["constraints"] = json!([
            dimension("length", "outline.side0", "20 mm"),
            dimension("width", "outline.side1", "10 mm")
        ]);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"], "{:?}", report.losses());
    let parameters = wire["parameters"].as_array().unwrap();
    assert_eq!(parameters.len(), 2, "{parameters:?}");
    let mut ordinals: Vec<u64> = parameters
        .iter()
        .map(|p| p["ordinal"].as_u64().unwrap())
        .collect();
    ordinals.sort_unstable();
    assert_eq!(ordinals, [0, 1]);
    assert_eq!(parameters[0]["owner"], parameters[1]["owner"]);
}

#[test]
fn sketch_dimension_ordinals_restart_for_each_owner() {
    let bytes = changed(|response| {
        let first = &mut response["features"][0];
        first["constraints"] = json!([{
            "entityId": "length", "constraintType": "LENGTH", "parameters": [
                {"parameterId": "localFirst", "value": "outline.side0"},
                {"parameterId": "length", "expression": "20 mm"}
            ]
        }]);
        let mut other = first.clone();
        other["featureId"] = json!("otherSketch");
        other.as_object_mut().unwrap().remove("name");
        response["features"]
            .as_array_mut()
            .unwrap()
            .insert(1, other);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(
        kinds(&wire),
        ["sketch", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let parameters = wire["parameters"].as_array().unwrap();
    assert_eq!(parameters.len(), 2, "{parameters:?}");
    assert_ne!(parameters[0]["owner"], parameters[1]["owner"]);
    assert!(parameters.iter().all(|parameter| parameter["ordinal"] == 0));
    let unnamed = wire["sketches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|sketch| sketch["id"] == id("sketch", "otherSketch"))
        .unwrap();
    assert!(unnamed.get("name").is_none());
}

#[test]
fn zlib_cap_queries_keep_face_sketches_in_the_source_frame() {
    use base64::Engine;
    let bytes = changed(|response| {
        let mut face_sketch = response["features"][0].clone();
        face_sketch["featureId"] = json!("cap-sketch");
        let creator = response["features"][1]["featureId"].as_str().unwrap();
        let cap = json!({"entityType":"FACE", "historyType":"CREATION", "queryType":"CAP_FACE",
            "operationId":[format!("{creator}.opExtrude")], "isStart":false});
        let literal = format!("%{}", compact(&cap));
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(literal.as_bytes()).unwrap();
        let encoded = format!(
            "&{:x}${}",
            literal.len(),
            base64::engine::general_purpose::STANDARD.encode(encoder.finish().unwrap())
        );
        face_sketch["parameters"][0]["queries"][0] = json!({"queryString":format!("query=qCompressed(1.0,{},id);", serde_json::to_string(&encoded).unwrap())});
        response["features"]
            .as_array_mut()
            .unwrap()
            .push(face_sketch);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(
        kinds(&wire),
        ["sketch", "extrude", "sketch"],
        "{:?}",
        report.losses()
    );
    let sketch = wire["sketches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id("sketch", "cap-sketch"))
        .unwrap();
    assert_eq!(
        sketch["placement"]["frame"]["origin"],
        topology::point([0.0, 0.0, 5.0])
    );
}

// A sketch on a side face: the zlib query names the face by its deterministic id
// in the rollback state before the sketch, which supplies the face's plane.
fn face_sketch_bundle(normal: [f64; 3], orientation: bool) -> Vec<u8> {
    use base64::Engine;
    let bytes = changed(|response| {
        let mut face_sketch = response["features"][0].clone();
        face_sketch["featureId"] = json!("side-sketch");
        let creator = response["features"][1]["featureId"].as_str().unwrap();
        let side = json!({"entityType":"FACE", "historyType":"CREATION", "queryType":"SWEPT_FACE",
            "operationId":[format!("{creator}.opExtrude")]});
        let literal = format!("%{}", compact(&side));
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(literal.as_bytes()).unwrap();
        let encoded = format!(
            "&{:x}${}",
            literal.len(),
            base64::engine::general_purpose::STANDARD.encode(encoder.finish().unwrap())
        );
        face_sketch["parameters"][0]["queries"][0] = json!({"deterministicIds":["side"],
            "queryString":format!("query=qCompressed(1.0,{},id);", serde_json::to_string(&encoded).unwrap())});
        response["features"]
            .as_array_mut()
            .unwrap()
            .push(face_sketch);
    });
    let rollback = json!({"btType":"BTExportModelBodiesResponse-734",
        "microversionId":{"theId":"synthetic-microversion"},
        "bodies":[{"id":"body","faces":[{"id":"side","orientation":orientation,"loops":[],
            "surface":{"type":"PLANE","origin":topology::point([0.01, -0.02, 0.005]),
            "normal":topology::point(normal)}}]}]});
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let name = member.name().unwrap().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        if name.ends_with("bodydetails/index.json") {
            bytes = b"[0, 1, 2]".to_vec();
        }
        output
            .start_file(name.as_str(), zip::write::SimpleFileOptions::default())
            .unwrap();
        output.write_all(&bytes).unwrap();
    }
    output
        .start_file(
            "partstudios/studio/bodydetails/2.json",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    output
        .write_all(&serde_json::to_vec(&rollback).unwrap())
        .unwrap();
    output.finish().unwrap().into_inner()
}

#[test]
fn zlib_face_queries_take_the_outward_face_plane_before_the_sketch() {
    // The surface normal points into the body; the face's orientation flips it.
    let (wire, report) = read(&face_sketch_bundle([0.0, 1.0, 0.0], false));
    assert_eq!(
        kinds(&wire),
        ["sketch", "extrude", "sketch"],
        "{:?}",
        report.losses()
    );
    let sketch = wire["sketches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id("sketch", "side-sketch"))
        .unwrap();
    assert_eq!(
        sketch["placement"]["frame"],
        json!({"origin": topology::point([0.0, -20.0, 0.0]),
            "normal": topology::point([0.0, -1.0, 0.0]),
            "u_axis": topology::point([1.0, 0.0, 0.0])})
    );
    let (wire, _) = read(&face_sketch_bundle([0.0, 0.0, 1.0], true));
    let sketch = wire["sketches"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["id"] == id("sketch", "side-sketch"))
        .unwrap();
    assert_eq!(
        sketch["placement"]["frame"],
        json!({"origin": topology::point([0.0, 0.0, 5.0]),
            "normal": topology::point([0.0, 0.0, 1.0]),
            "u_axis": topology::point([1.0, 0.0, 0.0])})
    );
}

#[test]
fn face_sketches_on_unmeasured_orientations_are_refused_by_name() {
    let (wire, report) = read(&face_sketch_bundle([0.6, 0.0, 0.8], true));
    assert_eq!(kinds(&wire)[2], "native");
    assert!(
        report.losses().iter().any(|loss| loss
            .detail
            .contains("not parallel to a default plane are not supported yet")),
        "{:?}",
        report.losses()
    );
}

#[test]
fn tangent_propagation_follows_vertices_and_refuses_incomplete_chains() {
    let edge = |name: &str, vertices: Value, tangent: Value| {
        json!({"id":name,"vertices":vertices,
        "geometry":{"startVector":tangent,"endVector":tangent}})
    };
    let state = topology::State::testing(vec![
        edge("first", json!(["a", "b"]), topology::point([1.0, 0.0, 0.0])),
        edge("next", json!(["b", "c"]), topology::point([-1.0, 0.0, 0.0])),
        edge(
            "corner",
            json!(["c", "d"]),
            topology::point([0.0, 1.0, 0.0]),
        ),
    ]);
    let selected = json!({"kind":"historical","value":{"state":"input","edges":["first"]}});
    let expanded = state.tangent_chain(&selected).unwrap();
    assert_eq!(expanded["value"]["edges"], json!(["first", "next"]));
    let state = topology::State::testing(vec![edge("first", json!(["a", "b"]), Value::Null)]);
    assert!(state
        .tangent_chain(&selected)
        .unwrap_err()
        .contains("endpoint tangents"));
}

#[test]
fn revolve_uses_a_construction_line_in_the_sketch_frame() {
    let mut native = String::new();
    let bytes = changed_fixture(
        include_bytes!("../../tests/fixtures/onshape-revolve.onshape-bundle"),
        |response| {
            native = response["features"][0]["featureId"]
                .as_str()
                .unwrap()
                .to_owned();
            response["features"][0]["entities"].as_array_mut().unwrap().push(json!({"btType":"BTMSketchCurveSegment-155","entityId":"axis-line","isConstruction":true,
            "geometry":{"btType":"BTCurveGeometryLine-117","dirY":1.0},"startParam":0.0,"endParam":0.015}));
            let query = json!({"entityType":"EDGE","historyType":"CREATION","queryType":"SKETCH_ENTITY","operationId":[format!("{native}.wireOp")],"sketchEntityId":"axis-line"});
            let axis = response["features"][1]["parameters"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|p| p["parameterId"] == "axis")
                .unwrap();
            replace_query(&mut axis["queries"][0], &query);
            response["features"][1]["parameters"]
                .as_array_mut()
                .unwrap()
                .retain(|p| p["parameterId"] != "revolveType");
            response["features"][1]["parameters"]
                .as_array_mut()
                .unwrap()
                .push(json!({"parameterId":"fullRevolve","value":true}));
        },
    );
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "revolve"], "{:?}", report.losses());
    assert_eq!(
        wire["features"][1]["definition"]["value"]["value"]["construction"]["value"]["axis"]
            ["direction"],
        topology::point([0.0, 1.0, 0.0])
    );
    assert_eq!(
        wire["features"][1]["definition"]["value"]["value"]["construction"]["value"]["axis"]
            ["reference"],
        json!({"kind":"sketch_curves","value":{"sketch":super::id("sketch", &native),
            "curves":[super::id("entity", &format!("{native}/axis-line"))]}})
    );
}

#[test]
fn origin_coincidence_and_midpoint_keep_explicit_point_relations() {
    let bytes = changed(|response| {
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .extend([
                json!({"btType":"BTMSketchPoint-158","entityId":"zero"}),
                json!({"btType":"BTMSketchPoint-158","entityId":"left-point","x":-0.004}),
                json!({"btType":"BTMSketchPoint-158","entityId":"right-point","x":0.004}),
            ]);
        response["features"][0]["constraints"] = json!([
            {"constraintType":"COINCIDENT","entityId":"origin-relation","parameters":[
                {"parameterId":"localFirst","value":"zero"},{"parameterId":"externalSecond","queries":[{"queryString":"qOrigin(EntityType.VERTEX);"}]}]},
            {"constraintType":"MIDPOINT","entityId":"middle-relation","parameters":[
                {"parameterId":"localMidpoint","value":"zero"},{"parameterId":"localEntity1","value":"left-point"},{"parameterId":"localEntity2","value":"right-point"}]}
        ]);
    });
    let (wire, report) = read(&bytes);
    let constraints = wire["sketch_constraints"].as_array().unwrap();
    assert!(
        constraints
            .iter()
            .any(|c| c["definition"]["kind"] == "point_coordinate_values"),
        "{:?}",
        report.losses()
    );
    assert!(
        constraints
            .iter()
            .any(|c| c["definition"]["kind"] == "midpoint"),
        "{:?}",
        report.losses()
    );
}

#[test]
fn variables_keep_dependencies_and_drive_a_symmetric_extrusion() {
    let bytes = changed(|response| {
        let rows = response["features"].as_array_mut().unwrap();
        rows[1]["parameters"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["parameterId"] == "depth")
            .unwrap()["expression"] = json!("2 * (#span + 1 mm)");
        rows[1]["parameters"]
            .as_array_mut()
            .unwrap()
            .push(json!({"parameterId":"symmetric","value":true}));
        rows.insert(0,json!({"featureId":"spacing-variable","featureType":"assignVariable","parameters":[
            {"parameterId":"mode","value":"ASSIGNED"},{"parameterId":"variableType","value":"LENGTH"},
            {"parameterId":"name","value":"span"},{"parameterId":"lengthValue","expression":"3 mm"}]}));
    });
    let (wire, report) = read(&bytes);
    assert_eq!(
        kinds(&wire),
        ["tree_node", "sketch", "extrude"],
        "{:?}",
        report.losses()
    );
    let extrude = &wire["features"][2]["definition"]["value"]["value"];
    assert_eq!(extrude["extent"]["kind"], "symmetric");
    assert_eq!(extrude["extent"]["side"]["termination"]["length"], 8.0);
    let owned = wire["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| {
            p.pointer("/properties/source_kind").and_then(Value::as_str) == Some("AlongDistance")
        })
        .unwrap();
    assert_eq!(
        owned["dependencies"],
        json!([id("parameter", "spacing-variable")])
    );
    assert!(owned["expression"].as_str().unwrap().contains("span"));
}

#[test]
fn variable_arithmetic_refuses_forward_references_wrong_units_and_excessive_nesting() {
    use super::expression::{evaluate, Dimension};
    let model = cadmpeg_ir::document::Model::default();
    for source in ["#missing", "1 mm + 2 deg", "1 mm / 0", "sin(2 mm)"] {
        assert!(
            evaluate(source, Dimension::Length, &model).is_err(),
            "{source}"
        );
    }
    let nested = format!("{}1 mm{}", "(".repeat(100), ")".repeat(100));
    assert!(evaluate(&nested, Dimension::Length, &model).is_err());
    assert_eq!(
        evaluate("2 * (3 mm + 1 cm)", Dimension::Length, &model)
            .unwrap()
            .value,
        26.0
    );
}

#[test]
fn compressed_hole_locations_keep_only_the_selected_sketch_point() {
    let bytes = changed(|response| {
        let sketch = response["features"][0]["featureId"]
            .as_str()
            .unwrap()
            .to_owned();
        response["features"][0]["entities"].as_array_mut().unwrap().extend([
            json!({"btType":"BTMSketchPoint-158","entityId":"chosen-point","x":0.002,"y":0.003}),
            json!({"btType":"BTMSketchPoint-158","entityId":"other-point","x":0.004,"y":0.003})]);
        let mut query = json!({});
        replace_query(
            &mut query,
            &json!({"entityType":"VERTEX","historyType":"CREATION","queryType":"SKETCH_ENTITY", "operationId":[format!("{sketch}.wireOp")],"sketchEntityId":"chosen-point"}),
        );
        response["features"].as_array_mut().unwrap().push(json!({"featureId":"hole","featureType":"hole","parameters":[
            {"parameterId":"locations","queries":[query]}, {"parameterId":"style","value":"SIMPLE"},
            {"parameterId":"endStyle","value":"THROUGH"},{"parameterId":"holeDiameter","expression":"1 mm"}]}));
    });
    let (wire, report) = read(&bytes);
    assert_eq!(
        kinds(&wire),
        ["sketch", "extrude", "hole"],
        "{:?}",
        report.losses()
    );
    let profile = &wire["features"][2]["definition"]["value"]["value"]["profile"];
    assert_eq!(profile["kind"], "sketch_entities");
    assert_eq!(profile["value"]["entities"].as_array().unwrap().len(), 1);
    assert!(profile["value"]["entities"][0]
        .as_str()
        .unwrap()
        .ends_with("chosen-point"));
}

#[test]
fn an_opaque_external_id_is_not_assumed_to_be_the_origin() {
    let bytes = changed(|response| {
        response["features"][0]["entities"]
            .as_array_mut()
            .unwrap()
            .push(json!({"btType":"BTMSketchPoint-158","entityId":"zero"}));
        response["features"][0]["constraints"] = json!([{"constraintType":"COINCIDENT","entityId":"external-relation","parameters":[
            {"parameterId":"localFirst","value":"zero"},{"parameterId":"externalSecond","queries":[{"queryString":"","deterministicIds":["unidentified-reference"]}]}]}]);
    });
    let (wire, report) = read(&bytes);
    assert_eq!(kinds(&wire), ["sketch", "extrude"]);
    assert!(report.losses().iter().any(|l| l.code.as_deref()
        == Some("onshape/sketch.constraint-not-supported")
        && l.detail.contains("does not identify the sketch origin")));
}

/// The revolve fixture's model with its body owned by one of two root products, each placed
/// once; `x` moves the second placement.
fn two_part_studio(x: f64) -> Model {
    let mut report = Report::default();
    let model = crate::read_bytes(
        Format::OnshapeBundle,
        std::path::Path::new("synthetic.onshape-bundle"),
        include_bytes!("../../tests/fixtures/onshape-revolve.onshape-bundle"),
        &mut report,
    )
    .unwrap();
    let mut wire = serde_json::to_value(&model.ir).unwrap();
    let body = wire["geometry"]["bodies"][0]["id"].clone();
    wire["model"]["product_definitions"] = json!([
        {"id": "synthetic:test:component#first", "kind": "part", "label": "Part 1", "bodies": [body], "bom_properties": {}, "sketches": []},
        {"id": "synthetic:test:component#second", "kind": "part", "label": "Part 2", "bodies": [], "bom_properties": {}, "sketches": []},
    ]);
    let placed = |key: u32, definition: &str, x: f64| {
        json!({"id": format!("synthetic:test:occurrence#{key}"),
            "prototype": {"kind": "local", "definition": format!("synthetic:test:component#{definition}")},
            "parent": {"kind": "root"}, "ordinal": key,
            "transform": [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
            "scale": [1.0, 1.0, 1.0]})
    };
    wire["model"]["occurrences"] = json!([placed(0, "first", 0.0), placed(1, "second", x)]);
    Model {
        ir: cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap(),
        ..model
    }
}

#[test]
fn part_studio_parts_are_one_design_but_moved_parts_stay_placed() {
    let mut model = two_part_studio(0.0);
    part_studio_bodies(&mut model);
    assert!(model.ir.model.occurrences.is_empty());
    assert_eq!(model.ir.model.product_definitions.len(), 2);
    assert!(crate::local_assembly(&model).unwrap().is_none());

    let mut moved = two_part_studio(30.0);
    part_studio_bodies(&mut moved);
    assert_eq!(moved.ir.model.occurrences.len(), 2);
    assert!(crate::local_assembly(&moved).unwrap().is_some());
}

/// The revolve fixture's bundle with `states` as its bodydetails, each a list of
/// (part id, part name).
fn with_parts(states: &[(usize, &[(&str, &str)])]) -> Vec<u8> {
    let fixture: &[u8] = include_bytes!("../../tests/fixtures/onshape-revolve.onshape-bundle");
    let mut archive = zip::ZipArchive::new(Cursor::new(fixture)).unwrap();
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).unwrap();
        let name = member.name().unwrap().into_owned();
        if name.contains("/bodydetails/") {
            continue;
        }
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes).unwrap();
        output.start_file(name.as_str(), options).unwrap();
        output.write_all(&bytes).unwrap();
    }
    for (ordinal, parts) in states {
        let bodies: Vec<Value> = parts
            .iter()
            .map(|(id, name)| json!({"id": id, "type": "SOLID", "properties": {"name": name}}))
            .collect();
        output
            .start_file(
                format!("partstudios/studio/bodydetails/{ordinal}.json"),
                options,
            )
            .unwrap();
        output
            .write_all(
                json!({"bodies": bodies, "microversionId": {"theId": "synthetic-microversion"}})
                    .to_string()
                    .as_bytes(),
            )
            .unwrap();
    }
    let indices: Vec<usize> = states.iter().map(|(ordinal, _)| *ordinal).collect();
    output
        .start_file("partstudios/studio/bodydetails/index.json", options)
        .unwrap();
    output
        .write_all(json!(indices).to_string().as_bytes())
        .unwrap();
    output.finish().unwrap().into_inner()
}

fn bound_parts(states: &[(usize, &[(&str, &str)])], creators: &[usize]) -> Vec<usize> {
    let mut report = Report::default();
    let mut model = crate::read_bytes(
        Format::OnshapeBundle,
        std::path::Path::new("synthetic.onshape-bundle"),
        include_bytes!("../../tests/fixtures/onshape-revolve.onshape-bundle"),
        &mut report,
    )
    .unwrap();
    model.ir.geometry.bodies[0].name = Some("Part B".to_owned());
    let bytes = with_parts(states);
    let bundle = archive::Bundle::open(&bytes).unwrap();
    let inputs =
        topology::Inputs::new(&bundle, "partstudios/studio", "synthetic-microversion").unwrap();
    let bound = part_bodies(creators, &inputs, &model);
    assert!(bound
        .values()
        .all(|body| *body == model.ir.geometry.bodies[0].id));
    bound.into_keys().collect()
}

#[test]
fn a_part_binds_to_the_feature_that_made_it_by_its_unique_name() {
    // The first feature makes a part the STEP export does not hold; the second makes the
    // STEP body's part.
    let states: [(usize, &[(&str, &str)]); 4] = [
        (0, &[]),
        (1, &[("a", "Part A")]),
        (2, &[("a", "Part A")]),
        (3, &[("a", "Part A"), ("b", "Part B")]),
    ];
    assert_eq!(bound_parts(&states, &[0, 2]), [2]);
    // Without the state after a feature, its part is not guessed.
    assert!(bound_parts(&states[..3], &[0, 2]).is_empty());
    // Two features claiming the one body of that name bind neither.
    let claimed: [(usize, &[(&str, &str)]); 4] = [
        (0, &[]),
        (1, &[("b", "Part B")]),
        (2, &[]),
        (3, &[("c", "Part B")]),
    ];
    assert!(bound_parts(&claimed, &[0, 2]).is_empty());
    // A feature making two parts binds neither.
    let two: [(usize, &[(&str, &str)]); 2] = [(0, &[]), (1, &[("b", "Part B"), ("c", "Part C")])];
    assert!(bound_parts(&two, &[0]).is_empty());
}
