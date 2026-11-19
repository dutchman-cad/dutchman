#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use crate::{convert, resolve, Options};
use serde_json::Value;
use std::path::Path;
const CUBE: &[u8] = include_bytes!("../../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
fn document(bytes: &[u8]) -> Value {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap()
}
#[test]
fn native_options_and_structured_report() {
    let run = |params: &str| {
        convert(
            "cube.f3d",
            serde_json::from_str::<Options>(params).unwrap(),
            &|_, _| Ok(CUBE.into()),
            &|_, _| None,
            None,
        )
        .unwrap()
    };
    let (design, report) = run("{}");
    let report: Value = serde_json::from_str(&report).unwrap();
    assert!(report["summary"].as_str().unwrap().len() <= 240);
    assert!(report["items"].is_array());
    let details = &report["details"];
    assert_eq!(details["schema"], "dutchman.plugin.loss/1");
    assert_eq!(details["stages"].as_array().unwrap().len(), 6);
    assert!(details["outcome"]["verification"]["status"] != "verified");
    let (stood, _) = run(r#"{"up_axis":"stand_up"}"#);
    let original = document(&design);
    let stood = document(&stood);
    assert_eq!(
        stood["features"].as_array().unwrap().len(),
        original["features"].as_array().unwrap().len()
    );
    assert_eq!(
        stood["components"].as_array().unwrap().len(),
        original["components"].as_array().unwrap().len() + 1
    );
    for occurrence in stood["occurrences"].as_array().unwrap() {
        let parent = occurrence["parent"].as_u64().unwrap();
        assert!(
            parent == 0
                || stood["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["id"] == parent),
            "occurrence parent must resolve to a component: {occurrence}"
        );
    }
    assert!(serde_json::from_str::<Options>(r#"{"up_axis":"other"}"#).is_err());
    assert!(serde_json::from_str::<Options>(r#"{"extra":true}"#).is_err());
}
#[test]
fn member_lookup_stays_relative_and_handles_stored_windows_names() {
    assert_eq!(
        resolve(r"C:\saved\part.ipt", Path::new(""), &|p| p
            == Path::new("part.ipt")),
        Some(std::path::PathBuf::from("part.ipt"))
    );
    assert_eq!(
        resolve(r"C:\project\parts\Pin.ipt", Path::new("assemblies"), &|p| p
            == Path::new("parts/Pin.ipt")),
        Some(std::path::PathBuf::from("parts/Pin.ipt"))
    );
    assert_eq!(
        resolve(".part.ipt", Path::new(""), &|p| p == Path::new(".part.ipt")),
        Some(std::path::PathBuf::from(".part.ipt"))
    );
    for name in ["../secret", "/etc/passwd", ".", "..", "C:secret"] {
        let found = resolve(name, Path::new(""), &|p| p == Path::new(name));
        assert!(found.is_none(), "{name}");
    }
}
#[test]
fn rejects_non_cad_inputs_and_bad_names() {
    assert!(crate::validate_name(".cube.f3d").is_ok());
    for name in ["../cube.f3d", "part.step", "", ".", "..", "part\0.f3d"] {
        assert!(convert(
            name,
            Options::default(),
            &|_, _| Ok(CUBE.into()),
            &|_, _| None,
            None
        )
        .is_err());
    }
}
#[test]
fn history_off_is_worded_as_the_plugin_option() {
    let (_, report) = convert(
        "cube.f3d",
        serde_json::from_str::<Options>(r#"{"editable_history":false}"#).unwrap(),
        &|_, _| Ok(CUBE.into()),
        &|_, _| None,
        None,
    )
    .unwrap();
    assert!(!report.contains("--import-only"), "{report}");
    let report: Value = serde_json::from_str(&report).unwrap();
    let verification = &report["details"]["outcome"]["verification"];
    assert_eq!(verification["status"], "not_applicable");
    assert_eq!(verification["message"], crate::EDITABLE_HISTORY_OFF);
    assert!(report["summary"]
        .as_str()
        .unwrap()
        .starts_with("Geometry without editable history"));
}
#[test]
fn history_off_keeps_the_command_line_wording_elsewhere() {
    use dutchman_core::{Check, Report, Writer};
    let mut report = Report::default();
    let model = dutchman_core::source::read(
        dutchman_core::Format::FusionArchive,
        Path::new("cube.f3d"),
        false,
        dutchman_core::PartPositions::Smart,
        &|_, _| Ok(CUBE.into()),
        &|_, _| None,
        &mut report,
    )
    .unwrap();
    dutchman_solidcraft::SolidCraftWriter::new()
        .threads(1)
        .verification(dutchman_solidcraft::Verification::Off)
        .import_only(true)
        .write(&model, &mut Vec::new(), &mut report)
        .unwrap();
    assert_eq!(
        report.outcome().unwrap().check,
        Check::NotApplicable("--import-only".to_owned())
    );
}

#[test]
fn every_stood_up_assembly_occurrence_has_a_component_parent() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/freecad-1.1.4-A01");
    let (design, _) = convert(
        "A01a.FCStd",
        Options {
            editable_history: false,
            up_axis: crate::UpAxis::StandUp,
            ..Options::default()
        },
        &|path, _| {
            std::fs::read(root.join(path)).map_err(|e| dutchman_core::Error::Decode(e.to_string()))
        },
        &|stored, folder| resolve(stored, folder, &|path| root.join(path).is_file()),
        None,
    )
    .unwrap();
    let doc = document(&design);
    assert!(doc["occurrences"].as_array().unwrap().len() > 2);
    for occurrence in doc["occurrences"].as_array().unwrap() {
        let parent = occurrence["parent"].as_u64().unwrap();
        assert!(
            parent == 0
                || doc["components"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|c| c["id"] == parent),
            "occurrence parent must resolve to a component: {occurrence}"
        );
    }
}
