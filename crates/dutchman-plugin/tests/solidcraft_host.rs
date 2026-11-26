#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
//! Dutchman's package on a real SolidCraft: its own `plugin check`, `plugin test` and `exec`.
//! Run with `SOLIDCRAFT_CLI=/absolute/path/solidcraft-cli DUTCHMAN_PLUGIN_COMPONENT=/absolute/path/dutchman.wasm
//! cargo test --release -p dutchman-plugin --features host-tests --test solidcraft_host -- --nocapture`,
//! with a SolidCraft built from a commit that has plugin importer support (file first, folder
//! access, minimum version, per-user settings; SolidCraft #20425). Neither variable may be
//! missing: this test never silently skips. The package is the shipped `plugin.json` and the
//! component the release packages; nothing is stubbed.
use serde_json::{json, Value};
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Output},
};

fn variable(name: &str) -> PathBuf {
    std::env::var_os(name)
        .map(PathBuf::from)
        .unwrap_or_else(|| panic!("Set {name} to an absolute path; see this file's header"))
}
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../dutchman-core/tests/fixtures")
}
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("solidcraft-host-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The package folder, named after the plugin's id as SolidCraft expects, with a `tests`
/// folder of cases beside it.
fn package(parent: &Path) -> PathBuf {
    let folder = parent.join("dutchman");
    std::fs::create_dir_all(folder.join("icons")).unwrap();
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    std::fs::copy(crate_dir.join("plugin.json"), folder.join("plugin.json")).unwrap();
    std::fs::copy(
        crate_dir.join("icons/dutchman.svg"),
        folder.join("icons/dutchman.svg"),
    )
    .unwrap();
    std::fs::copy(
        variable("DUTCHMAN_PLUGIN_COMPONENT"),
        folder.join("dutchman.wasm"),
    )
    .unwrap();
    folder
}

fn solidcraft(args: &[&str], plugins: &Path, settings: &Path) -> Output {
    Command::new(variable("SOLIDCRAFT_CLI"))
        .args(["--plugins-dir"])
        .arg(plugins)
        .arg("--keep-plugin-approvals")
        .args(args)
        .env("SOLIDCRAFT_SETTINGS_DIR", settings)
        .output()
        .unwrap()
}

/// The package as a zip, which is what Install reads.
fn zip_of(folder: &Path, out: &Path) {
    let mut zip = zip::ZipWriter::new(std::fs::File::create(out).unwrap());
    for name in ["plugin.json", "dutchman.wasm", "icons/dutchman.svg"] {
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(&std::fs::read(folder.join(name)).unwrap())
            .unwrap();
    }
    zip.finish().unwrap();
}

/// Install as the app does after the person reviews it: the review, then the approvals.
fn install(zip: &Path, plugins: &Path, settings: &Path, approve: Value) -> Value {
    let reviewed = solidcraft(
        &[
            "exec",
            "plugin.install",
            &json!({"path": zip, "dry_run": true}).to_string(),
        ],
        plugins,
        settings,
    );
    assert!(reviewed.status.success(), "{}", text(&reviewed.stderr));
    let review: Value = serde_json::from_slice(&reviewed.stdout).unwrap();
    let installed = solidcraft(
        &[
            "exec",
            "plugin.install",
            &json!({
                "path": zip,
                "approval": review["approval"],
                "archive": review["archive"],
                "approve": approve,
            })
            .to_string(),
        ],
        plugins,
        settings,
    );
    assert!(
        installed.status.success(),
        "{}{}",
        text(&installed.stdout),
        text(&installed.stderr)
    );
    review
}

#[test]
fn solidcraft_accepts_the_package_and_registers_one_file_first_importer() {
    let dir = scratch("register");
    let folder = package(&dir);
    let checked = solidcraft(
        &["plugin", "check", &folder.to_string_lossy(), "--json"],
        &dir.join("plugins"),
        &dir.join("settings"),
    );
    assert!(
        checked.status.success(),
        "{}{}",
        text(&checked.stdout),
        text(&checked.stderr)
    );
    let report: Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(report["ok"], true, "{report}");
    // The one thing it says is that the plugin asks for more memory than the default
    // ceiling, which Install asks the person to allow.
    let problems = report["problems"].as_array().unwrap();
    assert!(
        problems
            .iter()
            .all(|p| p["severity"] == "warning" && p["field"] == "runtime.limits.memory_mib"),
        "{report}"
    );
    assert_eq!(report["id"], "dutchman");
    let zip = dir.join("dutchman-plugin.zip");
    zip_of(&folder, &zip);
    let review = install(
        &zip,
        &dir.join("plugins"),
        &dir.join("settings"),
        json!({"input_folder": true, "memory_mib": 2048, "fuel": 100_000_000_000u64}),
    );
    // The review shows the minimum version and the capabilities this SolidCraft grants.
    assert!(review["min_host_version"].is_string(), "{review}");
    let granted = review["granted_capabilities"].as_array().unwrap();
    for capability in ["folders@0.1", "user-storage@0.1", "dialog@0.1"] {
        assert!(
            granted.contains(&json!(capability)),
            "{capability}: {review}"
        );
    }
    let listed = solidcraft(
        &["exec", "plugin.list", "{}"],
        &dir.join("plugins"),
        &dir.join("settings"),
    );
    assert!(listed.status.success(), "{}", text(&listed.stderr));
    let listed: Value = serde_json::from_slice(&listed.stdout).unwrap();
    let importers = listed["importers"].as_array().unwrap();
    let ours: Vec<_> = importers
        .iter()
        .filter(|i| i["id"] == "dutchman.convert")
        .collect();
    assert_eq!(ours.len(), 1, "{listed}");
    let mut extensions: Vec<_> = ours[0]["extensions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_owned())
        .collect();
    extensions.sort();
    assert_eq!(
        extensions,
        ["f3d", "f3z", "fcstd", "iam", "ipt", "sldasm", "sldprt"]
    );
    // The manifest's file-first request reached SolidCraft's entry.
    let installed = listed["plugins"][0]["manifest"]["runtime"]["entries"][0].clone();
    assert_eq!(installed["file_first"], true, "{listed}");
}

fn write_case(folder: &Path, name: &str, case: Value) {
    std::fs::create_dir_all(folder.join("tests")).unwrap();
    std::fs::write(
        folder.join("tests").join(format!("{name}.json")),
        case.to_string(),
    )
    .unwrap();
}

#[test]
fn a_file_handed_over_directly_opens_and_folder_denials_name_the_missing_grant() {
    let dir = scratch("cases");
    let folder = package(&dir);
    let tests = folder.join("tests");
    std::fs::create_dir_all(&tests).unwrap();
    let fixtures = fixtures();
    for (from, to) in [
        ("generated_unit_cube.f3d", "cube.f3d"),
        ("placed_cubes.sldasm", "placed_cubes.sldasm"),
        ("solidworks_cube.sldprt", "solidworks_cube.sldprt"),
        ("freecad-1.1.4-A01/A01a.FCStd", "A01a.FCStd"),
        ("freecad-1.1.4-A01/P01a.FCStd", "P01a.FCStd"),
        ("freecad-1.1.4-A01/P02a.FCStd", "P02a.FCStd"),
    ] {
        std::fs::copy(fixtures.join(from), tests.join(to)).unwrap();
    }
    // A file handed straight to the importer, with no options: the defaults.
    write_case(
        &folder,
        "file-handed-over-directly",
        json!({"entry":"convert","file":"cube.f3d","expect":{"reply":{"bodies":1}}}),
    );
    // A part file beside the assembly, with the install approval for the input folder.
    write_case(
        &folder,
        "assembly-with-its-input-folder",
        json!({"entry":"convert","file":"A01a.FCStd","params":{"editable_history":false}}),
    );
    // Without that approval, an assembly stops before reading and says which grant is missing.
    write_case(
        &folder,
        "assembly-without-the-input-folder",
        json!({"entry":"convert","file":"placed_cubes.sldasm","grants":{"memory_mib":2048},
            "expect":{"error":"permission_denied",
                "message":"needs the input_folder permission"}}),
    );
    write_case(
        &folder,
        "freecad-parts-without-the-input-folder",
        json!({"entry":"convert","file":"A01a.FCStd","grants":{"memory_mib":2048},
            "expect":{"error":"permission_denied","message":"needs the input_folder permission"}}),
    );
    // A script cannot grant the picked folder by naming it: only the dialog does. SolidCraft
    // refuses the call itself, before the plugin runs, and says no folder grant applies.
    write_case(
        &folder,
        "parts-folder-named-by-a-script",
        json!({"entry":"convert","file":"A01a.FCStd",
            "params":{"parts_folder": tests.to_string_lossy()},
            "expect":{"error":"permission_denied","message":"no applicable folder grant"}}),
    );
    let output = solidcraft(
        &["plugin", "test", &folder.to_string_lossy(), "--json"],
        &dir.join("plugins"),
        &dir.join("settings"),
    );
    let report: Value = serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}{}", text(&output.stdout), text(&output.stderr)));
    assert!(
        output.status.success(),
        "{report:#}\n{}",
        text(&output.stderr)
    );
    assert_eq!(
        (report["passed"].as_u64(), report["failed"].as_u64()),
        (Some(5), Some(0)),
        "{report:#}"
    );
}

/// The options of an accepted import are kept for the person, and only by an accepted one.
#[test]
fn per_user_settings_persist_after_an_accepted_import_only() {
    let dir = scratch("settings");
    let folder = package(&dir);
    let zip = dir.join("dutchman-plugin.zip");
    zip_of(&folder, &zip);
    let (plugins, settings) = (dir.join("plugins"), dir.join("settings"));
    install(
        &zip,
        &plugins,
        &settings,
        json!({"input_folder": true, "memory_mib": 2048, "fuel": 100_000_000_000u64}),
    );
    let cube = dir.join("cube.f3d");
    std::fs::copy(fixtures().join("generated_unit_cube.f3d"), &cube).unwrap();
    let broken = dir.join("broken.f3d");
    std::fs::write(&broken, vec![0u8; 64]).unwrap();
    let record = settings.join("plugin-user-data/dutchman.json");
    let import = |path: &Path, options: Value| {
        solidcraft(
            &[
                "exec",
                "file.import",
                &json!({"path": path, "format": "dutchman.convert", "options": options})
                    .to_string(),
            ],
            &plugins,
            &settings,
        )
    };
    let saved = || -> Value { serde_json::from_slice(&std::fs::read(&record).unwrap()).unwrap() };
    assert!(!record.exists(), "nothing is saved before an import");
    // An import SolidCraft accepts saves the options it ran with.
    let first =
        json!({"editable_history": false, "up_axis": "stand_up", "include_hidden_bodies": true});
    let done = import(&cube, first.clone());
    assert!(
        done.status.success(),
        "{}{}",
        text(&done.stdout),
        text(&done.stderr)
    );
    assert_eq!(saved(), json!({"options": first}));
    // A failed import changes nothing.
    let failed = import(&broken, json!({"editable_history": true}));
    assert!(!failed.status.success());
    assert_eq!(saved(), json!({"options": first}));
    // The next accepted import replaces them, without keeping a folder the person picked.
    let second =
        json!({"editable_history": true, "up_axis": "as_is", "include_hidden_bodies": false});
    let done = import(&cube, second.clone());
    assert!(
        done.status.success(),
        "{}{}",
        text(&done.stdout),
        text(&done.stderr)
    );
    assert_eq!(saved(), json!({"options": second}));
}
