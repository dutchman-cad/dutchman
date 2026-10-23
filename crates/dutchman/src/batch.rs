//! `dutchman convert-all`: many native files converted in one run, several at a time.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use dutchman_core::{Format, Writer};
use serde_json::{json, Value};

use crate::json as report;
use crate::worker::{self, Limits};
use crate::{ConvertOptions, PartSearch};

/// The schema of `dutchman convert-all --json`.
pub const SCHEMA: &str = "dutchman.convert-all/1";

/// This Dutchman's version, which a journal's records must match.
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The journal's own format. A report recorded in an older one may lack what this version
/// records (the names an anonymous report takes out, among others), so its file is converted
/// again, as one recorded by another version of Dutchman is.
const JOURNAL_FORMAT: u64 = 2;

/// The most files one run converts.
const MAX_FILES: usize = 10_000;

/// How deep folders are searched.
const MAX_DEPTH: usize = 32;

/// The most files converted at once by default.
const DEFAULT_JOBS: usize = 8;

/// One file to convert, and where its design goes.
#[derive(Clone, Debug)]
pub struct Job {
    pub input: PathBuf,
    pub output: PathBuf,
    /// Whether an existing design at `output` is replaced.
    pub replace: bool,
    /// Where an assembly's part files are looked for: in the folder given that holds the
    /// file as its project, unless the options name one.
    pub search: PartSearch,
}

/// What happens to a job while a batch runs.
#[derive(Debug)]
pub enum Event {
    /// The job at this index started converting.
    Started(usize),
    /// The job at this index finished, with its `dutchman.convert/1` report.
    Finished(usize, Value),
}

/// How a batch runs.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    /// How many files to convert at once (default: [`workers`]).
    pub jobs: Option<NonZeroUsize>,
    /// The program each file is converted in, as a child process of its own within
    /// `limits`; `None` converts in this process, without limits.
    pub worker: Option<PathBuf>,
    /// What one file's conversion may use, when converted in a child.
    pub limits: Limits,
    /// A file that records each file's report as it finishes; files it already records, and
    /// that have not changed since, are not converted again.
    pub journal: Option<PathBuf>,
}

impl Settings {
    /// Settings that convert each file in a child process of the running program.
    pub fn isolated() -> Result<Self, String> {
        let program = std::env::current_exe()
            .map_err(|err| format!("cannot find the running program: {err}"))?;
        Ok(Settings {
            worker: Some(program),
            ..Settings::default()
        })
    }
}

/// Sets planned `jobs` to replace designs and look for part files as `options` say: in the
/// options' project folder when they name one, else in the folder given that holds each file,
/// then in the options' other folders.
pub fn prepare(jobs: &mut [Job], options: &ConvertOptions) {
    for job in jobs {
        let _ = job.output.set_extension(options.to.extension());
        job.replace = !options.no_clobber;
        if options.project.is_some() {
            job.search.project.clone_from(&options.project);
        }
        job.search.also.clone_from(&options.search);
    }
}

