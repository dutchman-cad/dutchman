//! `dutchman-mcp`: Dutchman as a Model Context Protocol server on stdio, for AI agents.
//!
//! Start it from an MCP client; it reads requests on stdin and answers on stdout until stdin
//! closes. See the README for client configuration and `docs/json.md` for the reports its
//! tools return.

use std::process::ExitCode;

fn main() -> ExitCode {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    match dutchman::mcp::serve(stdin.lock(), stdout.lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("dutchman-mcp: {err}");
            ExitCode::FAILURE
        }
    }
}
