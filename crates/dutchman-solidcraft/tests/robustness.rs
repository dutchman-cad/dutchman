//! Hostile input never crashes a conversion: every fixture, cut short and corrupted in many
//! deterministic ways, is read and written, and each attempt ends in a design or an error.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};

use dutchman_core::convert;
use dutchman_solidcraft::{SolidCraftWriter, Verification};

/// Cuts per fixture, at evenly spread lengths.
const CUTS: usize = 24;
/// Single-byte corruptions per fixture.
const FLIPS: usize = 48;
/// Many-byte corruptions per fixture.
const SCRAMBLES: usize = 8;

fn fixtures() -> Vec<PathBuf> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dutchman-core/tests/fixtures");
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| dutchman_core::Format::from_path(path).is_some())
        .collect();
    files.sort();
    files
}

/// A small deterministic generator, so a failure reproduces.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

/// Every hostile variant of `bytes`, each with a label naming how it was made.
fn variants(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    let len = bytes.len();
    for k in 0..CUTS {
        let at = len * k / CUTS;
        out.push((format!("cut at {at}"), bytes[..at].to_vec()));
    }
    let mut rng = Xorshift(0x5eed_d07c_4a11_u64);
    for k in 0..FLIPS {
        if len == 0 {
            break;
        }
        let at = (rng.next() as usize) % len;
        let mut copy = bytes.to_vec();
        copy[at] = if k % 2 == 0 { copy[at] ^ 0xff } else { 0 };
        out.push((format!("byte {at} changed"), copy));
    }
    for k in 0..SCRAMBLES {
        let mut copy = bytes.to_vec();
        for _ in 0..(8 << k.min(6)) {
            if len == 0 {
                break;
            }
            let at = (rng.next() as usize) % len;
            copy[at] = rng.next() as u8;
        }
        out.push((format!("scramble {k}"), copy));
    }
    out
}

#[test]
fn hostile_inputs_end_in_a_design_or_an_error() {
    let writer = SolidCraftWriter::new().verification(Verification::Off);
    let dir = tempfile::tempdir().unwrap();
    let mut crashes = Vec::new();
    let mut attempts = 0;
    for fixture in fixtures() {
        let bytes = std::fs::read(&fixture).unwrap();
        let ext = fixture.extension().unwrap().to_string_lossy().into_owned();
        let name = fixture.file_name().unwrap().to_string_lossy().into_owned();
        for (label, variant) in variants(&bytes) {
            attempts += 1;
            let input = dir.path().join(format!("hostile.{ext}"));
            let output = dir.path().join("hostile.solidcraft");
            std::fs::write(&input, &variant).unwrap();
            let _ = std::fs::remove_file(&output);
            let outcome = catch_unwind(AssertUnwindSafe(|| convert(&input, &output, &writer)));
            match outcome {
                Err(_) => crashes.push(format!("{name}: {label}")),
                Ok(Err(_)) => assert!(
                    !output.exists(),
                    "{name}: {label}: a failed conversion wrote a file"
                ),
                Ok(Ok(_)) => {}
            }
        }
    }
    assert!(attempts > 0);
    assert!(
        crashes.is_empty(),
        "{} of {attempts} attempts crashed: {crashes:#?}",
        crashes.len()
    );
}
