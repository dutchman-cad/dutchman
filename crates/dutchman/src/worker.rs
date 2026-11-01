//! Converting one file in a child process of its own, so a batch survives any one file: a
//! file that takes too long or uses too much memory is stopped, and one that crashes the
//! converter fails alone. The child is the running Dutchman program started again with
//! [`WORKER_ARG`]; it prints the file's convert report on stdout and, on stderr, a line as
//! each stage of the conversion begins, so a stopped file's report says where it stopped.

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clap::Parser;
use dutchman_core::Stage;
use serde_json::Value;

use crate::batch::Job;
use crate::json::{self, CONVERT};
use crate::{ConvertOptions, PartSearch};

/// The first argument that makes a Dutchman program convert one file as a batch's child.
pub const WORKER_ARG: &str = "--dutchman-convert-worker";

/// How a stage is marked on the child's stderr.
const STAGE_MARK: &str = "dutchman-stage: ";

/// The most a child's report may hold, in bytes.
const MAX_REPORT_BYTES: u64 = 64 << 20;

/// How often a running child is looked at.
const POLL: Duration = Duration::from_millis(50);

/// How often a running child's memory is measured.
#[cfg(target_os = "linux")]
const MEMORY_POLL: Duration = Duration::from_millis(250);
#[cfg(not(target_os = "linux"))]
const MEMORY_POLL: Duration = Duration::from_secs(1);

/// What one file's conversion may use.
#[derive(Clone, Debug)]
pub struct Limits {
    /// How long it may run.
    pub timeout: Duration,
    /// How much memory it may hold, in bytes; `None` for no limit.
    pub max_memory: Option<u64>,
}

/// The default time one file may take.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(900);

/// The default memory one file may hold, in megabytes.
pub const DEFAULT_MAX_MEMORY_MB: u64 = 8192;

impl Default for Limits {
    fn default() -> Self {
        Limits {
            timeout: DEFAULT_TIMEOUT,
            max_memory: Some(DEFAULT_MAX_MEMORY_MB << 20),
        }
    }
}

/// The child's arguments after [`WORKER_ARG`].
#[derive(Parser)]
#[command(name = "dutchman-convert-worker")]
struct WorkerArgs {
    input: PathBuf,
    output: PathBuf,
    #[command(flatten)]
    options: ConvertOptions,
}

/// When this process was started as a batch's child, converts its file and returns the exit
/// code; otherwise `None`, and the program carries on as itself.
pub fn run_if_worker() -> Option<ExitCode> {
    let mut args = std::env::args_os();
    let program = args.next()?;
    let first = args.next()?;
    if first == crate::diagnose::WORKER_ARG {
        return Some(crate::diagnose::run_worker(program, args));
    }
    if first != WORKER_ARG {
        return None;
    }
    let parsed = match WorkerArgs::try_parse_from(std::iter::once(program).chain(args)) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return Some(ExitCode::from(2));
        }
    };
    let callback = |event| eprintln!("{}", json::progress(&parsed.input, event));
    let members: Option<Arc<dyn dutchman_core::MemberObserver>> =
        std::env::var_os("DUTCHMAN_MEMBER_PROGRESS")
            .map(|_| Arc::new(MemberProgress) as Arc<dyn dutchman_core::MemberObserver>);
    let report = crate::convert_options_with_observer(
        &parsed.input,
        &parsed.output,
        &parsed.options,
        !parsed.options.no_clobber,
        &parsed.options.part_search(),
        &callback,
        members,
    );
    println!("{report}");
    Some(ExitCode::SUCCESS)
}

/// Private worker protocol; names travel only to the supervising process's hashing sink.
struct MemberProgress;

impl dutchman_core::MemberObserver for MemberProgress {
    fn phase_event(
        &self,
        phase: &'static str,
        event: dutchman_core::MemberEvent,
        ms: u64,
        bytes: u64,
    ) {
        let event = if event == dutchman_core::MemberEvent::Start {
            "start"
        } else {
            "finish"
        };
        eprintln!(
            "{}",
            serde_json::json!({"dutchman_phase": phase, "event": event, "ms": ms, "bytes": bytes})
        );
    }
    fn member_event(
        &self,
        path: &str,
        event: dutchman_core::MemberEvent,
        ms: u64,
        code: Option<&'static str>,
    ) {
        let event = match event {
            dutchman_core::MemberEvent::Start => "start",
            dutchman_core::MemberEvent::Finish => "finish",
            dutchman_core::MemberEvent::Fail => "fail",
        };
        eprintln!(
            "{}",
            serde_json::json!({"dutchman_member": path, "event": event, "ms": ms, "code": code})
        );
    }
}

