//! `dutchman sync`: keep converted designs up to date with their source files.
//!
//! Every conversion writes a link record beside its design (see [`crate::link`]). Sync finds the
//! records under a folder, compares each source's SHA-256 with the record, and converts again
//! only the sources that changed, with the options the record holds. The new design replaces the
//! old one atomically; a conversion that fails leaves the old design and record as they were.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use clap::Args;
use serde_json::Value;

use crate::{batch, link, watch, worker, ConvertOptions};

/// One line of sync output.
pub const SCHEMA: &str = "dutchman.sync-event/1";

/// Command-line settings for keeping a folder of designs up to date.
#[derive(Args)]
pub struct Command {
    /// Folder to search, with its subfolders, for link records (`<design>.link.json`).
    pub folder: PathBuf,
    /// Keep running: wait for a source to be saved and settle, then convert it again.
    #[arg(long)]
    pub watch: bool,
    /// With --watch: seconds between scans.
    #[arg(long, default_value = "5")]
    pub interval: std::num::NonZeroU64,
    /// With --watch: seconds a source's size and modification time must stay unchanged.
    #[arg(long, default_value_t = 10)]
    pub settle: u64,
    /// Seconds allowed per conversion.
    #[arg(long, default_value_t = worker::DEFAULT_TIMEOUT.as_secs())]
    pub timeout: u64,
    /// Megabytes allowed per conversion (0: unlimited).
    #[arg(long, default_value_t = worker::DEFAULT_MAX_MEMORY_MB)]
    pub max_memory: u64,
    /// Write history without checking it in SolidCraft.
    #[arg(long)]
    pub no_verify: bool,
    /// The SolidCraft CLI that checks a written history against the original geometry.
    #[arg(long, value_name = "PATH", conflicts_with = "no_verify")]
    pub solidcraft: Option<PathBuf>,
    /// Print one JSON event per line.
    #[arg(long)]
    pub json: bool,
}

/// How one design stands after a check.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Event {
    Unchanged,
    Updated,
    Failed,
    SourceMissing,
    /// The source changed and the design was not converted again (auto-update is off).
    Changed,
}

impl Event {
    /// The `event` of a JSON line, and the word that leads a text line.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Event::Unchanged => ("unchanged", "unchanged"),
            Event::Updated => ("updated", "updated"),
            Event::Failed => ("failed", "failed"),
            Event::SourceMissing => ("source_missing", "source missing"),
            Event::Changed => ("changed", "changed"),
        }
    }
}

struct Outcome {
    event: Event,
    message: String,
    report: Option<Value>,
}

impl Outcome {
    fn new(event: Event, message: impl Into<String>) -> Self {
        Self {
            event,
            message: message.into(),
            report: None,
        }
    }
}

/// A design whose record has been read.
struct Design {
    folder: PathBuf,
    record: Value,
    output: PathBuf,
    source: PathBuf,
}

/// What a check needs besides the design.
struct Context {
    program: PathBuf,
    limits: worker::Limits,
    no_verify: bool,
    solidcraft: Option<PathBuf>,
}

/// Runs once over the folder, or until a signal with `--watch`. `Ok(false)` when a design
/// failed to update.
pub fn run(command: &Command) -> Result<bool, String> {
    let folder = std::path::absolute(&command.folder).map_err(|e| e.to_string())?;
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", command.folder.display()));
    }
    let context = Context {
        program: batch::Settings::isolated()?
            .worker
            .ok_or("cannot find converter worker")?,
        limits: worker::Limits {
            timeout: Duration::from_secs(command.timeout.max(1)),
            max_memory: (command.max_memory > 0)
                .then_some(command.max_memory.saturating_mul(1 << 20)),
            design: Some(worker::DEFAULT_DESIGN_LIMIT),
        },
        no_verify: command.no_verify,
        solidcraft: command.solidcraft.clone(),
    };
    if command.watch {
        let stop = Arc::new(AtomicBool::new(false));
        let signal = Arc::clone(&stop);
        ctrlc::set_handler(move || signal.store(true, Ordering::Relaxed))
            .map_err(|e| format!("cannot listen for shutdown: {e}"))?;
        let mut printer = Printer {
            command,
            folder: &folder,
        };
        return watch_folder(
            &Timing {
                settle: Duration::from_secs(command.settle),
                interval: Duration::from_secs(command.interval.get()),
            },
            &folder,
            &context,
            &stop,
            &mut printer,
        );
    }
    let records = find_records(&folder);
    if records.is_empty() && !command.json {
        eprintln!("dutchman: no link records under {}", folder.display());
    }
    let mut ok = true;
    for record in records {
        let started = Instant::now();
        let outcome = check_record(&record, &context, None, true);
        ok &= outcome.event != Event::Failed;
        emit(command, &folder, &record, started, &outcome)?;
    }
    Ok(ok)
}

