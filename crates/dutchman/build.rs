//! Records the cadmpeg version Dutchman is built with, from the workspace's lock file, for
//! the reports to name.

use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let mut version = None;
    for folder in Path::new(&manifest).ancestors() {
        let lock = folder.join("Cargo.lock");
        if let Ok(text) = std::fs::read_to_string(&lock) {
            println!("cargo:rerun-if-changed={}", lock.display());
            version = cadmpeg_version(&text);
            break;
        }
    }
    println!(
        "cargo:rustc-env=DUTCHMAN_CADMPEG_VERSION={}",
        version.unwrap_or_else(|| "unknown".to_owned())
    );
}

/// The version of the `cadmpeg-core` package a lock file holds.
fn cadmpeg_version(lock: &str) -> Option<String> {
    let mut lines = lock.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "name = \"cadmpeg-core\"" {
            let next = lines.next()?;
            return next
                .trim()
                .strip_prefix("version = \"")
                .and_then(|rest| rest.strip_suffix('"'))
                .map(str::to_owned);
        }
    }
    None
}
