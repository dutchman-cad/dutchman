//! `dutchman`: the command-line converter.
//!
//! Exit codes: 0 when the design was converted (or inspected), 1 when it could not be, 2 for a
//! usage error. See `docs/json.md` for the `--json` reports.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dutchman::{anon, batch, formats, json, worker, ConvertOptions};
use dutchman_core::{Check, FeatureStatus, Format, Mode};
use serde_json::Value;

/// Ferries CAD designs between formats.
#[derive(Parser)]
#[command(name = "dutchman", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Print each loss note in full (the text reports cut them to their first sentence).
    #[arg(long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Convert a native CAD file into a SolidCraft design.
    Convert {
        /// The native design file to read (for example a `.f3d`, `.ipt` or `.sldprt`).
        input: PathBuf,
        /// The `.solidcraft` file to write. An existing file is replaced, unless --no-clobber.
        output: PathBuf,
        #[command(flatten)]
        options: ConvertOptions,
        /// Print one machine-readable report on stdout instead of the text report.
        #[arg(long)]
        json: bool,
    },
    /// Convert every native CAD file given, and those in the folders given, into SolidCraft
    /// designs under one folder.
    ConvertAll {
        /// Files, and folders to search (with their subfolders) for files Dutchman knows.
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// The folder to write the designs to, keeping each input folder's layout. Designs
        /// already there are replaced, unless --no-clobber.
        #[arg(long, value_name = "DIR")]
        out_dir: PathBuf,
        /// How many files to convert at once (default: the machine's cores, at most 8).
        #[arg(long, value_name = "N")]
        jobs: Option<std::num::NonZeroUsize>,
        /// Seconds one file may take before it is stopped and reported as failed.
        #[arg(long, value_name = "SECONDS", default_value_t = worker::DEFAULT_TIMEOUT.as_secs())]
        timeout: u64,
        /// Megabytes of memory one file may hold before it is stopped and reported as failed
        /// (0: no limit).
        #[arg(long, value_name = "MB", default_value_t = worker::DEFAULT_MAX_MEMORY_MB)]
        max_memory: u64,
        /// Record each file's result in FILE as it finishes; files already recorded there,
        /// unchanged since, are not converted again, so a stopped run carries on.
        #[arg(long, value_name = "FILE")]
        journal: Option<PathBuf>,
        /// Also write an anonymous report to FILE: counts, causes and timings with no file
        /// names, paths or geometry, safe to share (see docs/report-anon.md).
        #[arg(long, value_name = "FILE")]
        report_anon: Option<PathBuf>,
        /// The salt for the anonymous report's file ids (32 hex digits), to get the same ids as
        /// an earlier run (default: a new one, printed when the run ends).
        #[arg(long, value_name = "HEX", requires = "report_anon")]
        report_salt: Option<String>,
        /// Write the key from each anonymous id to its file's path to FILE. Keep it private.
        #[arg(long, value_name = "FILE", requires = "report_anon")]
        report_key: Option<PathBuf>,
        #[command(flatten)]
        options: ConvertOptions,
        /// Print one machine-readable report on stdout instead of the text report.
        #[arg(long)]
        json: bool,
    },
    /// List the native formats Dutchman knows and what converts from each.
    Formats {
        /// Print one machine-readable report on stdout instead of the text listing.
        #[arg(long)]
        json: bool,
    },
    /// Show what converting a native CAD file would carry across, without writing anything.
    Inspect {
        /// The native design file to read.
        input: PathBuf,
        /// The project folder an assembly's part files are searched in when they are not at
        /// their stored paths (default: the nearest folder above the assembly holding an
        /// Inventor .ipj, else two folders up).
        #[arg(long, value_name = "DIR")]
        project: Option<PathBuf>,
        /// Another folder to search, with its subfolders, for part files the project folder
        /// does not hold; may be given more than once.
        #[arg(long = "search", value_name = "DIR")]
        search: Vec<PathBuf>,
        /// Print one machine-readable report on stdout instead of the text report.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    if let Some(code) = worker::run_if_worker() {
        return code;
    }
    let cli = Cli::parse();
    let verbose = cli.verbose;
    match cli.command {
        Command::Convert {
            input,
            output,
            options,
            json,
        } => convert(&input, &output, &options, json, verbose),
        Command::ConvertAll {
            inputs,
            out_dir,
            jobs,
            timeout,
            max_memory,
            journal,
            report_anon,
            report_salt,
            report_key,
            options,
            json,
        } => {
            let anon = match report_salt.as_deref().map(anon::Salt::parse).transpose() {
                Ok(salt) => report_anon.map(|path| anon::Request {
                    path,
                    salt,
                    key: report_key,
                }),
                Err(err) => {
                    eprintln!("dutchman: --report-salt: {err}");
                    return ExitCode::from(2);
                }
            };
            let settings = match batch::Settings::isolated() {
                Ok(settings) => batch::Settings {
                    jobs,
                    journal,
                    limits: worker::Limits {
                        timeout: std::time::Duration::from_secs(timeout.max(1)),
                        max_memory: (max_memory > 0).then_some(max_memory.saturating_mul(1 << 20)),
                    },
                    ..settings
                },
                Err(err) => {
                    eprintln!("dutchman: {err}");
                    return ExitCode::FAILURE;
                }
            };
            let report = match batch::convert_all(&inputs, &out_dir, &options, &settings) {
                Ok(report) => report,
                Err(err) => {
                    eprintln!("dutchman: {err}");
                    return ExitCode::FAILURE;
                }
            };
            if json {
                print_json(&report);
            } else {
                print!("{}", batch::text(&report, verbose));
            }
            if let Some(request) = anon {
                match anon::write(&request, &report, &inputs, &out_dir, &options, &settings) {
                    Ok(salt) => eprintln!(
                        "dutchman: anonymous report written to {}. Its file ids use the salt {}: \
                         keep the salt to yourself, and give --report-salt {} to get the same ids \
                         next time.",
                        request.path.display(),
                        salt.hex(),
                        salt.hex()
                    ),
                    Err(err) => {
                        eprintln!("dutchman: cannot write the anonymous report: {err}");
                        return ExitCode::FAILURE;
                    }
                }
            }
            exit(&report)
        }
        Command::Inspect {
            input,
            project,
            search,
            json,
        } => inspect(
            &input,
            &dutchman::PartSearch {
                project,
                also: search,
            },
            json,
            verbose,
        ),
        Command::Formats { json } => {
            if json {
                print_json(&formats::json());
            } else {
                print!("{}", formats::text());
            }
            ExitCode::SUCCESS
        }
    }
}

