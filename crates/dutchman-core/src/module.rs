//! Optional modules: separate programs that read a file Dutchman does not.
//!
//! A module is found the way every Dutchman companion program is: `dutchman-<name>` beside the
//! running executable, then on `PATH`. It answers `info --json` (what it reads), `detect <file>
//! --json` (does it read this one) and `read <file> --out <dir> --json` (write a placed-parts
//! package, see [`crate::package`]). Dutchman reads that package like any other input.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::package::read_package;
use crate::{Error, Format, Model, Report, Severity, Stage};

/// The command name of the electronics module: `dutchman-electronics`.
pub const ELECTRONICS: &str = "electronics";

/// The module interface version this Dutchman speaks.
pub const INTERFACE: u64 = 1;

/// Where to get the electronics module.
pub const ELECTRONICS_URL: &str = "https://github.com/dutchman-cad/dutchman-electronics";

/// The extensions recognised while the module is absent, so a board file gets "needs the
/// electronics module" instead of an unknown-file error. With the module installed, its own
/// `info` listing decides.
pub const FALLBACK_EXTENSIONS: &[&str] = &["PcbDoc"];

/// The most a module may write into its output folder.
const MAX_OUTPUT_BYTES: u64 = 4 << 30;
/// The most files a module may write into its output folder.
const MAX_OUTPUT_FILES: u64 = 100_000;
/// The most of a module's output on stdout or stderr Dutchman keeps.
const MAX_STDOUT_BYTES: u64 = 16 << 20;
const MAX_STDERR_BYTES: u64 = 64 << 10;
/// The longest part of a module's own message shown in an error.
const MAX_MESSAGE_CHARS: usize = 2000;

/// What bounds one run of a module.
#[derive(Clone, Debug)]
pub struct Limits {
    /// How long `read` may run before it is stopped.
    pub timeout: Duration,
    /// How long the quick questions, `info` and `detect`, may take.
    pub query_timeout: Duration,
    /// The most bytes of files the module may leave in its output folder.
    pub output_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(600),
            query_timeout: Duration::from_secs(15),
            output_bytes: MAX_OUTPUT_BYTES,
        }
    }
}

/// What `info --json` says about a module.
#[derive(Clone, Debug)]
pub struct Info {
    pub name: String,
    pub version: String,
    pub interface: u64,
    /// Every extension the module reads, lowercase, without the dot.
    pub extensions: Vec<String>,
}

/// The message for a board file when the module is not installed.
pub fn missing_message() -> String {
    format!(
        "this is a circuit board file, and Dutchman reads those with its electronics module, which is \
         not installed. Install `dutchman-electronics` next to `dutchman` or on your PATH: {ELECTRONICS_URL}"
    )
}

/// The electronics module, when it is installed.
pub fn find() -> Option<PathBuf> {
    dutchman_launcher::find_tool(ELECTRONICS)
}

/// Whether the electronics module is installed.
pub fn installed() -> bool {
    find().is_some()
}

/// A module program as it was when asked: path, modification time and size.
type Stamp = (PathBuf, Option<std::time::SystemTime>, u64);
type Cache = Mutex<HashMap<Stamp, Info>>;

/// Asks a module what it reads. Answers are remembered per program file as it is now (path,
/// modification time and size), so an upgraded module is asked again. A failure is never
/// remembered: the next call tries again.
pub fn info(program: &Path, limits: &Limits) -> Result<Info, Error> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let stamp = std::fs::metadata(program)
        .ok()
        .map(|meta| (program.to_path_buf(), meta.modified().ok(), meta.len()));
    if let Some(stamp) = &stamp {
        if let Ok(cache) = cache.lock() {
            if let Some(found) = cache.get(stamp) {
                return Ok(found.clone());
            }
        }
    }
    let fresh = ask_info(program, limits).map_err(Error::Module)?;
    if let (Some(stamp), Ok(mut cache)) = (stamp, cache.lock()) {
        cache.retain(|(path, _, _), _| path != &stamp.0);
        cache.insert(stamp, fresh.clone());
    }
    Ok(fresh)
}

