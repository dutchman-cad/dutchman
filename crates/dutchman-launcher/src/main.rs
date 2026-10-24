//! Small front command for installed Dutchman CAD commands.

use std::process::ExitCode;

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    let first = args.next();
    match first.as_deref().and_then(|arg| arg.to_str()) {
        None if first.is_none() => help(),
        Some("--help" | "-h") => help(),
        Some("--version" | "-V") => {
            println!("dutchman launcher {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        _ => {
            let name = first.as_deref().and_then(|arg| arg.to_str()).unwrap_or("");
            if let Some(path) = dutchman_launcher::find_tool(name) {
                match dutchman_launcher::exec(&path, &args.collect::<Vec<_>>()) {
                    Ok(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
                    Err(err) => {
                        eprintln!("dutchman: cannot run {}: {err}", path.display());
                        ExitCode::FAILURE
                    }
                }
            } else {
                eprintln!("{}", dutchman_launcher::not_installed_message(name));
                ExitCode::FAILURE
            }
        }
    }
}

fn help() -> ExitCode {
    println!("Usage: dutchman <tool> [args...]\n");
    print!("{}", dutchman_launcher::tools_listing());
    println!(
        "Dutchman Convert, the full dutchman command: https://github.com/dutchman-cad/dutchman"
    );
    ExitCode::SUCCESS
}
