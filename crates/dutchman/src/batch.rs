//! `dutchman convert-all`: many native files converted in one run, several at a time.

use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use dutchman_core::Format;
use dutchman_solidcraft::SolidCraftWriter;
use serde_json::{json, Value};

use crate::json as report;

/// The schema of `dutchman convert-all --json`.
pub const SCHEMA: &str = "dutchman.convert-all/1";

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
}

/// What happens to a job while a batch runs.
#[derive(Debug)]
pub enum Event {
    /// The job at this index started converting.
    Started(usize),
    /// The job at this index finished, with its `dutchman.convert/1` report.
    Finished(usize, Value),
}

/// Converts every file `inputs` names or holds into `out_dir`, `jobs` at a time, writing
/// nothing outside `out_dir` and replacing designs already there only when `replace` is set,
/// and returns the `dutchman.convert-all/1` report.
pub fn convert_all(
    inputs: &[PathBuf],
    out_dir: &Path,
    writer: &SolidCraftWriter,
    jobs: Option<NonZeroUsize>,
    replace: bool,
) -> Value {
    let Plan {
        jobs: mut work,
        problems,
        skipped,
    } = plan(inputs, out_dir);
    for job in &mut work {
        job.replace = replace;
    }
    let results = run(&work, writer, workers(jobs));

    let failed = results
        .iter()
        .filter(|r| r.get("ok") != Some(&json!(true)))
        .count()
        + problems.len();
    let converted = results.len() + problems.len() - failed;
    let mut all: Vec<Value> = problems;
    all.extend(results);
    json!({
        "schema": SCHEMA,
        "ok": failed == 0,
        "converted": converted,
        "failed": failed,
        "skipped": skipped,
        "results": all,
    })
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
        for loss in result
            .get("losses")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let (line, shortened) = report::loss_line(loss, verbose);
            cut |= shortened;
            out.push_str("           ");
            out.push_str(&line);
            out.push('\n');
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
}

/// The files to convert, with their output paths; failure reports for inputs that are
/// neither a known file nor a readable folder; and how many files in the folders searched
/// were skipped as not in a format Dutchman knows.
pub fn plan(inputs: &[PathBuf], out_dir: &Path) -> Plan {
    let mut jobs = Vec::new();
    let mut problems = Vec::new();
    let mut skipped = 0;
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
            });
        }
    }
    Plan {
        jobs,
        problems,
        skipped,
    }
}

/// The files Dutchman knows under `folder`, with their paths relative to `root`. Symbolic
/// links are not followed, so a link cannot make the search loop.
fn search(
    root: &Path,
    folder: &Path,
    depth: usize,
    out: &mut Vec<(PathBuf, PathBuf)>,
    skipped: &mut usize,
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
            *skipped += 1;
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

/// Converts every job with `workers` threads, and returns each file's report in input order.
fn run(work: &[Job], writer: &SolidCraftWriter, workers: usize) -> Vec<Value> {
    let results: Mutex<Vec<Option<Value>>> = Mutex::new(vec![None; work.len()]);
    run_each(work, writer, workers, &AtomicBool::new(false), |event| {
        if let Event::Finished(index, report) = event {
            if let Ok(mut results) = results.lock() {
                if let Some(slot) = results.get_mut(index) {
                    *slot = Some(report);
                }
            }
        }
    });
    results
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect()
}

/// Converts every job with `workers` threads, telling `on_event` as each starts and finishes.
/// Once `stop` is set, no further job starts; those already converting finish.
pub fn run_each(
    work: &[Job],
    writer: &SolidCraftWriter,
    workers: usize,
    stop: &AtomicBool,
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
                on_event(Event::Finished(index, convert_job(job, writer)));
            });
        }
    });
}

/// One file converted, its output folder made first, as its convert report.
pub fn convert_job(job: &Job, writer: &SolidCraftWriter) -> Value {
    if let Some(parent) = job.output.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            return report::failure(
                report::CONVERT,
                report::input(&job.input),
                &format!("{}: {err}", parent.display()),
            );
        }
    }
    crate::convert(&job.input, &job.output, writer, job.replace)
}
