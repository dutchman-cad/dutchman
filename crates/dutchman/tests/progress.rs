//! Manual paired measurement of coarse progress overhead on one public fixture.
#![allow(clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dutchman::{ConvertOptions, PartSearch};

#[test]
#[ignore = "manual paired progress overhead measurement"]
fn measure_progress_overhead() {
    let input = std::env::var_os("DUTCHMAN_PROGRESS_BENCH_INPUT")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d")
        });
    let dir = tempfile::tempdir().unwrap();
    let output = dir.path().join("measured.solidcraft");
    let writer = ConvertOptions {
        verify: false,
        ..ConvertOptions::default()
    }
    .writer();
    let search = PartSearch::default();
    let callback = |event| {
        std::hint::black_box(dutchman::json::progress(&input, event).to_string());
    };
    let run = |progress| {
        let start = Instant::now();
        let report = if progress {
            dutchman::convert_with_progress(&input, &output, &writer, true, &search, &callback)
        } else {
            dutchman::convert(&input, &output, &writer, true, &search)
        };
        let elapsed = start.elapsed();
        assert_eq!(report["ok"], true, "{report}");
        std::fs::remove_file(&output).unwrap();
        elapsed
    };
    run(false);
    run(true);
    let mut baseline = Vec::new();
    let mut progress = Vec::new();
    for pair in 0..11 {
        if pair % 2 == 0 {
            baseline.push(run(false));
            progress.push(run(true));
        } else {
            progress.push(run(true));
            baseline.push(run(false));
        }
    }
    fn median(samples: &mut [Duration]) -> f64 {
        samples.sort();
        samples.get(samples.len() / 2).unwrap().as_secs_f64() * 1000.0
    }
    eprintln!("baseline_ms={baseline:?}\nprogress_ms={progress:?}");
    let before = median(&mut baseline);
    let after = median(&mut progress);
    eprintln!(
        "median baseline={before:.3} ms, progress={after:.3} ms, change={:.2}%",
        (after / before - 1.0) * 100.0
    );
}

#[test]
#[ignore = "manual fixed reporting overhead measurement"]
fn measure_fixed_reporting_overhead() {
    use dutchman_core::{Progress, Stage};
    let input = Path::new("fixture.f3z");
    let callback = |event| {
        std::hint::black_box(dutchman::json::progress(input, event).to_string());
    };
    let mut samples = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        let progress = Progress::new(&callback);
        progress.run(|| {
            for stage in [
                Stage::Input,
                Stage::Decode,
                Stage::Project,
                Stage::Map,
                Stage::Write,
            ] {
                progress.stage(stage);
            }
            progress.designs_total(1);
            progress.design_read();
        });
        samples.push(start.elapsed());
    }
    samples.sort();
    let micros = |index: usize| samples.get(index).unwrap().as_secs_f64() * 1_000_000.0;
    eprintln!("seven events plus timer startup/shutdown: median={:.3} us, p95={:.3} us, min={:.3} us, max={:.3} us", micros(50), micros(95), micros(0), micros(99));
}

#[test]
fn verbose_plain_cli_progress_contains_the_running_part() {
    let dir = tempfile::tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
    let local_input = dir.path().join("Bracket.f3d");
    std::fs::copy(input, &local_input).unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(["convert", "--verbose", "--no-verify"])
        .arg(local_input)
        .arg(dir.path().join("cube.sc"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("Building history for Bracket:"), "{stderr}");
}

#[cfg(debug_assertions)]
#[test]
fn a_debug_slow_part_emits_a_named_heartbeat_through_the_cli() {
    let dir = tempfile::tempdir().unwrap();
    let input = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
    let local_input = dir.path().join("Bracket.f3d");
    std::fs::copy(input, &local_input).unwrap();
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    use std::sync::mpsc;

    let release = dir.path().join("release");
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .args(["convert", "--json", "--no-verify"])
        .env("DUTCHMAN_TEST_SLOW_PART", "Bracket")
        .env("DUTCHMAN_TEST_SLOW_PART_RELEASE", &release)
        .arg(local_input)
        .arg(dir.path().join("cube.sc"))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let (send, receive) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        let mut lines = Vec::new();
        for line in BufReader::new(stderr).lines() {
            let line = line.unwrap();
            if let Ok(record) = serde_json::from_str::<serde_json::Value>(&line) {
                if record["heartbeat"] == true
                    && record["running"]["kind"] == "preparing"
                    && record["running"]["name"] == "Bracket"
                    && record["running"]["elapsed_ms"]
                        .as_u64()
                        .is_some_and(|ms| ms >= 4_000)
                {
                    let _ = send.send(());
                }
            }
            lines.push(line);
        }
        lines
    });
    // Wait for the event itself, keeping the named step alive for a delayed timer.
    let heartbeat = receive.recv_timeout(Duration::from_secs(45));
    std::fs::write(&release, b"release").unwrap();
    if heartbeat.is_err() {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    let lines = reader.join().unwrap();
    assert!(heartbeat.is_ok(), "no named heartbeat: {lines:?}");
    assert!(status.success(), "{lines:?}");
}