fn convert(
    input: &Path,
    output: &Path,
    options: &ConvertOptions,
    as_json: bool,
    verbose: bool,
) -> ExitCode {
    let report = dutchman::convert(
        input,
        output,
        &options.writer(),
        !options.no_clobber,
        &options.part_search(),
    );
    if as_json {
        print_json(&report);
    } else if let Some(message) = report.pointer("/error/message").and_then(Value::as_str) {
        eprintln!("dutchman: {message}");
    } else {
        // What was written first, on stdout; then what it could not carry across.
        println!("{}", batch::line(&report));
        print_losses(
            report
                .get("losses")
                .and_then(Value::as_array)
                .map_or(&[][..], Vec::as_slice),
            verbose,
        );
    }
    exit(&report)
}

fn inspect(input: &Path, search: &dutchman::PartSearch, as_json: bool, verbose: bool) -> ExitCode {
    let inspection = match dutchman::inspect(input, search) {
        Ok(inspection) => inspection,
        Err(err) => {
            if as_json {
                print_json(&json::error(json::INSPECT, json::input(input), &err));
            } else {
                eprintln!("dutchman: {err}");
            }
            return ExitCode::FAILURE;
        }
    };
    if as_json {
        print_json(&inspection.json(input));
        return ExitCode::SUCCESS;
    }
    let format = Format::from_path(input).map_or("design", Format::name);
    let count =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    match inspection.assembly {
        Some((parts, placements)) => println!(
            "{format}: {} placed, from {}",
            count(placements, "part", "parts"),
            count(parts, "part file", "part files")
        ),
        None => println!(
            "{format}: {}, {}",
            count(inspection.bodies, "body", "bodies"),
            count(inspection.faces, "face", "faces")
        ),
    }
    if let Some(outcome) = inspection.report.outcome() {
        match (&inspection.conversion, &outcome.mode, &outcome.check) {
            (Err(reason), _, _) => println!("would write: nothing; converting it fails: {reason}"),
            (Ok(()), Mode::History, _) => println!("would write: editable history"),
            (Ok(()), Mode::Import, Check::Flattened(reason) | Check::NotApplicable(reason)) => {
                println!("would write: geometry without history ({reason})");
            }
            (Ok(()), Mode::Import, _) => println!("would write: geometry without history"),
        }
        for feature in &outcome.features {
            let status = match feature.status {
                FeatureStatus::Mapped => "mapped",
                FeatureStatus::NotNeeded => "not needed",
                FeatureStatus::LeftOut => "left out",
                FeatureStatus::Blocked => "blocked",
            };
            let name = feature.name.as_deref().unwrap_or("(unnamed)");
            match &feature.reason {
                Some(reason) => println!("  {status:<10} {} \"{name}\": {reason}", feature.kind),
                None => println!("  {status:<10} {} \"{name}\"", feature.kind),
            }
        }
    }
    let losses = json::losses(&inspection.report);
    print_losses(losses.as_array().map_or(&[][..], Vec::as_slice), verbose);
    ExitCode::SUCCESS
}

/// Success when the report says the run succeeded.
fn exit(report: &Value) -> ExitCode {
    if report.get("ok") == Some(&Value::Bool(true)) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn print_json(value: &Value) {
    println!("{value}");
}

/// Losses as the text reports print them, one line each on stderr, then once a note on how
/// to see the ones cut short in full.
fn print_losses(losses: &[Value], verbose: bool) {
    let mut cut = false;
    for loss in losses {
        let (line, shortened) = json::loss_line(loss, verbose);
        cut |= shortened;
        eprintln!("dutchman: {line}");
    }
    if cut {
        eprintln!("dutchman: ({})", json::CUT_NOTE);
    }
}
