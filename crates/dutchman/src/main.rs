//! `dutchman`: the command-line converter.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use dutchman_core::Report;

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
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Convert { input, output } => {
            let writer = dutchman_solidcraft::SolidCraftWriter::new();
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
