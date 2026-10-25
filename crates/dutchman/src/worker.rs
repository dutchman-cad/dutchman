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
    if args.next()? != WORKER_ARG {
        return None;
    }
    let parsed = match WorkerArgs::try_parse_from(std::iter::once(program).chain(args)) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return Some(ExitCode::from(2));
        }
    };
    let writer = parsed.options.writer();
    let report = match dutchman_core::convert_traced(
        &parsed.input,
        &parsed.output,
        &writer,
        !parsed.options.no_clobber,
        &parsed.options.part_search(),
        &mut |stage| eprintln!("{STAGE_MARK}{}", stage.id()),
    ) {
        Ok(report) => crate::convert_success(&parsed.input, &parsed.output, &report),
        Err(err) => json::error(CONVERT, json::input(&parsed.input), &err),
    };
    println!("{report}");
    Some(ExitCode::SUCCESS)
}

impl ConvertOptions {
    /// These options as the command-line flags that give them, for a child process; where
    /// part files are looked for is the job's (see [`search_args`]).
    pub fn to_args(&self) -> Vec<OsString> {
        let mut args: Vec<OsString> = Vec::new();
        // Only a choice other than the default is passed, so the arguments of a SolidCraft run
        // stay those every `dutchman` takes.
        if self.to != crate::OutputFormat::Solidcraft {
            args.push("--to".into());
            args.push(
                if self.to == crate::OutputFormat::Fcstd {
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
        if self.import_only {
            args.push("--import-only".into());
        }
        if let Some(cli) = &self.solidcraft {
            args.push("--solidcraft".into());
            args.push(cli.clone().into_os_string());
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
fn search_args(search: &PartSearch) -> Vec<OsString> {
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
        std::thread::spawn(move || follow_stages(err, &stage))
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
fn follow_stages(stream: impl Read, stage: &Mutex<Stage>) {
    use std::io::BufRead;
    for line in std::io::BufReader::new(stream).lines() {
        let Ok(line) = line else { break };
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
                if let Some(held) = resident_bytes(child.id()).filter(|held| *held > limit) {
                    stop(child);
                    return Stopped::OverMemory(held);
                }
            }
        }
        std::thread::sleep(POLL);
    }
}

fn stop(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// The memory process `pid` holds, in bytes, when it can be measured.
#[cfg(target_os = "linux")]
fn resident_bytes(pid: u32) -> Option<u64> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    kb.checked_mul(1024)
}

/// The memory process `pid` holds, in bytes, when it can be measured.
#[cfg(all(unix, not(target_os = "linux")))]
fn resident_bytes(pid: u32) -> Option<u64> {
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
fn resident_bytes(pid: u32) -> Option<u64> {
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
fn resident_bytes(_pid: u32) -> Option<u64> {
    None
}
