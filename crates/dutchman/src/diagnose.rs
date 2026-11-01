//! `dutchman diagnose`: finding which part of a large design fails, runs slowly or takes the
//! memory, on the user's own machine, without sharing the design.
//!
//! The design is read once, in a child process of the running program. Its parts are numbered
//! (see [`dutchman_core::parts`]) and each is then converted on its own, one after another,
//! in that same child, with the time and memory it uses measured. The supervising process
//! watches the child: a part that runs past the time limit, or takes the child past the memory
//! limit, is stopped by stopping the child, which costs only that part; a new child reads the
//! design again and carries on with the next part. A part that crashes the converter, or that
//! the system's out-of-memory killer ends, is handled the same way.
//!
//! The report is written as it goes: `diagnose/report.json` (schema `dutchman.diagnose/1`) and
//! `diagnose/report.txt` hold part numbers, outcomes, stages, error kinds, finding codes,
//! times and sizes, and nothing else: no names, paths or identities, so they can be shared.
//! `diagnose/local-key.json` maps the numbers back to the design's part names and must not be
//! shared. A run stopped half way leaves a report with `status` `running` that holds every part
//! finished so far.
//!
//! Library use: [`run`] takes the file, an output folder and [`Settings`] and returns the
//! report; [`report_from`] reads a written report back; [`failing_part`] names the part to
//! look at first.
//!
//! [`narrow`] goes on from such a report by itself, down to the first failing feature of the
//! failing part, and records the failure's [`signature`].

#![allow(clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use clap::Parser;
use dutchman_core::{parts, Format, Report, Writer};
use serde_json::{json, Map, Value};

use crate::json as wire;
use crate::{diagnostics, worker, ConvertOptions};

mod checks;
pub mod known;
pub mod narrow;
pub mod recipes;
pub mod signature;
pub mod step;

/// The schema of `diagnose/report.json`.
pub const SCHEMA: &str = wire::DIAGNOSE;

/// The schema of `diagnose/local-key.json`.
pub const LOCAL_SCHEMA: &str = "dutchman.diagnose-local/1";

/// The first argument that makes a Dutchman program diagnose one file as a supervisor's child.
pub const WORKER_ARG: &str = "--dutchman-diagnose-worker";

/// The folder inside `--out` the files are written to.
pub const FOLDER: &str = "diagnose";

/// The default time one part may take.
pub const DEFAULT_TIME_LIMIT: Duration = Duration::from_secs(300);

/// Reading the design may take this many times the time one part may, and at least
/// [`MIN_READ_LIMIT`].
const READ_FACTOR: u32 = 6;
const MIN_READ_LIMIT: Duration = Duration::from_secs(1800);

/// How many times the design is read again, after a part stopped the reader, before the parts
/// left are given up as not run.
const MAX_RESTARTS: usize = 64;

/// How often the supervisor looks at the child's messages, time and memory.
const POLL: Duration = Duration::from_millis(50);
const MEMORY_POLL: Duration = Duration::from_millis(200);

/// How often the worker measures its own memory while a part is converted.
#[cfg(target_os = "linux")]
const SAMPLE: Duration = Duration::from_millis(10);
#[cfg(not(target_os = "linux"))]
const SAMPLE: Duration = Duration::from_millis(250);

/// How a diagnose run is set up.
#[derive(Clone, Debug)]
pub struct Settings {
    /// How long one part may take.
    pub time_limit: Duration,
    /// How much memory the converter may hold while it works on one part (the design it has
    /// read included), in bytes; `None` for no limit.
    pub memory_limit: Option<u64>,
    /// How to look for an assembly's part files, and the rest of the conversion's choices.
    pub options: ConvertOptions,
    /// The program the child is started from (default: the running program).
    pub program: Option<PathBuf>,
    /// Matching STEP export; otherwise an adjacent same-name export is discovered.
    pub step_path: Option<PathBuf>,
    /// The numbered part to target in the report.
    pub target_part: Option<usize>,
    /// Written SolidCraft design to check against stored source geometry (doctor only).
    pub converted_path: Option<PathBuf>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            time_limit: DEFAULT_TIME_LIMIT,
            memory_limit: default_memory_limit(),
            options: ConvertOptions::default(),
            program: None,
            step_path: None,
            target_part: None,
            converted_path: None,
        }
    }
}

/// Four fifths of the memory free now, when it can be measured, else 8 GB.
pub fn default_memory_limit() -> Option<u64> {
    Some(
        crate::choices::available_memory()
            .map_or(8 << 30, |free| free / 5 * 4)
            .max(1 << 30),
    )
}

/// What happens while a run goes on, for a front end to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// The design is being read.
    Reading,
    /// The design was read: this many parts, in this many milliseconds.
    Read { parts: usize, ms: u64 },
    /// The design could not be read.
    ReadFailed { outcome: &'static str },
    /// A part finished: how many have, of how many, and which this was.
    Part {
        done: usize,
        total: usize,
        number: usize,
        outcome: &'static str,
        ms: u64,
    },
    /// The reader was stopped for a part and started again.
    Restarted,
}

/// How a part ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Outcome {
    Ok,
    Failed,
    TimedOut,
    OutOfMemory,
    /// A sub-assembly, or a component, with no geometry of its own: nothing to convert.
    Container,
    NotRun,
}

impl Outcome {
    fn id(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::OutOfMemory => "out_of_memory",
            Self::Container => "container",
            Self::NotRun => "not_run",
        }
    }

    /// The outcome with this report id. A part not run has none: it has yet to run.
    fn from_id(id: &str) -> Option<Self> {
        [
            Self::Ok,
            Self::Failed,
            Self::TimedOut,
            Self::OutOfMemory,
            Self::Container,
        ]
        .into_iter()
        .find(|outcome| outcome.id() == id)
    }

    /// Where it sorts: failures, parts not run, parts that converted, containers.
    fn order(self) -> u8 {
        match self {
            Self::Failed | Self::TimedOut | Self::OutOfMemory => 0,
            Self::NotRun => 1,
            Self::Ok => 2,
            Self::Container => 3,
        }
    }
}