fn ask_info(program: &Path, limits: &Limits) -> Result<Info, String> {
    let run = run(
        program,
        &["info".as_ref(), "--json".as_ref()],
        limits.query_timeout,
    )
    .map_err(|err| cannot_start(program, &err))?;
    if run.timed_out || !run.status.is_some_and(|status| status.success()) {
        return Err(format!(
            "the electronics module at {} did not answer `info --json`{}",
            program.display(),
            detail(&run)
        ));
    }
    let value: Value = serde_json::from_slice(&run.stdout).map_err(|err| {
        format!(
            "the electronics module at {} answered `info --json` with something that is not JSON: {err}",
            program.display()
        )
    })?;
    let name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or(ELECTRONICS)
        .to_owned();
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    let interface = value.get("interface").and_then(Value::as_u64).unwrap_or(0);
    let extensions = value
        .get("extensions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|ext| ext.trim_start_matches('.').to_ascii_lowercase())
        .filter(|ext| !ext.is_empty())
        .collect();
    Ok(Info {
        name,
        version,
        interface,
        extensions,
    })
}

/// Extensions of circuit board files, used only when the installed module cannot be run, so such
/// a file gets the module's own error and not "unknown format".
const BOARD_LIKE_EXTENSIONS: &[&str] = &["PcbDoc", "kicad_pcb", "brd", "SchDoc"];

/// Whether the installed electronics module reads files with this extension. When the module is
/// installed but cannot be run, a board-like extension still counts, and `read` then says why
/// the module could not be run.
pub fn reads_extension(ext: &str) -> bool {
    let Some(program) = find() else {
        return false;
    };
    let ext = ext.trim_start_matches('.');
    match info(&program, &Limits::default()) {
        Ok(info) => info.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext)),
        Err(_) => BOARD_LIKE_EXTENSIONS
            .iter()
            .any(|e| e.eq_ignore_ascii_case(ext)),
    }
}

/// Which copper layers of a board the module is asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CopperLayers {
    /// The two outer faces only (the default).
    #[default]
    Outer,
    /// The outer faces and every inner copper layer (`read --copper all`).
    All,
}

impl CopperLayers {
    /// Stable command-line and JSON spelling.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Outer => "outer",
            Self::All => "all",
        }
    }
}

impl std::fmt::Display for CopperLayers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

impl std::str::FromStr for CopperLayers {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "outer" => Ok(Self::Outer),
            "all" => Ok(Self::All),
            _ => Err("copper must be outer or all".to_owned()),
        }
    }
}

/// Reads `input` with the installed electronics module.
pub fn read(input: &Path, copper: CopperLayers, report: &mut Report) -> Result<Model, Error> {
    let program = find().ok_or(Error::NotSupportedYet {
        format: Format::ModuleBoard,
    })?;
    read_with(&program, input, copper, &Limits::default(), report)
}

