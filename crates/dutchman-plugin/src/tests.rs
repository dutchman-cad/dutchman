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
    assert_eq!(details["schema"], "dutchman.plugin.report/2");
    assert!(details["shape_check"]["result"] != "Checked against the original shape");
    // The default is automatic, and a design whose source states no up axis keeps its axes.
    assert!(matches!(Options::default().up_axis, crate::UpAxis::Auto));
    assert!(matches!(
        serde_json::from_str::<Options>("{}").unwrap().up_axis,
        crate::UpAxis::Auto
    ));
    let (kept, kept_report) = run(r#"{"up_axis":"as_is"}"#);
    assert_eq!(document(&kept), document(&design));
    let kept_report: Value = serde_json::from_str(&kept_report).unwrap();
    assert!(kept_report
        .to_string()
        .contains("Kept the file's axes as asked"));
    let (stood, stood_report) = run(r#"{"up_axis":"stand_up"}"#);
    assert!(stood_report.contains("Stood up as asked"));
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
    let verification = &report["details"]["shape_check"];
    assert_eq!(verification["result"], "No editable history to check");
    assert_eq!(verification["reason"], crate::EDITABLE_HISTORY_OFF);
    assert!(report["summary"]
        .as_str()
        .unwrap()
        .starts_with("Converted, no history"));
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

#[test]
fn large_synthetic_design_is_refused_before_native_decoding() {
    use std::{cell::Cell, io::Write, time::Instant};
    // This is deliberately not a CAD model: decoding it would fail with no geometry.
    // The ZIP directory advertises a large stream without allocating its contents.
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("synthetic", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"not native CAD").unwrap();
    let mut data = zip.finish().unwrap().into_inner();
    let directory = data.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    data[directory + 24..directory + 28].copy_from_slice(&400_000_000u32.to_le_bytes());
    let reads = Cell::new(0);
    let start = Instant::now();
    let error = convert(
        "synthetic.f3d",
        Options::default(),
        &|_, _| {
            reads.set(reads.get() + 1);
            Ok(data.clone())
        },
        &|_, _| panic!("Refused before assembly resolution"),
        None,
    )
    .unwrap_err();
    assert!(
        matches!(error, dutchman_core::Error::MemoryBudget(_)),
        "{error}"
    );
    let message = crate::notice::failure(&error);
    assert!(
        message.starts_with(
            "This design is too large to open inside SolidCraft (about 1 part). Open it in the Dutchman app"
        ),
        "{message}"
    );
    assert!(message
        .contains("Open it in the Dutchman app, which converts it to a .sc you can open here."));
    assert!(message.contains(crate::preflight::DOWNLOAD));
    assert_eq!(reads.get(), 1);
    assert!(start.elapsed() < std::time::Duration::from_secs(2));
}

#[test]
fn report_and_failure_offer_discord_before_github_without_archive_paths() {
    let reason =
        "Feature Fillet1: FusionAssetName[Active]/Breps.BlobParts/opaque is not supported yet";
    let failure = crate::notice::failure(&dutchman_core::Error::Decode(reason.into()));
    assert!(failure.contains("Fillet1"));
    assert!(!failure.contains("FusionAssetName"));
    assert!(!failure.contains("BlobParts"));
    assert!(failure.contains(
        "If this file opens in the app that made it, it's a bug in Dutchman. Help us fix it:"
    ));
    let (_, report) = convert(
        "cube.f3d",
        Options::default(),
        &|_, _| Ok(CUBE.into()),
        &|_, _| None,
        None,
    )
    .unwrap();
    let report: Value = serde_json::from_str(&report).unwrap();
    for text in [
        failure,
        report["details"]["help"].as_str().unwrap().to_owned(),
    ] {
        assert!(text.contains(crate::notice::QUESTION));
        assert!(text.find("Join our Discord").unwrap() < text.find("DM Dutchy").unwrap());
        assert!(text.find("DM Dutchy").unwrap() < text.find("GitHub issue").unwrap());
        assert!(text.contains("https://discord.gg/yTcseBEdeu"));
        assert!(text.contains("https://discord.com/users/1563591265810129088"));
    }
    let details = report["details"].to_string();
    assert!(!details.contains("not_applicable"));
    assert!(report["details"].get("mode").is_none());
    assert!(!report["items"].to_string().contains("Notes about"));
}

#[test]
fn all_conversion_results_use_the_desktop_words() {
    use dutchman_core::{Check, Mode, Outcome, Report};
    for (mode, check, expected) in [
        (Mode::History, Check::Verified, "Converted"),
        (
            Mode::PartialHistory,
            Check::Unverified("Not checked".into()),
            "Partly converted",
        ),
        (
            Mode::Import,
            Check::NotApplicable(crate::EDITABLE_HISTORY_OFF.into()),
            "Converted, no history",
        ),
        (
            Mode::History,
            Check::Flattened("Original shape retained".into()),
            "Converted, no history",
        ),
    ] {
        let mut report = Report::default();
        report.set_outcome(Outcome {
            mode,
            check,
            features: Vec::new(),
        });
        let host = crate::report::host(&report);
        assert_eq!(host["summary"], expected);
        assert_eq!(host["details"]["result"], expected);
    }
}

#[test]
fn manifest_entries_reference_the_packaged_ship_icon() {
    let manifest: Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
    for entry in manifest["runtime"]["entries"].as_array().unwrap() {
        assert_eq!(
            entry["input_schema"]["x-solidcraft-icon"],
            "icons/dutchman.svg"
        );
    }
    let icon = include_str!("../icons/dutchman.svg");
    assert!(icon.len() <= 64 * 1024);
    assert!(icon.contains("viewBox=\"0 0 64 64\""));
}

#[test]
fn manifest_tells_people_where_to_open_files() {
    let manifest: Value = serde_json::from_str(include_str!("../plugin.json")).unwrap();
    assert_eq!(
        manifest["description"],
        "Open Fusion, Inventor, SolidWorks or FreeCAD files with File > Open."
    );
}

#[test]
fn loss_rows_keep_readable_feature_names_and_match_desktop_counts() {
    use dutchman_core::{Report, Severity};
    let mut report = Report::default();
    report.push(Severity::Info, "Sketch1", "Sketch retained");
    report.push(
        Severity::Degraded,
        "Fillet1",
        "FusionAssetName[Active]/Breps.BlobParts/opaque is not supported yet",
    );
    report.push(Severity::Dropped, "Hole1", "Hole left out");
    let host = crate::report::host(&report);
    assert_eq!(
        host["summary"],
        "Converted; Losses: 1 Info, 1 Degraded, 1 Dropped"
    );
    assert_eq!(host["items"][1]["subject"], "Fillet1");
    assert!(!host.to_string().contains("FusionAssetName"));
    assert!(!host.to_string().contains("BlobParts"));
    assert!(!host.to_string().contains("Notes about"));
}

#[test]
fn failures_keep_a_readable_part_name_without_its_folders() {
    let text = crate::notice::plain(r"Parts/bracket.ipt: feature Fillet1 is not supported yet");
    assert!(text.contains("bracket.ipt"));
    assert!(text.contains("Fillet1"));
    assert!(!text.contains("Parts/"));
}

#[test]
fn folder_refusal_names_the_grant_and_its_purpose() {
    assert!(crate::notice::needs_part_folder("Frame.IAM"));
    assert!(crate::notice::needs_part_folder("rig.sldasm"));
    assert!(!crate::notice::needs_part_folder("bracket.ipt"));
    assert!(!crate::notice::needs_part_folder("arm.f3z"));
    let text = crate::notice::folder_access("rig.sldasm", "Input folder only");
    for needle in [
        "rig.sldasm",
        "part files",
        "read access to the input folder",
        "never writes",
        "Preferences > Plugins",
        "Input folder only",
    ] {
        assert!(text.contains(needle), "{needle}: {text}");
    }
}