/// What is known of one part's run.
#[derive(Clone, Debug)]
struct PartRun {
    outcome: Outcome,
    stage: Option<String>,
    kind: Option<String>,
    findings: Map<String, Value>,
    curve_kinds: Map<String, Value>,
    time_ms: u64,
    peak_bytes: u64,
    extra_bytes: u64,
}

impl PartRun {
    fn simple(outcome: Outcome) -> Self {
        PartRun {
            outcome,
            stage: None,
            kind: None,
            findings: Map::new(),
            curve_kinds: Map::new(),
            time_ms: 0,
            peak_bytes: 0,
            extra_bytes: 0,
        }
    }
}

/// A part as the worker listed it.
#[derive(Clone, Debug)]
struct Listed {
    number: usize,
    parent: Option<usize>,
    depth: usize,
    assembly: bool,
    bodies: usize,
    entities: u64,
    /// Private: for the local key only.
    name: String,
}

/// Everything a run knows.
struct State {
    out: PathBuf,
    input: PathBuf,
    settings: Settings,
    listed: Vec<Listed>,
    results: BTreeMap<usize, PartRun>,
    read: Option<PartRun>,
    read_ms: u64,
    read_peak: u64,
    restarts: usize,
    started: Instant,
    written: Option<Instant>,
    comparison: Value,
    placement_checks: Value,
    geometry_checks: Value,
}

/// Diagnoses `input`, writing `<out>/diagnose/report.json`, `report.txt` and `local-key.json`
/// as it goes, and returns the run's result: the report, where the files are and which is safe
/// to share. `Err` when the folder cannot be made (it is never reused: an existing
/// `diagnose` folder is refused) or written.
pub fn run(
    input: &Path,
    out: &Path,
    settings: &Settings,
    on_event: &(dyn Fn(Event) + Sync),
) -> Result<Value, String> {
    let folder = out.join(FOLDER);
    std::fs::create_dir_all(out).map_err(|e| format!("cannot create {}: {e}", out.display()))?;
    std::fs::create_dir(&folder).map_err(|e| {
        format!(
            "cannot create {}; choose a fresh --out folder: {e}",
            folder.display()
        )
    })?;
    if settings.target_part == Some(0) {
        return Err("Part numbers start at 1.".into());
    }
    let state = State::new(folder, input, settings);
    run_state(state, settings, on_event)
}

/// [`run`] in a folder a stopped run may have left: the parts it finished are kept and only the
/// others are converted. A finished run's report is returned as it is.
pub(crate) fn run_resuming(
    input: &Path,
    out: &Path,
    settings: &Settings,
    on_event: &(dyn Fn(Event) + Sync),
) -> Result<Value, String> {
    let folder = out.join(FOLDER);
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    if let Ok(report) = report_from(&folder) {
        if report["status"] != "running" {
            return Ok(json!({"status": report["status"], "report": report}));
        }
    }
    let mut state = State::new(folder, input, settings);
    state.restore();
    run_state(state, settings, on_event)
}

fn run_state(
    mut state: State,
    settings: &Settings,
    on_event: &(dyn Fn(Event) + Sync),
) -> Result<Value, String> {
    state.save("running", true)?;
    let program = match &settings.program {
        Some(program) => program.clone(),
        None => std::env::current_exe()
            .map_err(|err| format!("cannot find the running program: {err}"))?,
    };
    let mut status = "complete";
    let mut skip: Vec<usize> = state.results.keys().copied().collect();
    loop {
        on_event(Event::Reading);
        match state.work(&program, &skip, on_event)? {
            Ended::Done => break,
            Ended::ReadFailed => {
                status = "read_failed";
                break;
            }
            Ended::Stopped(Some(part)) => {
                skip.push(part);
                if state.results.len() >= state.listed.len() {
                    break;
                }
                state.restarts += 1;
                if state.restarts > MAX_RESTARTS {
                    for listed in &state.listed {
                        state
                            .results
                            .entry(listed.number)
                            .or_insert_with(|| PartRun::simple(Outcome::NotRun));
                    }
                    break;
                }
                on_event(Event::Restarted);
            }
            Ended::Stopped(None) => {
                status = "read_failed";
                break;
            }
        }
    }
    if let Some(target) = settings.target_part {
        if !state.listed.is_empty() && !state.listed.iter().any(|part| part.number == target) {
            return Err("Choose a part number listed in the diagnose report.".into());
        }
    }
    state.save(status, true)?;
    let report = state.report(status);
    let failing = failing_part_in(&report);
    let summary = summary_text(&report, failing);
    Ok(json!({
        "schema": SCHEMA, "ok": true, "summary": summary, "status": status,
        "paths": {"report": state.out.join("report.json"), "text": state.out.join("report.txt"),
                  "local_key": state.out.join("local-key.json")},
        "share": "report.json and report.txt",
        "private": "local-key.json",
        "report": report,
    }))
}

/// How a child ended.
enum Ended {
    /// It converted every part it was given.
    Done,
    /// The design could not be read.
    ReadFailed,
    /// It was stopped, or ended without finishing, while on this part (none: while reading).
    Stopped(Option<usize>),
}

/// What the supervisor knows of the child it is watching.
#[derive(Default)]
struct Ctx {
    /// The part it is on, and since when.
    current: Option<(usize, Instant)>,
    /// The most memory it was seen holding.
    seen_peak: u64,
    done: bool,
    read_failed: bool,
}

/// What the child said.
enum Message {
    Catalog {
        read_ms: u64,
        peak: u64,
        parts: Vec<Listed>,
    },
    ReadFailed {
        ms: u64,
        error: Value,
    },
    Start {
        part: usize,
    },
    Finish {
        part: usize,
        ms: u64,
        peak: u64,
        base: u64,
        error: Value,
    },
    Comparison(Value),
    Placements(Value),
    Geometry(Value),
    Done,
}

