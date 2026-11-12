//! Run the measurement script through buildq's Cargo-only queue, with no extra dependencies.
use std::path::Path;
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let Some(root) = Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2) else {
        eprintln!("Cannot find the workspace; run this helper from the Dutchman checkout");
        return ExitCode::FAILURE;
    };
    match Command::new("bash")
        .arg(root.join("scripts/measure-plugin-build.sh"))
        .current_dir(root)
        .status()
    {
        Ok(status) => ExitCode::from(
            status
                .code()
                .and_then(|code| u8::try_from(code).ok())
                .unwrap_or(1),
        ),
        Err(error) => {
            eprintln!("Cannot run the component measurement: {error}; install bash and GNU time");
            ExitCode::FAILURE
        }
    }
}
