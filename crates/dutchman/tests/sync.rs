//! `dutchman sync`: converting again only the sources that changed, and keeping the previous
//! design when that fails.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::io::{BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::time::Duration;

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

fn text(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// `in/part.f3d` (the unit cube) converted to `out/part.sc` with its link record, in a new
/// scratch folder.
fn converted() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("in/part.f3d");
    std::fs::create_dir_all(input.parent().unwrap()).unwrap();
    std::fs::create_dir_all(dir.path().join("out")).unwrap();
    std::fs::copy(fixture("generated_unit_cube.f3d"), &input).unwrap();
    let output = dir.path().join("out/part.sc");
    let result = dutchman(&[
        "convert",
        "--json",
        "--no-verify",
        "--solidcraft-version",
        "0.44.1",
        input.to_str().unwrap(),
        output.to_str().unwrap(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(output.with_file_name("part.link.json").is_file());
    (dir, input, output)
}

fn sync(dir: &tempfile::TempDir, extra: &[&str]) -> Output {
    let mut args = vec!["sync", "--no-verify"];
    args.extend_from_slice(extra);
    let out = dir.path().join("out");
    args.push(out.to_str().unwrap());
    dutchman(&args)
}

/// The same design in a different file: every entry copied, and one more that nothing reads, so
/// the source's bytes change and what is converted from it does not.
fn resave(from: &Path, to: &Path) {
    let mut archive = zip::ZipArchive::new(std::fs::File::open(from).unwrap()).unwrap();
    let mut writer = zip::ZipWriter::new(std::fs::File::create(to).unwrap());
    for index in 0..archive.len() {
        writer
            .raw_copy_file(archive.by_index_raw(index).unwrap())
            .unwrap();
    }
    writer
        .start_file(
            "zz-saved-again.txt",
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
    writer.write_all(b"saved again").unwrap();
    writer.finish().unwrap();
}

/// A design's entries, with the keys that hold times removed (the shape of the determinism check
/// in `link_record.rs`).
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

fn folder_listing(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn an_unchanged_source_converts_nothing() {
    let (dir, _input, output) = converted();
    let record_path = output.with_file_name("part.link.json");
    let (design, record) = (
        std::fs::read(&output).unwrap(),
        std::fs::read(&record_path).unwrap(),
    );
    let result = sync(&dir, &[]);
    assert!(result.status.success(), "{}", text(&result));
    assert_eq!(text(&result).lines().count(), 1, "{}", text(&result));
    assert!(
        text(&result).starts_with("unchanged part.sc"),
        "{}",
        text(&result)
    );
    // Not even rewritten: a conversion would have changed the record's time.
    assert_eq!(std::fs::read(&output).unwrap(), design);
    assert_eq!(std::fs::read(&record_path).unwrap(), record);
}

#[test]
fn a_changed_source_replaces_the_design_and_keeps_every_name_and_id() {
    let (dir, input, output) = converted();
    let record_path = output.with_file_name("part.link.json");
    let before = design_without_times(&output);
    let old_record = read_record(&record_path);
    let saved = dir.path().join("saved.f3d");
    resave(&input, &saved);
    std::fs::rename(&saved, &input).unwrap();

    let result = sync(&dir, &[]);
    assert!(result.status.success(), "{}", text(&result));
    assert!(
        text(&result).starts_with("updated part.sc"),
        "{}",
        text(&result)
    );
    assert_eq!(design_without_times(&output), before);

    let record = read_record(&record_path);
    assert_eq!(record["source"]["sha256"], sha256(&input));
    assert_ne!(record["source"]["sha256"], old_record["source"]["sha256"]);
    assert_eq!(record["output"]["sha256"], sha256(&output));
    assert_eq!(record["source"]["path"], "../in/part.f3d");
    assert_eq!(record["options"], old_record["options"]);
    // No staging folder is left behind.
    assert_eq!(
        folder_listing(&dir.path().join("out")),
        ["part.link.json", "part.sc"]
    );

    let again = sync(&dir, &[]);
    assert!(
        text(&again).starts_with("unchanged part.sc"),
        "{}",
        text(&again)
    );
}

#[test]
fn a_failing_conversion_keeps_the_previous_design_and_record() {
    let (dir, input, output) = converted();
    let record_path = output.with_file_name("part.link.json");
    let (design, record) = (
        std::fs::read(&output).unwrap(),
        std::fs::read(&record_path).unwrap(),
    );
    std::fs::write(&input, [0xffu8; 512]).unwrap();

    let result = sync(&dir, &[]);
    assert!(!result.status.success());
    assert!(
        text(&result).starts_with("failed part.sc"),
        "{}",
        text(&result)
    );
    assert_eq!(std::fs::read(&output).unwrap(), design);
    assert_eq!(std::fs::read(&record_path).unwrap(), record);
    assert_eq!(
        folder_listing(&dir.path().join("out")),
        ["part.link.json", "part.sc"]
    );
}

#[test]
fn a_missing_source_is_reported_and_nothing_changes() {
    let (dir, input, output) = converted();
    let record_path = output.with_file_name("part.link.json");
    let (design, record) = (
        std::fs::read(&output).unwrap(),
        std::fs::read(&record_path).unwrap(),
    );
    std::fs::remove_file(&input).unwrap();

    let result = sync(&dir, &[]);
    assert!(result.status.success(), "{}", text(&result));
    assert!(
        text(&result).starts_with("source missing part.sc"),
        "{}",
        text(&result)
    );
    assert_eq!(std::fs::read(&output).unwrap(), design);
    assert_eq!(std::fs::read(&record_path).unwrap(), record);
}

#[test]
fn records_are_found_in_subfolders() {
    let (dir, _input, _output) = converted();
    let nested = dir.path().join("out/more/deeper");
    std::fs::create_dir_all(&nested).unwrap();
    for name in ["part.sc", "part.link.json"] {
        std::fs::copy(dir.path().join("out").join(name), nested.join(name)).unwrap();
    }
    // The copy's source is named relative to its own folder, so it is missing there.
    let result = sync(&dir, &["--json"]);
    let events: Vec<Value> = text(&result)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(events.len(), 2, "{}", text(&result));
    let mut seen: Vec<_> = events
        .iter()
        .map(|event| {
            (
                event["path"].as_str().unwrap().replace('\\', "/"),
                event["event"].as_str().unwrap(),
            )
        })
        .collect();
    seen.sort();
    assert_eq!(
        seen,
        [
            ("more/deeper/part.sc".to_owned(), "source_missing"),
            ("part.sc".to_owned(), "unchanged"),
        ]
    );
}

#[test]
fn json_events_carry_the_schema_and_the_conversion_report() {
    let (dir, input, _output) = converted();
    let result = sync(&dir, &["--json"]);
    let lines: Vec<Value> = text(&result)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0]["schema"], "dutchman.sync-event/1");
    assert_eq!(lines[0]["event"], "unchanged");
    assert_eq!(lines[0]["path"], "part.sc");
    assert!(lines[0]["elapsed_ms"].is_u64());

    let saved = dir.path().join("saved.f3d");
    resave(&input, &saved);
    std::fs::rename(&saved, &input).unwrap();
    let result = sync(&dir, &["--json"]);
    let event: Value = serde_json::from_str(text(&result).lines().next().unwrap()).unwrap();
    assert_eq!(event["schema"], "dutchman.sync-event/1");
    assert_eq!(event["event"], "updated");
    assert_eq!(event["report"]["ok"], true);

    std::fs::write(&input, [0xffu8; 512]).unwrap();
    let result = sync(&dir, &["--json"]);
    let event: Value = serde_json::from_str(text(&result).lines().next().unwrap()).unwrap();
    assert_eq!(event["event"], "failed");
    assert_eq!(event["report"]["ok"], false);
    assert!(!event["message"].as_str().unwrap().is_empty());
}

#[test]
fn the_record_names_the_search_folders_and_options_a_sync_converts_with() {
    let (dir, input, output) = converted();
    let projects = dir.path().join("projects");
    std::fs::create_dir_all(projects.join("extra")).unwrap();
    let result = dutchman(&[
        "convert",
        "--json",
        "--no-verify",
        "--solidcraft-version",
        "0.44.1",
        "--preset",
        "full",
        "--project",
        projects.to_str().unwrap(),
        "--search",
        projects.join("extra").to_str().unwrap(),
        input.to_str().unwrap(),
        output.to_str().unwrap(),
    ]);
    assert!(result.status.success(), "{}", text(&result));
    let record_path = output.with_file_name("part.link.json");
    let record = read_record(&record_path);
    assert_eq!(record["options"]["project"], "../projects");
    assert_eq!(record["options"]["search"][0], "../projects/extra");

    // A sync converts with them, and the record keeps them.
    let saved = dir.path().join("saved.f3d");
    resave(&input, &saved);
    std::fs::rename(&saved, &input).unwrap();
    let result = sync(&dir, &[]);
    assert!(
        text(&result).starts_with("updated part.sc"),
        "{}",
        text(&result)
    );
    let after = read_record(&record_path);
    assert_eq!(after["options"], record["options"]);
    assert_eq!(after["options"]["preset"], "full");
}

/// Lines of `child`'s output, as they come.
fn lines_of(child: &mut std::process::Child) -> mpsc::Receiver<String> {
    let stdout = child.stdout.take().unwrap();
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
        {
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    receiver
}

#[test]
fn watching_converts_a_saved_source_once_it_settles() {
    let (dir, input, output) = converted();
    let before = design_without_times(&output);
    let out = dir.path().join("out");
    let mut child = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args([
            "sync",
            "--watch",
            "--no-verify",
            "--json",
            "--interval",
            "1",
            "--settle",
            "1",
            out.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let lines = lines_of(&mut child);
    let next = |wait: u64| -> Value {
        let line = lines
            .recv_timeout(Duration::from_secs(wait))
            .expect("the watch printed no event in time");
        serde_json::from_str(&line).unwrap()
    };

    // At start it reports what it found.
    let first = next(60);
    assert_eq!(first["schema"], "dutchman.sync-event/1");
    assert_eq!(first["event"], "unchanged");

    let saved = dir.path().join("saved.f3d");
    resave(&input, &saved);
    std::fs::rename(&saved, &input).unwrap();
    let second = next(120);
    assert_eq!(second["event"], "updated", "{second}");
    assert_eq!(second["path"], "part.sc");
    assert_eq!(design_without_times(&output), before);
    assert_eq!(
        read_record(&output.with_file_name("part.link.json"))["source"]["sha256"],
        sha256(&input)
    );

    // A save that cannot be converted is reported once, and the design stays.
    let design = std::fs::read(&output).unwrap();
    std::fs::write(&input, [0xffu8; 512]).unwrap();
    let third = next(120);
    assert_eq!(third["event"], "failed", "{third}");
    assert_eq!(std::fs::read(&output).unwrap(), design);
    assert!(lines.recv_timeout(Duration::from_secs(4)).is_err());

    child.kill().unwrap();
    child.wait().unwrap();
}
