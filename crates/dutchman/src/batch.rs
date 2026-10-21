//! `dutchman convert-all`: many native files converted in one run, several at a time.

use std::collections::BTreeSet;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use dutchman_core::Format;
use dutchman_solidcraft::SolidCraftWriter;
use serde_json::{json, Map, Value};

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
struct Job {
    input: PathBuf,
    output: PathBuf,
}

/// Converts every file `inputs` names or holds into `out_dir`, `jobs` at a time.
pub fn convert_all(
    inputs: &[PathBuf],
    out_dir: &Path,
    writer: &SolidCraftWriter,
    jobs: Option<NonZeroUsize>,
    as_json: bool,
) -> ExitCode {
    let (work, problems) = plan(inputs, out_dir);
    let workers = jobs.map_or_else(
        || {
            std::thread::available_parallelism()
                .map_or(1, NonZeroUsize::get)
                .min(DEFAULT_JOBS)
        },
        NonZeroUsize::get,
    );
    let results = run(&work, writer, workers);

    let failed = results
        .iter()
        .filter(|r| r.get("ok") != Some(&json!(true)))
        .count()
        + problems.len();
    let converted = results.len() + problems.len() - failed;
    if as_json {
        let mut all: Vec<Value> = problems;
        all.extend(results);
        println!(
            "{}",
            json!({
                "schema": SCHEMA,
                "ok": failed == 0,
                "converted": converted,
                "failed": failed,
                "results": all,
            })
        );
    } else {
        for problem in &problems {
            print_line(problem);
        }
        for result in &results {
            print_line(result);
        }
        println!("{converted} converted, {failed} failed");
    }
    if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// One line of the text report.
fn print_line(result: &Value) {
    let input = result
        .pointer("/input/path")
        .and_then(Value::as_str)
        .unwrap_or("?");
    if result.get("ok") == Some(&json!(true)) {
        let output = result
            .pointer("/output/path")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let mode = result
            .get("mode")
            .and_then(Value::as_str)
            .unwrap_or("import");
        println!("converted  {input} -> {output} ({mode})");
    } else {
        let message = result
            .pointer("/error/message")
            .and_then(Value::as_str)
            .unwrap_or("failed");
        println!("failed     {input}: {message}");
    }
}

/// The files to convert, with their output paths; and failure reports for inputs that are
/// neither a known file nor a readable folder.
fn plan(inputs: &[PathBuf], out_dir: &Path) -> (Vec<Job>, Vec<Value>) {
    let mut jobs = Vec::new();
    let mut problems = Vec::new();
    let mut outputs = BTreeSet::new();
    for input in inputs {
        let mut found = Vec::new();
        if input.is_dir() {
            search(input, input, 0, &mut found);
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
            });
        }
    }
    (jobs, problems)
}

/// The files Dutchman knows under `folder`, with their paths relative to `root`. Symbolic
/// links are not followed, so a link cannot make the search loop.
fn search(root: &Path, folder: &Path, depth: usize, out: &mut Vec<(PathBuf, PathBuf)>) {
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
            search(root, &path, depth + 1, out);
        } else if meta.is_file() && Format::from_path(&path).is_some() {
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
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Value>>> = Mutex::new(vec![None; work.len()]);
    std::thread::scope(|scope| {
        for _ in 0..workers.min(work.len()).max(1) {
            scope.spawn(|| loop {
                let index = next.fetch_add(1, Ordering::Relaxed);
                let Some(job) = work.get(index) else { break };
                let report = convert_one(job, writer);
                if let Ok(mut results) = results.lock() {
                    if let Some(slot) = results.get_mut(index) {
                        *slot = Some(report);
                    }
                }
            });
        }
    });
    results
        .into_inner()
        .unwrap_or_default()
        .into_iter()
        .flatten()
        .collect()
}

/// One file converted, as its convert report.
fn convert_one(job: &Job, writer: &SolidCraftWriter) -> Value {
    if let Some(parent) = job.output.parent() {
        if let Err(err) = std::fs::create_dir_all(parent) {
            return report::failure(
                report::CONVERT,
                report::input(&job.input),
                &format!("{}: {err}", parent.display()),
            );
        }
    }
    match dutchman_core::convert(&job.input, &job.output, writer) {
        Ok(converted) => {
            let mut out: Map<String, Value> =
                report::success(report::CONVERT, report::input(&job.input), &converted);
            out.insert(
                "output".to_owned(),
                json!({"path": job.output.display().to_string(), "format": dutchman_solidcraft::FORMAT}),
            );
            Value::Object(out)
        }
        Err(err) => report::failure(report::CONVERT, report::input(&job.input), &err.to_string()),
    }
}