/// Converts every file `inputs` names or holds into `out_dir` as `settings` say, writing
/// nothing outside `out_dir` (and the journal) and replacing designs already there unless
/// `options` turn that off, and returns the `dutchman.convert-all/1` report. `Err` when the
/// journal cannot be read or written.
pub fn convert_all(
    inputs: &[PathBuf],
    out_dir: &Path,
    options: &ConvertOptions,
    settings: &Settings,
) -> Result<Value, String> {
    let Plan {
        jobs: mut work,
        problems,
        skipped,
        skipped_by_extension,
    } = plan(inputs, out_dir);
    prepare(&mut work, options);
    let mut journal = match &settings.journal {
        Some(path) => Some(Journal::open(path)?),
        None => None,
    };
    // Files the journal already records are not converted again.
    let mut results: Vec<Option<Value>> = work
        .iter()
        .map(|job| journal.as_ref().and_then(|j| j.recorded(job)))
        .collect();
    let resumed = results.iter().flatten().count();
    let todo: Vec<usize> = (0..work.len())
        .filter(|&i| results.get(i).is_some_and(Option::is_none))
        .collect();
    let pending: Vec<Job> = todo.iter().filter_map(|&i| work.get(i).cloned()).collect();
    let fresh: Mutex<Vec<Option<Value>>> = Mutex::new(vec![None; pending.len()]);
    let journal_error: Mutex<Option<String>> = Mutex::new(None);
    let writer = options.writer();
    let journal_lock = Mutex::new(journal.take());
    let convert = |job: &Job| match &settings.worker {
        Some(program) => make_folder(job)
            .unwrap_or_else(|| worker::convert_isolated(program, job, options, &settings.limits)),
        None => convert_job(job, &writer),
    };
    run_with(
        &pending,
        workers(settings.jobs),
        &AtomicBool::new(false),
        convert,
        |event| {
            if let Event::Finished(index, report) = event {
                if let (Ok(mut journal), Some(job)) = (journal_lock.lock(), pending.get(index)) {
                    if let Some(journal) = journal.as_mut() {
                        if let Err(err) = journal.record(job, &report) {
                            if let Ok(mut slot) = journal_error.lock() {
                                slot.get_or_insert(err);
                            }
                        }
                    }
                }
                if let Ok(mut fresh) = fresh.lock() {
                    if let Some(slot) = fresh.get_mut(index) {
                        *slot = Some(report);
                    }
                }
            }
        },
    );
    if let Some(err) = journal_error.into_inner().unwrap_or_default() {
        return Err(err);
    }
    for (index, report) in todo.into_iter().zip(fresh.into_inner().unwrap_or_default()) {
        if let Some(slot) = results.get_mut(index) {
            *slot = report;
        }
    }
    let results: Vec<Value> = results.into_iter().flatten().collect();

    let failed = results
        .iter()
        .filter(|r| r.get("ok") != Some(&json!(true)))
        .count()
        + problems.len();
    let converted = results.len() + problems.len() - failed;
    let mut all: Vec<Value> = problems;
    all.extend(results);
    Ok(json!({
        "schema": SCHEMA,
        "ok": failed == 0,
        "converted": converted,
        "failed": failed,
        "skipped": skipped,
        "skipped_by_extension": skipped_by_extension,
        "resumed": resumed,
        "results": all,
    }))
}

/// How many files to convert at once: `jobs`, else the machine's cores, at most 8.
pub fn workers(jobs: Option<NonZeroUsize>) -> usize {
    jobs.map_or_else(
        || {
            std::thread::available_parallelism()
                .map_or(1, NonZeroUsize::get)
                .min(DEFAULT_JOBS)
        },
        NonZeroUsize::get,
    )
}

/// The text report of a `dutchman.convert-all/1` report: one line per file, each converted
/// file's losses indented beneath it (their notes cut short unless `verbose`), then the counts.
pub fn text(report: &Value, verbose: bool) -> String {
    let mut out = String::new();
    let mut cut = false;
    for result in report
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        out.push_str(&line(result));
        out.push('\n');
        for line in report::completeness_lines(result) {
            out.push_str("           ");
            out.push_str(&line);
            out.push('\n');
        }
        let losses = result
            .get("losses")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        if verbose || losses.len() <= report::LISTED {
            for loss in losses {
                let (line, shortened) = report::loss_line(loss, verbose);
                cut |= shortened;
                out.push_str("           ");
                out.push_str(&line);
                out.push('\n');
            }
        } else {
            for line in report::grouped_lines(result) {
                out.push_str("           ");
                out.push_str(&line);
                out.push('\n');
            }
        }
    }
    let count = |key: &str| report.get(key).and_then(Value::as_u64).unwrap_or(0);
    out.push_str(&format!(
        "{} converted, {} failed",
        count("converted"),
        count("failed")
    ));
    if count("skipped") > 0 {
        out.push_str(&format!(
            ", {} skipped (not in a format Dutchman knows)",
            count("skipped")
        ));
    }
    out.push('\n');
    if cut {
        out.push_str(&format!("({})\n", report::CUT_NOTE));
    }
    out
}

/// The name of the whole text report `convert-all` writes in its output folder.
pub const LOG_NAME: &str = "dutchman.log";

