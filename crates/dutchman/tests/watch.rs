#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, SystemTime};

fn cube(path: &Path) {
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../dutchman-core/tests/fixtures/generated_unit_cube.f3d"
        ),
        path,
    )
    .unwrap();
}

fn watch(input: &Path, output: &Path, settle: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("watch")
        .arg(input)
        .arg("--out-dir")
        .arg(output)
        .args([
            "--once",
            "--settle",
            settle,
            "--no-verify",
            "--json",
            "--jobs",
            "2",
        ])
        .output()
        .unwrap()
}

fn events(output: &Output) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn nested_conversion_hash_restart_and_changed_content() {
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("in");
    let out = root.path().join("out");
    std::fs::create_dir_all(inbox.join("nested")).unwrap();
    let input = inbox.join("nested/cube.f3d");
    cube(&input);
    let first = watch(&inbox, &out, "0");
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(events(&first)[0]["event"], "converted");
    assert_eq!(events(&first)[0]["schema"], "dutchman.watch-event/1");
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("nested/cube.report.json")).unwrap())
            .unwrap();
    assert_eq!(report["schema"], "dutchman.convert/1");
    assert_eq!(
        report["output"]["path"],
        std::fs::canonicalize(out.join("nested/cube.solidcraft"))
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert!(out.join("nested/cube.solidcraft").is_file());
    assert!(events(&watch(&inbox, &out, "0")).is_empty());
    std::fs::File::options()
        .write(true)
        .open(&input)
        .unwrap()
        .set_times(
            std::fs::FileTimes::new().set_modified(SystemTime::UNIX_EPOCH + Duration::from_secs(1)),
        )
        .unwrap();
    assert!(events(&watch(&inbox, &out, "0")).is_empty());
    // Trailing bytes change the content identity without changing the ZIP's design.
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&input)
        .unwrap()
        .write_all(b"changed")
        .unwrap();
    let changed = watch(&inbox, &out, "0");
    assert!(
        changed.status.success(),
        "{}",
        String::from_utf8_lossy(&changed.stderr)
    );
    assert_eq!(events(&changed)[0]["event"], "converted");
    assert!(events(&watch(&inbox, &out, "0")).is_empty());
}

#[test]
fn ignored_uploads_and_recent_or_future_modifications_are_not_ready() {
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("in");
    let out = root.path().join("out");
    std::fs::create_dir_all(inbox.join(".hidden")).unwrap();
    for name in [
        "cube.f3d",
        ".secret.f3d",
        "~$cube.f3d",
        "cube.f3d.part",
        "cube.tmp",
        "cube.crdownload",
        "unknown.xyz",
        ".hidden/cube.f3d",
    ] {
        cube(&inbox.join(name));
    }
    let future = SystemTime::now() + Duration::from_secs(60);
    std::fs::File::options()
        .write(true)
        .open(inbox.join("cube.f3d"))
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(future))
        .unwrap();
    assert!(events(&watch(&inbox, &out, "10")).is_empty());
    std::fs::File::options()
        .write(true)
        .open(inbox.join("cube.f3d"))
        .unwrap()
        .set_times(std::fs::FileTimes::new().set_modified(SystemTime::now()))
        .unwrap();
    assert!(events(&watch(&inbox, &out, "10")).is_empty());
    assert!(!out.join("cube.solidcraft").exists());
}

#[test]
fn failure_is_recorded_once_and_unrelated_outputs_are_protected() {
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("in");
    let out = root.path().join("out");
    std::fs::create_dir_all(&inbox).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(inbox.join("broken.f3d"), b"synthetic unreadable archive").unwrap();
    cube(&inbox.join("cube.f3d"));
    std::fs::write(out.join("cube.solidcraft"), b"unrelated").unwrap();
    let first = watch(&inbox, &out, "0");
    assert_eq!(first.status.code(), Some(1));
    let lines = events(&first);
    assert!(lines
        .iter()
        .any(|e| e["event"] == "failed" && e["path"] == "broken.f3d"));
    assert!(lines
        .iter()
        .any(|e| e["event"] == "skipped" && e["path"] == "cube.f3d"));
    let failure: serde_json::Value =
        serde_json::from_slice(&std::fs::read(out.join("broken.failed.json")).unwrap()).unwrap();
    assert_eq!(failure["ok"], false);
    assert!(failure["error"]["message"].is_string());
    assert_eq!(
        std::fs::read(out.join("cube.solidcraft")).unwrap(),
        b"unrelated"
    );
    let again = watch(&inbox, &out, "0");
    assert!(again.status.success());
    assert!(events(&again).is_empty());
    assert_eq!(std::fs::read_dir(&inbox).unwrap().count(), 2);
}

#[cfg(unix)]
#[test]
fn read_only_inbox_and_modified_owned_output_are_protected() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("in");
    let out = root.path().join("out");
    std::fs::create_dir(&inbox).unwrap();
    let input = inbox.join("cube.f3d");
    cube(&input);
    std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o444)).unwrap();
    std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o555)).unwrap();
    let result = watch(&inbox, &out, "0");
    std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&input, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(result.status.success());
    assert_eq!(std::fs::read_dir(&inbox).unwrap().count(), 1);
    std::fs::write(out.join("cube.solidcraft"), b"edited externally").unwrap();
    std::fs::write(&input, b"different synthetic input").unwrap();
    // Failure sidecars can still be written, but the existing design is retained.
    assert_eq!(watch(&inbox, &out, "0").status.code(), Some(1));
    assert_eq!(
        std::fs::read(out.join("cube.solidcraft")).unwrap(),
        b"edited externally"
    );
}

