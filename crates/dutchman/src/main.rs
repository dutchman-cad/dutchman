//! `dutchman`: the command-line converter.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Parser, Subcommand};
use dutchman_core::Report;
use dutchman_solidcraft::{SolidCraftWriter, Verification, DEFAULT_TIMEOUT};

/// Carries CAD designs between formats.
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
        /// The native design file to read (for example a `.ipt`).
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
            match dutchman_core::convert(&input, &output, &writer) {
                Ok(report) => {
                    print_report(&report);
                    ExitCode::SUCCESS
                }
                Err(err) => {
                    eprintln!("dutchman: {err}");
                    ExitCode::FAILURE
                }
            }
        }
    }
}

fn print_report(report: &Report) {
    for loss in report.losses() {
        eprintln!("dutchman: {loss}");
    }
}