/// Reads `input` with the module `program`: checks its interface and that it reads this file,
/// runs `read` into a temporary folder, reads the package it wrote there and deletes the folder.
pub fn read_with(
    program: &Path,
    input: &Path,
    copper: CopperLayers,
    limits: &Limits,
    report: &mut Report,
) -> Result<Model, Error> {
    let info = info(program, limits).map_err(|err| match err {
        Error::Module(reason) => Error::Module(format!(
            "the electronics module is installed but could not be run: {reason}"
        )),
        other => other,
    })?;
    if info.interface != INTERFACE {
        return Err(Error::Module(format!(
            "the electronics module {} speaks module interface {}, and this Dutchman speaks interface {INTERFACE}. \
             Install a matching version: {ELECTRONICS_URL}",
            info.version, info.interface
        )));
    }
    let extension = input
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or_default();
    if !info
        .extensions
        .iter()
        .any(|e| e.eq_ignore_ascii_case(extension))
    {
        return Err(Error::Module(format!(
            "the installed electronics module {} does not read .{extension} files; it reads {}",
            info.version,
            info.extensions
                .iter()
                .map(|e| format!(".{e}"))
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if let Some(false) = detects(program, input, limits) {
        return Err(Error::Module(format!(
            "the electronics module {} says {} is not a circuit board it reads, so Dutchman does not treat it as one (the module decides this from the file's contents and its name)",
            info.version,
            input.display()
        )));
    }
    let scratch = tempfile::Builder::new()
        .prefix("dutchman-module-")
        .tempdir()
        .map_err(|source| Error::Io {
            path: std::env::temp_dir(),
            source,
        })?;
    let out = scratch.path();
    // `--copper all` (when asked for) first, then one `--accept-overlay <kind>` for each kind
    // the module only sends when told. An older module rejects an option it does not know.
    let read_args = |copper_all: bool, accept: bool| -> Vec<&OsStr> {
        let mut args: Vec<&OsStr> = vec![
            "read".as_ref(),
            input.as_os_str(),
            "--out".as_ref(),
            out.as_os_str(),
            "--json".as_ref(),
        ];
        if copper_all {
            args.push(OsStr::new("--copper"));
            args.push(OsStr::new("all"));
        }
        if accept {
            for kind in crate::package::ACCEPTED_OVERLAY_KINDS {
                args.push(OsStr::new("--accept-overlay"));
                args.push(OsStr::new(kind));
            }
        }
        args
    };
    let want_copper = copper == CopperLayers::All;
    let mut run = run(program, &read_args(want_copper, true), limits.timeout)
        .map_err(|err| Error::Module(cannot_start(program, &err)))?;
    // Which options an older module turned down: the one extra read leaves out only those.
    let (mut copper_unsupported, mut accept_unsupported) = (false, false);
    if !run.timed_out && !run.status.is_some_and(|s| s.success()) {
        let said = format!("{}{}", lossy(&run.stderr), lossy(&run.stdout)).to_ascii_lowercase();
        let rejected = said.contains("unexpected argument")
            || said.contains("unrecognized")
            || said.contains("unknown option")
            || said.contains("unknown argument");
        let names_copper = said.contains("--copper");
        let names_accept = said.contains("--accept-overlay");
        if rejected || names_copper || names_accept {
            // `--copper` came before `--accept-overlay` in modules, so one that does not know
            // `--copper` does not know `--accept-overlay` either. An error that names only
            // `--accept-overlay` keeps `--copper`; one that names neither leaves both out.
            accept_unsupported = true;
            copper_unsupported = want_copper && (names_copper || !names_accept);
            run = self::run(
                program,
                &read_args(want_copper && !copper_unsupported, false),
                limits.timeout,
            )
            .map_err(|err| Error::Module(cannot_start(program, &err)))?;
        }
    }
    let stderr = lossy(&run.stderr);
    let fail = |report: &mut Report, message: String| {
        if !stderr.trim().is_empty() {
            report.push_at(
                Stage::Decode,
                Severity::Info,
                "electronics module",
                clip(&stderr),
            );
        }
        Err(Error::Module(message))
    };
    if run.timed_out {
        return fail(
            report,
            format!(
                "the electronics module did not finish reading this file within {} seconds and was stopped",
                limits.timeout.as_secs()
            ),
        );
    }
    let said = serde_json::from_slice::<Value>(&run.stdout).ok();
    let refused = said
        .as_ref()
        .is_some_and(|v| v.get("ok") == Some(&Value::Bool(false)));
    let success = run.status.is_some_and(|status| status.success());
    if !success || refused {
        let message = said
            .as_ref()
            .and_then(|v| {
                ["message", "error", "reason"]
                    .iter()
                    .find_map(|key| v.get(key).and_then(Value::as_str))
            })
            .map(str::to_owned)
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| stderr.trim().to_owned());
        return fail(
            report,
            format!(
                "the electronics module could not read this file ({}){}",
                status_words(run.status),
                if message.is_empty() {
                    String::new()
                } else {
                    format!(": {}", clip(&message))
                }
            ),
        );
    }
    if let Err(message) = measure(out, limits.output_bytes) {
        return fail(report, message);
    }
    let manifest = out.join("manifest.json");
    let bytes = match std::fs::symlink_metadata(&manifest) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_STDOUT_BYTES * 4 => {
            std::fs::read(&manifest).map_err(|source| Error::Io {
                path: manifest.clone(),
                source,
            })?
        }
        Ok(_) => {
            return fail(
                report,
                "the electronics module wrote a manifest.json that is not a regular file or is too large"
                    .to_owned(),
            )
        }
        Err(_) => {
            return fail(
                report,
                "the electronics module finished but wrote no manifest.json, so there is nothing to read"
                    .to_owned(),
            )
        }
    };
    if accept_unsupported {
        report.push_at(
            Stage::Decode,
            Severity::Info,
            "electronics module",
            format!(
                "the installed electronics module {} does not support `--accept-overlay`, so it sent no laminate and an opaque solder mask; install a newer module for those: {ELECTRONICS_URL}",
                info.version
            ),
        );
    }
    if copper_unsupported {
        report.push_at(
            Stage::Decode,
            Severity::Degraded,
            "electronics module",
            format!(
                "the installed electronics module {} does not support `--copper all`, so only the top and bottom copper layers were read; install a newer module for inner layers: {ELECTRONICS_URL}",
                info.version
            ),
        );
    }
    read_package(out, &bytes, report)
}

/// `Some(true)` when the module says it reads the file, `Some(false)` when it says it does not,
/// `None` when it gave no usable answer (`read` then says what is wrong).
fn detects(program: &Path, input: &Path, limits: &Limits) -> Option<bool> {
    let run = run(
        program,
        &["detect".as_ref(), input.as_os_str(), "--json".as_ref()],
        limits.query_timeout,
    )
    .ok()?;
    if run.timed_out || !run.status.is_some_and(|status| status.success()) {
        return None;
    }
    let value: Value = serde_json::from_slice(&run.stdout).ok()?;
    match value.get("detection").and_then(Value::as_str)? {
        "no" => Some(false),
        "yes" | "maybe_by_name" => Some(true),
        _ => None,
    }
}

/// Fails when the folder holds more than `limit` bytes or too many files.
fn measure(dir: &Path, limit: u64) -> Result<(), String> {
    let (mut bytes, mut files) = (0u64, 0u64);
    let mut pending = vec![dir.to_path_buf()];
    while let Some(folder) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(meta) = entry.metadata() else { continue };
            files += 1;
            if meta.is_dir() {
                pending.push(entry.path());
            } else {
                bytes = bytes.saturating_add(meta.len());
            }
            if bytes > limit || files > MAX_OUTPUT_FILES {
                return Err(format!(
                    "the electronics module wrote more than Dutchman accepts ({} GiB or {MAX_OUTPUT_FILES} files), so the file was not read",
                    limit >> 30
                ));
            }
        }
    }
    Ok(())
}