impl ConvertOptions {
    /// These options as the command-line flags that give them, for a child process; where
    /// part files are looked for is the job's (see [`search_args`]).
    pub fn to_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = Vec::new();
        args.push("--threads".into());
        args.push(self.threads.to_string().into());
        // Only a choice other than the default is passed, so the arguments of a SolidCraft run
        // stay those every `dutchman` takes.
        if self.to != crate::OutputFormat::Solidcraft {
            args.push("--to".into());
            args.push(
                if self.to == crate::OutputFormat::Dxf {
                    "dxf"
                } else if self.to == crate::OutputFormat::Fcstd {
                    "fcstd"
                } else {
                    "freecad"
                }
                .into(),
            );
        }
        if self.verify {
            args.push("--verify".into());
        }
        if self.freecad_timeout != crate::ConvertOptions::default().freecad_timeout {
            args.push("--freecad-timeout".into());
            args.push(self.freecad_timeout.to_string().into());
        }
        args.push("--preset".into());
        args.push(self.preset.name().into());
        for (enabled, flag) in [
            (self.no_sketches, "--no-sketches"),
            (self.no_joints, "--no-joints"),
            (self.no_memory_check, "--no-memory-check"),
        ] {
            if enabled {
                args.push(flag.into());
            }
        }
        if self.import_only {
            args.push("--import-only".into());
        }
        if let Some(cli) = &self.solidcraft {
            args.push("--solidcraft".into());
            args.push(cli.clone().into_os_string());
        }
        if let Some(version) = self.solidcraft_version {
            args.push("--solidcraft-version".into());
            args.push(version.to_string().into());
        }
        if self.no_verify {
            args.push("--no-verify".into());
        }
        if self.no_clobber {
            args.push("--no-clobber".into());
        }
        args.push("--verify-timeout".into());
        args.push(self.verify_timeout.to_string().into());
        args
    }
}

/// The flags that look for a job's part files as `search` says.
pub(crate) fn search_args(search: &PartSearch) -> Vec<OsString> {
    let mut args = Vec::new();
    if let Some(project) = &search.project {
        args.push("--project".into());
        args.push(project.clone().into_os_string());
    }
    for folder in &search.also {
        args.push("--search".into());
        args.push(folder.clone().into_os_string());
    }
    args
}

/// Converts `job` in a child process started from `program`, within `limits`, and returns
/// its convert report, or a failure report saying why the child stopped.
pub fn convert_isolated(
    program: &Path,
    job: &Job,
    options: &ConvertOptions,
    limits: &Limits,
) -> Value {
    convert_isolated_until(program, job, options, limits, None)
}

/// Converts in a child, allowing thirty seconds to finish after shutdown is requested.
pub fn convert_isolated_until(
    program: &Path,
    job: &Job,
    options: &ConvertOptions,
    limits: &Limits,
    shutdown: Option<&std::sync::atomic::AtomicBool>,
) -> Value {
    convert_isolated_with_progress(program, job, options, limits, shutdown, &|_| {})
}

/// Converts in a child and forwards its coarse progress, including heartbeats.
pub fn convert_isolated_with_progress(
    program: &Path,
    job: &Job,
    options: &ConvertOptions,
    limits: &Limits,
    shutdown: Option<&std::sync::atomic::AtomicBool>,
    on_progress: &(dyn Fn(dutchman_core::ProgressEvent) + Send + Sync),
) -> Value {
    crate::diagnostics::run(&job.input, options, on_progress, |progress, log| {
        convert_isolated_inner(program, job, options, limits, shutdown, progress, log)
    })
}

