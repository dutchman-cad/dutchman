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
    // SolidCraft refuses any hidden name in a folder read, and a refusal ends the import, so
    // a stored path that goes through one (a synced drive's `.shortcut-targets-by-id`) is never
    // asked for: the shorter tails are, and the part beside the assembly is found.
    let asked = std::cell::RefCell::new(Vec::new());
    let found = resolve(
        r"C:\Drive\.shortcut-targets-by-id\abc\Parts\Swerve\Gear.iam",
        Path::new(""),
        &|p| {
            asked.borrow_mut().push(p.to_path_buf());
            p == Path::new("Gear.iam")
        },
    );
    assert_eq!(found, Some(std::path::PathBuf::from("Gear.iam")));
    assert!(asked
        .borrow()
        .iter()
        .all(|p| !p.to_string_lossy().contains("shortcut")));
    assert_eq!(
        resolve(".part.ipt", Path::new(""), &|p| p == Path::new(".part.ipt")),
        None
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
        message.starts_with("This design is too large to open inside SolidCraft: it needs about "),
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
        "Open Fusion, Inventor, SolidWorks or FreeCAD files with File > Open. Dutchman is in active development and provided as is. Keep a copy of your designs somewhere else, just in case."
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

#[test]
fn assembly_parts_are_read_once_each_and_only_after_the_assembly() {
    use std::cell::RefCell;
    use std::path::PathBuf;
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/freecad-1.1.4-A01");
    let asked: RefCell<Vec<PathBuf>> = RefCell::new(Vec::new());
    // The reader hands out one file's bytes per call and keeps none: the plugin holds a part's
    // bytes only while the core decodes that part, so no two parts' bytes are held together.
    let reader = |path: &Path, limit: u64| {
        asked.borrow_mut().push(path.into());
        let bytes = std::fs::read(dir.join(path))
            .map_err(|e| dutchman_core::Error::Decode(e.to_string()))?;
        assert!(bytes.len() as u64 <= limit);
        Ok(bytes)
    };
    let resolve = |stored: &str, folder: &Path| resolve(stored, folder, &|c| dir.join(c).is_file());
    let (design, _) = convert("A01a.FCStd", Options::default(), &reader, &resolve, None).unwrap();
    assert!(!design.is_empty());
    let asked = asked.into_inner();
    assert_eq!(
        asked[0],
        Path::new("A01a.FCStd"),
        "the assembly comes first"
    );
    assert!(asked.len() >= 2, "{asked:?}");
    let mut unique = asked.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), asked.len(), "a file is read once: {asked:?}");
}

#[test]
fn a_failure_carries_the_full_convert_report_with_its_reason_codes() {
    use dutchman_core::Error;
    let error = Error::Decode("Parts/bracket.ipt: feature Fillet1 is not supported yet".into());
    let report = crate::notice::failure_report("bracket.ipt", &error);
    assert_eq!(report["schema"], "dutchman.convert/1");
    assert_eq!(report["ok"], false);
    assert_eq!(report["input"]["path"], "bracket.ipt");
    assert_eq!(report["input"]["extension"], "ipt");
    assert!(report["input"]["format_name"].is_string());
    assert_eq!(report["error"]["kind"], "decode");
    assert!(report["error"]["stage"].is_string());
    assert!(report["error"].get("upstream").is_some());
    assert!(!report.to_string().contains("Parts/"), "{report}");
    // The message holds the same report, after the plain reason and the help links.
    let text = crate::notice::failure_in("bracket.ipt", &error);
    assert!(text.starts_with("feature Fillet1 is not supported yet") || text.contains("Fillet1"));
    assert!(text.contains("bracket.failed.json"), "{text}");
    assert!(
        text.contains("dutchman.convert/1") && text.contains("Dutchman app"),
        "{text}"
    );
    assert!(text.contains(crate::preflight::DOWNLOAD), "{text}");
    let last = text.lines().last().unwrap();
    assert_eq!(serde_json::from_str::<Value>(last).unwrap(), report);
    // A size refusal stays one plain line that points to the app.
    let big = Error::MemoryBudget("This design is too large".into());
    assert_eq!(
        crate::notice::failure_in("big.iam", &big),
        crate::notice::failure(&big)
    );
    // A very long reason never makes the report unbounded.
    let long = Error::Decode("x ".repeat(50_000));
    assert!(crate::notice::failure_in("a.ipt", &long).len() < 6_000);
}

fn manifest() -> Value {
    serde_json::from_str(include_str!("../plugin.json")).unwrap()
}