/// The link records under `folder`, in path order. Hidden folders (conversions in progress,
/// version control) and symbolic links are not followed.
fn find_records(folder: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![folder.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                if !name.starts_with('.') {
                    pending.push(path);
                }
            } else if kind.is_file() && name.ends_with(".link.json") && !name.starts_with('.') {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

fn load(record_path: &Path) -> Result<Design, String> {
    let bytes = std::fs::read(record_path).map_err(|e| format!("cannot read the record: {e}"))?;
    let record: Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("the record is not valid JSON: {e}"))?;
    if record
        .get("schema")
        .and_then(Value::as_str)
        .is_none_or(|schema| !schema.starts_with("dutchman.link/"))
    {
        return Err("not a dutchman link record".to_owned());
    }
    let folder = record_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or("the record has no folder")?;
    let file = record
        .pointer("/output/file")
        .and_then(Value::as_str)
        .filter(|file| {
            !file.is_empty() && Path::new(file).file_name() == Some(std::ffi::OsStr::new(file))
        })
        .ok_or("the record names no output file")?;
    let source = record
        .pointer("/source/path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
        .ok_or("the record names no source")?;
    let source = Path::new(source);
    let source = if source.is_absolute() {
        source.to_path_buf()
    } else {
        folder.join(source)
    };
    Ok(Design {
        output: folder.join(file),
        folder,
        record,
        source,
    })
}

/// Looks at one design and converts it again when its source changed.
fn check_record(
    record_path: &Path,
    context: &Context,
    stop: Option<&AtomicBool>,
    convert: bool,
) -> Outcome {
    let design = match load(record_path) {
        Ok(design) => design,
        Err(why) => return Outcome::new(Event::Failed, why),
    };
    let meta = match std::fs::metadata(&design.source) {
        Ok(meta) if meta.is_file() => meta,
        Ok(_) => return missing(&design),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return missing(&design),
        Err(e) => {
            return Outcome::new(
                Event::Failed,
                format!("cannot read {}: {e}", design.source.display()),
            )
        }
    };
    let has_output = design.output.is_file();
    if has_output && link::stamp_unchanged(&design.record, &meta) {
        return Outcome::new(Event::Unchanged, "");
    }
    let digest = match watch::hash(&design.source) {
        Ok(digest) => digest,
        Err(e) => {
            return Outcome::new(
                Event::Failed,
                format!("cannot read {}: {e}", design.source.display()),
            )
        }
    };
    let recorded = design
        .record
        .pointer("/source/sha256")
        .and_then(Value::as_str);
    if has_output && recorded == Some(digest.as_str()) {
        return Outcome::new(Event::Unchanged, "");
    }
    if !convert {
        return Outcome::new(Event::Changed, "");
    }
    let source = link::Source::with_metadata(digest, &meta);
    match reconvert(&design, &source, context, stop) {
        Ok(report) => Outcome {
            event: Event::Updated,
            message: String::new(),
            report: Some(report),
        },
        Err((message, report)) => Outcome {
            event: Event::Failed,
            message,
            report,
        },
    }
}

fn missing(design: &Design) -> Outcome {
    Outcome::new(
        Event::SourceMissing,
        format!("{} not found", design.source.display()),
    )
}

/// Converts the source again and replaces the design and its record. On failure the design and
/// record are as they were, and the message (with the conversion's report, when it made one)
/// says why.
fn reconvert(
    design: &Design,
    source: &link::Source,
    context: &Context,
    stop: Option<&AtomicBool>,
) -> Result<Value, (String, Option<Value>)> {
    let fail = |message: String| (message, None);
    let mut options = link::options_from_record(&design.record, &design.folder).map_err(fail)?;
    options.no_verify = context.no_verify;
    options.solidcraft.clone_from(&context.solidcraft);
    // The record names the published design, so it is written after publishing.
    options.no_link_record = true;
    let stage = tempfile::Builder::new()
        .prefix(".dutchman-sync-")
        .tempdir_in(&design.folder)
        .map_err(|e| fail(format!("cannot stage the conversion: {e}")))?;
    let staged = stage
        .path()
        .join("result")
        .with_extension(design.output.extension().unwrap_or_default());
    let mut job = batch::Job {
        project_files: None,
        fallback: None,
        input: design.source.clone(),
        output: design.output.clone(),
        replace: false,
        search: crate::PartSearch::default(),
    };
    batch::prepare(std::slice::from_mut(&mut job), &options);
    job.output.clone_from(&staged);
    job.replace = false;
    let report = match options.part {
        Some(part) => convert_part(context, &job, &options, part),
        None => {
            batch::convert_isolated_job(&context.program, &job, &options, &context.limits, stop)
        }
    };
    if report.get("ok").and_then(Value::as_bool) != Some(true) {
        let message = report
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("the conversion failed")
            .to_owned();
        return Err((message, Some(report)));
    }
    if !staged.is_file() {
        return Err(("the conversion wrote no design".to_owned(), Some(report)));
    }
    std::fs::rename(&staged, &design.output).map_err(|e| {
        (
            format!("cannot replace {}: {e}", design.output.display()),
            Some(report.clone()),
        )
    })?;
    let written = link::record(&design.source, &design.output, &options, &report, source)
        .and_then(|record| link::write_file(&link::path_for(&design.output), &record));
    match written {
        Ok(()) => Ok(report),
        Err(why) => Err((
            format!("converted, but the link record is not updated: {why}"),
            Some(report),
        )),
    }
}

/// One part of a design is converted by `dutchman convert --part`, in a child of its own.
fn convert_part(
    context: &Context,
    job: &batch::Job,
    options: &ConvertOptions,
    part: dutchman_core::PartSelection,
) -> Value {
    let failure = |message: &str| {
        crate::json::failure(
            crate::json::CONVERT,
            crate::json::input(&job.input),
            message,
        )
    };
    let mut command = std::process::Command::new(&context.program);
    command.args(["convert", "--json"]);
    command.args(options.to_args());
    command.args(worker::search_args(&job.search));
    command.args(["--part", &part.number.to_string()]);
    if part.children {
        command.arg("--with-children");
    }
    if let Some(feature) = part.stop_at_feature {
        command.args(["--stop-at-feature", &feature.to_string()]);
    }
    command.arg(&job.input).arg(&job.output);
    command.stdin(std::process::Stdio::null());
    match command.output() {
        Ok(output) => serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
            failure(&format!(
                "the conversion printed no report: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ))
        }),
        Err(e) => failure(&format!("cannot start the converter: {e}")),
    }
}

fn emit(
    command: &Command,
    folder: &Path,
    record: &Path,
    started: Instant,
    outcome: &Outcome,
) -> Result<(), String> {
    // The design (the record without its `.link.json`), as the folder given names it; a record
    // that cannot be read is named by itself.
    let path = load(record).map_or_else(|_| record.to_path_buf(), |design| design.output);
    let path = path.strip_prefix(folder).unwrap_or(&path);
    watch::write_event(
        SCHEMA,
        command.json,
        outcome.event.names(),
        path,
        started,
        &outcome.message,
        outcome.report.as_ref(),
    )
}

/// What a source (or record) looked like when last seen.
#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    size: u64,
    modified: SystemTime,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let meta = std::fs::metadata(path).ok().filter(|meta| meta.is_file())?;
    Some(Stamp {
        size: meta.len(),
        modified: meta.modified().ok()?,
    })
}

/// What the watch knows of one record.
#[derive(Default)]
struct Seen {
    /// The source as last observed, and since when it has been unchanged.
    source: Option<(Stamp, Instant)>,
    /// The source and record as they were when last checked.
    handled: Option<(Stamp, Option<Stamp>)>,
    /// The record as it was when it was last reported unreadable.
    broken: Option<Option<Stamp>>,
    /// Whether the source is reported missing.
    missing: bool,
    /// Whether a line has been written for this design.
    announced: bool,
    /// Whether the source changed and was left alone because auto-update was off.
    held: bool,
}

/// How often the watch scans and how long a source must rest before it counts as saved.
struct Timing {
    settle: Duration,
    interval: Duration,
}

/// What the watch loop tells its caller, and asks of it.
trait Sink {
    /// Whether a changed source is converted again; read at every check, so it can change
    /// while the watch runs. When it is off, a change is only reported (`Event::Changed`) and
    /// converted once this turns on.
    fn auto_update(&self) -> bool {
        true
    }
    /// One design's outcome.
    fn event(&mut self, record: &Path, started: Instant, outcome: &Outcome) -> Result<(), String>;
    /// A scan of the folder ended.
    fn scanned(&mut self) {}
}

/// Prints each event, as `dutchman sync --watch` does.
struct Printer<'a> {
    command: &'a Command,
    folder: &'a Path,
}

