//! A circuit board file converts through the optional electronics module, a separate program
//! found next to `dutchman` or on PATH. The module here is a shell script that answers
//! `info`, `detect` and `read` the way the real one does.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
#![cfg(unix)]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Mutex;

use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput};
use cadmpeg_registry::{build_encoder, Format};

// A fork must not inherit a write descriptor for an executable another test will run.
static EXECUTABLE_PUBLICATION: Mutex<()> = Mutex::new(());

fn cube_step() -> Vec<u8> {
    let ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let encoder = build_encoder(Format::Step);
    let (_, target) = encoder.targets().default().unwrap();
    let plan = encoder
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(target.id.as_str()),
        )
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    bytes
}

/// Installs `dutchman-electronics` in `dir`: `read_body` is the shell text for `read`, run with
/// `$in` (the board file), `$out` (the folder to fill) and `$cube` (a STEP file to copy).
fn module(dir: &Path, read_body: &str) {
    let cube = dir.join("cube.step");
    std::fs::write(&cube, cube_step()).unwrap();
    let script = format!(
        r#"#!/bin/sh
cube='{cube}'
case "$1" in
info) echo '{{"schema":"dutchman.module-info/1","name":"electronics","display_name":"Electronics","version":"0.1.0","repository":"x","interface":1,"extensions":["PcbDoc","kicad_pcb"],"formats":[]}}' ;;
detect) echo '{{"schema":"dutchman.module-detect/1","detection":"yes","format":"altium_pcbdoc"}}' ;;
read) in="$2"; out="$4"
{read_body}
;;
esac
"#,
        cube = cube.display()
    );
    let path = dir.join("dutchman-electronics");
    let _guard = EXECUTABLE_PUBLICATION.lock().unwrap();
    let mut temporary = tempfile::NamedTempFile::new_in(dir).unwrap();
    temporary.write_all(script.as_bytes()).unwrap();
    temporary.as_file().sync_all().unwrap();
    let temporary = temporary.into_temp_path();
    std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&temporary, path).unwrap();
}

const GOOD_READ: &str = r#"cp "$cube" "$out/board.step"
printf '%s' '{"schema":"dutchman.parts/1","name":"Tiny board","models":[{"id":"b","file":"board.step","name":"Board"}],"parts":[{"name":"Board","model":"b"}]}' > "$out/manifest.json"
cat "$out/manifest.json""#;

fn board(dir: &Path) -> PathBuf {
    let path = dir.join("tiny.PcbDoc");
    std::fs::write(&path, b"not a real board; the fake module does not look").unwrap();
    path
}

fn convert(path_dir: Option<&Path>, input: &Path, output: &Path) -> Output {
    let path = match path_dir {
        Some(dir) => format!("{}:/usr/bin:/bin", dir.display()),
        None => "/usr/bin:/bin".to_owned(),
    };
    let _guard = EXECUTABLE_PUBLICATION.lock().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_dutchman"));
    command
        .args(["convert", "--no-verify", "--json"])
        .arg(input)
        .arg(output)
        .env("PATH", path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut retries = 0;
    let child = loop {
        match command.spawn() {
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy && retries < 5 => {
                retries += 1;
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            result => break result.unwrap(),
        }
    };
    drop(_guard);
    child.wait_with_output().unwrap()
}

fn text(out: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
}

#[test]
fn a_board_converts_to_a_design_through_the_module() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), GOOD_READ);
    let input = board(dir.path());
    let output = dir.path().join("tiny.sc");
    let out = convert(Some(dir.path()), &input, &output);
    assert!(out.status.success(), "{}", text(&out));
    assert!(output.is_file(), "{}", text(&out));
    let report: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(report["ok"], true, "{report}");
}

#[test]
fn a_board_without_the_module_says_to_install_it() {
    let dir = tempfile::tempdir().unwrap();
    let input = board(dir.path());
    let out = convert(None, &input, &dir.path().join("tiny.sc"));
    assert!(!out.status.success());
    let text = text(&out);
    assert!(text.contains("electronics module"), "{text}");
    assert!(text.contains("not installed"), "{text}");
    assert!(
        !text.contains("does not convert this kind of file"),
        "{text}"
    );
}

#[test]
fn a_failing_module_is_a_named_error_with_its_message() {
    let dir = tempfile::tempdir().unwrap();
    module(
        dir.path(),
        "echo 'copper layer 3 is unreadable' >&2\nexit 2",
    );
    let input = board(dir.path());
    let out = convert(Some(dir.path()), &input, &dir.path().join("tiny.sc"));
    assert!(!out.status.success());
    let text = text(&out);
    assert!(text.contains("electronics module could not read"), "{text}");
    assert!(text.contains("copper layer 3 is unreadable"), "{text}");
}

#[test]
fn a_module_that_wrote_no_manifest_is_a_named_error() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), "exit 0");
    let input = board(dir.path());
    let out = convert(Some(dir.path()), &input, &dir.path().join("tiny.sc"));
    assert!(!out.status.success());
    assert!(text(&out).contains("no manifest.json"), "{}", text(&out));
}

#[test]
fn a_module_that_cannot_run_info_is_named() {
    let dir = tempfile::tempdir().unwrap();
    module(dir.path(), GOOD_READ);
    let path = dir.path().join("dutchman-electronics");
    let _guard = EXECUTABLE_PUBLICATION.lock().unwrap();
    std::fs::write(&path, "#!/bin/sh\necho broken >&2\nexit 3\n").unwrap();
    drop(_guard);
    let input = board(dir.path());
    let out = convert(Some(dir.path()), &input, &dir.path().join("tiny.sc"));
    assert!(!out.status.success());
    let text = text(&out);
    assert!(text.contains("installed but could not be run"), "{text}");
    assert!(!text.contains("unknown"), "{text}");
}
