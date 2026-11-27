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
}

impl Event {
    /// The `event` of a JSON line, and the word that leads a text line.
    fn names(self) -> (&'static str, &'static str) {
        match self {
            Event::Unchanged => ("unchanged", "unchanged"),
            Event::Updated => ("updated", "updated"),
            Event::Failed => ("failed", "failed"),
            Event::SourceMissing => ("source_missing", "source missing"),
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

/// What converting a design again needs besides the design.
pub struct Context {
    /// The program each conversion runs in, as a child within `limits`; `None` converts in
    /// this process, without limits.
    pub program: Option<PathBuf>,
    pub limits: worker::Limits,
    /// Write history without checking it in SolidCraft.
    pub no_verify: bool,
    /// The SolidCraft CLI that checks a written history.
    pub solidcraft: Option<PathBuf>,
}

/// Runs once over the folder, or until a signal with `--watch`. `Ok(false)` when a design
/// failed to update.
pub fn run(command: &Command) -> Result<bool, String> {
    let folder = std::path::absolute(&command.folder).map_err(|e| e.to_string())?;
    if !folder.is_dir() {
        return Err(format!("{} is not a folder", command.folder.display()));
    }
    let context = Context {
        program: Some(
            batch::Settings::isolated()?
                .worker
                .ok_or("cannot find converter worker")?,
        ),
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
        return watch_folder(command, &folder, &context, &stop);
    }
    let records = find_records(&folder);
    if records.is_empty() && !command.json {
        eprintln!("dutchman: no link records under {}", folder.display());
    }
    let mut ok = true;
    for record in records {
        let started = Instant::now();
        let outcome = check_record(&record, &context, None);
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

/// How a design's source stands against its record.
enum Standing {
    /// The source has the recorded bytes.
    Current,
    /// The source differs; what it is now.
    Changed(link::Source),
    Missing,
    /// The source could not be read.
    Unreadable(String),
}

/// Compares a design's source with its record, converting nothing.
fn inspect(design: &Design) -> Standing {
    let meta = match std::fs::metadata(&design.source) {
        Ok(meta) if meta.is_file() => meta,
        Ok(_) => return Standing::Missing,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Standing::Missing,
        Err(e) => {
            return Standing::Unreadable(format!("cannot read {}: {e}", design.source.display()));
        }
    };
    let has_output = design.output.is_file();
    if has_output && link::stamp_unchanged(&design.record, &meta) {
        return Standing::Current;
    }
    let digest = match watch::hash(&design.source) {
        Ok(digest) => digest,
        Err(e) => {
            return Standing::Unreadable(format!("cannot read {}: {e}", design.source.display()));
        }
    };
    let recorded = design
        .record
        .pointer("/source/sha256")
        .and_then(Value::as_str);
    if has_output && recorded == Some(digest.as_str()) {
        return Standing::Current;
    }
    Standing::Changed(link::Source::with_metadata(digest, &meta))
}

/// Looks at one design and converts it again when its source changed.
fn check_record(record_path: &Path, context: &Context, stop: Option<&AtomicBool>) -> Outcome {
    let design = match load(record_path) {
        Ok(design) => design,
        Err(why) => return Outcome::new(Event::Failed, why),
    };
    match inspect(&design) {
        Standing::Current => Outcome::new(Event::Unchanged, ""),
        Standing::Missing => missing(&design),
        Standing::Unreadable(why) => Outcome::new(Event::Failed, why),
        Standing::Changed(source) => match reconvert(&design, &source, context, stop, &|_| {}) {
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
        },
    }
}

/// How a design found by [`scan`] stands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// The source has the recorded bytes.
    Current,
    /// The source changed since the design was converted.
    Changed,
    /// The source file is not there.
    Missing,
    /// The record or the source could not be read, and why.
    Unreadable(String),
}

/// One design with a link record.
#[derive(Clone, Debug)]
pub struct Entry {
    /// The link record (`<design>.link.json`).
    pub record: PathBuf,
    /// The converted design, when the record names it; else the record.
    pub output: PathBuf,
    /// The source file the design was converted from; empty when the record is unreadable.
    pub source: PathBuf,
    /// The program the source comes from (Fusion, Inventor, ...); empty when not known.
    pub program: String,
    pub status: Status,
}

/// Looks at every design with a link record under `folder`, in path order, and says how it
/// stands. Converts nothing and writes nothing.
pub fn scan(folder: &Path) -> Vec<Entry> {
    find_records(folder)
        .into_iter()
        .map(|record| match load(&record) {
            Err(why) => Entry {
                output: record.clone(),
                record,
                source: PathBuf::new(),
                program: String::new(),
                status: Status::Unreadable(why),
            },
            Ok(design) => {
                let status = match inspect(&design) {
                    Standing::Current => Status::Current,
                    Standing::Changed(_) => Status::Changed,
                    Standing::Missing => Status::Missing,
                    Standing::Unreadable(why) => Status::Unreadable(why),
                };
                Entry {
                    program: dutchman_core::Format::from_path(&design.source)
                        .map(|format| format.family().to_owned())
                        .unwrap_or_default(),
                    output: design.output,
                    source: design.source,
                    record,
                    status,
                }
            }
        })
        .collect()
}

/// Converts the source of `entry` again and replaces its design and record, with the options the
/// record holds. Returns the conversion's `dutchman.convert/1` report, naming the design. On
/// failure the design and record are as they were; the message says why, with the conversion's
/// report when it made one.
pub fn update(
    entry: &Entry,
    context: &Context,
    stop: Option<&AtomicBool>,
    on_progress: &(dyn Fn(dutchman_core::ProgressEvent) + Send + Sync),
) -> Result<Value, (String, Option<Value>)> {
    let design = load(&entry.record).map_err(|why| (why, None))?;
    let meta = std::fs::metadata(&design.source)
        .ok()
        .filter(std::fs::Metadata::is_file)
        .ok_or_else(|| (format!("{} not found", design.source.display()), None))?;
    let digest = watch::hash(&design.source).map_err(|e| {
        (
            format!("cannot read {}: {e}", design.source.display()),
            None,
        )
    })?;
    let source = link::Source::with_metadata(digest, &meta);
    reconvert(&design, &source, context, stop, on_progress)
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
    on_progress: &(dyn Fn(dutchman_core::ProgressEvent) + Send + Sync),
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
    let report = match (&context.program, options.part) {
        (None, _) => crate::convert_options_with_progress(
            &job.input,
            &job.output,
            &options,
            false,
            &job.search,
            on_progress,
        ),
        (Some(program), Some(part)) => convert_part(program, &job, &options, part),
        (Some(program), None) => batch::convert_isolated_job_with_progress(
            program,
            &job,
            &options,
            &context.limits,
            stop,
            on_progress,
        ),
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
    let mut report = report;
    // The conversion wrote to its staging file; the design is where the record says.
    if let Some(path) = report.pointer_mut("/output/path") {
        *path = Value::String(design.output.display().to_string());
    }
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
    program: &Path,
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
    let mut command = std::process::Command::new(program);
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
}

fn watch_folder(
    command: &Command,
    folder: &Path,
    context: &Context,
    stop: &AtomicBool,
) -> Result<bool, String> {
    let settle = Duration::from_secs(command.settle);
    let interval = Duration::from_secs(command.interval.get());
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
                        emit(
                            command,
                            folder,
                            &record,
                            Instant::now(),
                            &Outcome::new(Event::Failed, why),
                        )?;
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
                    emit(command, folder, &record, Instant::now(), &missing(&design))?;
                }
                continue;
            };
            state.missing = false;
            if state.handled.as_ref() == Some(&(current.clone(), record_stamp.clone())) {
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
            let outcome = check_record(&record, context, Some(stop));
            // Remember the source as it was before converting: a save during the conversion
            // makes it differ, and so settles and converts again.
            state.handled = Some((current, stamp(&record)));
            ok &= outcome.event != Event::Failed;
            if outcome.event != Event::Unchanged || !state.announced {
                emit(command, folder, &record, started, &outcome)?;
            }
            state.announced = true;
        }
        first_scan = false;
        next_scan = Instant::now()
            .checked_add(interval)
            .ok_or("polling interval is too large; choose fewer seconds")?;
    }
    Ok(ok)
}
