//! Synthetic cube assignments, from decoding through writing and anonymous aggregation.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use dutchman::{anon, json as reports};
use dutchman_core::{read, Format, Report, Writer};
use dutchman_solidcraft::{SolidCraftWriter, Verification};
use serde_json::{json, Value};
use std::path::Path;

#[test]
fn appearance_causes_are_counted_without_design_identifiers() {
    for (mode, expected) in [
        ("equal", None),
        ("kept", None),
        ("wire", Some("wire")),
        ("edge", Some("wire")),
        ("missed", Some("face-pick-missed")),
        ("absent", Some("target-absent")),
        ("unwritable", Some("unwritable-body")),
        ("other", Some("other")),
    ] {
        let mut model = read(
            Format::InventorPart,
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../dutchman-core/tests/fixtures/synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        let mut wire = serde_json::to_value(&model.ir).unwrap();
        let m = &mut wire["model"];
        m["features"] = json!([]);
        m["appearances"] = json!([
            {"id":"synthetic:private:appearance#red","name":"Private paint","base_color":{"r":1.0,"g":0.0,"b":0.0,"a":1.0}},
            {"id":"synthetic:private:appearance#blue","base_color":{"r":0.0,"g":0.0,"b":1.0,"a":1.0}}
        ]);
        let body = m["bodies"][0]["id"].clone();
        let face = m["faces"][0]["id"].clone();
        let edge = m["edges"][0]["id"].clone();
        let mut target = json!({"kind":"face","id":face});
        let mut colour = "synthetic:private:appearance#blue";
        match mode {
            "equal" => colour = "synthetic:private:appearance#red",
            "wire" | "edge" | "unwritable" => {
                m["bodies"].as_array_mut().unwrap().push(json!({"id":"synthetic:private:body#extra","kind":if mode == "unwritable" {"solid"} else {"wire"},"regions":["synthetic:private:region#extra"]}));
                m["regions"].as_array_mut().unwrap().push(json!({"id":"synthetic:private:region#extra","body":"synthetic:private:body#extra","shells":["synthetic:private:shell#extra"]}));
                m["shells"].as_array_mut().unwrap().push(json!({"id":"synthetic:private:shell#extra","region":"synthetic:private:region#extra","members":[{"kind":"wire_edge","id":edge}]}));
                target = if mode == "edge" {
                    json!({"kind":"edge","id":edge})
                } else {
                    json!({"kind":"body","id":"synthetic:private:body#extra"})
                };
            }
            "missed" => m["faces"][0]["loops"]["loops"] = json!([]),
            "absent" => target["id"] = json!("synthetic:private:face#missing"),
            "other" => colour = "synthetic:private:appearance#missing",
            _ => {}
        }
        m["appearance_bindings"] = json!([
            {"id":"synthetic:private:binding#body","target":{"kind":"body","id":body},"appearance":"synthetic:private:appearance#red"},
            {"id":"synthetic:private:binding#target","target":target,"appearance":colour},
            {"id":"synthetic:private:binding#repeat","target":target,"appearance":colour}
        ]);
        model.ir = serde_json::from_value(wire).unwrap();
        let mut report = Report::default();
        SolidCraftWriter::new()
            .verification(Verification::Off)
            .write(&model, &mut Vec::new(), &mut report)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("diagnostics.jsonl");
        let mut log = dutchman::diagnostics::Log::open(
            &log_path,
            Path::new("Private design.ipt"),
            &dutchman::ConvertOptions::default(),
        )
        .unwrap();
        log.end(&json!({"ok":true,"losses":reports::losses(&report)}))
            .unwrap();
        let log_text = std::fs::read_to_string(log_path).unwrap();
        let end: Value = serde_json::from_str(log_text.lines().last().unwrap()).unwrap();
        let log_counts: serde_json::Map<String, Value> = end["losses"]
            .as_object()
            .unwrap()
            .iter()
            .filter(|(code, _)| code.starts_with("dutchman/appearance"))
            .map(|(code, count)| (code.clone(), count.clone()))
            .collect();
        if let Some(cause) = expected {
            assert_eq!(
                log_counts,
                serde_json::from_value::<serde_json::Map<String, Value>>(
                    json!({format!("dutchman/appearance.{cause}"):2})
                )
                .unwrap(),
                "{mode}"
            );
        } else {
            assert!(log_counts.is_empty(), "{mode}: {log_counts:?}");
        }
        for private in ["synthetic:", "Private", "#target", "#body", "#extra"] {
            assert!(!log_text.contains(private), "{mode}: {log_text}");
        }
        let batch = json!({"results":[{"ok":true,"input":{"path":"/synthetic/Private design.ipt","format":"inventor_part"},"losses":reports::losses(&report)}]});
        let (anon, _) = anon::report(
            &batch,
            &[],
            Path::new("/synthetic/out"),
            &anon::Salt::parse("00112233445566778899aabbccddeeff").unwrap(),
            &anon::Run::default(),
        );
        let losses: Vec<&Value> = anon["losses"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|l| {
                l["code"]
                    .as_str()
                    .unwrap()
                    .starts_with("dutchman/appearance")
            })
            .collect();
        if let Some(cause) = expected {
            assert_eq!(losses.len(), 1, "{mode}: {:?}", report.losses());
            assert_eq!(losses[0]["code"], format!("dutchman/appearance.{cause}"));
            assert_eq!(losses[0]["occurrences"], 2);
            assert!(losses[0].get("example").is_none());
        } else {
            assert!(losses.is_empty(), "{mode}: {:?}", report.losses());
        }
        let appearance_text = serde_json::to_string(&losses).unwrap();
        for id in [face.as_str().unwrap(), body.as_str().unwrap()] {
            assert!(!appearance_text.contains(id), "{mode}: {appearance_text}");
        }
        // These new categories carry counts only, independently of other loss types.
        let text = appearance_text;
        for private in ["synthetic:", "Private", "#target", "#body", "#extra"] {
            assert!(!text.contains(private), "{mode}: {text}");
        }
    }
}