fn message(line: &str) -> Option<Message> {
    let value: Value = serde_json::from_str(line).ok()?;
    let event = value.get("dutchman_diagnose")?.as_str()?;
    let number = |v: &Value| v.as_u64().and_then(|n| usize::try_from(n).ok());
    Some(match event {
        "catalog" => Message::Catalog {
            read_ms: value["read_ms"].as_u64().unwrap_or(0),
            peak: value["peak"].as_u64().unwrap_or(0),
            parts: value["parts"]
                .as_array()?
                .iter()
                .filter_map(|p| {
                    Some(Listed {
                        number: number(&p["number"])?,
                        parent: number(&p["parent"]),
                        depth: number(&p["depth"]).unwrap_or(0),
                        assembly: p["assembly"].as_bool().unwrap_or(false),
                        bodies: number(&p["bodies"]).unwrap_or(0),
                        entities: p["entities"].as_u64().unwrap_or(0),
                        name: p["name"].as_str().unwrap_or_default().to_owned(),
                    })
                })
                .collect(),
        },
        "read_failed" => Message::ReadFailed {
            ms: value["ms"].as_u64().unwrap_or(0),
            error: value["error"].clone(),
        },
        "start" => Message::Start {
            part: number(&value["part"])?,
        },
        "finish" => Message::Finish {
            part: number(&value["part"])?,
            ms: value["ms"].as_u64().unwrap_or(0),
            peak: value["peak"].as_u64().unwrap_or(0),
            base: value["base"].as_u64().unwrap_or(0),
            error: value["error"].clone(),
        },
        "comparison" => Message::Comparison(value.get("report")?.clone()),
        "placements" => Message::Placements(value.get("report")?.clone()),
        "geometry" => Message::Geometry(value.get("report")?.clone()),
        "done" => Message::Done,
        _ => return None,
    })
}

impl State {
    fn new(out: PathBuf, input: &Path, settings: &Settings) -> Self {
        State {
            out,
            input: input.to_path_buf(),
            settings: settings.clone(),
            listed: Vec::new(),
            results: BTreeMap::new(),
            read: None,
            read_ms: 0,
            read_peak: 0,
            restarts: 0,
            started: Instant::now(),
            written: None,
            comparison: Value::Null,
            placement_checks: Value::Null,
            geometry_checks: Value::Null,
        }
    }