impl Sink for Printer<'_> {
    fn event(&mut self, record: &Path, started: Instant, outcome: &Outcome) -> Result<(), String> {
        emit(self.command, self.folder, record, started, outcome)
    }
}

fn watch_folder(
    timing: &Timing,
    folder: &Path,
    context: &Context,
    stop: &AtomicBool,
    sink: &mut dyn Sink,
) -> Result<bool, String> {
    let settle = timing.settle;
    let interval = timing.interval;
    let mut states: BTreeMap<PathBuf, Seen> = BTreeMap::new();
    let mut next_scan = Instant::now();
    let mut first_scan = true;
    let mut ok = true;
    while !stop.load(Ordering::Relaxed) {
        if Instant::now() < next_scan {
            std::thread::sleep(Duration::from_millis(25));
            continue;
        }
        let records = find_records(folder);
        states.retain(|path, _| records.contains(path));
        for record in records {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            let state = states.entry(record.clone()).or_default();
            let record_stamp = stamp(&record);
            let design = match load(&record) {
                Ok(design) => design,
                Err(why) => {
                    if state.broken.as_ref() != Some(&record_stamp) {
                        state.broken = Some(record_stamp);
                        sink.event(&record, Instant::now(), &Outcome::new(Event::Failed, why))?;
                        ok = false;
                    }
                    continue;
                }
            };
            state.broken = None;
            let Some(current) = stamp(&design.source) else {
                state.source = None;
                state.handled = None;
                if !state.missing {
                    state.missing = true;
                    sink.event(&record, Instant::now(), &missing(&design))?;
                }
                continue;
            };
            state.missing = false;
            let converting = sink.auto_update();
            if state.handled.as_ref() == Some(&(current.clone(), record_stamp.clone()))
                && !(state.held && converting)
            {
                continue;
            }
            let observation = state.source.get_or_insert_with(|| {
                // Copies can keep their source's modification time, so a watch waits a whole
                // settle period from first seeing a file; at startup a file's own age counts.
                let since = if first_scan {
                    let age = SystemTime::now()
                        .duration_since(current.modified)
                        .unwrap_or_default();
                    Instant::now()
                        .checked_sub(age.min(settle))
                        .unwrap_or_else(Instant::now)
                } else {
                    Instant::now()
                };
                (current.clone(), since)
            });
            if observation.0 != current {
                *observation = (current.clone(), Instant::now());
                state.handled = None;
                state.held = false;
            }
            let age = SystemTime::now()
                .duration_since(current.modified)
                .unwrap_or_default();
            if current.modified > SystemTime::now()
                || observation.1.elapsed() < settle
                || age < settle
            {
                continue;
            }
            let started = Instant::now();
            let outcome = check_record(&record, context, Some(stop), converting);
            state.held = outcome.event == Event::Changed;
            // Remember the source as it was before converting: a save during the conversion
            // makes it differ, and so settles and converts again.
            state.handled = Some((current, stamp(&record)));
            ok &= outcome.event != Event::Failed;
            if outcome.event != Event::Unchanged || !state.announced {
                sink.event(&record, started, &outcome)?;
            }
            state.announced = true;
        }
        sink.scanned();
        first_scan = false;
        next_scan = Instant::now()
            .checked_add(interval)
            .ok_or("polling interval is too large; choose fewer seconds")?;
    }
    Ok(ok)
}

