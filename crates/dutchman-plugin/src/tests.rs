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
            &|_, _| None
        )
        .is_err());
    }
}