    /// Takes up what a stopped run wrote to this folder: its parts, and the results of the
    /// parts it finished. A part it was on when it stopped runs again.
    fn restore(&mut self) {
        let Ok(report) = report_from(&self.out) else {
            return;
        };
        self.geometry_checks = report
            .get("geometry_checks")
            .cloned()
            .unwrap_or(Value::Null);
        self.placement_checks = report
            .get("placement_checks")
            .cloned()
            .unwrap_or(Value::Null);
        self.comparison = report
            .get("step_comparison")
            .cloned()
            .unwrap_or(Value::Null);
        let key: Value = std::fs::read(self.out.join("local-key.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or(Value::Null);
        let names: BTreeMap<u64, &str> = key["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some((p["part"].as_u64()?, p["name"].as_str()?)))
            .collect();
        let number = |v: &Value| v.as_u64().and_then(|n| usize::try_from(n).ok());
        let mut rows: Vec<&Value> = report["parts"].as_array().into_iter().flatten().collect();
        rows.sort_by_key(|row| row["part"].as_u64());
        for row in rows {
            let Some(part) = number(&row["part"]) else {
                continue;
            };
            self.listed.push(Listed {
                number: part,
                parent: number(&row["parent"]),
                depth: number(&row["depth"]).unwrap_or(0),
                assembly: row["kind"] == "assembly",
                bodies: number(&row["bodies"]).unwrap_or(0),
                entities: row["entities"].as_u64().unwrap_or(0),
                name: names
                    .get(&(part as u64))
                    .copied()
                    .unwrap_or_default()
                    .to_owned(),
            });
            let Some(outcome) = row["outcome"].as_str().and_then(Outcome::from_id) else {
                continue;
            };
            let text = |key: &str| row[key].as_str().map(str::to_owned);
            let map = |key: &str| row[key].as_object().cloned().unwrap_or_default();
            self.results.insert(
                part,
                PartRun {
                    outcome,
                    stage: text("stage"),
                    kind: text("error_kind"),
                    findings: map("findings"),
                    curve_kinds: map("curve_kinds"),
                    time_ms: row["time_ms"].as_u64().unwrap_or(0),
                    peak_bytes: row["peak_memory_mb"].as_u64().unwrap_or(0) << 20,
                    extra_bytes: row["extra_memory_mb"].as_u64().unwrap_or(0) << 20,
                },
            );
        }
        self.read_ms = report
            .pointer("/read/time_ms")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        self.read_peak = report
            .pointer("/read/peak_memory_mb")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            << 20;
        self.restarts = number(&report["summary"]["restarts"]).unwrap_or(0);
    }

    /// Runs one child until it ends or is stopped.
    fn work(
        &mut self,
        program: &Path,
        skip: &[usize],
        on_event: &(dyn Fn(Event) + Sync),
    ) -> Result<Ended, String> {
        let mut command = Command::new(program);
        command.arg(WORKER_ARG).arg(&self.input);
        if !skip.is_empty() {
            command.arg("--skip").arg(
                skip.iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(","),
            );
        }
        if let Some(step) = self
            .settings
            .step_path
            .clone()
            .or_else(|| step::matching(&self.input))
        {
            command.arg("--step").arg(step);
        }
        command.arg("--preview").arg(self.out.join("preview"));
        if let Some(converted) = &self.settings.converted_path {
            command.arg("--converted").arg(converted);
        }
        command
            .args(self.settings.options.to_args())
            .args(worker::search_args(&self.settings.options.part_search()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped());
        let mut child = command
            .spawn()
            .map_err(|err| format!("cannot start the converter: {err}"))?;
        let stderr = child.stderr.take();
        let (send, receive) = mpsc::channel::<Message>();
        let reader = std::thread::spawn(move || {
            let Some(stderr) = stderr else { return };
            for line in std::io::BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                if let Some(message) = message(&line) {
                    if send.send(message).is_err() {
                        break;
                    }
                }
            }
        });

        let started = Instant::now();
        let read_limit = (self.settings.time_limit * READ_FACTOR).max(MIN_READ_LIMIT);
        let mut ctx = Ctx::default();
        let mut measured = Instant::now();
        let mut stopped_by: Option<Outcome> = None;
        let exit = loop {
            while let Ok(message) = receive.try_recv() {
                self.handle(message, &mut ctx, on_event)?;
            }
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break Some(status);
            }
            if let Some((_, since)) = ctx.current {
                if since.elapsed() > self.settings.time_limit {
                    stopped_by = Some(Outcome::TimedOut);
                    worker::stop(&mut child);
                    break None;
                }
            } else if self.listed.is_empty() && started.elapsed() > read_limit {
                stopped_by = Some(Outcome::TimedOut);
                worker::stop(&mut child);
                break None;
            }
            if measured.elapsed() >= MEMORY_POLL {
                measured = Instant::now();
                if let Some(held) = worker::resident_tree_bytes(child.id()) {
                    ctx.seen_peak = ctx.seen_peak.max(held);
                    if self.settings.memory_limit.is_some_and(|limit| held > limit) {
                        stopped_by = Some(Outcome::OutOfMemory);
                        worker::stop(&mut child);
                        break None;
                    }
                }
            }
            std::thread::sleep(POLL);
        };
        let _ = reader.join();
        // What the child said just before it ended, in order.
        while let Ok(message) = receive.try_recv() {
            self.handle(message, &mut ctx, on_event)?;
        }
        if ctx.done {
            return Ok(Ended::Done);
        }
        if ctx.read_failed {
            return Ok(Ended::ReadFailed);
        }
        // The child ended without finishing: on the part it was on, or while reading.
        let held = ctx.seen_peak.max(self.settings.memory_limit.unwrap_or(0));
        let run_for = |time: Duration| -> PartRun {
            let mut run = PartRun::simple(match stopped_by {
                Some(outcome) => outcome,
                None if killed_by_system(exit) => Outcome::OutOfMemory,
                None => Outcome::Failed,
            });
            run.stage = Some("map".to_owned());
            run.time_ms = u64::try_from(time.as_millis()).unwrap_or(u64::MAX);
            run.peak_bytes = if run.outcome == Outcome::OutOfMemory {
                held
            } else {
                ctx.seen_peak
            };
            if run.outcome == Outcome::Failed {
                run.kind = Some("crash".to_owned());
            }
            run
        };
        match ctx.current {
            Some((part, since)) => {
                let run = run_for(since.elapsed());
                self.finished(part, run, on_event);
                Ok(Ended::Stopped(Some(part)))
            }
            None => {
                let mut run = run_for(started.elapsed());
                run.stage = Some("decode".to_owned());
                self.read = Some(run);
                on_event(Event::ReadFailed { outcome: "failed" });
                Ok(Ended::Stopped(None))
            }
        }
    }

    fn handle(
        &mut self,
        message: Message,
        ctx: &mut Ctx,
        on_event: &(dyn Fn(Event) + Sync),
    ) -> Result<(), String> {
        match message {
            Message::Catalog {
                read_ms,
                peak,
                parts,
            } => {
                if self.listed.is_empty() {
                    self.read_ms = read_ms;
                    self.read_peak = peak;
                    self.listed = parts;
                    self.save_key()?;
                    on_event(Event::Read {
                        parts: self.listed.len(),
                        ms: read_ms,
                    });
                } else {
                    // A later child read the same design again.
                    self.read_peak = self.read_peak.max(peak);
                }
                self.save("running", false)?;
            }
            Message::ReadFailed { ms, error } => {
                ctx.read_failed = true;
                let mut run = classify(&error);
                run.outcome = Outcome::Failed;
                run.time_ms = ms;
                self.read = Some(run);
                on_event(Event::ReadFailed { outcome: "failed" });
            }
            Message::Start { part } => {
                ctx.current = Some((part, Instant::now()));
                ctx.seen_peak = 0;
            }
            Message::Finish {
                part,
                ms,
                peak,
                base,
                error,
            } => {
                ctx.current = None;
                let mut run = classify(&error);
                run.time_ms = ms;
                run.peak_bytes = peak;
                run.extra_bytes = peak.saturating_sub(base);
                self.finished(part, run, on_event);
            }
            Message::Comparison(report) => self.comparison = report,
            Message::Placements(report) => self.placement_checks = report,
            Message::Geometry(report) => self.geometry_checks = report,
            Message::Done => ctx.done = true,
        }
        Ok(())
    }

    fn finished(&mut self, part: usize, run: PartRun, on_event: &(dyn Fn(Event) + Sync)) {
        let (outcome, ms) = (run.outcome.id(), run.time_ms);
        self.results.insert(part, run);
        on_event(Event::Part {
            done: self.results.len(),
            total: self.listed.len(),
            number: part,
            outcome,
            ms,
        });
        let _ = self.save("running", false);
    }

    fn save_key(&self) -> Result<(), String> {
        let key = json!({
            "schema": LOCAL_SCHEMA,
            "warning": "Do not share this file. It names the parts of your design.",
            "input": self.input,
            "parts": self.listed.iter().map(|l| json!({"part": l.number, "name": l.name})).collect::<Vec<_>>(),
        });
        write_file(
            &self.out.join("local-key.json"),
            &serde_json::to_vec_pretty(&key).map_err(|e| e.to_string())?,
            true,
        )
    }

    /// Writes the report files; when `force` is not set, at most four times a second.
    fn save(&mut self, status: &str, force: bool) -> Result<(), String> {
        if !force
            && self
                .written
                .is_some_and(|at| at.elapsed() < Duration::from_millis(250))
        {
            return Ok(());
        }
        self.written = Some(Instant::now());
        let report = self.report(status);
        write_file(
            &self.out.join("report.json"),
            &serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
            false,
        )?;
        write_file(
            &self.out.join("report.txt"),
            text(&report).as_bytes(),
            false,
        )
    }

    /// The report so far.
    fn report(&self, status: &str) -> Value {
        let mut rows: Vec<(&Listed, PartRun)> = self
            .listed
            .iter()
            .map(|listed| {
                let run = self
                    .results
                    .get(&listed.number)
                    .cloned()
                    .unwrap_or_else(|| PartRun::simple(Outcome::NotRun));
                (listed, run)
            })
            .collect();
        rows.sort_by(|(a, x), (b, y)| {
            x.outcome
                .order()
                .cmp(&y.outcome.order())
                .then(y.time_ms.cmp(&x.time_ms))
                .then(b.entities.cmp(&a.entities))
                .then(a.number.cmp(&b.number))
        });
        let count = |outcome: Outcome| {
            rows.iter()
                .filter(|(_, run)| run.outcome == outcome)
                .count()
        };
        let finished = self.results.len();
        let parts: Vec<Value> = rows
            .iter()
            .enumerate()
            .map(|(rank, (listed, run))| {
                json!({
                    "rank": rank + 1, "part": listed.number, "parent": listed.parent,
                    "kind": if listed.assembly { "assembly" } else { "part" },
                    "depth": listed.depth,
                    "outcome": run.outcome.id(), "stage": run.stage, "error_kind": run.kind,
                    "findings": run.findings, "curve_kinds": run.curve_kinds,
                    "time_ms": run.time_ms, "peak_memory_mb": run.peak_bytes >> 20,
                    "extra_memory_mb": run.extra_bytes >> 20,
                    "bodies": listed.bodies, "entities": listed.entities,
                })
            })
            .collect();
        let size_class = std::fs::metadata(&self.input).ok().map(|m| match m.len() {
            0..=1_048_575 => "under_1_mib",
            1_048_576..=16_777_215 => "1_to_16_mib",
            16_777_216..=268_435_455 => "16_to_256_mib",
            _ => "256_mib_or_more",
        });
        let mut report = json!({
            "schema": SCHEMA,
            "versions": {"dutchman": env!("CARGO_PKG_VERSION"), "cadmpeg": crate::anon::CADMPEG_VERSION},
            "input": {"format": Format::from_path(&self.input).map(|f| f.id()), "size_class": size_class},
            "status": status,
            "limits": {"time_limit_s": self.settings.time_limit.as_secs_f64(),
                       "memory_limit_mb": self.settings.memory_limit.map(|b| b >> 20)},
            "read": self.read.as_ref().map(|run| json!({
                "outcome": run.outcome.id(), "stage": run.stage, "error_kind": run.kind,
                "findings": run.findings, "time_ms": run.time_ms,
                "peak_memory_mb": run.peak_bytes >> 20,
            })).unwrap_or_else(|| json!({
                "outcome": if self.listed.is_empty() { Value::Null } else { json!("ok") },
                "time_ms": self.read_ms, "peak_memory_mb": self.read_peak >> 20,
            })),
            "summary": {
                "parts": self.listed.len(), "finished": finished,
                "ok": count(Outcome::Ok), "failed": count(Outcome::Failed),
                "timed_out": count(Outcome::TimedOut), "out_of_memory": count(Outcome::OutOfMemory),
                "container": count(Outcome::Container), "not_run": count(Outcome::NotRun),
                "restarts": self.restarts,
                "elapsed_ms": u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
            },
            "parts": parts,
            "target_part": self.settings.target_part,
            "step_comparison": self.comparison,
            "placement_checks": self.placement_checks,
            "geometry_checks": self.geometry_checks,
        });
        let advice = advice(&report);
        if let Some(fields) = report.as_object_mut() {
            fields.insert("advice".into(), Value::Array(advice));
        }
        report
    }
}

/// The `error` the worker sent for a part, as how the part ended.
fn classify(error: &Value) -> PartRun {
    if error.is_null() {
        return PartRun::simple(Outcome::Ok);
    }
    if error.get("container") == Some(&json!(true)) {
        return PartRun::simple(Outcome::Container);
    }
    if error.get("panic") == Some(&json!(true)) {
        let mut run = PartRun::simple(Outcome::Failed);
        run.stage = Some("map".to_owned());
        run.kind = Some("panic".to_owned());
        return run;
    }
    let facts = diagnostics::anonymous_failure(error);
    let mut run = PartRun::simple(Outcome::Failed);
    run.stage = facts["stage"].as_str().map(str::to_owned);
    run.kind = facts["kind"].as_str().map(str::to_owned);
    run.findings = facts["findings"].as_object().cloned().unwrap_or_default();
    run.curve_kinds = facts["curve_kinds"]
        .as_object()
        .cloned()
        .unwrap_or_default();
    run
}

#[cfg(unix)]
fn killed_by_system(status: Option<std::process::ExitStatus>) -> bool {
    use std::os::unix::process::ExitStatusExt;
    // SIGKILL nobody here sent: the system's out-of-memory killer.
    status.is_some_and(|status| status.signal() == Some(9))
}
#[cfg(not(unix))]
fn killed_by_system(_status: Option<std::process::ExitStatus>) -> bool {
    false
}

pub(crate) fn write_file(path: &Path, bytes: &[u8], private: bool) -> Result<(), String> {
    use std::io::Write;
    let temporary = path.with_extension("tmp");
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    #[cfg(not(unix))]
    let _ = private;
    let mut file = options
        .open(&temporary)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    drop(file);
    std::fs::rename(&temporary, path).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

/// What to do next, as fixed codes with the part number they are about.
fn advice(report: &Value) -> Vec<Value> {
    let mut out = Vec::new();
    if report.pointer("/read/outcome") == Some(&json!("failed")) {
        out.push(json!({"code": "read_failed"}));
        return out;
    }
    if let Some(part) = failing_part_in(report) {
        out.push(json!({"code": "convert_part", "part": part}));
        out.push(json!({"code": "repro", "part": part}));
    } else if report["summary"]["parts"].as_u64().unwrap_or(0) > 0
        && report["summary"]["not_run"].as_u64() == Some(0)
        && report["status"] == "complete"
    {
        out.push(json!({"code": "every_part_converts"}));
    }
    out
}

/// The first failing part of a report: the one to look at first.
fn failing_part_in(report: &Value) -> Option<usize> {
    report["parts"]
        .as_array()?
        .iter()
        .find(|part| {
            matches!(
                part["outcome"].as_str(),
                Some("failed" | "timed_out" | "out_of_memory")
            )
        })
        .and_then(|part| part["part"].as_u64())
        .and_then(|part| usize::try_from(part).ok())
}

/// A written report, read back: `folder` is the `--out` folder a run used, or its `diagnose`
/// folder, or the report file itself.
pub fn report_from(folder: &Path) -> Result<Value, String> {
    let candidates = [
        folder.to_path_buf(),
        folder.join("report.json"),
        folder.join(FOLDER).join("report.json"),
    ];
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let report: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not a diagnose report: {e}", path.display()))?;
        if report["schema"] == SCHEMA {
            return Ok(report);
        }
    }
    Err(format!(
        "no diagnose report in {}; run `dutchman diagnose` first",
        folder.display()
    ))
}

/// Record a part chosen during local review. This does not claim that conversion failed.
/// `folder` is the diagnose folder containing report.json.
pub fn select_target(folder: &Path, number: usize) -> Result<Value, String> {
    let mut report = report_from(folder)?;
    let exists = report
        .get("parts")
        .and_then(Value::as_array)
        .is_some_and(|parts| {
            parts
                .iter()
                .any(|p| p.get("part").and_then(Value::as_u64) == Some(number as u64))
        });
    if !exists {
        return Err("Choose a part number listed in this report.".into());
    }
    if let Some(fields) = report.as_object_mut() {
        fields.insert("target_part".into(), json!(number));
    }
    write_file(
        &folder.join("report.json"),
        &serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?,
        false,
    )?;
    write_file(&folder.join("report.txt"), text(&report).as_bytes(), false)?;
    Ok(report)
}

/// The part to look at first in a written report (see [`report_from`]), with how many parts
/// there are and how it ended: `None` when no part failed.
pub fn failing_part(report: &Value) -> Option<FailingPart> {
    let number = failing_part_in(report)?;
    let row = report["parts"]
        .as_array()?
        .iter()
        .find(|part| part["part"].as_u64() == Some(number as u64))?;
    Some(FailingPart {
        number,
        total: report["summary"]["parts"].as_u64().unwrap_or(0) as usize,
        outcome: row["outcome"].as_str().unwrap_or("failed").to_owned(),
    })
}

/// The part a diagnose report ranks first among those that failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailingPart {
    /// Its number.
    pub number: usize,
    /// How many parts the design has.
    pub total: usize,
    /// `failed`, `timed_out` or `out_of_memory`.
    pub outcome: String,
}

fn seconds(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms} ms")
    } else if ms < 60_000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{} min {} s", ms / 60_000, ms / 1000 % 60)
    }
}

