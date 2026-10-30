//! Anonymous progress that survives an unfinished conversion. No report messages or names
//! are written; paths enter only the anonymous report's salted id function.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

pub use dutchman_core::MemberEvent;
use dutchman_core::{MemberObserver, ProgressEvent, Stage};

mod location;
pub use location::{default_folder, new_default_log, unfinished_default_log};
use serde_json::{json, Value};

use crate::{anon, choices, ConvertOptions};

pub const SCHEMA: &str = "dutchman.diagnostics/1";
// Serialize whole lines across simultaneous conversions, even with separate file handles.
static WRITE: Mutex<()> = Mutex::new(());

struct State {
    file: File,
    stage: Stage,
    designs: Option<(u64, Option<u64>)>,
    member: Option<String>,
    error: Option<String>,
    member_decodes: BTreeMap<String, u64>,
    synced: Instant,
}

/// One conversion's live diagnostic writer. Dropping it flushes and stops its timer, without
/// inventing an end record. A parent supervising a worker sets its pid for resource samples.
pub struct Log {
    sink: Sink,
    pid: Arc<AtomicU32>,
    stop: Option<mpsc::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Log {
    pub fn open(path: &Path, input: &Path, options: &ConvertOptions) -> Result<Self, String> {
        let interval = std::env::var("DUTCHMAN_DIAGNOSTICS_HEARTBEAT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|ms| *ms > 0)
            .map_or(Duration::from_secs(30), Duration::from_millis);
        Self::open_every(path, input, options, interval)
    }

    fn open_every(
        path: &Path,
        input: &Path,
        options: &ConvertOptions,
        interval: Duration,
    ) -> Result<Self, String> {
        let salt = options
            .diagnostics_salt
            .clone()
            .map_or_else(anon::Salt::random, Ok)?;
        if path == input
            || std::fs::canonicalize(path)
                .ok()
                .zip(std::fs::canonicalize(input).ok())
                .is_some_and(|(a, b)| a == b)
        {
            return Err(
                "the diagnostic log would change the input; choose another --diagnostics path"
                    .into(),
            );
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| format!("cannot open diagnostic log {}: {err}", path.display()))?;
        let state = Arc::new(Mutex::new(State {
            file,
            stage: Stage::Input,
            designs: None,
            member: None,
            error: None,
            member_decodes: BTreeMap::new(),
            synced: Instant::now(),
        }));
        let pid = Arc::new(AtomicU32::new(std::process::id()));
        let started = Instant::now();
        let id = salt.file_id(input);
        let mut log = Self {
            sink: Sink {
                state,
                started,
                id,
                salt,
            },
            pid,
            stop: None,
            thread: None,
        };
        log.record(json!({
            "type": "start", "dutchman": env!("CARGO_PKG_VERSION"),
            "cadmpeg": anon::CADMPEG_VERSION,
            "os": {"family": std::env::consts::FAMILY, "name": std::env::consts::OS,
                   "version": os_version()},
            "cpu_count": std::thread::available_parallelism().ok().map(usize::from),
            "total_memory_bytes": total_memory(), "available_memory_bytes": choices::available_memory(),
            "choices": crate::json::choice_values(options.preset, options.choices()),
            "threads": options.diagnostics_threads,
            "input": {"format": dutchman_core::Format::from_path(input).map(|f| f.id()),
                      "size_bytes": std::fs::metadata(input).ok().map(|m| m.len())},
            "members": members(input, &log.sink.salt),
        }));
        log.check()?;
        let (stop, receive) = mpsc::channel::<()>();
        let state = Arc::clone(&log.sink.state);
        let pid = Arc::clone(&log.pid);
        let id = log.sink.id.clone();
        log.thread = Some(std::thread::spawn(move || {
            let mut heartbeat = Instant::now();
            while matches!(
                receive.recv_timeout(interval.min(Duration::from_secs(5))),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                if let Ok(mut state) = state.lock() {
                    if heartbeat.elapsed() < interval {
                        sync(&mut state);
                        continue;
                    }
                    heartbeat = Instant::now();
                    let (rss, ticks) = resources(pid.load(Ordering::Relaxed));
                    let stage = state.stage.id();
                    let member = state.member.clone();
                    write_record(
                        &mut state,
                        &id,
                        started,
                        json!({
                            "type": "heartbeat", "stage": stage, "rss_bytes": rss,
                            "cpu_ticks": ticks, "cpu_ticks_per_second": clock_ticks(),
                            "cpu_ms": ticks.zip(clock_ticks()).and_then(|(t, hz)| t.checked_mul(1000)?.checked_div(hz)),
                            "member": member,
                        }),
                    );
                }
            }
        }));
        log.stop = Some(stop);
        Ok(log)
    }

    pub fn process(&self, pid: u32) {
        self.pid.store(pid, Ordering::Relaxed);
    }

    pub fn progress(&self, event: ProgressEvent) {
        if let Ok(mut state) = self.sink.state.lock() {
            if state.stage != event.stage || state.designs.is_none() {
                state.stage = event.stage;
                write_record(
                    &mut state,
                    &self.sink.id,
                    self.sink.started,
                    json!({"type": "stage", "stage": event.stage.id()}),
                );
            }
            let designs = (event.designs_read, event.designs_total);
            if state.designs != Some(designs) {
                state.designs = Some(designs);
                write_record(
                    &mut state,
                    &self.sink.id,
                    self.sink.started,
                    json!({"type": "design", "count": designs.0, "total": designs.1}),
                );
            }
        }
    }

    /// A cloneable sink for the decoder or the isolated worker's progress stream.
    pub fn observer(&self) -> Arc<dyn MemberObserver> {
        Arc::new(self.sink.clone())
    }

    pub fn member_event(
        &self,
        member: &str,
        event: MemberEvent,
        ms: u64,
        code: Option<&'static str>,
    ) {
        self.sink.member_event(member, event, ms, code);
    }

    fn record(&self, record: Value) {
        if let Ok(mut state) = self.sink.state.lock() {
            write_record(&mut state, &self.sink.id, self.sink.started, record);
        }
    }

    fn check(&self) -> Result<(), String> {
        let state = self
            .sink
            .state
            .lock()
            .map_err(|_| "diagnostic log writer stopped".to_owned())?;
        match &state.error {
            Some(error) => Err(format!(
                "cannot write diagnostic log: {error}; check free space and permissions"
            )),
            None => Ok(()),
        }
    }

    pub fn end(&mut self, report: &Value) -> Result<(), String> {
        self.stop_timer();
        let stage = self.sink.state.lock().map_or(Stage::Input, |s| s.stage);
        let mut counts = BTreeMap::<String, u64>::new();
        let mut identical = BTreeMap::<String, u64>::new();
        for loss in report
            .get("losses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let code = loss.get("code").and_then(Value::as_str).map_or_else(
                || {
                    format!(
                        "dutchman/{}",
                        anon::subject_kind(
                            loss.get("subject").and_then(Value::as_str).unwrap_or("")
                        )
                    )
                },
                |c| anon::generic(c, "cadmpeg/other"),
            );
            *counts.entry(code.clone()).or_default() += 1;
            // Messages distinguish identical losses in memory only. They are never written.
            let key = json!([
                code,
                loss.get("level"),
                loss.get("stage"),
                loss.get("subject"),
                loss.get("message")
            ])
            .to_string();
            *identical.entry(key).or_default() += 1;
        }
        let repeats: u64 = identical.values().map(|n| n.saturating_sub(1)).sum();
        let member_decodes = self
            .sink
            .state
            .lock()
            .map(|s| s.member_decodes.clone())
            .unwrap_or_default();
        let redecoded: u64 = member_decodes.values().map(|n| n.saturating_sub(1)).sum();
        self.record(
            json!({"type": "end", "ok": report.get("ok") == Some(&Value::Bool(true)),
            "stage": stage.id(), "losses": counts, "repeated_losses": repeats,
            "member_decodes": member_decodes, "redecoded": redecoded}),
        );
        self.check()
    }

    fn stop_timer(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Log {
    fn drop(&mut self) {
        self.stop_timer();
        if let Ok(mut state) = self.sink.state.lock() {
            sync(&mut state);
        }
    }
}

#[derive(Clone)]
struct Sink {
    state: Arc<Mutex<State>>,
    started: Instant,
    id: String,
    salt: anon::Salt,
}

impl MemberObserver for Sink {
    fn member_event(&self, member: &str, event: MemberEvent, ms: u64, code: Option<&'static str>) {
        if let Ok(mut state) = self.state.lock() {
            let id = self.salt.member_id(member);
            if event == MemberEvent::Start {
                state.member = Some(id.clone());
                if state.member_decodes.len() < 100_000 || state.member_decodes.contains_key(&id) {
                    *state.member_decodes.entry(id.clone()).or_default() += 1;
                }
            }
            let code = if event == MemberEvent::Fail {
                code.map(member_code)
            } else {
                None
            };
            let event = match event {
                MemberEvent::Start => "start",
                MemberEvent::Finish => "finish",
                MemberEvent::Fail => "fail",
            };
            write_record(
                &mut state,
                &self.id,
                self.started,
                json!({
                    "type": "member", "member": id, "event": event, "ms": ms,
                    "code": code,
                }),
            );
        }
    }
}

/// Accept only decoder categories, including across the worker protocol.
pub(crate) fn member_code(code: &str) -> &'static str {
    match code {
        "wrong_format" => "wrong_format",
        "malformed" => "malformed",
        "invalid_input" => "invalid_input",
        "truncated" => "truncated",
        "resource_limit" => "resource_limit",
        "unsupported_dialect" => "unsupported_dialect",
        "unsupported_target" => "unsupported_target",
        "insufficient_depth" => "insufficient_depth",
        "not_implemented" => "not_implemented",
        "io" => "io",
        _ => "other",
    }
}

fn sync(state: &mut State) {
    if let Err(error) = state.file.sync_data() {
        state.error = Some(error.to_string());
    }
    state.synced = Instant::now();
}

fn write_record(state: &mut State, id: &str, started: Instant, mut record: Value) {
    if state.error.is_some() {
        return;
    }
    if let Some(fields) = record.as_object_mut() {
        fields.insert("schema".into(), json!(SCHEMA));
        fields.insert("file".into(), json!(id));
        fields.insert(
            "elapsed_ms".into(),
            json!(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX)),
        );
    }
    let durable = record
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| matches!(kind, "stage" | "start" | "end"))
        || state.synced.elapsed() >= Duration::from_secs(5);
    let result = (|| -> Result<(), String> {
        let mut bytes = serde_json::to_vec(&record).map_err(|err| err.to_string())?;
        bytes.push(b'\n');
        let _write = WRITE
            .lock()
            .map_err(|_| "diagnostic log writer stopped".to_owned())?;
        state
            .file
            .write_all(&bytes)
            .and_then(|()| state.file.flush())
            .map_err(|err| err.to_string())
    })();
    if let Err(error) = result {
        state.error = Some(error);
    }
    if durable {
        sync(state);
    }
}