/// The whole text report of a `dutchman.convert-all/1` report, every note in full: each
/// file's line, how complete it is and its losses by component, then the counts.
pub fn full_log(report: &Value) -> String {
    let mut out = String::new();
    for result in report
        .get("results")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        out.push_str(&report::full_log(result, &line(result)));
        out.push('\n');
    }
    let count = |key: &str| report.get(key).and_then(Value::as_u64).unwrap_or(0);
    out.push_str(&format!(
        "{} converted, {} failed, {} skipped\n",
        count("converted"),
        count("failed"),
        count("skipped")
    ));
    out
}

/// One file's line of the text reports: what it became, or why it failed.
pub fn line(result: &Value) -> String {
    let input = result
        .pointer("/input/path")
        .and_then(Value::as_str)
        .unwrap_or("?");
    if result.get("ok") == Some(&json!(true)) {
        let output = result
            .pointer("/output/path")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let written = match (
            result.get("mode").and_then(Value::as_str),
            result
                .pointer("/verification/status")
                .and_then(Value::as_str),
        ) {
            (Some("history"), Some("verified")) => "editable history, verified in SolidCraft",
            (Some("history"), _) => "editable history, unverified",
            _ => "geometry without history",
        };
        let freecad = result
            .pointer("/output/format")
            .and_then(Value::as_str)
            .is_some_and(|format| format.starts_with("freecad-"));
        let written = if freecad {
            match result
                .pointer("/verification/status")
                .and_then(Value::as_str)
            {
                Some("verified") => "geometry verified in FreeCAD".to_owned(),
                Some("differs" | "unverified") => {
                    let reason = result
                        .pointer("/verification/reason")
                        .and_then(Value::as_str)
                        .unwrap_or("not checked");
                    format!("FreeCAD rebuild: {reason}")
                }
                _ => written.to_owned(),
            }
        } else {
            written.to_owned()
        };
        format!("converted  {input} -> {output} ({written})")
    } else {
        let message = result
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("failed");
        format!("failed     {input}: {message}")
    }
}

/// What a run will convert.
#[derive(Debug)]
pub struct Plan {
    /// The files to convert, with their output paths.
    pub jobs: Vec<Job>,
    /// Failure reports for inputs that are neither a known file nor a readable folder.
    pub problems: Vec<Value>,
    /// Files in the folders searched that are not in a format Dutchman knows.
    pub skipped: usize,
    /// Those files, counted by their extension (see [`extension_of`]).
    pub skipped_by_extension: BTreeMap<String, usize>,
}

/// The files to convert, with their output paths; failure reports for inputs that are
/// neither a known file nor a readable folder; and how many files in the folders searched
/// were skipped as not in a format Dutchman knows.
pub fn plan(inputs: &[PathBuf], out_dir: &Path) -> Plan {
    let mut jobs = Vec::new();
    let mut problems = Vec::new();
    let mut skipped = BTreeMap::new();
    let mut outputs = BTreeSet::new();
    for input in inputs {
        let mut found = Vec::new();
        if input.is_dir() {
            search(input, input, 0, &mut found, &mut skipped);
        } else if input.is_file() {
            let name = input.file_name().map(PathBuf::from).unwrap_or_default();
            found.push((input.clone(), name));
        } else {
            problems.push(report::failure(
                report::CONVERT,
                report::input(input),
                "no such file or folder",
            ));
            continue;
        }
        for (path, relative) in found {
            if jobs.len() >= MAX_FILES {
                problems.push(report::failure(
                    report::CONVERT,
                    report::input(&path),
                    &format!("over Dutchman's limit of {MAX_FILES} files in one run"),
                ));
                continue;
            }
            let output = unique(
                out_dir.join(relative).with_extension("solidcraft"),
                &mut outputs,
            );
            jobs.push(Job {
                input: path,
                output,
                replace: true,
                search: PartSearch {
                    project: input.is_dir().then(|| input.clone()),
                    also: Vec::new(),
                },
            });
        }
    }
    Plan {
        jobs,
        problems,
        skipped: skipped.values().sum(),
        skipped_by_extension: skipped,
    }
}