fn megabytes(mb: u64) -> String {
    if mb >= 1024 {
        format!("{:.1} GB", mb as f64 / 1024.0)
    } else {
        format!("{mb} MB")
    }
}

/// `report.txt`: the ranked summary, with part numbers only.
pub fn text(report: &Value) -> String {
    let mut out = String::from(
        "Dutchman diagnose report. Safe to share: it has part numbers, outcomes and sizes, and no\n\
         names, paths or geometry. Do not share local-key.json or the design itself.\n\n",
    );
    if let Some(part) = report.get("target_part").and_then(Value::as_u64) {
        out.push_str(&format!("Report target: part {part}.\n"));
    }
    let summary = &report["summary"];
    let n = |field: &str| summary[field].as_u64().unwrap_or(0);
    out.push_str(&format!(
        "Status: {}\nParts: {} ({} ok, {} failed, {} timed out, {} out of memory, {} not run, {} only hold other parts)\n",
        report["status"].as_str().unwrap_or("running"),
        n("parts"),
        n("ok"),
        n("failed"),
        n("timed_out"),
        n("out_of_memory"),
        n("not_run"),
        n("container"),
    ));
    out.push_str(&format!(
        "Reading the design: {}, {}\n",
        report
            .pointer("/read/time_ms")
            .and_then(Value::as_u64)
            .map_or("not finished".to_owned(), seconds),
        megabytes(
            report
                .pointer("/read/peak_memory_mb")
                .and_then(Value::as_u64)
                .unwrap_or(0)
        )
    ));
    if let Some(kind) = report.pointer("/read/error_kind").and_then(Value::as_str) {
        out.push_str(&format!(
            "The design could not be read ({kind}, at the {} stage).\n",
            report
                .pointer("/read/stage")
                .and_then(Value::as_str)
                .unwrap_or("decode")
        ));
    }
    out.push_str(&format!(
        "Limits per part: {}, {}\n\n",
        report["limits"]["time_limit_s"]
            .as_f64()
            .map_or(String::new(), |s| seconds((s * 1000.0) as u64)),
        report["limits"]["memory_limit_mb"]
            .as_u64()
            .map_or("no memory limit".to_owned(), megabytes)
    ));
    out.push_str("Rank  Part  Outcome        Stage    Error        Time        Memory     Size\n");
    for part in report["parts"].as_array().into_iter().flatten() {
        let codes: Vec<&str> = part["findings"]
            .as_object()
            .into_iter()
            .flat_map(|m| m.keys().map(String::as_str))
            .collect();
        let error = match (part["error_kind"].as_str(), codes.is_empty()) {
            (Some(kind), true) => kind.to_owned(),
            (Some(kind), false) => format!("{kind}: {}", codes.join(", ")),
            (None, _) => "-".to_owned(),
        };
        out.push_str(&format!(
            "{:<5} {:<5} {:<14} {:<8} {:<12} {:<11} {:<10} {}{}\n",
            part["rank"].as_u64().unwrap_or(0),
            part["part"].as_u64().unwrap_or(0),
            part["outcome"].as_str().unwrap_or(""),
            part["stage"].as_str().unwrap_or("-"),
            error,
            seconds(part["time_ms"].as_u64().unwrap_or(0)),
            megabytes(part["peak_memory_mb"].as_u64().unwrap_or(0)),
            part["entities"].as_u64().unwrap_or(0),
            if part["kind"] == "assembly" {
                " (holds parts)"
            } else {
                ""
            },
        ));
    }
    out.push('\n');
    for item in report["advice"].as_array().into_iter().flatten() {
        let part = item["part"].as_u64().unwrap_or(0);
        match item["code"].as_str() {
            Some("convert_part") => out.push_str(&format!(
                "Next: dutchman convert <your file> --part {part}   (converts only part {part})\n"
            )),
            Some("repro") => out.push_str(&format!(
                "Then: dutchman repro <your file> --out <this folder>   (narrows part {part} to the feature to rebuild)\n"
            )),
            Some("every_part_converts") => out.push_str(
                "Every part converted on its own. If the whole file still fails or runs out of\nmemory, the cause is in combining the parts or in their total size.\n",
            ),
            Some("read_failed") => out.push_str(
                "The design could not be read, so no part was tried. If it ran out of time or memory, try\na machine with more memory or a longer --time-limit.\n",
            ),
            _ => {}
        }
    }
    if let Some(comparison) = report.get("step_comparison").filter(|v| !v.is_null()) {
        out.push_str(&format!(
            "\nMatching STEP: {} (advisory; confirm findings).\n",
            comparison
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unavailable")
        ));
        for row in comparison
            .get("parts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            out.push_str(&format!(
                "Body {}: {}\n",
                row.get("body").unwrap_or(&Value::Null),
                row.get("findings").unwrap_or(&Value::Null)
            ));
        }
    }
    out
}

