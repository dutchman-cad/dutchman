//! `dutchman`: the command-line converter.
//!
//! Exit codes: 0 when the design was converted (or inspected), 1 when it could not be, 2 for a
//! usage error. See `docs/json.md` for the `--json` reports.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dutchman::{batch, formats, json, ConvertOptions};
use dutchman_core::{Check, FeatureStatus, Format, Mode};
use dutchman_solidcraft::SolidCraftWriter;
use serde_json::Value;

/// Ferries CAD designs between formats.
#[derive(Parser)]
#[command(name = "dutchman", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Convert a native CAD file into a SolidCraft design.
    Convert {
        /// The native design file to read (for example a `.f3d`, `.ipt` or `.sldprt`).
        input: PathBuf,
        /// The `.solidcraft` file to write.
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
        /// The folder to write the designs to, keeping each input folder's layout.
        #[arg(long, value_name = "DIR")]
        out_dir: PathBuf,
        /// How many files to convert at once (default: the machine's cores, at most 8).
        #[arg(long, value_name = "N")]
        jobs: Option<std::num::NonZeroUsize>,
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
        /// Print one machine-readable report on stdout instead of the text report.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Convert {
            input,
            output,
            options,
            json,
        } => convert(&input, &output, &options.writer(), json),
        Command::ConvertAll {
            inputs,
            out_dir,
            jobs,
            options,
            json,
        } => {
            let report = batch::convert_all(&inputs, &out_dir, &options.writer(), jobs);
            if json {
                print_json(&report);
            } else {
                print!("{}", batch::text(&report));
            }
            exit(&report)
        }
        Command::Inspect { input, json } => inspect(&input, json),
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

fn convert(input: &Path, output: &Path, writer: &SolidCraftWriter, as_json: bool) -> ExitCode {
    let report = dutchman::convert(input, output, writer);
    if as_json {
        print_json(&report);
    } else if let Some(message) = report.pointer("/error/message").and_then(Value::as_str) {
        eprintln!("dutchman: {message}");
    } else {
        print_losses(&report);
    }
    exit(&report)
}

fn inspect(input: &Path, as_json: bool) -> ExitCode {
    let inspection = match dutchman::inspect(input) {
        Ok(inspection) => inspection,
        Err(err) => {
            if as_json {
                print_json(&json::failure(
                    json::INSPECT,
                    json::input(input),
                    &err.to_string(),
                ));
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
        match (&outcome.mode, &outcome.check) {
            (Mode::History, _) => println!("would write: editable history"),
            (Mode::Import, Check::Flattened(reason) | Check::NotApplicable(reason)) => {
                println!("would write: geometry without history ({reason})");
            }
            (Mode::Import, _) => println!("would write: geometry without history"),
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
    for loss in inspection.report.losses() {
        eprintln!("dutchman: {loss}");
    }
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

/// The losses of a convert report, one line each on stderr, as the text report prints them.
fn print_losses(report: &Value) {
    for loss in report
        .get("losses")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let field = |name: &str| loss.get(name).and_then(Value::as_str).unwrap_or("");
        eprintln!(
            "dutchman: {}: {}: {}",
            field("level"),
            field("subject"),
            field("message")
        );
    }
}