#[test]
fn manifest_registers_one_importer_for_every_format_dutchman_reads() {
    let manifest = manifest();
    let entries = manifest["runtime"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    let entry = &entries[0];
    assert_eq!(
        (entry["name"].as_str(), entry["kind"].as_str()),
        (Some("convert"), Some("importer"))
    );
    let extensions: Vec<_> = entry["extensions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap())
        .collect();
    assert_eq!(
        extensions,
        ["f3d", "f3z", "ipt", "iam", "sldprt", "sldasm", "fcstd"]
    );
    // Each registered extension is a file the plugin converts, and the other way round.
    for extension in &extensions {
        let name = format!("design.{extension}");
        let format = dutchman_core::Format::from_path(Path::new(&name)).unwrap();
        assert!(
            !matches!(format, dutchman_core::Format::Step),
            "{extension}: not a native CAD format"
        );
    }
    // The manifest has an input form for exactly the plugin's options and nothing else.
    let mut properties: Vec<_> = entry["input_schema"]["properties"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    properties.sort();
    assert_eq!(
        properties,
        [
            "editable_history",
            "include_hidden_bodies",
            "parts_folder",
            "up_axis"
        ]
    );
}

#[test]
fn manifest_asks_for_the_file_first_and_a_read_only_parts_folder() {
    let manifest = manifest();
    let entry = &manifest["runtime"]["entries"][0];
    assert_eq!(entry["file_first"], true);
    let folder = &entry["input_schema"]["properties"][crate::PARTS_FOLDER];
    assert_eq!(folder["type"], "string");
    assert_eq!(folder["format"], "directory");
    // Read only, and never a default: SolidCraft counts only a folder the person picks.
    assert_eq!(folder["x-solidcraft-grant"], "read");
    assert!(folder.get("default").is_none());
    // The install grant for assemblies whose parts sit beside them stays.
    assert_eq!(manifest["runtime"]["permissions"]["input_folder"], true);
    assert!(manifest["runtime"]["permissions"].get("files").is_none());
}

#[test]
fn manifest_names_the_oldest_solidcraft_that_serves_what_it_uses() {
    let manifest = manifest();
    let minimum = manifest["min_host_version"].as_str().unwrap();
    // A strict semantic version with a patch number, as SolidCraft requires.
    let parts: Vec<_> = minimum.split('.').collect();
    assert_eq!(parts.len(), 3, "{minimum}");
    assert!(parts.iter().all(|p| p.parse::<u32>().is_ok()), "{minimum}");
    // Importer support (#20425) first shipped in SolidCraft 0.50.0; 0.49.1 lacks it.
    let numbers: Vec<u32> = parts.iter().map(|p| p.parse().unwrap()).collect();
    assert!(numbers >= vec![0, 50, 0], "{minimum}");
    let capabilities: Vec<_> = manifest["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c.as_str().unwrap())
        .collect();
    // Everything the component imports or exports is required: a static import cannot be optional.
    assert_eq!(
        capabilities,
        [
            "dialog@0.1",
            "evaluate@0.2",
            "folders@0.1",
            "formats@0.2",
            "user-storage@0.1"
        ]
    );
    assert!(manifest.get("optional_capabilities").is_none());
}

#[test]
fn only_the_three_dialog_options_are_saved_and_offered() {
    use crate::settings::{encode, field_defaults};
    let options = Options {
        editable_history: false,
        up_axis: crate::UpAxis::StandUp,
        include_hidden_bodies: true,
        parts_folder: Some("/a/folder/that/is/not/saved".into()),
    };
    let saved = encode(&options);
    assert_eq!(
        serde_json::from_str::<Value>(&saved).unwrap(),
        serde_json::json!({"editable_history":false,"up_axis":"stand_up","include_hidden_bodies":true})
    );
    // What is saved comes back as the form's defaults, and the manifest offers every one.
    let offered: Value = serde_json::from_str(&field_defaults(Some(&saved))).unwrap();
    let schema = &manifest()["runtime"]["entries"][0]["input_schema"]["properties"];
    for (field, default) in [
        ("editable_history", serde_json::json!(false)),
        ("up_axis", serde_json::json!("stand_up")),
        ("include_hidden_bodies", serde_json::json!(true)),
    ] {
        assert_eq!(offered["fields"][field]["default"], default);
        assert!(schema.get(field).is_some());
    }
    assert!(offered["fields"].get(crate::PARTS_FOLDER).is_none());
    assert!(
        serde_json::from_str::<Options>(&saved).is_ok(),
        "stored options are valid options"
    );
    // The automatic axis, the default, is saved and offered like the others.
    let automatic = encode(&Options::default());
    assert_eq!(
        serde_json::from_str::<Value>(&field_defaults(Some(&automatic))).unwrap()["fields"]
            ["up_axis"]["default"],
        "auto"
    );
    // Nothing, damage or unknown values change nothing; one valid value still applies.
    for stored in [
        None,
        Some("not json"),
        Some("[]"),
        Some("{}"),
        Some(r#"{"up_axis":"left"}"#),
    ] {
        assert_eq!(field_defaults(stored), r#"{"fields":{}}"#, "{stored:?}");
    }
    assert_eq!(
        field_defaults(Some(
            r#"{"editable_history":"yes","include_hidden_bodies":false}"#
        )),
        r#"{"fields":{"include_hidden_bodies":{"default":false}}}"#
    );
}

#[test]
fn picked_folder_refusal_names_its_own_grant_and_not_the_install_grant() {
    let text = crate::notice::parts_folder_access("rig.sldasm", "no approved directory");
    for needle in [
        "parts folder for rig.sldasm",
        "\"Folder with the parts\"",
        "not granted",
        "never writes",
        "no approved directory",
    ] {
        assert!(text.contains(needle), "{needle}: {text}");
    }
    assert!(!text.contains("input folder"), "{text}");
    // The field's title in the message is the title in the manifest.
    assert_eq!(
        manifest()["runtime"]["entries"][0]["input_schema"]["properties"][crate::PARTS_FOLDER]
            ["title"],
        "Folder with the parts"
    );
}