/// Pick the same random salt source as report-anon, once for a front-end run.
pub fn prepare(options: &mut ConvertOptions, threads: usize) -> Result<(), String> {
    options.diagnostics_threads = threads;
    if options.diagnostics_default && options.diagnostics.is_none() {
        options.diagnostics = Some(new_default_log()?);
    }
    if options.diagnostics.is_some() && options.diagnostics_salt.is_none() {
        options.diagnostics_salt = Some(anon::Salt::random()?);
    }
    Ok(())
}

/// Wrap front-end work, including pre-read refusals, with a live log and final summary.
pub(crate) fn run(
    input: &Path,
    options: &ConvertOptions,
    progress: &(dyn Fn(ProgressEvent) + Send + Sync),
    work: impl FnOnce(&(dyn Fn(ProgressEvent) + Send + Sync), Option<&Log>) -> Value,
) -> Value {
    let mut prepared = options.clone();
    if let Err(message) = prepare(&mut prepared, options.diagnostics_threads) {
        return crate::json::failure(crate::json::CONVERT, crate::json::input(input), &message);
    }
    let options = &prepared;
    let mut log = match options
        .diagnostics
        .as_deref()
        .map(|path| Log::open(path, input, options))
        .transpose()
    {
        Ok(log) => log,
        Err(message) => {
            return crate::json::failure(crate::json::CONVERT, crate::json::input(input), &message)
        }
    };
    let callback = |event| {
        if let Some(log) = &log {
            log.progress(event);
        }
        progress(event);
    };
    let report = work(&callback, log.as_ref());
    if let Some(log) = &mut log {
        if let Err(message) = log.end(&report) {
            return crate::json::failure(crate::json::CONVERT, crate::json::input(input), &message);
        }
    }
    report
}