/// A file's extension as the reports count it: lower case, letters and digits only, at most
/// eight of them; `other` for anything else and `none` for no extension, so no part of a
/// file's name can show through.
pub fn extension_of(path: &Path) -> String {
    match path.extension().and_then(|e| e.to_str()) {
        None => "none".to_owned(),
        Some(ext)
            if !ext.is_empty()
                && ext.len() <= 8
                && ext.chars().all(|c| c.is_ascii_alphanumeric()) =>
        {
            ext.to_ascii_lowercase()
        }
        Some(_) => "other".to_owned(),
    }
}

/// The files Dutchman knows under `folder`, with their paths relative to `root`. Symbolic
/// links are not followed, so a link cannot make the search loop.
fn search(
    root: &Path,
    folder: &Path,
    depth: usize,
    out: &mut Vec<(PathBuf, PathBuf)>,
    skipped: &mut BTreeMap<String, usize>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            search(root, &path, depth + 1, out, skipped);
        } else if meta.is_file() && Format::from_path(&path).is_none() {
            *skipped.entry(extension_of(&path)).or_insert(0) += 1;
        } else if meta.is_file() {
            let relative = path
                .strip_prefix(root)
                .map(Path::to_path_buf)
                .unwrap_or_default();
            out.push((path, relative));
        }
    }
}

/// `path`, numbered (`part-2.solidcraft`) when an earlier file already writes there.
fn unique(path: PathBuf, taken: &mut BTreeSet<PathBuf>) -> PathBuf {
    let mut candidate = path.clone();
    let mut n = 2;
    while taken.contains(&candidate) {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        candidate = path.with_file_name(format!("{stem}-{n}.solidcraft"));
        n += 1;
    }
    taken.insert(candidate.clone());
    candidate
}

/// Converts every job with `workers` threads, telling `on_event` as each starts and finishes.
/// Once `stop` is set, no further job starts; those already converting finish.
pub fn run_each(
    work: &[Job],
    writer: &(dyn Writer + Sync),
    workers: usize,
    stop: &AtomicBool,
    on_event: impl Fn(Event) + Sync,
) {
    run_with(
        work,
        workers,
        stop,
        |job| convert_job(job, writer),
        on_event,
    );
}

/// Runs `convert` on every job with `workers` threads, as [`run_each`] does, adding each
/// file's time to its report as `elapsed_ms`.
fn run_with(
    work: &[Job],
    workers: usize,
    stop: &AtomicBool,
    convert: impl Fn(&Job) -> Value + Sync,
    on_event: impl Fn(Event) + Sync,
) {
    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..workers.min(work.len()).max(1) {
            scope.spawn(|| loop {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = work.get(index) else { break };
                on_event(Event::Started(index));
                let started = Instant::now();
                let mut report = convert(job);
                if let Some(fields) = report.as_object_mut() {
                    let elapsed = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
                    fields.insert("elapsed_ms".to_owned(), json!(elapsed));
                }
                on_event(Event::Finished(index, report));
            });
        }
    });
}

/// The record of a batch's finished files, one JSON line each, kept as they finish so a run
/// stopped part way can carry on where it stopped.
struct Journal {
    file: std::fs::File,
    /// What it already records: each file's report, by the file's identity.
    known: BTreeMap<String, Value>,
}

