//! CC0 recipe regressions for construction helpers.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{Cursor, Read, Write};
use std::path::Path;

use dutchman_core::{Check, FeatureStatus, Format, Report, Writer};
use dutchman_solidcraft::{SolidCraftWriter, CLI_ENV};
use zip::{write::SimpleFileOptions, ZipArchive, ZipWriter};

fn fixture(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn convert(path: &Path) -> Report {
    let mut report = Report::default();
    let model = dutchman_core::read(Format::FreeCadDocument, path, &mut report).unwrap();
    SolidCraftWriter::default()
        .write(&model, &mut Vec::new(), &mut report)
        .unwrap();
    report
}

#[test]
fn unused_coordinate_system_does_not_flatten_a_pad() {
    if !Format::FreeCadDocument.can_convert() {
        eprintln!("enable dutchman-core/freecad to run the CC0 helper regression");
        return;
    }
    let report = convert(&fixture("freecad_0_21_2_F18b.FCStd"));
    let outcome = report.outcome().unwrap();
    let helper = outcome
        .features
        .iter()
        .find(|f| f.kind == "datum_coordinate_system")
        .unwrap();
    assert_eq!(helper.status, FeatureStatus::NotNeeded);
    assert!(report
        .losses()
        .iter()
        .any(|loss| loss.detail == "helper not converted: nothing uses it"));
    assert!(!matches!(outcome.check, Check::Flattened(_)));
    if std::env::var_os(CLI_ENV).is_some() {
        assert_eq!(outcome.check, Check::Verified);
    }
}

#[test]
fn used_point_reports_the_missing_dependent_placement() {
    if !Format::FreeCadDocument.can_convert() {
        eprintln!("enable dutchman-core/freecad to run the CC0 helper regression");
        return;
    }
    let report = convert(&fixture("freecad_0_21_2_F14b.FCStd"));
    let helper = report
        .outcome()
        .unwrap()
        .features
        .iter()
        .find(|f| f.kind == "datum_point")
        .unwrap();
    assert_eq!(helper.status, FeatureStatus::Blocked);
    assert!(helper
        .reason
        .as_deref()
        .unwrap()
        .contains("dependent feature does not carry its resolved placement"));
}

#[test]
fn used_subshape_binder_reports_missing_source_geometry() {
    if !Format::FreeCadDocument.can_convert() {
        eprintln!("enable dutchman-core/freecad to run the CC0 helper regression");
        return;
    }
    let report = convert(&fixture("freecad_0_21_2_F16b.FCStd"));
    let helper = report
        .outcome()
        .unwrap()
        .features
        .iter()
        .find(|f| f.name.as_deref() == Some("DatumReference"))
        .unwrap();
    assert_eq!(helper.status, FeatureStatus::Blocked);
    assert!(helper
        .reason
        .as_deref()
        .unwrap()
        .contains("a binder used by a later feature is not mapped yet"));
}

#[test]
fn shape_binder_source_plane_is_carried_in_the_attached_sketch() {
    if !Format::FreeCadDocument.can_convert() {
        eprintln!("enable dutchman-core/freecad to run the CC0 helper regression");
        return;
    }
    let original = std::fs::read(fixture("freecad_0_21_2_F16b.FCStd")).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(original)).unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name().unwrap() == "Document.xml" {
            bytes = String::from_utf8(bytes)
                .unwrap()
                .replace("PartDesign::SubShapeBinder", "PartDesign::ShapeBinder")
                .into_bytes();
        }
        writer
            .start_file(entry.name().unwrap(), SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("shape_binder.FCStd");
    std::fs::write(&path, writer.finish().unwrap().into_inner()).unwrap();
    let model =
        dutchman_core::read(Format::FreeCadDocument, &path, &mut Report::default()).unwrap();
    let mut report = Report::default();
    let mapping = dutchman_solidcraft::mapped_history(&model, &mut report);
    // This two-Body recipe has a separate replay body-count mismatch. Verify the helper's
    // source-plane mapping independently of that unrelated body grouping.
    assert!(mapping.features.is_ok());
    let attached = mapping
        .features
        .as_ref()
        .unwrap()
        .iter()
        .find(|feature| {
            feature.get("name").and_then(serde_json::Value::as_str) == Some("Sketch001")
        })
        .unwrap();
    assert_eq!(
        attached.pointer("/plane/plane/origin"),
        Some(&serde_json::json!([0.0, 0.0, 20.0]))
    );
    let helper = mapping
        .outcomes
        .iter()
        .find(|f| f.name.as_deref() == Some("DatumReference"))
        .unwrap();
    assert_eq!(helper.status, FeatureStatus::NotNeeded);
    assert!(report.losses().iter().any(|loss| loss
        .detail
        .contains("its source plane is carried by the sketches")));
}

#[test]
fn a_body_inside_a_hidden_part_does_not_add_a_solid() {
    if !Format::FreeCadDocument.can_convert() {
        eprintln!("enable dutchman-core/freecad to run the CC0 helper regression");
        return;
    }
    let original = std::fs::read(fixture("freecad_0_21_2_F16b.FCStd")).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(original)).unwrap();
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        if entry.name().unwrap() == "Document.xml" {
            let text = String::from_utf8(bytes)
                .unwrap()
                .replace("<Objects Count=\"22\"", "<Objects Count=\"23\"")
                .replace("<ObjectData Count=\"22\"", "<ObjectData Count=\"23\"");
            let text = text.replacen("<Object type=", "<ObjectDeps Name=\"HiddenPart\" Count=\"1\"><Dep Name=\"Body\"/></ObjectDeps><Object type=", 1);
            let text = text.replacen(
                "</Objects>",
                "<Object type=\"App::Part\" name=\"HiddenPart\" id=\"1000\"/></Objects>",
                1,
            );
            bytes = text.replacen("</ObjectData>", "<Object name=\"HiddenPart\"><Properties Count=\"2\"><Property name=\"Visibility\" type=\"App::PropertyBool\"><Bool value=\"false\"/></Property><Property name=\"Group\" type=\"App::PropertyLinkList\"><LinkList count=\"1\"><Link value=\"Body\"/></LinkList></Property></Properties></Object></ObjectData>", 1).into_bytes();
        }
        writer
            .start_file(entry.name().unwrap(), SimpleFileOptions::default())
            .unwrap();
        writer.write_all(&bytes).unwrap();
    }
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("hidden_container.FCStd");
    std::fs::write(&path, writer.finish().unwrap().into_inner()).unwrap();
    let model =
        dutchman_core::read(Format::FreeCadDocument, &path, &mut Report::default()).unwrap();
    assert_eq!(model.ir.model.bodies.len(), 1);
}

#[test]
fn a_point_used_by_a_resolved_coordinate_system_keeps_history() {
    if !Format::FreeCadDocument.can_convert() {
        eprintln!("enable dutchman-core/freecad to run the CC0 helper regression");
        return;
    }
    let report = convert(&fixture("freecad_0_21_2_F18c.FCStd"));
    let outcome = report.outcome().unwrap();
    assert!(outcome
        .features
        .iter()
        .filter(|f| matches!(f.kind.as_str(), "datum_point" | "datum_coordinate_system"))
        .all(|f| f.status == FeatureStatus::NotNeeded));
    assert!(!matches!(outcome.check, Check::Flattened(_)));
    if std::env::var_os(CLI_ENV).is_some() {
        assert_eq!(outcome.check, Check::Verified);
    }
}
