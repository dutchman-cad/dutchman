//! `dutchman`: the command-line converter.
//!
//! Exit codes: 0 when the design was converted (or inspected), 1 when it could not be, 2 for a
//! usage error. See `docs/json.md` for the `--json` reports.

mod formats;
mod json;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use dutchman_core::{Check, Error, FeatureStatus, Format, Mode, Report};
use dutchman_solidcraft::{SolidCraftWriter, Verification, DEFAULT_TIMEOUT};
use serde_json::{json, Value};

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
        /// The native design file to read (for example a `.f3d` or `.ipt`).
        input: PathBuf,
        /// The `.solidcraft` file to write.
        output: PathBuf,
        /// Write the geometry as one non-parametric import, never the feature history.
        #[arg(long)]
        import_only: bool,
        /// The SolidCraft CLI that checks a written history against the original geometry
        /// (default: `$SOLIDCRAFT_CLI`, else `solidcraft-cli` on `PATH`).
        #[arg(long, value_name = "PATH", conflicts_with = "no_verify")]
        solidcraft: Option<PathBuf>,
        /// Write history without checking it in SolidCraft.
        #[arg(long)]
        no_verify: bool,
        /// Seconds each SolidCraft check may run before the history is written unverified.
        #[arg(long, value_name = "SECONDS", default_value_t = DEFAULT_TIMEOUT.as_secs())]
        verify_timeout: u64,
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
            import_only,
            solidcraft,
            no_verify,
            verify_timeout,
            json,
        } => {
            let verification = match (no_verify, solidcraft) {
                (true, _) => Verification::Off,
                (false, Some(path)) => Verification::With(path),
                (false, None) => Verification::Auto,
            };
            let writer = SolidCraftWriter::new()
                .import_only(import_only)
                .verification(verification)
                .timeout(Duration::from_secs(verify_timeout));
            convert(&input, &output, &writer, json)
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

fn convert(input: &Path, output: &Path, writer: &SolidCraftWriter, json: bool) -> ExitCode {
    match dutchman_core::convert(input, output, writer) {
        Ok(report) if json => {
            let mut out = json::success(json::CONVERT, json::input(input), &report);
            out.insert(
                "output".to_owned(),
                json!({"path": output.display().to_string(), "format": dutchman_solidcraft::FORMAT}),
            );
            print_json(&Value::Object(out));
            ExitCode::SUCCESS
        }
        Ok(report) => {
            print_losses(&report);
            ExitCode::SUCCESS
        }
        Err(err) => fail(json::CONVERT, input, &err, json),
    }
}

fn inspect(input: &Path, json: bool) -> ExitCode {
    let mut report = Report::default();
    let model = match Format::from_path(input)
        .ok_or_else(|| Error::UnknownFormat {
            path: input.to_path_buf(),
        })
        .and_then(|format| dutchman_core::read(format, input, &mut report))
    {
        Ok(model) => model,
        Err(err) => return fail(json::INSPECT, input, &err, json),
    };
    let outcome = SolidCraftWriter::new().plan(&model, &mut report);
    let bodies = model.ir.model.bodies.len();
    let faces = model.ir.model.faces.len();
    if json {
        report.set_outcome(outcome);
        let mut out = json::success(json::INSPECT, json::input(input), &report);
        out.insert(
            "geometry".to_owned(),
            json!({"bodies": bodies, "faces": faces}),
        );
        if let Some(assembly) = &model.assembly {
            out.insert(
                "assembly".to_owned(),
                json!({"parts": assembly.parts.len(), "placements": assembly.placements.len()}),
            );
        }
        print_json(&Value::Object(out));
    } else {
        let format = Format::from_path(input).map_or("design", Format::name);
        let count =
            |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
        match &model.assembly {
            Some(assembly) => println!(
                "{format}: {} placed, from {}",
                count(assembly.placements.len(), "part", "parts"),
                count(assembly.parts.len(), "part file", "part files")
            ),
            None => println!(
                "{format}: {}, {}",
                count(bodies, "body", "bodies"),
                count(faces, "face", "faces")
            ),
        }
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
        print_losses(&report);
    }
    ExitCode::SUCCESS
}

fn fail(schema: &str, input: &Path, err: &Error, json: bool) -> ExitCode {
    if json {
        print_json(&json::failure(schema, json::input(input), &err.to_string()));
    } else {
        eprintln!("dutchman: {err}");
    }
    ExitCode::FAILURE
}

fn print_json(value: &Value) {
    println!("{value}");
}

fn print_losses(report: &Report) {
    for loss in report.losses() {
        eprintln!("dutchman: {loss}");
    }
}
