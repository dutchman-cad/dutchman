use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dutchman_core::preflight::{scan, scan_bytes, Budget, Prediction, Readability, Reason};

#[test]
fn fixtures_use_only_input_and_build_facts() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut count = 0;
    let started = Instant::now();
    let mut folders = vec![folder.clone()];
    let mut files = Vec::new();
    while let Some(current) = folders.pop() {
        for entry in std::fs::read_dir(current).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files.sort();
    for path in files {
        if !path.is_file()
            || path
                .extension()
                .is_none_or(|ext| ext == "md" || ext == "py" || ext == "rs")
        {
            continue;
        }
        let result = scan(&path, Budget::default());
        let available =
            dutchman_core::Format::from_path(&path).is_some_and(dutchman_core::Format::can_convert);
        if available {
            assert_eq!(result.readable, Readability::Unknown, "{path:?}");
            assert_eq!(result.predict("test-writer").outcome, Prediction::Unknown);
        } else {
            assert_eq!(result.reason, Reason::FormatNotSupported, "{path:?}");
            assert_eq!(result.predict("test-writer").outcome, Prediction::Refused);
        }
        assert!(result.feature_count.is_none());
        assert!(result.feature_kinds.is_none());
        assert!(result.bytes_read <= Budget::default().max_bytes);
        let bytes = std::fs::read(&path).unwrap();
        let memory = scan_bytes(&path, &bytes, Budget::default());
        if result.reason != Reason::TimeBudget && memory.reason != Reason::TimeBudget {
            assert_eq!(result.detected_codec, memory.detected_codec);
        }
        count += 1;
    }
    assert!(count >= 10);
    println!("Scanned {count} fixtures in {:?}", started.elapsed());
}

#[test]
fn zero_and_short_budgets_do_not_claim_readability() {
    for (budget, reason) in [
        (
            Budget {
                max_bytes: 0,
                ..Budget::default()
            },
            Reason::ByteBudget,
        ),
        (
            Budget {
                max_time: Duration::ZERO,
                ..Budget::default()
            },
            Reason::TimeBudget,
        ),
        (
            Budget {
                max_features: 0,
                ..Budget::default()
            },
            Reason::FeatureBudget,
        ),
    ] {
        let result = scan_bytes(Path::new("part.f3d"), b"anything", budget);
        assert_eq!(result.reason, reason);
        assert_eq!(result.bytes_read, 0);
        assert_eq!(result.readable, Readability::Unknown);
        // An exhausted budget must not even try opening a missing path.
        assert_eq!(scan(Path::new("missing.f3d"), budget).reason, reason);
    }
    let result = scan_bytes(
        Path::new("part.f3d"),
        &[42; 1024],
        Budget {
            max_bytes: 1,
            ..Budget::default()
        },
    );
    assert_eq!(result.reason, Reason::ByteBudget);
    assert_eq!(result.bytes_read, 1);
}

#[test]
fn hostile_prefixes_are_bounded_and_never_optimistic() {
    for bytes in [
        vec![0; 100_000],
        vec![255; 100_000],
        b"PK\x03\x04".to_vec(),
        b"{\"features\":999999999999999999999}".to_vec(),
    ] {
        let result = scan_bytes(Path::new("hostile.prt.12"), &bytes, Budget::default());
        assert_eq!(result.readable, Readability::Unknown);
        assert!(result.bytes_read <= Budget::default().max_bytes);
        assert_eq!(result.predict("test-writer").outcome, Prediction::Unknown);
    }
}

#[test]
fn definite_input_refusals_are_distinct_from_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let missing = scan(&dir.path().join("missing.f3d"), Budget::default());
    assert_eq!(missing.reason, Reason::InputUnavailable);
    assert_eq!(missing.predict("test-writer").outcome, Prediction::Refused);
    assert_eq!(scan(dir.path(), Budget::default()).reason, Reason::NotAFile);
    let path = dir.path().join("empty.f3d");
    std::fs::write(&path, []).unwrap();
    assert_eq!(scan(&path, Budget::default()).reason, Reason::EmptyInput);
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(dutchman_core::MAX_INPUT_BYTES + 1).unwrap();
    let result = scan(&path, Budget::default());
    assert_eq!(result.reason, Reason::InputTooLarge);
    assert_eq!(result.bytes_read, 0);
}

#[test]
fn oversized_caller_budgets_still_use_the_detector_ceiling() {
    let bytes = vec![0; cadmpeg_registry::DETECTION_PREFIX_LEN * 2];
    let result = scan_bytes(
        Path::new("hostile.prt"),
        &bytes,
        Budget {
            max_bytes: u64::MAX,
            max_features: usize::MAX,
            ..Budget::default()
        },
    );
    assert_eq!(
        result.bytes_read,
        cadmpeg_registry::DETECTION_PREFIX_LEN as u64
    );
    assert_eq!(result.readable, Readability::Unknown);
}

#[test]
fn known_suffix_refusals_match_the_conversion_gate_without_reading_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("unsupported.iges");
    let bytes = b"This payload is never decoded";
    std::fs::write(&path, bytes).unwrap();
    for result in [
        scan(&path, Budget::default()),
        scan_bytes(&path, bytes, Budget::default()),
    ] {
        assert_eq!(result.reason, Reason::FormatNotSupported);
        assert_eq!(result.readable, Readability::NotReadable);
        assert_eq!(result.predict("test-writer").outcome, Prediction::Refused);
        assert_eq!(result.bytes_read, 0);
    }
    // Shared suffixes can identify an available reader after content detection.
    let shared = scan_bytes(Path::new("part.prt.12"), bytes, Budget::default());
    assert_eq!(shared.predict("test-writer").outcome, Prediction::Unknown);
}

#[test]
fn an_expired_planning_deadline_stops_before_opening_the_input() {
    let budget = Budget {
        max_time: Duration::from_millis(1),
        ..Budget::default()
    };
    let started = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    let prepared =
        dutchman_core::preflight::prepare(Path::new("missing-preflight-part.f3d"), budget, started);
    assert_eq!(prepared.stop_reason, Some(Reason::TimeBudget));
    assert_eq!(prepared.scan.reason, Reason::TimeBudget);
    assert_eq!(prepared.scan.readable, Readability::Unknown);
    assert_eq!(prepared.scan.bytes_read, 0);
    assert!(prepared.model.is_none());
}

#[cfg(feature = "onshape")]
#[test]
fn onshape_planning_uses_the_bundle_reader_and_preserves_budget_unknowns() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/onshape-plate-fillet.onshape-bundle");
    let budget = Budget {
        max_time: Duration::from_secs(30),
        ..Budget::default()
    };
    let prepared = dutchman_core::preflight::prepare(&path, budget, Instant::now());
    assert_eq!(prepared.scan.readable, Readability::Readable);
    assert!(prepared.model.is_some());
    assert!(prepared.scan.feature_count.unwrap() > 1);
    for (budget, reason) in [
        (
            Budget {
                max_features: 1,
                ..budget
            },
            Reason::FeatureBudget,
        ),
        (
            Budget {
                max_bytes: std::fs::metadata(&path).unwrap().len() * 2,
                ..budget
            },
            Reason::ResourceLimit,
        ),
    ] {
        let prepared = dutchman_core::preflight::prepare(&path, budget, Instant::now());
        assert_eq!(prepared.stop_reason, Some(reason));
        assert_eq!(prepared.scan.readable, Readability::Unknown);
        assert!(prepared.model.is_none());
    }
}