fn summary_text(report: &Value, failing: Option<usize>) -> String {
    let s = &report["summary"];
    let n = |field: &str| s[field].as_u64().unwrap_or(0);
    let mut text = format!(
        "{} parts: {} ok, {} failed, {} timed out, {} out of memory.",
        n("parts"),
        n("ok"),
        n("failed"),
        n("timed_out"),
        n("out_of_memory")
    );
    if report["status"] == "read_failed" {
        text = "The design could not be read, so no part was tried.".to_owned();
    }
    if let Some(part) = failing {
        text.push_str(&format!(
            " Look at part {part} first: dutchman convert <file> --part {part}."
        ));
    }
    text.push_str(
        " Safe to share: report.json and report.txt (part numbers only). Never share \
         local-key.json or the design.",
    );
    text
}

// ---- the child ----

/// The child's arguments after [`WORKER_ARG`].
#[derive(Parser)]
#[command(name = "dutchman-diagnose-worker")]
struct WorkerArgs {
    input: PathBuf,
    /// Parts already finished or given up, by number.
    #[arg(long, value_delimiter = ',')]
    skip: Vec<usize>,
    /// Once the design is read, take trials on standard input instead of converting each part
    /// (see [`narrow`]).
    #[arg(long)]
    serve: bool,
    #[arg(long)]
    step: Option<PathBuf>,
    #[arg(long)]
    converted: Option<PathBuf>,
    #[arg(long)]
    preview: Option<PathBuf>,
    #[command(flatten)]
    options: ConvertOptions,
}