fn convert_isolated_inner(
    program: &Path,
    job: &Job,
    options: &ConvertOptions,
    limits: &Limits,
    shutdown: Option<&std::sync::atomic::AtomicBool>,
    on_progress: &(dyn Fn(dutchman_core::ProgressEvent) + Send + Sync),
    log: Option<&crate::diagnostics::Log>,
) -> Value {
    let mut options = options.clone();
    options.no_clobber = !job.replace;
    let mut command = Command::new(program);
    #[cfg(unix)]
    if shutdown.is_some() {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    if shutdown.is_some() {
        use std::os::windows::process::CommandExt;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(CREATE_NEW_PROCESS_GROUP);
    }
    if log.is_some() {
        command.env("DUTCHMAN_MEMBER_PROGRESS", "1");
    } else {
        command.env_remove("DUTCHMAN_MEMBER_PROGRESS");
    }
    let child = command
        .arg(WORKER_ARG)
        .arg(&job.input)
        .arg(&job.output)
        .args(options.to_args())
        .args(search_args(&job.search))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(err) => {
            return json::failure(
                CONVERT,
                json::input(&job.input),
                &format!("cannot start the converter: {err}"),
            )
        }
    };
    if let Some(log) = log {
        log.process(child.id());
    }
    std::thread::scope(|scope| {
        let stage = Arc::new(Mutex::new(Stage::Input));
        let stdout = child.stdout.take().map(|out| {
            std::thread::spawn(move || {
                let mut text = String::new();
                let _ = out.take(MAX_REPORT_BYTES).read_to_string(&mut text);
                text
            })
        });
        let stderr = child.stderr.take().map(|err| {
            let stage = Arc::clone(&stage);
            scope.spawn(move || follow_stages(err, &stage, on_progress, log))
        });

        let stopped = supervise(&mut child, limits, shutdown);
        let text = stdout
            .and_then(|reader| reader.join().ok())
            .unwrap_or_default();
        if let Some(reader) = stderr {
            let _ = reader.join();
        }
        let at = stage.lock().map_or(Stage::Input, |stage| *stage);
        let fail = |message: String| {
            if at == Stage::Write {
                // Stopped while writing: what is there is not a whole design.
                let _ = std::fs::remove_file(&job.output);
            }
            json::failure_at(
                CONVERT,
                json::input(&job.input),
                &message,
                at,
                upstream_of(at),
            )
        };
        match stopped {
            Stopped::Exited(status) => match serde_json::from_str::<Value>(text.trim()) {
                Ok(report) if report.is_object() => report,
                _ => fail(format!(
                    "the converter stopped unexpectedly while at the {} stage ({status})",
                    at.id()
                )),
            },
            Stopped::TimedOut => fail(format!(
                "stopped after {} s, Dutchman's limit for one file, while at the {} stage",
                limits.timeout.as_secs(),
                at.id()
            )),
            Stopped::Shutdown => fail(format!(
                "stopped after the 30 s shutdown grace period while at the {} stage",
                at.id()
            )),
            Stopped::OverMemory(bytes) => fail(format!(
            "stopped when it held {} MB, over Dutchman's limit of {} MB for one file, while at \
             the {} stage",
            bytes >> 20,
            limits.max_memory.unwrap_or(0) >> 20,
            at.id()
        )),
            Stopped::Lost(err) => fail(format!("lost track of the converter: {err}")),
        }
    })
}

/// The project whose code a stage runs, when it is not Dutchman's own.
fn upstream_of(stage: Stage) -> Option<&'static str> {
    match stage {
        Stage::Decode | Stage::Project => Some("cadmpeg"),
        Stage::SolidCraft => Some("solidcraft"),
        _ => None,
    }
}

