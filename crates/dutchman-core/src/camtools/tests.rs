//! Synthetic Fusion libraries, written by hand, converted and read back the way CamCraft's
//! loader reads a `.camtools` (the rules of its `parse_tool_library`, restated here).

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::io::Write;

use serde_json::{json, Map, Value};

use super::*;
use crate::report::Severity;

/// A Fusion tool entry as an export writes it.
fn tool(kind: &str, name: &str, geometry: Value) -> Value {
    json!({
        "BMC": "carbide",
        "GRADE": "Mill Generic",
        "description": name,
        "expressions": {"tool_description": format!("'{name}'")},
        "geometry": geometry,
        "guid": "00000000-0000-0000-0000-000000000000",
        "post-process": {
            "break-control": false, "comment": "", "diameter-offset": 3, "length-offset": 3,
            "live": true, "manual-tool-change": false, "number": 3, "turret": 0
        },
        "product-id": "P-100",
        "start-values": {"presets": [preset("", "all", 8000.0, 1200.0)]},
        "type": kind,
        "unit": "millimeters",
        "vendor": "Acme"
    })
}

fn preset(name: &str, category: &str, n: f64, v_f: f64) -> Value {
    json!({
        "name": name, "description": "", "n": n, "v_f": v_f, "n_ramp": n,
        "v_f_plunge": v_f / 2.0, "v_f_ramp": v_f / 2.0, "tool-coolant": "flood",
        "material": {"category": category, "query": "", "use-hardness": false},
        "use-feed-per-revolution": false, "use-stepdown": false, "use-stepover": false
    })
}

fn geo(pairs: &[(&str, f64)]) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert((*k).to_owned(), json!(v));
    }
    Value::Object(m)
}

fn library(tools: Vec<Value>) -> Value {
    json!({"data": tools, "version": 22})
}

fn contents(lib: &Value) -> Contents {
    let mut c = Contents::default();
    read::read_json(
        serde_json::to_string(lib).unwrap().as_bytes(),
        "lib.json",
        &mut c,
    )
    .unwrap();
    c
}

fn convert_lib(lib: &Value) -> Converted {
    convert(&contents(lib))
}