struct Run {
    status: Option<ExitStatus>,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
    timed_out: bool,
}

fn drain(mut pipe: impl Read + Send + 'static, keep: u64) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let _ = (&mut pipe).take(keep).read_to_end(&mut kept);
        // Read the rest so the module never blocks on a full pipe.
        let _ = std::io::copy(&mut pipe, &mut std::io::sink());
        kept
    })
}

/// Runs `program`, keeping a bounded part of its output, and stops it at `timeout`.
fn run(program: &Path, args: &[&OsStr], timeout: Duration) -> std::io::Result<Run> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let stdout = child.stdout.take().map(|p| drain(p, MAX_STDOUT_BYTES));
    let stderr = child.stderr.take().map(|p| drain(p, MAX_STDERR_BYTES));
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if Instant::now() >= deadline {
            timed_out = true;
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    // After a timeout a child of the module may still hold the pipes: leave the readers behind.
    let collect = |handle: Option<std::thread::JoinHandle<Vec<u8>>>| {
        if timed_out {
            return Vec::new();
        }
        handle.and_then(|h| h.join().ok()).unwrap_or_default()
    };
    Ok(Run {
        status,
        stdout: collect(stdout),
        stderr: collect(stderr),
        timed_out,
    })
}

fn cannot_start(program: &Path, err: &std::io::Error) -> String {
    format!(
        "the electronics module at {} could not be started: {err}",
        program.display()
    )
}

