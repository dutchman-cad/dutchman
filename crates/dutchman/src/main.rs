//! `dutchman`: the command-line converter.
//!
//! Exit codes: 0 when the design was converted (or inspected), 1 when it could not be, 2 for a
//! usage error. See `docs/json.md` for the `--json` reports.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use dutchman::{anon, batch, formats, json, worker, ConvertOptions};
use dutchman_core::{Check, FeatureStatus, Format, Mode};
use serde_json::Value;

/// Convert CAD files: dutchman part.f3d writes part.solidcraft.
#[derive(Parser)]
#[command(
    name = "dutchman",
    version,
    about,
    allow_external_subcommands = true,
    after_help = "Examples:\n  dutchman part.f3d\n  dutchman part.f3d out.solidcraft\n  dutchman convert part.f3d out.solidcraft"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
    /// Print each loss note in full (the text reports cut them to their first sentence).
    #[arg(long, global = true)]
    verbose: bool,
    /// Hide the volunteer invitation after conversion results.
    #[arg(long, global = true)]
    quiet: bool,
}

#[derive(Subcommand)]
enum Command {
    #[command(external_subcommand)]
    External(Vec<OsString>),
    /// Drive SolidCraft from plain-language requests with Pilot.
    #[cfg(feature = "pilot")]
    #[command(name = "pilot")]
    Pilot(dutchman_pilot::cli::Command),
    /// Convert a native CAD file into a SolidCraft design, FreeCAD macro or document.
    Convert {
        /// The native design file to read (for example a `.f3d`, `.ipt` or `.sldprt`).
        input: PathBuf,
        /// The `.solidcraft`, `.FCMacro` or `.FCStd` file to write, matching --to.
        /// Omit to write beside the input with the extension chosen by --to.
        /// An explicit output is replaced, unless --no-clobber; an existing default needs --force.
        #[arg(conflicts_with = "output_flag")]
        output: Option<PathBuf>,
        /// Output path (an alternative to the positional output).
        #[arg(short = 'o', long = "output", value_name = "FILE")]
        output_flag: Option<PathBuf>,
        /// Replace an existing default output beside the input.
        #[arg(long, conflicts_with = "no_clobber")]
        force: bool,
        #[command(flatten)]
        options: ConvertOptions,
        /// Also write the whole text report, every loss in full, to FILE.
        #[arg(long, value_name = "FILE")]
        log: Option<PathBuf>,
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
        /// Write the whole text report, every loss of every file in full, to FILE (default:
        /// dutchman.log in the output folder).
        #[arg(long, value_name = "FILE")]
        log: Option<PathBuf>,
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
    let mut args: Vec<OsString> = std::env::args_os().collect();
    if let Some((index, arg)) = args
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, arg)| *arg != "--verbose" && *arg != "--quiet")
    {
        let name = arg.to_str().unwrap_or("");
        let builtin = Cli::command()
            .get_subcommands()
            .any(|cmd| cmd.get_name() == name)
            || matches!(name, "pilot" | "help");
        if name == "pilot" || !builtin && !name.starts_with('-') {
            if let Some(path) = dutchman_launcher::find_tool(name) {
                let rest: Vec<_> = args.iter().skip(index + 1).cloned().collect();
                return run_tool(&path, &rest);
            }
        }
        if !builtin && (!name.starts_with('-') || name.is_empty()) && is_input(Path::new(arg)) {
            args.insert(index, OsString::from("convert"));
        }
    }
    // Name the command `dutchman` in every usage line, also where argv[0] is `dutchman.exe`.
    let mut command = Cli::command().bin_name("dutchman");
    if args.len() == 1
        || args
            .iter()
            .any(|arg| arg == "--help" || arg == "-h" || arg == "help")
    {
        let listing = dutchman_launcher::tools_listing();
        if !listing.is_empty() {
            let examples = command
                .get_after_help()
                .map(ToString::to_string)
                .unwrap_or_default();
            command = command.after_help(format!("{examples}\n\n{listing}"));
        }
    }
    #[cfg(feature = "pilot")]
    let mut command = command.mut_subcommand("pilot", |c| {
        dutchman_pilot::cli::with_bin_name(c, "dutchman pilot")
    });
    if args.len() == 1 {
        let _ = command.print_help();
        println!();
        return ExitCode::SUCCESS;
    }
    let suggestion_command = command
        .clone()
        .external_subcommand_value_parser(None::<clap::builder::ValueParser>)
        .allow_external_subcommands(false);
    let matches = command.get_matches_from(&args);
    let cli = match Cli::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(err) => err.exit(),
    };
    let verbose = cli.verbose;
    let quiet = cli.quiet;
    match cli.command {
        Command::External(args) => {
            let mut args = args.into_iter();
            let name = args.next().unwrap_or_default();
            let name = name.to_str().unwrap_or("");
            if let Some(path) = dutchman_launcher::find_tool(name) {
                return run_tool(&path, &args.collect::<Vec<_>>());
            }
            if let Err(error) = suggestion_command.try_get_matches_from(std::env::args_os()) {
                if error
                    .get(clap::error::ContextKind::SuggestedSubcommand)
                    .is_some()
                {
                    error.exit();
                }
            }
            eprintln!("{}", dutchman_launcher::not_installed_message(name));
            ExitCode::FAILURE
        }
        #[cfg(feature = "pilot")]
        Command::Pilot(cmd) => match dutchman_pilot::cli::run_named(cmd, "dutchman pilot") {
            Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
            Err(err) => {
                eprintln!("dutchman: {err}");
                ExitCode::FAILURE
            }
        },
        Command::Convert {
            input,
            output,
            output_flag,
            force,
            mut options,
            log,
            json,
        } => {
            let explicit = output.or(output_flag);
            let derived = explicit.is_none();
            let output = explicit.unwrap_or_else(|| {
                dutchman_core::Format::part_revision(&input)
                    .map_or_else(|| input.clone(), |(part, _)| part)
                    .with_extension(options.to.extension())
            });
            if derived {
                if input == output
                    || std::fs::canonicalize(&input)
                        .ok()
                        .zip(std::fs::canonicalize(&output).ok())
                        .is_some_and(|(input, output)| input == output)
                {
                    eprintln!("dutchman: default output {} is the input file; choose another output with -o PATH", output.display());
                    return ExitCode::from(2);
                }
                if output.try_exists().unwrap_or(true) && !force {
                    eprintln!("dutchman: {} already exists; choose another output with -o PATH, or replace it with --force", output.display());
                    return ExitCode::from(2);
                }
                options.no_clobber |= !force;
            }
            convert(
                &input,
                &output,
                &options,
                log.as_deref(),
                json,
                verbose,
                quiet,
            )
        }
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
            log,
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
                if !quiet {
                    if let Some(pitch) = json::fixtures_pitch(
                        report
                            .get("results")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten(),
                    ) {
                        println!("{pitch}");
                    }
                }
            }
            let log = log.unwrap_or_else(|| out_dir.join(batch::LOG_NAME));
            let written = log
                .parent()
                .filter(|folder| !folder.as_os_str().is_empty())
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&log, batch::full_log(&report)));
            match written {
                Ok(()) if !json => eprintln!("dutchman: every loss, in full: {}", log.display()),
                Ok(()) => {}
                Err(err) => eprintln!("dutchman: cannot write {}: {err}", log.display()),
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

/// Recognise files and known CAD suffixes for the conversion shorthand.
fn is_input(path: &Path) -> bool {
    path.is_file() || Format::from_path(path).is_some()
}

fn run_tool(path: &Path, args: &[OsString]) -> ExitCode {
    match dutchman_launcher::exec(path, args) {
        Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
        Err(err) => {
            eprintln!("dutchman: cannot run {}: {err}", path.display());
            ExitCode::FAILURE
        }
    }
}

fn convert(
    input: &Path,
    output: &Path,
    options: &ConvertOptions,
    log: Option<&Path>,
    as_json: bool,
    verbose: bool,
    quiet: bool,
) -> ExitCode {
    if !as_json {
        if let Some(status) = json::status_line(&serde_json::json!({"input": json::input(input)})) {
            println!("{status}");
        }
    }
    let report = dutchman::convert(
        input,
        output,
        &options.writer(),
        !options.no_clobber,
        &options.part_search(),
    );
    if let Some(log) = log {
        let text = json::full_log(&report, &batch::line(&report));
        if let Err(err) = std::fs::write(log, text) {
            eprintln!("dutchman: cannot write {}: {err}", log.display());
        }
    }
    if as_json {
        print_json(&report);
    } else if let Some(message) = report.pointer("/error/message").and_then(Value::as_str) {
        eprintln!("dutchman: {message}");
    } else {
        // What was written first, on stdout; then what it could not carry across.
        println!("{}", batch::line(&report));
        for line in json::completeness_lines(&report) {
            println!("{line}");
        }
        let losses = report
            .get("losses")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        if verbose || losses.len() <= json::LISTED {
            print_losses(losses, verbose);
        } else {
            for line in json::grouped_lines(&report) {
                eprintln!("dutchman: {line}");
            }
            match log {
                Some(log) => eprintln!("dutchman: every loss, in full: {}", log.display()),
                None => eprintln!(
                    "dutchman: (--verbose prints every loss; --log FILE writes them to FILE)"
                ),
            }
        }
    }
    if !as_json && !quiet {
        if let Some(pitch) = json::fixtures_pitch([&report]) {
            println!("{pitch}");
        }
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
    if let Some(completeness) = inspection.report.completeness() {
        let lines = json::completeness_lines(&serde_json::json!({
            "completeness": json::completeness_json(completeness),
        }));
        for line in lines {
            println!("would place: {}", line.trim_start_matches("assembly: "));
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

#[cfg(test)]
mod output_tests {
    use super::*;

    #[test]
    fn freecad_output_accepts_short_output_flag_and_positional_output() {
        for args in [
            vec![
                "dutchman",
                "convert",
                "in.f3d",
                "--to",
                "freecad",
                "-o",
                "out.FCMacro",
            ],
            vec![
                "dutchman",
                "convert",
                "in.f3d",
                "out.FCMacro",
                "--to",
                "freecad",
            ],
        ] {
            let cli = Cli::try_parse_from(args).unwrap();
            let Command::Convert {
                options,
                output,
                output_flag,
                ..
            } = cli.command
            else {
                panic!("expected convert");
            };
            assert_eq!(options.to, dutchman::OutputFormat::Freecad);
            assert_eq!(output.or(output_flag), Some(PathBuf::from("out.FCMacro")));
        }
    }

    #[test]
    fn output_path_is_optional_and_cannot_be_given_twice() {
        assert!(Cli::try_parse_from(["dutchman", "convert", "in.f3d", "--to", "freecad"]).is_ok());
        assert!(Cli::try_parse_from([
            "dutchman",
            "convert",
            "in.f3d",
            "a.FCMacro",
            "-o",
            "b.FCMacro",
        ])
        .is_err());
        assert!(Cli::try_parse_from([
            "dutchman",
            "convert",
            "in.f3d",
            "-o",
            "a.FCMacro",
            "--to",
            "unknown",
        ])
        .is_err());
    }

    #[test]
    fn freecad_native_and_verified_macro_flags_reach_batch_workers() {
        for (format, extension, verify) in [("fcstd", "FCStd", false), ("freecad", "FCMacro", true)]
        {
            let mut args = vec![
                "dutchman",
                "convert",
                "in.f3d",
                "-o",
                "out",
                "--to",
                format,
                "--freecad-timeout",
                "17",
            ];
            if verify {
                args.push("--verify");
            }
            let cli = Cli::try_parse_from(args).unwrap();
            let Command::Convert { options, .. } = cli.command else {
                panic!("expected convert");
            };
            assert_eq!(options.to.extension(), extension);
            assert_eq!(options.verify, verify);
            assert_eq!(options.freecad_timeout, 17);
            let mut worker_args = vec![
                std::ffi::OsString::from("dutchman"),
                "convert".into(),
                "in.f3d".into(),
                "out".into(),
            ];
            worker_args.extend(options.to_args());
            let worker = Cli::try_parse_from(worker_args).unwrap();
            let Command::Convert {
                options: forwarded, ..
            } = worker.command
            else {
                panic!("expected convert");
            };
            assert_eq!(forwarded.to, options.to);
            assert_eq!(forwarded.verify, options.verify);
            assert_eq!(forwarded.freecad_timeout, options.freecad_timeout);
        }
    }
}