/// Records each stage the child marks on `stream` in `stage`, until the stream ends.
fn follow_stages(
    stream: impl Read,
    stage: &Mutex<Stage>,
    on_progress: &(dyn Fn(dutchman_core::ProgressEvent) + Send + Sync),
    log: Option<&crate::diagnostics::Log>,
) {
    use std::io::BufRead;
    for line in std::io::BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
        if let (Some(log), Ok(record)) = (log, serde_json::from_str::<Value>(&line)) {
            if let Some(phase) = record.get("dutchman_phase").and_then(Value::as_str) {
                let event = match record.get("event").and_then(Value::as_str) {
                    Some("start") => Some(dutchman_core::MemberEvent::Start),
                    Some("finish") => Some(dutchman_core::MemberEvent::Finish),
                    _ => None,
                };
                if let Some(event) = event {
                    log.phase_event(
                        phase,
                        event,
                        record.get("ms").and_then(Value::as_u64).unwrap_or(0),
                        record.get("bytes").and_then(Value::as_u64).unwrap_or(0),
                    );
                }
                continue;
            }
            if let Some(member) = record.get("dutchman_member").and_then(Value::as_str) {
                let event = match record.get("event").and_then(Value::as_str) {
                    Some("start") => Some(dutchman_core::MemberEvent::Start),
                    Some("finish") => Some(dutchman_core::MemberEvent::Finish),
                    Some("fail") => Some(dutchman_core::MemberEvent::Fail),
                    _ => None,
                };
                if let Some(event) = event {
                    log.member_event(
                        member,
                        event,
                        record.get("ms").and_then(Value::as_u64).unwrap_or(0),
                        record
                            .get("code")
                            .and_then(Value::as_str)
                            .map(crate::diagnostics::member_code),
                    );
                }
                continue;
            }
        }
        if let Some(event) = serde_json::from_str::<Value>(&line)
            .ok()
            .as_ref()
            .and_then(json::parse_progress)
        {
            if let Ok(mut stage) = stage.lock() {
                *stage = event.stage;
            }
            on_progress(event);
            continue;
        }
        let Some(id) = line.strip_prefix(STAGE_MARK) else {
            continue;
        };
        if let (Some(found), Ok(mut stage)) = (
            Stage::ALL.into_iter().find(|s| s.id() == id.trim()),
            stage.lock(),
        ) {
            *stage = found;
        }
    }
}

/// How a child ended.
enum Stopped {
    /// On its own, with this status.
    Exited(std::process::ExitStatus),
    /// Stopped for running past its time.
    TimedOut,
    /// Stopped after the shutdown grace period.
    Shutdown,
    /// Stopped for holding this many bytes, over its limit.
    OverMemory(u64),
    /// It could not be waited on.
    Lost(std::io::Error),
}

/// Waits for `child`, stopping it when it runs past `limits`.
fn supervise(
    child: &mut Child,
    limits: &Limits,
    shutdown: Option<&std::sync::atomic::AtomicBool>,
) -> Stopped {
    let mut stopping = None;
    let started = Instant::now();
    let mut measured = started;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Stopped::Exited(status),
            Ok(None) => {}
            Err(err) => {
                let _ = child.kill();
                return Stopped::Lost(err);
            }
        }
        if shutdown.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
            let since = stopping.get_or_insert_with(Instant::now);
            if since.elapsed() >= Duration::from_secs(30) {
                stop(child);
                return Stopped::Shutdown;
            }
        }
        if started.elapsed() > limits.timeout {
            stop(child);
            return Stopped::TimedOut;
        }
        if let Some(limit) = limits.max_memory {
            if measured.elapsed() >= MEMORY_POLL {
                measured = Instant::now();
                if let Some(held) = resident_tree_bytes(child.id()).filter(|held| *held > limit) {
                    stop(child);
                    return Stopped::OverMemory(held);
                }
            }
        }
        std::thread::sleep(POLL);
    }
}

