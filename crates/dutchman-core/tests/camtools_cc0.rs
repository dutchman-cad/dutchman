//! A Fusion tool library made for these tests (see `fixtures/README.md`), converted to
//! `.camtools` from both of its forms.

#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::Path;

use dutchman_core::camtools;
use dutchman_core::Severity;
use serde_json::Value;

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn near(v: &Value, key: &str, want: f64) {
    let got = v[key]
        .as_f64()
        .unwrap_or_else(|| panic!("no number {key} in {v}"));
    assert!((got - want).abs() < 1e-6, "{key}: {got} is not {want}");
}

#[test]
fn both_forms_convert_to_the_same_eleven_tools() {
    let json = fixture("fusion_tool_library_cc0.json");
    let tools = fixture("fusion_tool_library_cc0.tools");
    assert!(camtools::is_library(&json) && camtools::is_library(&tools));
    let from_json = camtools::convert_path(&json).unwrap();
    let from_tools = camtools::convert_path(&tools).unwrap();
    assert_eq!(from_json.tools_read, 11);
    assert_eq!(
        from_json.tools_written,
        11,
        "{:#?}",
        from_json.report.losses()
    );
    assert_eq!(from_json.text, from_tools.text);
    assert!(from_json
        .report
        .losses()
        .iter()
        .all(|l| l.severity != Severity::Dropped));

    let out: Value = serde_json::from_str(from_json.text.as_deref().unwrap()).unwrap();
    assert_eq!(out["format"], "camcraft-tools");
    assert_eq!(out["version"], 1);
    let list = out["tools"].as_array().unwrap();
    let kinds: Vec<&str> = list
        .iter()
        .map(|t| t["definition"]["kind"].as_str().unwrap())
        .collect();
    assert_eq!(
        kinds,
        [
            "end_mill",
            "ball_nose",
            "bull_nose",
            "taper",
            "drill",
            "tap",
            "thread_mill",
            "turning",
            "bore",
            "groove",
            "thread"
        ]
    );
    let def = |i: usize| &list[i]["definition"];

    // 6 mm flat end mill: two presets for all materials; the first is the tool's own.
    let d = def(0);
    near(d, "diameter", 6.0);
    near(d, "flute_len", 16.0);
    near(d, "shank_len", 6.0);
    near(d, "rpm", 12000.0);
    near(d, "feed", 1080.0);
    assert_eq!(d["flutes"], 3);
    assert_eq!(d["tool_material"], "carbide");
    assert_eq!(d["part_number"], "CC0-FEM-6");
    assert!(d["holder_d"].as_f64().unwrap() > 0.0 && d["holder_len"].as_f64().unwrap() > 0.0);

    // The inch ball mill: 1/4 in -> 6.35 mm, 60 in/min -> 1524 mm/min.
    let d = def(1);
    near(d, "diameter", 6.35);
    near(d, "corner_r", 3.175);
    near(d, "flute_len", 12.7);
    near(d, "shank_len", 6.35);
    near(d, "feed", 1524.0);
    near(d, "rpm", 12000.0);

    let d = def(2);
    near(d, "diameter", 10.0);
    near(d, "corner_r", 1.0);
    near(d, "feed", 509.3);

    let d = def(3);
    near(d, "diameter", 12.0);
    near(d, "tip_angle", 90.0);
    near(d, "flute_len", 5.0);

    // A drill and a tap state their feed as the plunge feed.
    let d = def(4);
    near(d, "diameter", 8.5);
    near(d, "tip_angle", 118.0);
    near(d, "feed", 112.3);
    assert_eq!(d["tool_material"], "hss");
    let d = def(5);
    near(d, "pitch", 1.5);
    near(d, "rpm", 360.0);
    near(d, "feed", 540.0);

    let d = def(6);
    near(d, "diameter", 7.0);
    near(d, "pitch", 1.5);
    near(d, "tip_angle", 60.0);
    near(d, "flute_len", 15.0);

    // The four turning tools: constant surface speed becomes CamCraft's turning data.
    let d = def(7);
    near(d, "diameter", 12.7);
    near(d, "nose_radius", 0.8);
    near(&d["turning"], "vc", 220.0);
    near(&d["turning"], "feed_per_rev", 0.2);
    let d = def(8);
    near(d, "diameter", 6.35);
    near(d, "nose_radius", 0.4);
    near(&d["turning"], "vc", 180.0);
    near(&d["turning"], "feed_per_rev", 0.1);
    let d = def(9);
    near(d, "diameter", 10.0);
    near(d, "width", 3.0);
    near(&d["turning"], "vc", 120.0);
    near(&d["turning"], "feed_per_rev", 0.05);
    let d = def(10);
    near(d, "width", 1.299);
    near(d, "tip_angle", 60.0);
    near(d, "pitch", 1.5);
    near(d, "rpm", 500.0);
    near(d, "feed", 750.0);
    assert!(d.get("turning").is_none());

    // What CamCraft has no place for is named, per tool.
    assert!(from_json.report.losses().iter().any(|l| {
        l.code.as_deref() == Some("camtools/tool.field-not-carried")
            && l.detail.contains("geometry.tip-diameter")
    }));
}