impl Journal {
    /// Opens the journal at `path`, reading what it records; makes it when it is not there.
    fn open(path: &Path) -> Result<Journal, String> {
        let io = |err: std::io::Error| format!("journal {}: {err}", path.display());
        let mut known = BTreeMap::new();
        match std::fs::read_to_string(path) {
            Ok(text) => {
                for line in text.lines() {
                    // A line cut short by a stopped run is simply not recorded.
                    let Ok(entry) = serde_json::from_str::<Value>(line) else {
                        continue;
                    };
                    // A file another version of Dutchman, or another journal format, recorded is
                    // converted again.
                    if entry.get("dutchman").and_then(Value::as_str) != Some(VERSION)
                        || entry.get("journal").and_then(Value::as_u64) != Some(JOURNAL_FORMAT)
                    {
                        continue;
                    }
                    if let (Some(key), Some(report)) = (
                        entry.get("file").and_then(Value::as_str),
                        entry.get("report"),
                    ) {
                        known.insert(key.to_owned(), report.clone());
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(io(err)),
        }
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(io)?;
        Ok(Journal { file, known })
    }

    /// The report the journal holds for `job`, when the file has not changed since and its
    /// design, if it converted, is still where the journal says.
    fn recorded(&self, job: &Job) -> Option<Value> {
        let report = self.known.get(&identity(job)?)?;
        let output = job.output.display().to_string();
        let converted = report.get("ok") == Some(&json!(true));
        let still_there = report.pointer("/output/path").and_then(Value::as_str)
            == Some(output.as_str())
            && job.output.is_file();
        (!converted || still_there).then(|| report.clone())
    }

    /// Records `report` as `job`'s.
    fn record(&mut self, job: &Job, report: &Value) -> Result<(), String> {
        let Some(file) = identity(job) else {
            return Ok(());
        };
        let line = json!({
            "dutchman": VERSION,
            "journal": JOURNAL_FORMAT,
            "file": file,
            "report": report,
        });
        writeln!(self.file, "{line}")
            .and_then(|()| self.file.flush())
            .map_err(|err| format!("cannot record a file in the journal: {err}"))
    }
}

/// What tells one input file from another, and from itself once changed: its full path, its
/// size and when it was last changed.
fn identity(job: &Job) -> Option<String> {
    let path = std::fs::canonicalize(&job.input).ok()?;
    let meta = std::fs::metadata(&path).ok()?;
    let changed = meta
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos());
    Some(format!("{}|{}|{changed}", path.display(), meta.len()))
}

/// One file converted, its output folder made first, as its convert report.
pub fn convert_job(job: &Job, writer: &(dyn Writer + Sync)) -> Value {
    match make_folder(job) {
        Some(failure) => failure,
        None => crate::convert(&job.input, &job.output, writer, job.replace, &job.search),
    }
}

/// Makes the folder `job`'s design goes in; a failure report when it cannot be made.
fn make_folder(job: &Job) -> Option<Value> {
    let parent = job.output.parent()?;
    std::fs::create_dir_all(parent).err().map(|err| {
        report::failure(
            report::CONVERT,
            report::input(&job.input),
            &format!("{}: {err}", parent.display()),
        )
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_file_recorded_by_another_version_or_journal_format_is_converted_again() {
        // An older report lacks what this version records (the names an anonymous report
        // takes out, among others), so it is never reused, even under the same version number.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("journal.jsonl");
        let report = json!({"ok": false, "error": {"message": "x"}});
        let lines = [
            json!({"dutchman": "0.0.9", "journal": JOURNAL_FORMAT, "file": "/a|1|1",
                   "report": report}),
            json!({"dutchman": VERSION, "file": "/b|1|1", "report": report}),
            json!({"dutchman": VERSION, "journal": 1, "file": "/c|1|1", "report": report}),
            json!({"dutchman": VERSION, "journal": JOURNAL_FORMAT, "file": "/d|1|1",
                   "report": report}),
        ];
        let text: String = lines.iter().map(|line| format!("{line}\n")).collect();
        std::fs::write(&path, text).unwrap();
        let journal = Journal::open(&path).unwrap();
        let known: Vec<&str> = journal.known.keys().map(String::as_str).collect();
        assert_eq!(known, ["/d|1|1"]);
    }

    #[test]
    fn files_in_a_folder_given_are_found_in_that_folder_as_a_project() {
        let dir = tempfile::tempdir().unwrap();
        let project = dir.path().join("Robot");
        std::fs::create_dir_all(project.join("Assemblies")).unwrap();
        std::fs::write(project.join("Assemblies/top.iam"), b"").unwrap();
        let single = dir.path().join("cube.ipt");
        std::fs::write(&single, b"").unwrap();
        let plan = plan(&[project.clone(), single], &dir.path().join("out"));
        let projects: Vec<Option<PathBuf>> = plan
            .jobs
            .into_iter()
            .map(|job| job.search.project)
            .collect();
        assert_eq!(projects, [Some(project), None]);
    }
}