/// Does a diagnose run's child's work (see [`WORKER_ARG`]) and returns its exit code.
pub fn run_worker(
    program: std::ffi::OsString,
    args: impl Iterator<Item = std::ffi::OsString>,
) -> ExitCode {
    let parsed = match WorkerArgs::try_parse_from(std::iter::once(program).chain(args)) {
        Ok(parsed) => parsed,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::from(2);
        }
    };
    work(&parsed)
}

fn emit(value: &Value) {
    eprintln!("{value}");
}

fn work(args: &WorkerArgs) -> ExitCode {
    let mut options = args.options.clone();
    options.no_verify = true;
    options.solidcraft = None;
    let started = Instant::now();
    let sampler = Sampler::start();
    let mut report = Report::default();
    let read = Format::from_path(&args.input)
        .ok_or_else(|| dutchman_core::Error::UnknownFormat {
            path: args.input.clone(),
        })
        .and_then(|format| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                dutchman_core::read_in(format, &args.input, &options.part_search(), &mut report)
            }))
            .unwrap_or_else(|_| {
                Err(dutchman_core::Error::Decode(
                    "the converter stopped unexpectedly while reading".into(),
                ))
            })
        });
    let read_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let model = match read {
        Ok(model) => model,
        Err(err) => {
            emit(&json!({"dutchman_diagnose": "read_failed", "ms": read_ms,
                "error": wire::error(wire::CONVERT, Value::Null, &err)}));
            return ExitCode::SUCCESS;
        }
    };
    emit(
        &json!({"dutchman_diagnose": "placements", "report": checks::placements(&model, args.options.choices().joints)}),
    );
    if let Some(step) = &args.step {
        let comparison = step::compare(
            &model,
            step,
            &args.options,
            Duration::from_secs(args.options.verify_timeout),
            args.preview.as_ref().map(|p| p.join("step")).as_deref(),
        );
        emit(&json!({"dutchman_diagnose": "comparison", "report": comparison}));
    }
    if let Some(converted) = &args.converted {
        let comparison = std::fs::metadata(converted).ok().filter(|m| m.len() <= (512 << 20))
            .and_then(|_| std::fs::read(converted).ok()).map(|bytes| {
                dutchman_solidcraft::compare::stored_output(&model, &bytes, args.options.choices().joints,
                    &args.options.solidcraft.clone().map_or(dutchman_solidcraft::Verification::Auto, dutchman_solidcraft::Verification::With),
                    Duration::from_secs(args.options.verify_timeout),
                    &args.preview.clone().unwrap_or_else(|| converted.with_extension("preview")).join("geometry"))
            }).unwrap_or_else(|| json!({"status": "unavailable", "parts": [], "reason": "The converted design could not be staged within the size limit."}));
        emit(&json!({"dutchman_diagnose": "geometry", "report": comparison}));
    }
    let catalog = parts::catalog(&model);
    emit(&json!({
        "dutchman_diagnose": "catalog", "read_ms": read_ms, "peak": sampler.take(),
        "parts": catalog.iter().map(|p| json!({
            "number": p.number, "parent": p.parent, "depth": p.depth, "assembly": p.assembly,
            "bodies": p.bodies, "entities": p.entities, "name": p.name,
        })).collect::<Vec<_>>(),
    }));
    let writer = options.writer();
    if args.serve {
        return narrow::serve(&model, &writer);
    }
    for part in &catalog {
        if args.skip.contains(&part.number) {
            continue;
        }
        emit(&json!({"dutchman_diagnose": "start", "part": part.number}));
        let base = worker::resident_bytes(std::process::id()).unwrap_or(0);
        sampler.take();
        let begun = Instant::now();
        let error = convert_part(&model, part.number, &writer);
        let ms = u64::try_from(begun.elapsed().as_millis()).unwrap_or(u64::MAX);
        emit(
            &json!({"dutchman_diagnose": "finish", "part": part.number, "ms": ms,
            "peak": sampler.take().max(base), "base": base, "error": error}),
        );
    }
    emit(&json!({"dutchman_diagnose": "done"}));
    ExitCode::SUCCESS
}