fn members(input: &Path, salt: &anon::Salt) -> Option<Vec<Value>> {
    if !input
        .extension()?
        .to_str()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("f3d") || ext.eq_ignore_ascii_case("f3z"))
    {
        return None;
    }
    let mut archive = choices::bounded_archive(File::open(input).ok()?)?;
    let mut budget = 64_u64 << 20;
    let mut members = Vec::new();
    for index in 0..archive.len() {
        let mut member = archive.by_index(index).ok()?;
        let name = member.name().ok()?.into_owned();
        let nested = if name.to_ascii_lowercase().ends_with(".f3d") && member.size() <= budget {
            let size = member.size();
            budget = budget.saturating_sub(size);
            choices::nested_content_size(&mut member, size)
        } else {
            None
        };
        members.push(
            json!({"id": salt.member_id(&name), "compressed_bytes": member.compressed_size(),
            "uncompressed_bytes": member.size(), "nested_content_bytes": nested}),
        );
    }
    Some(members)
}

fn os_version() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/sys/kernel/osrelease")
            .ok()
            .map(|s| s.trim().to_owned())
    }
    #[cfg(target_os = "macos")]
    {
        command_value("/usr/bin/sw_vers", &["-productVersion"])
    }
    #[cfg(windows)]
    {
        command_value("cmd", &["/C", "ver"])
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    None
}

