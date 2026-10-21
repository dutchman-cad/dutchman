//! Hostile input never crashes a conversion: every fixture, cut short and corrupted in many
//! deterministic ways, and named with every extension Dutchman knows, is read and written; each
//! attempt ends in a design or an error, in bounded time, and a failure leaves no file.
//!
//! Set `DUTCHMAN_ROBUSTNESS_SCALE` (a whole number, default 1) to multiply the number of
//! variants for a longer run.

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dutchman_core::{convert, Format};
use dutchman_solidcraft::{SolidCraftWriter, Verification};

/// Cuts per fixture, at evenly spread lengths.
const CUTS: usize = 24;
/// Single-byte corruptions per fixture.
const FLIPS: usize = 48;
/// Many-byte corruptions per fixture.
const SCRAMBLES: usize = 8;
/// The longest one attempt may take.
const SLOWEST: Duration = Duration::from_secs(10);

/// How many times over to run the variants (`DUTCHMAN_ROBUSTNESS_SCALE`).
fn scale() -> usize {
    std::env::var("DUTCHMAN_ROBUSTNESS_SCALE")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|n| *n >= 1)
        .unwrap_or(1)
}

fn fixtures() -> Vec<PathBuf> {
    let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../dutchman-core/tests/fixtures");
    let mut files: Vec<PathBuf> = std::fs::read_dir(folder)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| Format::from_path(path).is_some())
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

/// `bytes` unchanged, then every hostile variant of it, each labelled with how it was made.
fn variants(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
    let mut out = vec![("unchanged".to_owned(), bytes.to_vec())];
    let len = bytes.len();
    let (cuts, flips, scrambles) = (CUTS * scale(), FLIPS * scale(), SCRAMBLES * scale());
    for k in 0..cuts {
        let at = len * k / cuts;
        out.push((format!("cut at {at}"), bytes[..at].to_vec()));
    }
    if len == 0 {
        return out;
    }
    let mut rng = Xorshift(0x5eed_d07c_4a11_u64);
    for k in 0..flips {
        let at = (rng.next() as usize) % len;
        let mut copy = bytes.to_vec();
        copy[at] = if k % 2 == 0 { copy[at] ^ 0xff } else { 0 };
        out.push((format!("byte {at} changed"), copy));
    }
    for k in 0..scrambles {
        let mut copy = bytes.to_vec();
        for _ in 0..(8 << k.min(6)) {
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
        let own = fixture.extension().unwrap().to_string_lossy().into_owned();
        let name = fixture.file_name().unwrap().to_string_lossy().into_owned();
        for (label, variant) in variants(&bytes) {
            // Under its own extension, and once (unchanged) under every other one, so content
            // that does not match its name is met in every format's path, assemblies included.
            let extensions: Vec<String> = if label == "unchanged" {
                Format::ALL
                    .iter()
                    .map(|format| format.extension().to_owned())
                    .collect()
            } else {
                vec![own.clone()]
            };
            for ext in extensions {
                attempts += 1;
                let input = dir.path().join(format!("hostile.{ext}"));
                let output = dir.path().join("hostile.solidcraft");
                std::fs::write(&input, &variant).unwrap();
                let _ = std::fs::remove_file(&output);
                let started = Instant::now();
                let outcome = catch_unwind(AssertUnwindSafe(|| convert(&input, &output, &writer)));
                let took = started.elapsed();
                let what = format!("{name} as .{ext}: {label}");
                assert!(took < SLOWEST, "{what}: took {took:?}");
                match outcome {
                    Err(_) => crashes.push(what),
                    Ok(Err(_)) => {
                        assert!(!output.exists(), "{what}: a failed conversion wrote a file")
                    }
                    Ok(Ok(_)) => {}
                }
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

#[test]
fn an_input_over_the_size_cap_is_refused_before_it_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("huge.f3d");
    // A sparse file: its length is over the cap, though almost nothing is on disk.
    let file = std::fs::File::create(&input).unwrap();
    file.set_len(dutchman_core::MAX_INPUT_BYTES + 1).unwrap();
    drop(file);
    let output = dir.path().join("huge.solidcraft");
    let started = Instant::now();
    let err = convert(&input, &output, &SolidCraftWriter::new()).unwrap_err();
    assert!(
        matches!(err, dutchman_core::Error::TooLarge { .. }),
        "{err}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "refused without reading"
    );
    assert!(!output.exists());
}