/// Converts one part into nothing, and says how it went: null when it converted, the error
/// report when it did not, `{"container": true}` for a sub-assembly with nothing of its own.
fn convert_part(model: &dutchman_core::Model, number: usize, writer: &dyn Writer) -> Value {
    #[cfg(debug_assertions)]
    test_fault(number);
    let part = match parts::select(model, number, false) {
        Ok(part) => part,
        Err(parts::SelectError::HoldsOnly { .. }) => return json!({"container": true}),
        Err(why) => {
            return wire::failure(wire::CONVERT, Value::Null, &why.to_string());
        }
    };
    if parts::entity_count(&part) == 0 {
        // A component with no geometry: the conversion skips it, so there is nothing to try.
        return json!({"container": true});
    }
    write_model(&part, number, writer)
}

/// Writes a model into nothing, and says how it went, as [`convert_part`] does.
fn write_model(part: &dutchman_core::Model, number: usize, writer: &dyn Writer) -> Value {
    #[cfg(debug_assertions)]
    if test_feature_fault(number, parts::history_length(part)) {
        return wire::error(
            wire::CONVERT,
            Value::Null,
            &dutchman_core::Error::Restate("injected failure".into()),
        );
    }
    #[cfg(not(debug_assertions))]
    let _ = number;
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        writer.write(part, &mut std::io::sink(), &mut Report::default())
    }));
    match result {
        Ok(Ok(())) => Value::Null,
        Ok(Err(err)) => wire::error(wire::CONVERT, Value::Null, &err),
        Err(_) => json!({"panic": true}),
    }
}

/// Tests make a part fail once its history reaches a feature: the value
/// `<part>:feature:<n>:<fail, crash or hang>`. True to fail; release builds have no such switch.
#[cfg(debug_assertions)]
fn test_feature_fault(number: usize, kept: usize) -> bool {
    let Ok(value) = std::env::var("DUTCHMAN_DIAGNOSE_TEST_FAULT") else {
        return false;
    };
    let mut fields = value.split(':');
    let (part, word, feature, fault) = (fields.next(), fields.next(), fields.next(), fields.next());
    if part.and_then(|p| p.parse().ok()) != Some(number) || word != Some("feature") {
        return false;
    }
    if feature
        .and_then(|f| f.parse::<usize>().ok())
        .is_none_or(|feature| kept < feature)
    {
        return false;
    }
    match fault {
        Some("hang") => std::thread::sleep(Duration::from_secs(3600)),
        Some("crash") => std::process::abort(),
        _ => {}
    }
    true
}

/// Tests make one part hang or crash, to see the others carry on; release builds have
/// no such switch.
#[cfg(debug_assertions)]
fn test_fault_is(number: usize, fault: &str) -> bool {
    std::env::var("DUTCHMAN_DIAGNOSE_TEST_FAULT")
        .ok()
        .is_some_and(|value| value == format!("{number}:{fault}"))
}

#[cfg(debug_assertions)]
fn test_fault(number: usize) {
    if test_fault_is(number, "hang") {
        std::thread::sleep(Duration::from_secs(3600));
    }
    if test_fault_is(number, "crash") {
        std::process::abort();
    }
}

/// Peak memory of this process, sampled in the background.
struct Sampler {
    peak: std::sync::Arc<std::sync::atomic::AtomicU64>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Sampler {
    fn start() -> Self {
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::Arc;
        let peak = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (p, s) = (Arc::clone(&peak), Arc::clone(&stop));
        let _ = std::thread::Builder::new().spawn(move || {
            while !s.load(Ordering::Relaxed) {
                if let Some(held) = worker::resident_bytes(std::process::id()) {
                    p.fetch_max(held, Ordering::Relaxed);
                }
                std::thread::sleep(SAMPLE);
            }
        });
        Sampler { peak, stop }
    }

    /// The highest memory seen since the last call, which starts the next measure from what is
    /// held now.
    fn take(&self) -> u64 {
        use std::sync::atomic::Ordering;
        let now = worker::resident_bytes(std::process::id()).unwrap_or(0);
        self.peak.swap(now, Ordering::Relaxed).max(now)
    }
}

impl Drop for Sampler {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// A time limit as people type it: seconds (`90`, `0.5`), or with `s`, `m` or `h`.
pub fn parse_duration(text: &str) -> Result<Duration, String> {
    let text = text.trim();
    let (number, scale) = match text.chars().last() {
        Some('s') => (&text[..text.len() - 1], 1.0),
        Some('m') => (&text[..text.len() - 1], 60.0),
        Some('h') => (&text[..text.len() - 1], 3600.0),
        _ => (text, 1.0),
    };
    let value: f64 = number
        .trim()
        .parse()
        .map_err(|_| format!("`{text}` is not a time: use seconds, or a number with s, m or h"))?;
    if !value.is_finite() || value <= 0.0 {
        return Err(format!("`{text}` is not a time above zero"));
    }
    Duration::try_from_secs_f64(value * scale)
        .map_err(|_| "the time limit is too large; use a smaller number of seconds".to_owned())
}

#[cfg(test)]
mod tests;