/// How a design stands, as [`watch`] reports it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// The design matches its source.
    Unchanged,
    /// The source changed and the design was converted again.
    Updated,
    /// The design could not be updated; the previous design is kept.
    Failed,
    /// The source file is gone.
    SourceMissing,
    /// The source changed and the design was left alone, as auto-update is off.
    Changed,
}

/// One design's outcome, from [`watch`].
#[derive(Clone, Debug)]
pub struct Observed {
    /// The design (`.sc`) the link record belongs to; the record path when it cannot be read.
    pub design: PathBuf,
    pub status: Status,
    /// Why, for a failure or a missing source; empty otherwise.
    pub message: String,
    /// The conversion's report, when one was made.
    pub report: Option<Value>,
}

/// What [`watch`] asks of its caller, and tells it.
pub trait Observer {
    /// Whether a changed source is converted again. Read at every check, so it can change
    /// while the watch runs: off reports `Status::Changed` and leaves the design as it is.
    fn auto_update(&self) -> bool {
        true
    }
    /// A design's outcome. An unchanged design is reported once, then only when it changes.
    fn event(&mut self, observed: Observed);
    /// A scan of the folder ended.
    fn scanned(&mut self) {}
}

/// Settings for [`watch`], for a program other than the command line.
#[derive(Clone, Debug)]
pub struct WatchSettings {
    /// The program that converts in a child process: `dutchman` itself, or a program that
    /// calls `dutchman::worker::run_if_worker` first.
    pub program: PathBuf,
    /// Time between scans.
    pub interval: Duration,
    /// How long a source's size and modification time must stay unchanged.
    pub settle: Duration,
    /// Time allowed per conversion.
    pub timeout: Duration,
    /// Megabytes allowed per conversion (0: unlimited).
    pub max_memory_mb: u64,
    /// Write history without checking it in SolidCraft.
    pub no_verify: bool,
    /// The SolidCraft CLI that checks a written history.
    pub solidcraft: Option<PathBuf>,
}

