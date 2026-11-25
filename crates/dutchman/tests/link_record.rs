//! The link record written beside a converted design, and the names a repeated conversion keeps.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use sha2::{Digest, Sha256};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures")
        .join(name)
}

fn dutchman(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(args)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn sha256(path: &Path) -> String {
    Sha256::digest(std::fs::read(path).unwrap())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn read_record(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

/// `in/part.f3d` (the unit cube) and an empty `out` folder, in a new scratch folder.
fn setup() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in/part.f3d");
    std::fs::create_dir_all(input.parent().unwrap()).unwrap();
    std::fs::create_dir_all(dir.path().join("out")).unwrap();
    std::fs::copy(fixture("generated_unit_cube.f3d"), &input).unwrap();
    let output = dir.path().join("out/part.sc");
    (dir, input, output)
}

fn convert(input: &Path, output: &Path, extra: &[&str]) -> Output {
    let mut args = vec![
        "convert",
        "--json",
        "--no-verify",
        "--solidcraft-version",
        "0.44.1",
    ];
    args.extend_from_slice(extra);
    args.extend([input.to_str().unwrap(), output.to_str().unwrap()]);
    dutchman(&args)
}

#[test]
fn a_conversion_writes_a_record_with_the_digests_and_a_relative_source() {
    let (_dir, input, output) = setup();
    let result = convert(&input, &output, &[]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["link_record"], "part.link.json");

    let record = read_record(&output.with_file_name("part.link.json"));
    assert_eq!(record["schema"], "dutchman.link/1");
    assert_eq!(record["source"]["path"], "../in/part.f3d");
    assert_eq!(record["source"]["sha256"], sha256(&input));
    assert_eq!(
        record["source"]["size"],
        std::fs::metadata(&input).unwrap().len()
    );
    assert!(record["source"]["modified"]
        .as_str()
        .unwrap()
        .ends_with('Z'));
    assert_eq!(record["output"]["file"], "part.sc");
    assert_eq!(record["output"]["sha256"], sha256(&output));
    assert_eq!(record["dutchman"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(record["cadmpeg"]["version"].is_string());
    assert_eq!(record["options"]["to"], "solidcraft");
    assert_eq!(record["options"]["preset"], "standard");
    assert_eq!(record["options"]["solidcraft_version"], "0.44.1");
    assert!(record["converted_at"].as_str().unwrap().ends_with('Z'));
}

#[test]
fn convert_all_writes_a_record_per_design() {
    let (dir, _input, _output) = setup();
    let out = dir.path().join("batch");
    let result = dutchman(&[
        "convert-all",
        "--json",
        "--no-verify",
        "--out-dir",
        out.to_str().unwrap(),
        dir.path().join("in").to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let design = std::fs::read_dir(&out)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| {
            path.extension()
                .is_some_and(|ext| ext == "sc" || ext == "solidcraft")
        })
        .unwrap();
    let record = read_record(&design.with_extension("link.json"));
    assert_eq!(record["source"]["path"], "../in/part.f3d");
    assert_eq!(record["output"]["sha256"], sha256(&design));
}

#[test]
fn converting_again_rewrites_the_same_record() {
    let (_dir, input, output) = setup();
    let record_path = output.with_file_name("part.link.json");
    assert!(convert(&input, &output, &[]).status.success());
    let mut first = read_record(&record_path);
    assert!(convert(&input, &output, &[]).status.success());
    let mut second = read_record(&record_path);
    for record in [&mut first, &mut second] {
        record.as_object_mut().unwrap().remove("converted_at");
        record["source"].as_object_mut().unwrap().remove("read_at");
    }
    assert_eq!(first, second);
    let leftovers: Vec<_> = std::fs::read_dir(output.parent().unwrap())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(leftovers.len(), 2, "{leftovers:?}");
}

#[test]
fn no_record_with_the_opt_out() {
    let (_dir, input, output) = setup();
    let result = convert(&input, &output, &["--no-link-record"]);
    assert!(result.status.success());
    assert!(output.exists());
    assert!(!output.with_file_name("part.link.json").exists());
}

#[test]
fn a_failed_conversion_leaves_an_existing_record_alone() {
    let (_dir, input, output) = setup();
    let record_path = output.with_file_name("part.link.json");
    assert!(convert(&input, &output, &[]).status.success());
    let before = std::fs::read(&record_path).unwrap();
    std::fs::write(&input, [0xffu8; 512]).unwrap();
    let result = convert(&input, &output, &[]);
    assert!(!result.status.success());
    assert_eq!(std::fs::read(&record_path).unwrap(), before);

    let (_dir, input, output) = setup();
    std::fs::write(&input, [0xffu8; 512]).unwrap();
    assert!(!convert(&input, &output, &[]).status.success());
    assert!(!output.with_file_name("part.link.json").exists());
}

#[test]
fn a_refused_overwrite_writes_no_record() {
    let (_dir, input, output) = setup();
    std::fs::write(&output, b"mine").unwrap();
    let result = convert(&input, &output, &["--no-clobber"]);
    assert!(!result.status.success());
    assert!(!output.with_file_name("part.link.json").exists());
}

/// A design's entries, with the keys that hold times removed: what must not change between two
/// conversions of the same source for SolidCraft's Update Links to keep placements and joints.
fn design_without_times(path: &Path) -> Vec<(String, Value)> {
    fn strip(value: &mut Value) {
        match value {
            Value::Object(fields) => {
                fields.retain(|key, _| {
                    let key = key.to_ascii_lowercase();
                    !["time", "date", "created", "modified", "saved"]
                        .iter()
                        .any(|word| key.contains(word))
                });
                fields.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let mut entries = Vec::new();
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).unwrap();
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes).unwrap();
        let name = entry.name().unwrap().to_string();
        let value = match serde_json::from_slice::<Value>(&bytes) {
            Ok(mut value) => {
                strip(&mut value);
                value
            }
            Err(_) => Value::String(format!("{} bytes", bytes.len())),
        };
        entries.push((name, value));
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

#[test]
fn converting_the_same_source_twice_keeps_every_name_and_id() {
    // Fixtures that convert to geometry; the FreeCAD product holds several components.
    #[allow(unused_mut)]
    let mut cases = vec![
        ("generated_unit_cube.f3d", "standard"),
        ("generated_unit_cube.f3d", "full"),
    ];
    #[cfg(feature = "freecad")]
    cases.push(("core_design_product.FCStd", "standard"));
    for (fixture_name, preset) in cases {
        let dir = tempfile::tempdir().unwrap();
        let extension = Path::new(fixture_name).extension().unwrap();
        let input = dir.path().join("design").with_extension(extension);
        std::fs::copy(fixture(fixture_name), &input).unwrap();
        let mut designs = Vec::new();
        for round in 0..2 {
            let output = dir.path().join(format!("round{round}.sc"));
            let result = convert(&input, &output, &["--preset", preset, "--no-link-record"]);
            // A conversion can write its design and still exit 1 (for example parts that
            // couldn't be placed); determinism is about the design written, so require that.
            assert!(
                output.exists(),
                "{fixture_name} ({preset}) wrote no design, exit {:?}: {}",
                result.status.code(),
                String::from_utf8_lossy(&result.stdout)
            );
            designs.push(design_without_times(&output));
        }
        assert!(!designs[0].is_empty(), "{fixture_name}");
        assert_eq!(designs[0], designs[1], "{fixture_name} ({preset})");
    }
}