#[cfg(any(target_os = "macos", windows))]
fn command_value(program: &str, args: &[&str]) -> Option<String> {
    let mut command = std::process::Command::new(program);
    command.args(args).stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let output = command.output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn total_memory() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/proc/meminfo")
            .ok()?
            .lines()
            .find_map(|line| {
                line.strip_prefix("MemTotal:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()?
                    .checked_mul(1024)
            })
    }
    #[cfg(target_os = "macos")]
    {
        command_value("/usr/sbin/sysctl", &["-n", "hw.memsize"])?
            .parse()
            .ok()
    }
    #[cfg(windows)]
    {
        command_value(
            "powershell",
            &[
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-CimInstance Win32_OperatingSystem).TotalVisibleMemorySize",
            ],
        )?
        .parse::<u64>()
        .ok()?
        .checked_mul(1024)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    None
}

fn clock_ticks() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        static TICKS: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
        *TICKS.get_or_init(|| {
            let output = std::process::Command::new("getconf")
                .arg("CLK_TCK")
                .output()
                .ok()?;
            let hz = std::str::from_utf8(&output.stdout)
                .ok()?
                .trim()
                .parse()
                .ok()?;
            (hz > 0).then_some(hz)
        })
    }
    #[cfg(not(target_os = "linux"))]
    None
}