impl WatchSettings {
    /// The command line's defaults, converting with `program`.
    pub fn new(program: PathBuf) -> Self {
        Self {
            program,
            interval: Duration::from_secs(5),
            settle: Duration::from_secs(10),
            timeout: worker::DEFAULT_TIMEOUT,
            max_memory_mb: worker::DEFAULT_MAX_MEMORY_MB,
            no_verify: false,
            solidcraft: None,
        }
    }
}

struct Forward<'a>(&'a mut dyn Observer);

impl Sink for Forward<'_> {
    fn auto_update(&self) -> bool {
        self.0.auto_update()
    }
    fn event(&mut self, record: &Path, _started: Instant, outcome: &Outcome) -> Result<(), String> {
        let design = load(record).map_or_else(|_| record.to_path_buf(), |design| design.output);
        self.0.event(Observed {
            design,
            status: match outcome.event {
                Event::Unchanged => Status::Unchanged,
                Event::Updated => Status::Updated,
                Event::Failed => Status::Failed,
                Event::SourceMissing => Status::SourceMissing,
                Event::Changed => Status::Changed,
            },
            message: outcome.message.clone(),
            report: outcome.report.clone(),
        });
        Ok(())
    }
    fn scanned(&mut self) {
        self.0.scanned();
    }
}

/// Watches `folder` as `dutchman sync --watch` does, until `stop` is set, reporting to
/// `observer` instead of printing. `Err` when the folder is not one or the interval cannot
/// be waited.
pub fn watch(
    folder: &Path,
    settings: &WatchSettings,
    stop: &AtomicBool,
    observer: &mut dyn Observer,
) -> Result<(), String> {
    let folder = std::path::absolute(folder).map_err(|e| e.to_string())?;
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", folder.display()));
    }
    let context = Context {
        program: settings.program.clone(),
        limits: worker::Limits {
            timeout: settings.timeout.max(Duration::from_secs(1)),
            max_memory: (settings.max_memory_mb > 0)
                .then_some(settings.max_memory_mb.saturating_mul(1 << 20)),
            design: Some(worker::DEFAULT_DESIGN_LIMIT),
        },
        no_verify: settings.no_verify,
        solidcraft: settings.solidcraft.clone(),
    };
    let timing = Timing {
        settle: settings.settle,
        interval: settings.interval.max(Duration::from_millis(100)),
    };
    watch_folder(&timing, &folder, &context, stop, &mut Forward(observer)).map(|_| ())
}