/// CamCraft's reader, as `tool_library.rs` states it: the envelope, then each tool's
/// definition field by field (`ToolDto`), refusing the whole file for one bad tool.
fn camcraft_reads(text: &str) -> Result<Vec<(String, Value)>, String> {
    let root: Value = serde_json::from_str(text).map_err(|e| format!("bad JSON: {e}"))?;
    let o = root.as_object().ok_or("not an object")?;
    if o.get("format").and_then(Value::as_str) != Some("camcraft-tools") {
        return Err("format".into());
    }
    let version = o.get("version").and_then(Value::as_u64).ok_or("version")?;
    if !(1..=4).contains(&version) {
        return Err("version unsupported".into());
    }
    if o.get("units")
        .and_then(Value::as_str)
        .is_some_and(|u| u != "mm")
    {
        return Err("units".into());
    }
    let rows = o.get("tools").and_then(Value::as_array).ok_or("no tools")?;
    if rows.is_empty() {
        return Err("empty".into());
    }
    let kinds = [
        "end_mill",
        "ball_nose",
        "bull_nose",
        "drill",
        "tap",
        "bore",
        "turning",
        "groove",
        "thread",
        "form",
        "taper",
        "probe",
        "torch",
        "waterjet",
        "electrode",
        "thread_mill",
        "thread_mill_single",
        "wire",
        "t_slot",
        "keyseat",
        "dovetail",
        "lollipop",
    ];
    let materials = [
        "aluminium_6061",
        "mild_steel",
        "stainless_304",
        "grey_cast_iron",
        "brass",
        "plastics_pom",
        "softwood",
        "hardwood",
        "mdf",
        "plywood",
        "acrylic",
        "hdpe",
        "unknown",
    ];
    let mut out: Vec<(String, Value)> = Vec::new();
    for row in rows {
        let name = row
            .get("name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or("no name")?;
        if out.iter().any(|(n, _)| n == name) {
            return Err(format!("{name} twice"));
        }
        let d = row
            .get("definition")
            .and_then(Value::as_object)
            .ok_or("no definition")?;
        let kind = d.get("kind").and_then(Value::as_str).ok_or("kind")?;
        if !kinds.contains(&kind) {
            return Err(format!("unknown kind {kind}"));
        }
        for key in [
            "diameter",
            "corner_r",
            "flute_len",
            "shank_d",
            "shank_len",
            "holder_d",
            "holder_len",
            "rpm",
            "feed",
        ] {
            let v = d
                .get(key)
                .and_then(Value::as_f64)
                .ok_or(format!("{name}: {key} missing"))?;
            if !v.is_finite() || v < 0.0 {
                return Err(format!("{name}: {key} {v}"));
            }
        }
        d.get("material_group")
            .and_then(Value::as_str)
            .ok_or("material_group")?;
        for (k, v) in d {
            match k.as_str() {
                "tool_material" if !matches!(v.as_str(), Some("hss" | "carbide")) => {
                    return Err("tool_material".into())
                }
                "flutes" if !v.as_u64().is_some_and(|z| (1..=32).contains(&z)) => {
                    return Err("flutes".into())
                }
                "thread_hand" if !matches!(v.as_str(), Some("left" | "right")) => {
                    return Err("thread_hand".into())
                }
                "cutting_data" => {
                    for r in v.as_array().ok_or("cutting_data")? {
                        let m = r
                            .get("material")
                            .and_then(Value::as_str)
                            .ok_or("row material")?;
                        if !materials.contains(&m) {
                            return Err(format!("row material {m}"));
                        }
                        for key in ["rpm", "feed"] {
                            if r.get(key).and_then(Value::as_f64).is_none_or(|x| x <= 0.0) {
                                return Err(format!("row {key}"));
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        if d.get("feed")
            .and_then(Value::as_f64)
            .is_some_and(|f| f <= 0.0)
        {
            return Err("feed".into());
        }
        out.push((name.to_owned(), row["definition"].clone()));
    }
    Ok(out)
}

fn written(c: &Converted) -> Vec<(String, Value)> {
    camcraft_reads(c.text.as_deref().unwrap()).unwrap()
}

fn find<'a>(tools: &'a [(String, Value)], name: &str) -> &'a Value {
    &tools
        .iter()
        .find(|(n, _)| n == name)
        .unwrap_or_else(|| panic!("no tool {name}"))
        .1
}

fn f(v: &Value, key: &str) -> f64 {
    v[key].as_f64().unwrap_or_else(|| panic!("no number {key}"))
}

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn every_milling_type_maps_to_its_camcraft_kind() {
    let mill = geo(&[
        ("DC", 10.0),
        ("LCF", 20.0),
        ("LB", 45.0),
        ("OAL", 80.0),
        ("SFDM", 10.0),
        ("NOF", 3.0),
    ]);
    let cases: Vec<(&str, Value, &str)> = vec![
        ("flat end mill", mill.clone(), "end_mill"),
        ("ball end mill", mill.clone(), "ball_nose"),
        (
            "bull nose end mill",
            geo(&[
                ("DC", 10.0),
                ("LCF", 20.0),
                ("LB", 45.0),
                ("SFDM", 10.0),
                ("RE", 1.0),
            ]),
            "bull_nose",
        ),
        (
            "chamfer mill",
            geo(&[
                ("DC", 10.0),
                ("LCF", 5.0),
                ("LB", 30.0),
                ("SFDM", 10.0),
                ("TA", 45.0),
            ]),
            "taper",
        ),
        (
            "dovetail mill",
            geo(&[
                ("DC", 20.0),
                ("LCF", 5.0),
                ("LB", 35.0),
                ("SFDM", 8.0),
                ("TA", 30.0),
            ]),
            "dovetail",
        ),
        (
            "lollipop mill",
            geo(&[("DC", 10.0), ("LCF", 5.0), ("LB", 35.0), ("SFDM", 4.0)]),
            "lollipop",
        ),
        (
            "thread mill",
            geo(&[
                ("DC", 6.0),
                ("LCF", 12.5),
                ("LB", 40.0),
                ("SFDM", 6.0),
                ("TP", 1.25),
                ("thread-profile-angle", 60.0),
            ]),
            "thread_mill",
        ),
        (
            "drill",
            geo(&[
                ("DC", 5.0),
                ("LCF", 30.0),
                ("LB", 60.0),
                ("SFDM", 5.0),
                ("SIG", 135.0),
            ]),
            "drill",
        ),
        (
            "tap right hand",
            geo(&[
                ("DC", 6.0),
                ("LCF", 18.0),
                ("LB", 50.0),
                ("SFDM", 4.5),
                ("TP", 1.0),
            ]),
            "tap",
        ),
        (
            "boring head",
            geo(&[
                ("DC", 12.0),
                ("LCF", 20.0),
                ("LB", 60.0),
                ("SFDM", 12.0),
                ("RE", 0.4),
            ]),
            "bore",
        ),
        (
            "probe",
            geo(&[("DC", 4.0), ("LCF", 30.0), ("LB", 70.0), ("SFDM", 6.0)]),
            "probe",
        ),
        (
            "plasma cutter",
            geo(&[("DC", 1.5), ("LCF", 10.0), ("LB", 60.0), ("SFDM", 20.0)]),
            "torch",
        ),
        (
            "water jet cutter",
            geo(&[("DC", 0.8), ("LCF", 10.0), ("LB", 60.0), ("SFDM", 20.0)]),
            "waterjet",
        ),
        (
            "turning general",
            geo(&[
                ("DC", 12.7),
                ("LCF", 8.0),
                ("LB", 60.0),
                ("SFDM", 12.7),
                ("RE", 0.8),
            ]),
            "turning",
        ),
        (
            "turning grooving",
            geo(&[
                ("DC", 16.0),
                ("LCF", 8.0),
                ("LB", 60.0),
                ("SFDM", 16.0),
                ("W", 3.0),
            ]),
            "groove",
        ),
        (
            "turning threading",
            geo(&[
                ("DC", 16.0),
                ("LCF", 8.0),
                ("LB", 60.0),
                ("SFDM", 16.0),
                ("TP", 1.5),
            ]),
            "thread",
        ),
        (
            "turning form",
            geo(&[
                ("DC", 16.0),
                ("LCF", 8.0),
                ("LB", 60.0),
                ("SFDM", 16.0),
                ("W", 4.0),
                ("RE", 6.0),
            ]),
            "form",
        ),
        ("boring head", mill.clone(), "bore"),
    ];
    let tools: Vec<Value> = cases
        .iter()
        .enumerate()
        .map(|(i, (ty, g, _))| tool(ty, &format!("tool {i}"), g.clone()))
        .collect();
    let c = convert_lib(&library(tools));
    assert_eq!(c.tools_read, cases.len());
    assert_eq!(c.tools_written, cases.len(), "{:#?}", c.report.losses());
    let out = written(&c);
    for (i, (ty, _, kind)) in cases.iter().enumerate() {
        assert_eq!(find(&out, &format!("tool {i}"))["kind"], *kind, "{ty}");
    }
}

#[test]
fn a_flat_end_mill_is_carried_field_by_field() {
    let g = geo(&[
        ("DC", 10.0),
        ("LCF", 22.0),
        ("LB", 50.0),
        ("OAL", 90.0),
        ("SFDM", 10.0),
        ("NOF", 4.0),
        ("RE", 0.5),
    ]);
    let mut t = tool("flat end mill", "EM 10", g);
    t["holder"] = json!({"description": "BT30", "segments": [
        {"height": 20.0, "lower-diameter": 12.0, "upper-diameter": 40.0},
        {"height": 30.0, "lower-diameter": 40.0, "upper-diameter": 63.0}]});
    t["post-process"]["comment"] = json!("roughing");
    t["product-link"] = json!("https://example.com/p");
    let c = convert_lib(&library(vec![t]));
    let out = written(&c);
    let d = find(&out, "EM 10");
    assert_eq!(d["kind"], "end_mill");
    assert!(near(f(d, "diameter"), 10.0));
    assert!(near(f(d, "corner_r"), 0.5));
    assert!(near(f(d, "flute_len"), 22.0));
    assert!(near(f(d, "shank_d"), 10.0));
    assert!(near(f(d, "shank_len"), 28.0), "LB 50 less flute 22");
    assert!(near(f(d, "holder_d"), 63.0));
    assert!(near(f(d, "holder_len"), 50.0));
    assert!(near(f(d, "rpm"), 8000.0));
    assert!(near(f(d, "feed"), 1200.0));
    assert_eq!(d["tool_material"], "carbide");
    assert_eq!(d["flutes"], 4);
    assert_eq!(d["part_number"], "P-100");
    assert_eq!(d["vendor"], "Acme");
    assert_eq!(
        d["notes"],
        "Fusion tool number 3; https://example.com/p; roughing"
    );
    assert_eq!(d["material_group"], "all");
    assert!(
        d.get("center_cutting").is_none(),
        "centre cutting is the default"
    );
    let text = c.text.unwrap();
    assert!(text.contains("\"version\": 1"));
    assert!(text.contains("\"units\": \"mm\""));
}

#[test]
fn an_inch_library_is_converted_to_millimetres() {
    let mut t = tool(
        "flat end mill",
        "quarter inch",
        geo(&[
            ("DC", 0.25),
            ("LCF", 0.75),
            ("LB", 1.5),
            ("SFDM", 0.25),
            ("NOF", 3.0),
        ]),
    );
    t["unit"] = json!("inches");
    t["start-values"] = json!({"presets": [preset("", "all", 8000.0, 40.0)]});
    t["holder"] =
        json!({"segments": [{"height": 1.0, "lower-diameter": 1.0, "upper-diameter": 2.0}]});
    let out = written(&convert_lib(&library(vec![t])));
    let d = find(&out, "quarter inch");
    assert!(near(f(d, "diameter"), 6.35));
    assert!(near(f(d, "flute_len"), 19.05));
    assert!(near(f(d, "shank_d"), 6.35));
    assert!(near(f(d, "shank_len"), 19.05));
    assert!(near(f(d, "feed"), 1016.0), "40 in/min");
    assert!(near(f(d, "rpm"), 8000.0), "rpm is not a length");
    // Holder segments are millimetres, whatever the tool's unit.
    assert!(near(f(d, "holder_d"), 2.0));
    assert!(near(f(d, "holder_len"), 1.0));
}

#[test]
fn angles_and_pitches_follow_each_kind() {
    let tools = vec![
        tool(
            "chamfer mill",
            "chamfer",
            geo(&[
                ("DC", 10.0),
                ("LCF", 5.0),
                ("LB", 30.0),
                ("SFDM", 10.0),
                ("TA", 30.0),
            ]),
        ),
        tool(
            "counter sink",
            "csk",
            geo(&[
                ("DC", 12.0),
                ("LCF", 5.0),
                ("LB", 30.0),
                ("SFDM", 8.0),
                ("SIG", 82.0),
            ]),
        ),
        tool(
            "dovetail mill",
            "dt",
            geo(&[
                ("DC", 20.0),
                ("LCF", 5.0),
                ("LB", 35.0),
                ("SFDM", 8.0),
                ("TA", 30.0),
            ]),
        ),
        tool(
            "drill",
            "drill",
            geo(&[
                ("DC", 5.0),
                ("LCF", 30.0),
                ("LB", 60.0),
                ("SFDM", 5.0),
                ("SIG", 140.0),
            ]),
        ),
        tool(
            "tap left hand",
            "left tap",
            geo(&[
                ("DC", 6.0),
                ("LCF", 18.0),
                ("LB", 50.0),
                ("SFDM", 4.5),
                ("TP", 1.0),
            ]),
        ),
        tool(
            "turning threading",
            "thread insert",
            geo(&[
                ("DC", 16.0),
                ("LCF", 8.0),
                ("LB", 60.0),
                ("SFDM", 16.0),
                ("TP", 2.0),
            ]),
        ),
    ];
    let c = convert_lib(&library(tools));
    let out = written(&c);
    assert!(
        near(f(find(&out, "chamfer"), "tip_angle"), 60.0),
        "included angle is twice TA"
    );
    assert!(near(f(find(&out, "csk"), "tip_angle"), 82.0));
    assert!(
        near(f(find(&out, "dt"), "tip_angle"), 60.0),
        "flank to bottom face is 90 - TA"
    );
    assert!(near(f(find(&out, "drill"), "tip_angle"), 140.0));
    let tap = find(&out, "left tap");
    assert_eq!(tap["thread_hand"], "left");
    assert!(near(f(tap, "pitch"), 1.0));
    let insert = find(&out, "thread insert");
    assert!(near(f(insert, "width"), 1.732), "0.866 x pitch");
    assert!(near(f(insert, "tip_angle"), 60.0));
    assert!(
        c.text.unwrap().contains("\"version\": 2"),
        "a left-hand tap needs version 2"
    );
}

#[test]
fn presets_become_cutting_rows_and_the_rest_are_named() {
    let mut t = tool(
        "flat end mill",
        "multi",
        geo(&[
            ("DC", 8.0),
            ("LCF", 16.0),
            ("LB", 40.0),
            ("SFDM", 8.0),
            ("NOF", 3.0),
        ]),
    );
    let mut steel = preset("Steel", "steel", 5000.0, 800.0);
    steel["use-stepdown"] = json!(true);
    steel["stepdown"] = json!(4.0);
    t["start-values"] = json!({"presets": [
        preset("Alu", "aluminum", 12000.0, 2400.0),
        preset("Default", "all", 9000.0, 1800.0),
        steel,
        preset("Ti", "titanium", 3000.0, 300.0),
        preset("Alu again", "aluminum", 11000.0, 2000.0),
    ]});
    let c = convert_lib(&library(vec![t]));
    let out = written(&c);
    let d = find(&out, "multi");
    assert!(
        near(f(d, "rpm"), 9000.0),
        "the all-materials preset is the tool's own"
    );
    let rows = d["cutting_data"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["material"], "aluminium_6061");
    assert!(near(rows[0]["rpm"].as_f64().unwrap(), 12000.0));
    assert!(near(rows[0]["feed"].as_f64().unwrap(), 2400.0));
    assert_eq!(rows[1]["material"], "mild_steel");
    assert!(near(rows[1]["stepdown"].as_f64().unwrap(), 4.0));
    assert!(
        c.text.as_deref().unwrap().contains("\"version\": 3"),
        "a stepdown needs version 3"
    );
    let notes: Vec<_> = c.report.losses().iter().map(|l| l.detail.clone()).collect();
    assert!(
        notes
            .iter()
            .any(|n| n.contains("titanium") && n.contains("not carried")),
        "{notes:?}"
    );
    assert!(
        notes
            .iter()
            .any(|n| n.contains("Alu again") && n.contains("repeats")),
        "{notes:?}"
    );
}

#[test]
fn feed_per_revolution_and_per_tooth_give_a_feed_rate() {
    let mut per_rev = tool(
        "flat end mill",
        "per rev",
        geo(&[("DC", 8.0), ("LCF", 16.0), ("SFDM", 8.0)]),
    );
    per_rev["start-values"] = json!({"presets": [{"n": 4000.0, "f_n": 0.1, "use-feed-per-revolution": true, "material": {"category": "all"}}]});
    let mut per_tooth = tool(
        "flat end mill",
        "per tooth",
        geo(&[("DC", 8.0), ("LCF", 16.0), ("SFDM", 8.0), ("NOF", 2.0)]),
    );
    per_tooth["start-values"] =
        json!({"presets": [{"n": 5000.0, "f_z": 0.05, "material": {"category": "all"}}]});
    let out = written(&convert_lib(&library(vec![per_rev, per_tooth])));
    assert!(near(f(find(&out, "per rev"), "feed"), 400.0));
    assert!(near(f(find(&out, "per tooth"), "feed"), 500.0));
}

#[test]
fn a_tool_without_cutting_data_gets_placeholders_and_says_so() {
    let mut t = tool(
        "drill",
        "bare",
        geo(&[("DC", 5.0), ("LCF", 30.0), ("LB", 60.0), ("SFDM", 5.0)]),
    );
    t["start-values"] = json!({"presets": []});
    let c = convert_lib(&library(vec![t]));
    let d = &written(&c)[0].1;
    assert!(near(f(d, "rpm"), 1000.0));
    assert!(near(f(d, "feed"), 100.0));
    assert!(near(f(d, "tip_angle"), 118.0));
    let degraded: Vec<_> = c
        .report
        .losses()
        .iter()
        .filter(|l| l.severity == Severity::Degraded)
        .collect();
    assert_eq!(degraded.len(), 1);
    assert!(degraded[0].detail.contains("placeholders") && degraded[0].detail.contains("118"));
}

#[test]
fn approximated_types_are_written_and_marked_degraded() {
    let tools = vec![
        tool(
            "face mill",
            "face",
            geo(&[("DC", 50.0), ("LCF", 8.0), ("LB", 40.0), ("SFDM", 22.0)]),
        ),
        tool(
            "slot mill",
            "slot",
            geo(&[("DC", 40.0), ("LCF", 3.0), ("LB", 40.0), ("SFDM", 10.0)]),
        ),
        tool(
            "spot drill",
            "spot",
            geo(&[
                ("DC", 6.0),
                ("LCF", 8.0),
                ("LB", 40.0),
                ("SFDM", 6.0),
                ("SIG", 90.0),
            ]),
        ),
    ];
    let c = convert_lib(&library(tools));
    let out = written(&c);
    assert_eq!(find(&out, "face")["kind"], "end_mill");
    assert_eq!(find(&out, "face")["center_cutting"], false);
    assert_eq!(find(&out, "slot")["kind"], "keyseat");
    assert_eq!(find(&out, "spot")["kind"], "drill");
    for name in ["face", "slot", "spot"] {
        assert!(
            c.report
                .losses()
                .iter()
                .any(|l| l.severity == Severity::Degraded && l.subject.contains(name)),
            "{name}"
        );
    }
}

#[test]
fn tools_camcraft_cannot_hold_are_listed_and_the_rest_converts() {
    let ok = geo(&[("DC", 6.0), ("LCF", 12.0), ("LB", 40.0), ("SFDM", 6.0)]);
    let tools = vec![
        tool("flat end mill", "keeper", ok.clone()),
        tool("reamer", "reamer 6", ok.clone()),
        tool("tapered mill", "tapered 6", ok.clone()),
        tool("form mill", "form 6", ok.clone()),
        tool("laser cutter", "laser", ok.clone()),
        tool("hovercraft", "mystery", ok.clone()),
        tool(
            "bull nose end mill",
            "too round",
            geo(&[("DC", 6.0), ("LCF", 12.0), ("SFDM", 6.0), ("RE", 4.0)]),
        ),
        tool("flat end mill", "no diameter", geo(&[("LCF", 12.0)])),
        tool(
            "slot mill",
            "fat neck",
            geo(&[("DC", 20.0), ("LCF", 3.0), ("LB", 30.0), ("SFDM", 20.0)]),
        ),
    ];
    let c = convert_lib(&library(tools));
    assert_eq!(c.tools_read, 9);
    assert_eq!(c.tools_written, 1);
    assert_eq!(written(&c)[0].0, "keeper");
    let dropped: Vec<_> = c
        .report
        .losses()
        .iter()
        .filter(|l| l.severity == Severity::Dropped)
        .collect();
    assert_eq!(dropped.len(), 8);
    for name in [
        "reamer 6",
        "tapered 6",
        "form 6",
        "laser",
        "mystery",
        "too round",
        "no diameter",
        "fat neck",
    ] {
        assert!(
            dropped.iter().any(|l| l.subject.contains(name)),
            "{name} is not named"
        );
    }
    assert!(dropped
        .iter()
        .any(|l| l.subject.contains("too round") && l.detail.contains("CamCraft would refuse")));
    assert!(dropped
        .iter()
        .any(|l| l.subject.contains("mystery") && l.detail.contains("hovercraft")));
}

#[test]
fn a_library_with_nothing_convertible_writes_no_file() {
    let c = convert_lib(&library(vec![tool("reamer", "r", geo(&[("DC", 6.0)]))]));
    assert!(c.text.is_none());
    assert_eq!(c.tools_written, 0);
    assert_eq!(c.report.losses().len(), 1);
}

#[test]
fn fields_with_no_camcraft_place_are_named_never_dropped_silently() {
    let mut t = tool(
        "flat end mill",
        "rich",
        geo(&[
            ("DC", 6.0),
            ("LCF", 12.0),
            ("LB", 40.0),
            ("SFDM", 6.0),
            ("shoulder-length", 30.0),
            ("tip-diameter", 2.0),
        ]),
    );
    t["post-process"]["turret"] = json!(2);
    t["post-process"]["diameter-offset"] = json!(9);
    t["holder"] = json!({"description": "ER16", "gaugeLength": 50.0, "segments": [{"height": 30.0, "lower-diameter": 10.0, "upper-diameter": 28.0}]});
    t["extra-field"] = json!("something new");
    let c = convert_lib(&library(vec![t]));
    let info = c
        .report
        .losses()
        .iter()
        .find(|l| l.code.as_deref() == Some("camtools/tool.field-not-carried"))
        .unwrap();
    for field in [
        "geometry.shoulder-length",
        "geometry.tip-diameter",
        "post-process.turret",
        "post-process.diameter-offset",
        "holder.gaugeLength",
        "preset.v_f_plunge",
        "preset.v_f_ramp",
        "preset.tool-coolant",
        "preset.n_ramp",
        "extra-field",
    ] {
        assert!(
            info.detail.contains(field),
            "{field} missing from: {}",
            info.detail
        );
    }
    for quiet in [
        "guid",
        "GRADE",
        "expressions",
        "break-control",
        "length-offset",
        "number",
    ] {
        assert!(
            !info.detail.contains(quiet),
            "{quiet} is reported: {}",
            info.detail
        );
    }
}

#[test]
fn a_name_used_twice_is_renamed_and_a_missing_name_is_made_up() {
    let g = geo(&[("DC", 6.0), ("LCF", 12.0), ("LB", 40.0), ("SFDM", 6.0)]);
    let mut nameless = tool("flat end mill", "", g.clone());
    nameless["description"] = json!("   ");
    let out = written(&convert_lib(&library(vec![
        tool("flat end mill", "same", g.clone()),
        tool("flat end mill", "same", g.clone()),
        tool("flat end mill", "same", g),
        nameless,
    ])));
    let names: Vec<_> = out.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names[..3], ["same", "same (2)", "same (3)"]);
    assert!(names[3].starts_with("flat end mill 6"), "{names:?}");
}

#[test]
fn a_unit_that_is_neither_millimetres_nor_inches_is_listed() {
    let mut t = tool(
        "flat end mill",
        "cubits",
        geo(&[("DC", 6.0), ("LCF", 12.0)]),
    );
    t["unit"] = json!("cubits");
    let c = convert_lib(&library(vec![t]));
    assert_eq!(c.tools_written, 0);
    assert!(c.report.losses()[0].detail.contains("cubits"));
}

#[test]
fn holders_are_counted_and_not_written() {
    let g = geo(&[("DC", 6.0), ("LCF", 12.0), ("LB", 40.0), ("SFDM", 6.0)]);
    let mut holder = tool("holder", "BT30 ER32", g.clone());
    holder["geometry"] = json!({});
    let c = convert_lib(&library(vec![tool("flat end mill", "em", g), holder]));
    assert_eq!(c.tools_read, 1);
    assert!(c
        .report
        .losses()
        .iter()
        .any(|l| l.detail.contains("1 holder entry")));
}

#[test]
fn a_tools_file_is_a_zip_of_json_and_a_json_export_reads_the_same() {
    let g = geo(&[("DC", 6.0), ("LCF", 12.0), ("LB", 40.0), ("SFDM", 6.0)]);
    let lib = library(vec![tool("flat end mill", "from zip", g.clone())]);
    let second = library(vec![tool("ball end mill", "second file", g)]);
    let dir = tempfile::tempdir().unwrap();
    let tools = dir.path().join("lib.tools");
    {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&tools).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("a-library.json", options).unwrap();
        zip.write_all(lib.to_string().as_bytes()).unwrap();
        zip.start_file("b-library.json", options).unwrap();
        zip.write_all(second.to_string().as_bytes()).unwrap();
        zip.start_file("readme.txt", options).unwrap();
        zip.write_all(b"not a library").unwrap();
        zip.finish().unwrap();
    }
    assert!(is_library(&tools));
    let c = convert_path(&tools).unwrap();
    let names: Vec<_> = written(&c).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, ["from zip", "second file"]);

    let json = dir.path().join("lib.json");
    std::fs::write(&json, lib.to_string()).unwrap();
    assert!(is_library(&json));
    assert_eq!(written(&convert_path(&json).unwrap()).len(), 1);
}

#[test]
fn files_that_are_not_fusion_libraries_are_not_taken() {
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("other.json");
    std::fs::write(&other, r#"{"hello": "world"}"#).unwrap();
    assert!(!is_library(&other));
    let list = dir.path().join("list.json");
    std::fs::write(&list, r#"[1, 2, 3]"#).unwrap();
    assert!(!is_library(&list));
    let broken = dir.path().join("broken.tools");
    std::fs::write(&broken, "not a zip, not json").unwrap();
    assert!(is_library(&broken), "a .tools file is always tried");
    assert!(convert_path(&broken)
        .unwrap_err()
        .contains("not valid JSON"));
    let empty_zip = dir.path().join("empty.tools");
    zip::ZipWriter::new(std::fs::File::create(&empty_zip).unwrap())
        .finish()
        .unwrap();
    assert!(convert_path(&empty_zip)
        .unwrap_err()
        .contains("no JSON library"));
    assert!(!is_library(&dir.path().join("missing.tools")));
    assert!(!is_library(&dir.path().join("part.f3d")));
}

#[test]
fn spindleless_tools_state_zero_rpm_and_the_probe_one() {
    let tools = vec![
        tool(
            "plasma cutter",
            "torch",
            geo(&[("DC", 1.5), ("LCF", 10.0), ("LB", 60.0), ("SFDM", 20.0)]),
        ),
        tool(
            "water jet cutter",
            "jet",
            geo(&[("DC", 0.8), ("LCF", 10.0), ("LB", 60.0), ("SFDM", 20.0)]),
        ),
        tool(
            "probe",
            "probe",
            geo(&[("DC", 4.0), ("LCF", 30.0), ("LB", 70.0), ("SFDM", 6.0)]),
        ),
    ];
    let out = written(&convert_lib(&library(tools)));
    assert!(near(f(find(&out, "torch"), "rpm"), 0.0));
    assert!(near(f(find(&out, "jet"), "rpm"), 0.0));
    assert!(near(f(find(&out, "probe"), "rpm"), 1.0));
    assert!(
        near(f(find(&out, "probe"), "corner_r"), 2.0),
        "a probe's ball radius is half its diameter"
    );
    assert!(near(f(find(&out, "torch"), "feed"), 1200.0));
}

#[test]
fn the_report_titles_tool_losses_as_tools() {
    let c = convert_lib(&library(vec![tool("reamer", "r", geo(&[("DC", 6.0)]))]));
    assert_eq!(c.report.losses()[0].title(), "Tools left out");
}

#[test]
fn turning_tools_carry_their_fields() {
    let tools = vec![
        tool(
            "turning general",
            "CNMG",
            geo(&[
                ("IC", 12.7),
                ("LE", 12.0),
                ("LB", 70.0),
                ("SFDM", 20.0),
                ("RE", 0.8),
            ]),
        ),
        tool(
            "turning boring",
            "bar",
            geo(&[
                ("DC", 10.0),
                ("LCF", 25.0),
                ("LB", 80.0),
                ("SFDM", 10.0),
                ("RE", 0.4),
            ]),
        ),
        tool(
            "turning grooving",
            "groove",
            geo(&[
                ("DC", 16.0),
                ("LCF", 8.0),
                ("LB", 60.0),
                ("SFDM", 16.0),
                ("W", 3.0),
            ]),
        ),
        tool(
            "turning threading",
            "thread",
            geo(&[
                ("DC", 16.0),
                ("LCF", 8.0),
                ("LB", 60.0),
                ("SFDM", 16.0),
                ("W", 1.0),
                ("thread-profile-angle", 55.0),
            ]),
        ),
        tool(
            "turning form",
            "form",
            geo(&[
                ("DC", 16.0),
                ("LCF", 8.0),
                ("LB", 60.0),
                ("SFDM", 16.0),
                ("W", 4.0),
                ("RE", 3.0),
            ]),
        ),
        tool(
            "turning custom",
            "custom",
            geo(&[("DC", 16.0), ("LCF", 8.0)]),
        ),
        tool(
            "turning general",
            "no nose",
            geo(&[("DC", 12.7), ("LCF", 12.0)]),
        ),
    ];
    let c = convert_lib(&library(tools));
    assert_eq!(c.tools_written, 5);
    let out = written(&c);
    let t = find(&out, "CNMG");
    assert_eq!(t["kind"], "turning");
    assert!(near(f(t, "diameter"), 12.7) && near(f(t, "nose_radius"), 0.8));
    assert!(near(f(t, "flute_len"), 12.0) && near(f(t, "shank_len"), 58.0));
    let b = find(&out, "bar");
    assert_eq!(b["kind"], "bore");
    assert!(near(f(b, "nose_radius"), 0.4) && near(f(b, "shank_len"), 55.0));
    let g = find(&out, "groove");
    assert_eq!(g["kind"], "groove");
    assert!(near(f(g, "width"), 3.0));
    let th = find(&out, "thread");
    assert_eq!(th["kind"], "thread");
    assert!(near(f(th, "width"), 1.0) && near(f(th, "tip_angle"), 55.0));
    let fm = find(&out, "form");
    assert_eq!(fm["kind"], "form");
    assert!(near(f(fm, "width"), 4.0) && near(f(fm, "nose_radius"), 3.0));
    let dropped: Vec<_> = c
        .report
        .losses()
        .iter()
        .filter(|l| l.severity == Severity::Dropped)
        .collect();
    assert!(dropped.iter().any(|l| l.subject.contains("custom")));
    assert!(dropped
        .iter()
        .any(|l| l.subject.contains("no nose") && l.detail.contains("nose radius")));
}

/// A form mill's profile as Fusion writes it: `{"end": [radius, height]}` from the axis at the
/// tip, round and back to the axis (millimetres here).
fn profile(points: &[(f64, f64)]) -> Value {
    Value::Array(points.iter().map(|(r, z)| json!({"end": [r, z]})).collect())
}

/// A multi-form thread mill: `rows` teeth of 60 degrees included at `pitch`, crest radius 3,
/// root radius 2, a neck, then a 4 mm radius shank.
fn thread_profile(rows: usize, pitch: f64) -> Value {
    let depth = 1.0;
    let half = depth * 30f64.to_radians().tan();
    let mut p = vec![(0.0, 0.0), (2.0, 0.0), (2.0, 0.5)];
    for i in 0..rows {
        let z = 0.5 + i as f64 * pitch;
        p.push((3.0, z + half));
        if i + 1 < rows {
            p.push((2.0, z + 2.0 * half));
            p.push((2.0, z + pitch));
        }
    }
    let last = p[p.len() - 1].1;
    p.push((4.0, last + 6.0));
    p.push((4.0, 40.0));
    p.push((0.0, 40.0));
    profile(&p)
}

fn form_tool(name: &str, profile: Value) -> Value {
    let mut t = tool(
        "form mill",
        name,
        geo(&[
            ("DC", 6.0),
            ("LCF", 12.0),
            ("LB", 30.0),
            ("OAL", 40.0),
            ("NOF", 3.0),
        ]),
    );
    t["geometry"]["profile"] = profile;
    t
}

#[test]
fn a_form_mill_drawing_rows_of_teeth_is_a_multi_form_thread_mill() {
    let c = convert_lib(&library(vec![form_tool("multi", thread_profile(4, 1.5))]));
    let out = written(&c);
    let t = find(&out, "multi");
    assert_eq!(t["kind"], "thread_mill");
    assert_eq!(t["diameter"], 6.0);
    assert_eq!(t["pitch"], 1.5);
    assert_eq!(t["tip_angle"], 60.0);
    assert_eq!(t["flute_len"], 6.0);
    assert_eq!(t["shank_d"], 8.0);
    assert_eq!(t["flutes"], 3);
    let info = c
        .report
        .losses()
        .iter()
        .find(|l| l.code.as_deref() == Some("camtools/tool.field-not-carried"))
        .unwrap();
    assert!(info.detail.contains("geometry.profile"), "{}", info.detail);
}

#[test]
fn one_tooth_or_uneven_rows_or_free_forms_are_refused_by_what_they_draw() {
    let uneven = {
        let mut p = vec![(0.0, 0.0), (2.0, 0.0)];
        for z in [0.5, 2.0, 4.0] {
            p.push((2.0, z));
            p.push((3.0, z + 0.5));
            p.push((2.0, z + 1.0));
        }
        p.extend([(4.0, 8.0), (4.0, 40.0), (0.0, 40.0)]);
        profile(&p)
    };
    let free = json!([{"end": [0.0, 0.0]}, {"end": [2.0, 0.0], "center": [1.0, 1.0]}, {"end": [3.0, 5.0]}, {"end": [3.0, 9.0]}, {"end": [0.0, 9.0]}]);
    let double_angle = profile(&[
        (0.0, 0.0),
        (2.0, 0.0),
        (4.0, 0.8),
        (2.0, 1.6),
        (2.0, 9.0),
        (4.0, 11.0),
        (4.0, 40.0),
        (0.0, 40.0),
    ]);
    let c = convert_lib(&library(vec![
        form_tool("single", thread_profile(1, 1.5)),
        form_tool("uneven", uneven),
        form_tool("free", free),
        form_tool("double", double_angle),
        form_tool("keeper", thread_profile(3, 1.5)),
    ]));
    assert_eq!(c.tools_written, 1);
    assert_eq!(written(&c)[0].0, "keeper");
    let dropped: Vec<_> = c
        .report
        .losses()
        .iter()
        .filter(|l| l.severity == Severity::Dropped)
        .collect();
    let why = |name: &str| {
        dropped
            .iter()
            .find(|l| l.subject.contains(name))
            .unwrap_or_else(|| panic!("{name} not dropped"))
            .detail
            .clone()
    };
    assert!(why("single").contains("single-form thread mill needs a pitch range"));
    assert!(why("double").contains("degrees included"));
    assert!(why("uneven").contains("not at one pitch"));
    assert!(why("free").contains("curved segments"));
}

#[test]
fn a_tapered_mill_is_a_cone_only_when_its_tip_is_a_point() {
    let sharp = tool(
        "tapered mill",
        "sharp",
        geo(&[
            ("DC", 0.0),
            ("LCF", 10.0),
            ("LB", 30.0),
            ("SFDM", 8.0),
            ("TA", 30.0),
        ]),
    );
    let blunt = tool(
        "tapered mill",
        "blunt",
        geo(&[
            ("DC", 1.0),
            ("LCF", 10.0),
            ("LB", 30.0),
            ("SFDM", 8.0),
            ("TA", 30.0),
        ]),
    );
    let ball = tool(
        "tapered mill",
        "ball",
        geo(&[
            ("DC", 0.0),
            ("RE", 0.5),
            ("LCF", 10.0),
            ("LB", 30.0),
            ("SFDM", 8.0),
            ("TA", 30.0),
        ]),
    );
    let c = convert_lib(&library(vec![sharp, blunt, ball]));
    let out = written(&c);
    assert_eq!(c.tools_written, 1);
    let t = find(&out, "sharp");
    assert_eq!(t["kind"], "taper");
    assert_eq!(t["tip_angle"], 60.0);
    assert_eq!(t["flute_len"], 10.0);
    // The cone meets its diameter exactly at the cutting length: 2 * 10 * tan(30 degrees).
    assert!((t["diameter"].as_f64().unwrap() - 11.547005).abs() < 1e-6);
    let dropped: Vec<_> = c
        .report
        .losses()
        .iter()
        .filter(|l| l.severity == Severity::Dropped)
        .collect();
    assert_eq!(dropped.len(), 2);
    assert!(dropped.iter().all(|l| l.detail.contains("sharp point")));
}

#[test]
fn a_dovetail_whose_neck_meets_its_flank_is_taken_with_the_shoulder_as_the_neck() {
    // Inches: a 0.24 head, 0.0575 high, 45 degree flank, on a 0.125 shoulder under a 0.25 shank.
    let mut t = tool(
        "dovetail mill",
        "dove",
        geo(&[
            ("DC", 0.24),
            ("LCF", 0.0575),
            ("LB", 1.0),
            ("OAL", 1.5),
            ("SFDM", 0.25),
            ("TA", 45.0),
            ("shoulder-diameter", 0.125),
            ("shoulder-length", 1.0),
        ]),
    );
    t["unit"] = json!("inches");
    let c = convert_lib(&library(vec![t]));
    let out = written(&c);
    let d = find(&out, "dove");
    assert_eq!(d["kind"], "dovetail");
    assert_eq!(d["tip_angle"], 45.0);
    assert!((d["shank_d"].as_f64().unwrap() - 3.175).abs() < 1e-6);
    assert!((d["shank_len"].as_f64().unwrap() - 23.9395).abs() < 1e-6);
}