pub(crate) fn stop(child: &mut Child) {
    // Include kernels started by a worker, retaining the parent's normal signal group.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if let Some(processes) = process_tree(child.id()) {
        let _ = Command::new("kill")
            .arg("-KILL")
            .args(processes.into_iter().map(|(pid, _)| pid.to_string()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    #[cfg(windows)]
    let _ = Command::new("taskkill")
        .args(["/PID", &child.id().to_string(), "/T", "/F"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
}

/// Converter memory including kernel subprocesses where the process tree is available.
#[cfg(target_os = "linux")]
fn process_tree(pid: u32) -> Option<Vec<(u32, u64)>> {
    let mut pending = vec![pid];
    let mut seen = std::collections::BTreeSet::new();
    let mut processes = Vec::new();
    while let Some(current) = pending.pop() {
        if !seen.insert(current) {
            continue;
        }
        if seen.len() > 4096 {
            return None;
        }
        processes.push((current, resident_bytes(current).unwrap_or(0)));
        // Children are listed per thread, including kernels started by part workers.
        if let Ok(tasks) = std::fs::read_dir(format!("/proc/{current}/task")) {
            for task in tasks.take(4096).filter_map(Result::ok) {
                if let Ok(children) = std::fs::read_to_string(task.path().join("children")) {
                    pending.extend(
                        children
                            .split_whitespace()
                            .filter_map(|p| p.parse::<u32>().ok()),
                    );
                    if pending.len() > 4096 {
                        return None;
                    }
                }
            }
        }
    }
    Some(processes)
}

#[cfg(any(target_os = "macos", windows))]
fn process_tree(pid: u32) -> Option<Vec<(u32, u64)>> {
    use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_memory(),
    );
    let mut children = std::collections::BTreeMap::<Pid, Vec<Pid>>::new();
    for (id, process) in system.processes() {
        if let Some(parent) = process.parent() {
            children.entry(parent).or_default().push(*id);
        }
    }
    let mut pending = vec![Pid::from_u32(pid)];
    let mut seen = std::collections::BTreeSet::new();
    let mut processes = Vec::new();
    while let Some(current) = pending.pop() {
        if !seen.insert(current) {
            continue;
        }
        if seen.len() > 4096 {
            return None;
        }
        processes.push((
            current.as_u32(),
            system.process(current).map_or(0, |p| p.memory()),
        ));
        pending.extend(children.get(&current).into_iter().flatten().copied());
    }
    Some(processes)
}

#[cfg(any(target_os = "linux", target_os = "macos", windows))]
pub(crate) fn resident_tree_bytes(pid: u32) -> Option<u64> {
    match process_tree(pid) {
        Some(processes) => {
            let bytes = processes
                .into_iter()
                .map(|(_, memory)| memory)
                .fold(0u64, u64::saturating_add);
            (bytes > 0).then_some(bytes)
        }
        None => Some(u64::MAX),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
pub(crate) fn resident_tree_bytes(pid: u32) -> Option<u64> {
    resident_bytes(pid)
}

/// The memory process `pid` holds, in bytes, when it can be measured.
#[cfg(target_os = "linux")]
pub(crate) fn resident_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kb.checked_mul(1024)
}

/// The memory process `pid` holds, in bytes, when it can be measured.
#[cfg(all(unix, not(target_os = "linux")))]
pub(crate) fn resident_bytes(pid: u32) -> Option<u64> {
    let out = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let kb: u64 = String::from_utf8_lossy(&out.stdout).trim().parse().ok()?;
    kb.checked_mul(1024)
}

/// The memory process `pid` holds, in bytes, when it can be measured.
#[cfg(windows)]
pub(crate) fn resident_bytes(pid: u32) -> Option<u64> {
    // `"name","pid","session","#","12,345 K"`
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let field = text.lines().next()?.rsplit("\",\"").next()?;
    let kb: u64 = field
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .parse()
        .ok()?;
    kb.checked_mul(1024)
}

/// The memory process `pid` holds: not measured on this system.
#[cfg(not(any(unix, windows)))]
pub(crate) fn resident_bytes(_pid: u32) -> Option<u64> {
    None
}

#[cfg(all(test, target_os = "linux"))]
mod tree_tests {
    use super::*;
    #[test]
    fn a_worker_limit_counts_and_stops_its_kernel_children() {
        use std::os::unix::process::CommandExt;
        let mut command = Command::new("bash");
        command.args(["-c", "sleep 30 & wait"]).process_group(0);
        let mut child = command.spawn().unwrap();
        std::thread::sleep(Duration::from_millis(50));
        let own = resident_bytes(child.id()).unwrap();
        let tree = resident_tree_bytes(child.id()).unwrap();
        assert!(tree > own);
        let children =
            std::fs::read_to_string(format!("/proc/{}/task/{}/children", child.id(), child.id()))
                .unwrap();
        stop(&mut child);
        for pid in children.split_whitespace() {
            // A killed child may briefly remain a zombie until reaped, but holds no RSS.
            let pid = pid.parse().unwrap();
            let deadline = Instant::now() + Duration::from_secs(1);
            while resident_bytes(pid).unwrap_or(0) > 0 && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(10));
            }
            assert_eq!(resident_bytes(pid).unwrap_or(0), 0);
        }
    }
}