#[test]
fn a_scan_larger_than_the_worker_pool_finishes() {
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("in");
    let out = root.path().join("out");
    std::fs::create_dir(&inbox).unwrap();
    for n in 0..5 {
        cube(&inbox.join(format!("cube{n}.f3d")));
    }
    let result = watch(&inbox, &out, "0");
    assert!(result.status.success());
    assert_eq!(events(&result).len(), 5);
}

#[cfg(unix)]
#[test]
fn output_symlinks_and_outputs_inside_the_inbox_never_write_to_it() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("in");
    let out = root.path().join("out");
    std::fs::create_dir_all(inbox.join("nested")).unwrap();
    std::fs::create_dir_all(&out).unwrap();
    cube(&inbox.join("nested/cube.f3d"));
    symlink(inbox.join("nested"), out.join("nested")).unwrap();
    let result = watch(&inbox, &out, "0");
    assert!(result.status.success());
    assert_eq!(events(&result)[0]["event"], "skipped");
    assert_eq!(std::fs::read_dir(inbox.join("nested")).unwrap().count(), 1);
    assert!(!watch(&inbox, &inbox.join("must-not-exist"), "0")
        .status
        .success());
    assert!(!inbox.join("must-not-exist").exists());
}

#[cfg(unix)]
#[test]
fn polling_settles_changes_and_sigterm_saves_state() {
    use std::process::Stdio;
    use std::time::Instant;
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("in");
    let out = root.path().join("out");
    std::fs::create_dir(&inbox).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("watch")
        .arg(&inbox)
        .arg("--out-dir")
        .arg(&out)
        .args(["--interval", "1", "--settle", "1", "--no-verify", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let input = inbox.join("cube.f3d");
    cube(&input);
    // Keep changing both size and mtime while the service is polling.
    use std::io::Write;
    for _ in 0..8 {
        std::thread::sleep(Duration::from_millis(200));
        std::fs::OpenOptions::new()
            .append(true)
            .open(&input)
            .unwrap()
            .write_all(b"x")
            .unwrap();
        assert!(!out.join("cube.solidcraft").exists());
    }
    let deadline = Instant::now() + Duration::from_secs(15);
    while !out.join("cube.report.json").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let converted = out.join("cube.report.json").exists();
    Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while child.try_wait().unwrap().is_none() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let exited = child.try_wait().unwrap().is_some();
    if !exited {
        child.kill().unwrap();
    }
    let result = child.wait_with_output().unwrap();
    assert!(converted, "{}", String::from_utf8_lossy(&result.stderr));
    assert!(exited && result.status.success());
    assert_eq!(events(&result).len(), 1);
    assert!(events(&watch(&inbox, &out, "0")).is_empty());
}

#[test]
fn alternate_format_and_unrelated_report_sidecars() {
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("in");
    let out = root.path().join("out");
    std::fs::create_dir(&inbox).unwrap();
    std::fs::create_dir(&out).unwrap();
    cube(&inbox.join("cube.f3d"));
    std::fs::write(out.join("cube.report.json"), b"unrelated report").unwrap();
    let skipped = watch(&inbox, &out, "0");
    assert!(skipped.status.success());
    assert_eq!(events(&skipped)[0]["event"], "skipped");
    assert!(!out.join("cube.solidcraft").exists());
    assert_eq!(
        std::fs::read(out.join("cube.report.json")).unwrap(),
        b"unrelated report"
    );
    let macros = root.path().join("macros");
    let converted = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("watch")
        .arg(&inbox)
        .arg("--out-dir")
        .arg(&macros)
        .args(["--once", "--settle", "0", "--to", "freecad", "--json"])
        .output()
        .unwrap();
    assert!(converted.status.success());
    assert!(macros.join("cube.FCMacro").exists());
    assert_eq!(
        events(&converted)[0]["report"]["output"]["format"],
        "freecad-macro/1"
    );
}

#[test]
fn a_hard_restart_reuses_saved_hashes() {
    use std::process::Stdio;
    use std::time::Instant;
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("in");
    let out = root.path().join("out");
    std::fs::create_dir(&inbox).unwrap();
    cube(&inbox.join("cube.f3d"));
    let mut child = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("watch")
        .arg(&inbox)
        .arg("--out-dir")
        .arg(&out)
        .args(["--interval", "1", "--settle", "0", "--no-verify", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    while !out.join(".dutchman-watch.json").exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    let saved = out.join(".dutchman-watch.json").exists();
    child.kill().unwrap();
    let stopped = child.wait_with_output().unwrap();
    assert!(saved, "{}", String::from_utf8_lossy(&stopped.stderr));
    let restarted = watch(&inbox, &out, "0");
    assert!(
        restarted.status.success(),
        "{}",
        String::from_utf8_lossy(&restarted.stderr)
    );
    assert!(events(&restarted).is_empty());
}

#[test]
fn a_later_same_stem_input_cannot_replace_another_inputs_design() {
    let root = tempfile::tempdir().unwrap();
    let inbox = root.path().join("in");
    let out = root.path().join("out");
    std::fs::create_dir(&inbox).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../dutchman-core/tests/fixtures/synthetic_cube.ipt"
        ),
        inbox.join("cube.ipt"),
    )
    .unwrap();
    assert!(watch(&inbox, &out, "0").status.success());
    let design = std::fs::read(out.join("cube.solidcraft")).unwrap();
    let report = std::fs::read(out.join("cube.report.json")).unwrap();
    // This sorts before the existing input and would otherwise take its output name.
    cube(&inbox.join("cube.f3d"));
    let result = watch(&inbox, &out, "0");
    assert!(result.status.success());
    assert_eq!(events(&result).len(), 1);
    assert_eq!(events(&result)[0]["event"], "skipped");
    assert_eq!(std::fs::read(out.join("cube.solidcraft")).unwrap(), design);
    assert_eq!(std::fs::read(out.join("cube.report.json")).unwrap(), report);
}
