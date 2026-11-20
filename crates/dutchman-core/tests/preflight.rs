use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use dutchman_core::preflight::{scan, scan_bytes, Budget, Prediction, Readability, Reason};

// Semantic assertions must not race the production wall-clock ceiling.
fn test_budget() -> Budget {
    Budget {
        max_time: Duration::MAX,
        ..Budget::default()
    }
}

#[test]
fn fixtures_use_only_input_and_build_facts() {
    let folder = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut count = 0;
    let started = Instant::now();
    // Enumerate source inputs explicitly: concurrent conversion tests may create
    // output directories containing logs, reports and copies of native files.
    // Never discover those artifacts by walking the shared fixture directory.
    let inputs = [
        "body_history.sldprt",
        "cadmpeg-techdraw-dimensions.FCStd",
        "cadmpeg-techdraw-variants.FCStd",
        "cadmpeg-techdraw-views.FCStd",
        "container_metadata.sldprt",
        "container_metadata_only.f3d",
        "core_design_product.FCStd",
        "design_copy_paste.f3d",
        "design_history.FCStd",
        "design_sketch_constraints.f3d",
        "freecad-1.1.4-A01/A01a.FCStd",
        "freecad-1.1.4-A01/P01a.FCStd",
        "freecad-1.1.4-A01/P02a.FCStd",
        "freecad-1.1.4-N04/N04a.FCStd",
        "freecad-1.1.4-N04/N04b.FCStd",
        "freecad-1.1.4-P04/P04a.FCStd",
        "generated_unit_cube.f3d",
        "local_cubes.sldasm",
        "nested_cubes.sldasm",
        "onshape-default-plane-regions.onshape-bundle",
        "onshape-derived.onshape-bundle",
        "onshape-hole-forms.onshape-bundle",
        "onshape-hole.onshape-bundle",
        "onshape-plate-fillet.onshape-bundle",
        "onshape-revolve.onshape-bundle",
        "onshape-unsupported.onshape-bundle",
        "placed_cubes.sldasm",
        "product_assembly.FCStd",
        "solidworks_cube.sldprt",
        "synthetic_cube.ipt",
        "techdraw-pages.FCStd",
        "techdraw.FCStd",
    ];
    for input in inputs {
        let path = folder.join(input);
        let result = scan(&path, test_budget());
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
        assert!(result.bytes_read <= test_budget().max_bytes);
        let bytes = std::fs::read(&path).unwrap();
        let memory = scan_bytes(&path, &bytes, test_budget());
        if result.reason != Reason::TimeBudget && memory.reason != Reason::TimeBudget {
            assert_eq!(result.detected_codec, memory.detected_codec);
        }
        count += 1;
    }
    assert!(count >= 10);
    println!("Scanned {count} fixtures in {:?}", started.elapsed());
}

#[test]
fn recognised_suffix_refusals_follow_build_features() {
    for (name, enabled) in [
        ("part.FCStd", cfg!(feature = "freecad")),
        ("part.onshape-bundle", cfg!(feature = "onshape")),
    ] {
        let result = scan_bytes(
            Path::new(name),
            b"not decoded by a prefix scan",
            test_budget(),
        );
        assert_eq!(
            result.reason,
            if enabled {
                Reason::ReaderUnavailable
            } else {
                Reason::FormatNotSupported
            },
            "{name}"
        );
        assert_eq!(
            result.predict("test-writer").outcome,
            if enabled {
                Prediction::Unknown
            } else {
                Prediction::Refused
            },
            "{name}"
        );
    }
}

#[test]
fn an_unrecognised_suffix_and_inconclusive_prefix_remain_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("dutchman.log");
    let bytes = b"Conversion completed.\n";
    std::fs::write(&path, bytes).unwrap();
    for result in [
        scan(&path, test_budget()),
        scan_bytes(&path, bytes, test_budget()),
    ] {
        assert_eq!(result.format_hint, None);
        assert_eq!(result.detected_codec, None);
        assert_eq!(result.reason, Reason::ReaderUnavailable);
        assert_eq!(result.readable, Readability::Unknown);
        assert_eq!(result.predict("test-writer").outcome, Prediction::Unknown);
    }
}

#[test]
fn zero_and_short_budgets_do_not_claim_readability() {
    for (budget, reason) in [
        (
            Budget {
                max_bytes: 0,
                ..test_budget()
            },
            Reason::ByteBudget,
        ),
        (
            Budget {
                max_time: Duration::ZERO,
                ..test_budget()
            },
            Reason::TimeBudget,
        ),
        (
            Budget {
                max_features: 0,
                ..test_budget()
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
            ..test_budget()
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
        let result = scan_bytes(Path::new("hostile.prt.12"), &bytes, test_budget());
        assert_eq!(result.readable, Readability::Unknown);
        assert!(result.bytes_read <= test_budget().max_bytes);
        assert_eq!(result.predict("test-writer").outcome, Prediction::Unknown);
    }
}

#[test]
fn definite_input_refusals_are_distinct_from_unknown() {
    let dir = tempfile::tempdir().unwrap();
    let missing = scan(&dir.path().join("missing.f3d"), test_budget());
    assert_eq!(missing.reason, Reason::InputUnavailable);
    assert_eq!(missing.predict("test-writer").outcome, Prediction::Refused);
    assert_eq!(scan(dir.path(), test_budget()).reason, Reason::NotAFile);
    let path = dir.path().join("empty.f3d");
    std::fs::write(&path, []).unwrap();
    assert_eq!(scan(&path, test_budget()).reason, Reason::EmptyInput);
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(dutchman_core::MAX_INPUT_BYTES + 1).unwrap();
    let result = scan(&path, test_budget());
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
            ..test_budget()
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
        scan(&path, test_budget()),
        scan_bytes(&path, bytes, test_budget()),
    ] {
        assert_eq!(result.reason, Reason::FormatNotSupported);
        assert_eq!(result.readable, Readability::NotReadable);
        assert_eq!(result.predict("test-writer").outcome, Prediction::Refused);
        assert_eq!(result.bytes_read, 0);
    }
    // Shared suffixes can identify an available reader after content detection.
    let shared = scan_bytes(Path::new("part.prt.12"), bytes, test_budget());
    assert_eq!(shared.predict("test-writer").outcome, Prediction::Unknown);
}

#[test]
fn an_expired_planning_deadline_stops_before_opening_the_input() {
    let budget = Budget {
        max_time: Duration::from_millis(1),
        ..test_budget()
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
    let budget = test_budget();
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