fn resources(pid: u32) -> (Option<u64>, Option<u64>) {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok();
        let rss = status.as_deref().and_then(|s| {
            s.lines().find_map(|line| {
                line.strip_prefix("VmRSS:")?
                    .split_whitespace()
                    .next()?
                    .parse::<u64>()
                    .ok()?
                    .checked_mul(1024)
            })
        });
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok();
        let ticks = stat.as_deref().and_then(|s| {
            // comm (field 2) can contain spaces and parentheses. Fields after its last ')' start at 3.
            let mut fields = s.rsplit_once(')')?.1.split_whitespace();
            let user: u64 = fields.nth(11)?.parse().ok()?;
            let system: u64 = fields.next()?.parse().ok()?;
            user.checked_add(system)
        });
        (rss, ticks)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = pid;
        (None, None)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn lines(path: &Path) -> Vec<Value> {
        std::fs::read_to_string(path)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    #[test]
    fn members_redecodes_failures_and_identical_losses_have_no_messages() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.jsonl");
        let options = ConvertOptions {
            diagnostics_salt: Some(anon::Salt::parse("00112233445566778899aabbccddeeff").unwrap()),
            ..Default::default()
        };
        let mut log = Log::open(&path, Path::new("ZebraWidget.f3z"), &options).unwrap();
        for event in [
            MemberEvent::Start,
            MemberEvent::Fail,
            MemberEvent::Start,
            MemberEvent::Finish,
        ] {
            log.member_event("SecretMember.f3d", event, 7, Some("malformed"));
        }
        log.member_event(
            "SecretMember.f3d",
            MemberEvent::Fail,
            8,
            Some("SecretMessage"),
        );
        log.end(&json!({"ok": false, "losses": [
            {"code": "f3d/unknown-feature", "message": "ZebraWidget secret"},
            {"code": "f3d/unknown-feature", "message": "ZebraWidget secret"},
            {"code": "f3d/unknown-feature", "message": "other secret"}
        ]}))
        .unwrap();
        let records = lines(&path);
        let member = &records[1]["member"];
        assert_eq!(
            member,
            &json!(options
                .diagnostics_salt
                .unwrap()
                .member_id("SecretMember.f3d"))
        );
        let end = records.last().unwrap();
        assert_eq!(end["member_decodes"][member.as_str().unwrap()], 2);
        assert_eq!(end["redecoded"], 1);
        assert_eq!(end["repeated_losses"], 1);
        assert_eq!(end["losses"]["f3d/unknown-feature"], 3);
        assert_eq!(records[5]["code"], "other");
        let text = std::fs::read_to_string(path).unwrap();
        for secret in ["ZebraWidget", "SecretMember", "SecretMessage", "secret"] {
            assert!(!text.contains(secret));
        }
    }

    #[test]
    fn dropped_log_has_heartbeat_and_no_end() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("log.jsonl");
        let log = Log::open_every(
            &path,
            Path::new("fixture.f3d"),
            &ConvertOptions::default(),
            Duration::from_millis(5),
        )
        .unwrap();
        log.member_event("Private.f3d", MemberEvent::Start, 0, None);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if lines(&path).iter().any(|line| line["type"] == "heartbeat") {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        drop(log);
        let records = lines(&path);
        assert_eq!(records[0]["type"], "start");
        let heartbeat = records
            .iter()
            .find(|line| line["type"] == "heartbeat")
            .unwrap();
        assert_eq!(heartbeat["member"], records[1]["member"]);
        assert!(records.iter().all(|line| line["type"] != "end"));
        #[cfg(target_os = "linux")]
        {
            assert!(heartbeat["rss_bytes"].as_u64().unwrap() > 0);
            assert!(heartbeat["cpu_ms"].as_u64().is_some());
        }
    }
}