fn status_words(status: Option<ExitStatus>) -> String {
    match status {
        Some(status) => match status.code() {
            Some(code) => format!("exit status {code}"),
            None => "stopped by a signal".to_owned(),
        },
        None => "no exit status".to_owned(),
    }
}

fn detail(run: &Run) -> String {
    let stderr = lossy(&run.stderr);
    let stderr = stderr.trim();
    let how = if run.timed_out {
        "it timed out".to_owned()
    } else {
        status_words(run.status)
    };
    if stderr.is_empty() {
        format!(" ({how})")
    } else {
        format!(" ({how}): {}", clip(stderr))
    }
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn clip(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= MAX_MESSAGE_CHARS {
        return text.to_owned();
    }
    let kept: String = text.chars().take(MAX_MESSAGE_CHARS).collect();
    format!("{kept}...")
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Held by every test here: each writes a module script and runs it. A script still open
    /// for writing when another test forks is inherited by that child, and running it then
    /// fails with "text file busy".
    fn scripts() -> std::sync::MutexGuard<'static, ()> {
        static SCRIPTS: std::sync::Mutex<()> = std::sync::Mutex::new(());
        SCRIPTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A module script: `read` body is the shell text run for `read`.
    fn fake(dir: &Path, interface: u64, read: &str) -> PathBuf {
        let path = dir.join("dutchman-electronics");
        let script = format!(
            r#"#!/bin/sh
case "$1" in
info) echo '{{"schema":"dutchman.module-info/1","name":"electronics","version":"9.9.9","interface":{interface},"extensions":["PcbDoc","kicad_pcb"]}}' ;;
detect) echo '{{"schema":"dutchman.module-detect/1","detection":"yes","format":"altium_pcbdoc"}}' ;;
read) in="$2"; out="$4"
{read}
;;
esac
"#
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn ok_read() -> String {
        r#"echo step > "$out/b.step"
printf '%s' '{"schema":"dutchman.parts/1","name":"Tiny","models":[{"id":"m","file":"b.step"}],"parts":[{"name":"Board","model":"m"}]}' > "$out/manifest.json"
cat "$out/manifest.json""#
            .to_owned()
    }

    fn board(dir: &Path) -> PathBuf {
        let path = dir.join("tiny.PcbDoc");
        std::fs::write(&path, b"board").unwrap();
        path
    }

    fn run_read(read: &str, limits: &Limits) -> Result<Model, Error> {
        let dir = tempfile::tempdir().unwrap();
        let program = fake(dir.path(), 1, read);
        read_with(
            &program,
            &board(dir.path()),
            CopperLayers::Outer,
            limits,
            &mut Report::default(),
        )
    }

    #[test]
    fn a_failing_module_is_a_named_error_with_its_message() {
        let _scripts = scripts();
        let error = run_read(
            "echo 'layer table is truncated' >&2; exit 3",
            &Limits::default(),
        )
        .unwrap_err();
        let text = error.to_string();
        assert!(matches!(error, Error::Module(_)), "{text}");
        assert!(text.contains("exit status 3"), "{text}");
        assert!(text.contains("layer table is truncated"), "{text}");
    }

    #[test]
    fn a_module_saying_not_ok_shows_its_message() {
        let _scripts = scripts();
        let error = run_read(
            r#"echo '{"ok":false,"message":"unsupported board version"}'; exit 1"#,
            &Limits::default(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("unsupported board version"),
            "{error}"
        );
    }

    #[test]
    fn a_module_that_never_finishes_is_stopped() {
        let _scripts = scripts();
        let limits = Limits {
            timeout: Duration::from_millis(300),
            ..Limits::default()
        };
        let started = Instant::now();
        let error = run_read("sleep 30", &limits).unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(20));
        assert!(error.to_string().contains("did not finish"), "{error}");
    }

    #[test]
    fn a_module_without_a_manifest_is_a_named_error() {
        let _scripts = scripts();
        let error = run_read("exit 0", &Limits::default()).unwrap_err();
        assert!(error.to_string().contains("no manifest.json"), "{error}");
    }

    /// A module that records each `read` command line it is given beside the board.
    fn copper_echo() -> String {
        r#"echo "$*" >> "$(dirname "$in")/calls.log"
echo step > "$out/b.step"
printf '%s' '{"schema":"dutchman.parts/1","name":"Tiny","models":[{"id":"m","file":"b.step"}],"parts":[{"name":"Board","model":"m"}]}' > "$out/manifest.json"
cat "$out/manifest.json""#
            .to_owned()
    }

    /// Reads with the module `read` script and returns the report and each command line the
    /// module was run with. The package itself is not what these tests are about.
    fn read_copper(read: &str, copper: CopperLayers) -> (Report, Vec<String>) {
        let dir = tempfile::tempdir().unwrap();
        let program = fake(dir.path(), 1, read);
        let mut report = Report::default();
        read_with(
            &program,
            &board(dir.path()),
            copper,
            &Limits::default(),
            &mut report,
        )
        .ok();
        let calls = std::fs::read_to_string(dir.path().join("calls.log")).unwrap_or_default();
        (report, calls.lines().map(str::to_owned).collect())
    }

    /// A `read` script that rejects any command line naming `flag`, as an older module does,
    /// and otherwise acts like `copper_echo`. Each run is recorded, so a rejected run shows once
    /// and an accepted one twice.
    fn rejecting(flag: &str) -> String {
        format!(
            "echo \"$*\" >> \"$(dirname \"$in\")/calls.log\"\ncase \"$*\" in *{flag}*) echo \"error: unexpected argument '{flag}' found\" >&2; exit 2 ;; esac\n{}",
            copper_echo()
        )
    }

    #[test]
    fn an_argparse_style_rejection_that_names_no_option_still_gets_one_plain_retry() {
        let _scripts = scripts();
        let older = format!(
            "echo \"$*\" >> \"$(dirname \"$in\")/calls.log\"\ncase \"$*\" in *--copper*|*--accept-overlay*) echo \"usage: electronics read [-h]\nerror: unrecognized arguments\" >&2; exit 2 ;; esac\n{}",
            copper_echo()
        );
        let (report, calls) = read_copper(&older, CopperLayers::All);
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert!(
            !calls[2].contains("--copper") && !calls[2].contains("--accept-overlay"),
            "{calls:?}"
        );
        assert!(note(&report, "does not support `--accept-overlay`").is_some());
    }

    fn note<'a>(report: &'a Report, needle: &str) -> Option<&'a crate::report::Loss> {
        report.losses().iter().find(|l| l.detail.contains(needle))
    }

    #[test]
    fn both_options_are_passed_to_the_module_and_only_the_accepted_kinds_when_outer() {
        let _scripts = scripts();
        let (report, all) = read_copper(&copper_echo(), CopperLayers::All);
        assert_eq!(all.len(), 1, "{all:?}");
        assert!(
            all[0].ends_with("--json --copper all --accept-overlay laminate"),
            "{all:?}"
        );
        assert!(note(&report, "does not support").is_none());
        let (_, outer) = read_copper(&copper_echo(), CopperLayers::Outer);
        assert_eq!(outer.len(), 1, "{outer:?}");
        assert!(!outer[0].contains("--copper"), "{outer:?}");
        assert!(outer[0].ends_with("--accept-overlay laminate"), "{outer:?}");
        assert_eq!("all".parse::<CopperLayers>(), Ok(CopperLayers::All));
        assert!("inner".parse::<CopperLayers>().is_err());
    }

    #[test]
    fn an_older_module_without_copper_falls_back_with_both_notes_in_one_extra_read() {
        let _scripts = scripts();
        let (report, calls) = read_copper(&rejecting("--copper"), CopperLayers::All);
        // Asked with both, refused, then asked once more with neither.
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert!(calls[0].contains("--copper all"), "{calls:?}");
        assert!(
            !calls[2].contains("--copper") && !calls[2].contains("--accept-overlay"),
            "{calls:?}"
        );
        let copper = note(&report, "does not support `--copper all`").expect("copper note");
        assert_eq!(copper.severity, Severity::Degraded);
        assert!(note(&report, "does not support `--accept-overlay`").is_some());
    }

    #[test]
    fn a_module_without_accept_overlay_keeps_copper_and_notes_the_missing_kinds() {
        let _scripts = scripts();
        let (report, calls) = read_copper(&rejecting("--accept-overlay"), CopperLayers::All);
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert!(calls[0].contains("--accept-overlay laminate"), "{calls:?}");
        assert!(calls[2].ends_with("--copper all"), "{calls:?}");
        assert!(note(&report, "does not support `--accept-overlay`").is_some());
        assert!(note(&report, "does not support `--copper all`").is_none());
    }

    #[test]
    fn a_module_with_another_interface_is_refused() {
        let _scripts = scripts();
        let dir = tempfile::tempdir().unwrap();
        let program = fake(dir.path(), 2, &ok_read());
        let error = read_with(
            &program,
            &board(dir.path()),
            CopperLayers::Outer,
            &Limits::default(),
            &mut Report::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("interface 2"), "{error}");
    }

    #[test]
    fn too_much_output_is_refused() {
        let _scripts = scripts();
        let limits = Limits {
            output_bytes: 10,
            ..Limits::default()
        };
        let error = run_read(&ok_read(), &limits).unwrap_err();
        assert!(
            error.to_string().contains("more than Dutchman accepts"),
            "{error}"
        );
    }

    #[test]
    fn a_failed_info_is_retried_and_an_upgraded_module_is_read_again() {
        let _scripts = scripts();
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("dutchman-electronics");
        let write = |body: &str| {
            std::fs::write(&program, body).unwrap();
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        };
        let limits = Limits::default();
        write("#!/bin/sh\nexit 4\n");
        assert!(info(&program, &limits).is_err());
        write("#!/bin/sh\necho '{\"version\":\"1.0.0\",\"interface\":1,\"extensions\":[\"PcbDoc\"]}'\n");
        assert_eq!(info(&program, &limits).unwrap().version, "1.0.0");
        // Same size, a new modification time and new answer.
        let later = std::time::SystemTime::now() + Duration::from_secs(5);
        write("#!/bin/sh\necho '{\"version\":\"2.0.0\",\"interface\":1,\"extensions\":[\"PcbDoc\"]}'\n");
        std::fs::File::options()
            .write(true)
            .open(&program)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert_eq!(info(&program, &limits).unwrap().version, "2.0.0");
    }

    #[test]
    fn a_module_that_cannot_be_run_is_named() {
        let _scripts = scripts();
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("dutchman-electronics");
        std::fs::write(&program, "#!/bin/sh\necho broken install >&2\nexit 1\n").unwrap();
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = read_with(
            &program,
            &board(dir.path()),
            CopperLayers::Outer,
            &Limits::default(),
            &mut Report::default(),
        )
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("the electronics module is installed but could not be run"),
            "{error}"
        );
    }

    #[test]
    fn a_detect_no_means_not_a_board() {
        let _scripts = scripts();
        let dir = tempfile::tempdir().unwrap();
        let program = fake(dir.path(), 1, &ok_read());
        let script = std::fs::read_to_string(&program)
            .unwrap()
            .replace(r#""detection":"yes""#, r#""detection":"no""#);
        std::fs::write(&program, script).unwrap();
        let error = read_with(
            &program,
            &board(dir.path()),
            CopperLayers::Outer,
            &Limits::default(),
            &mut Report::default(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("is not a circuit board"),
            "{error}"
        );
    }
}
