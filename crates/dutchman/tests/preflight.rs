use std::path::PathBuf;
use std::process::Command;

use dutchman::preflight::{preflight, Tier};
use dutchman_core::preflight::{scan, Budget, Prediction, Reason};
use std::time::{Duration, Instant};

// Semantic assertions must not race the production wall-clock ceiling.
fn test_budget() -> Budget {
    Budget {
        max_time: Duration::MAX,
        ..Budget::default()
    }
}

#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
fn assert_cli_refusal_or_timeout(output: &std::process::Output, reason: &str) -> serde_json::Value {
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    if value["scan"]["reason"] == "time_budget" {
        assert_eq!(output.status.code(), Some(0));
        assert_eq!(value["scan"]["readable"], "unknown");
        assert!(value["targets"]
            .as_array()
            .unwrap()
            .iter()
            .all(|target| target["outcome"] == "unknown"));
    } else {
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(value["scan"]["reason"], reason);
        assert_eq!(value["scan"]["readable"], "not_readable");
    }
    value
}

#[test]
fn cli_exposes_advisory_json_and_refusals() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("part.f3d");
    std::fs::write(&path, b"PK\x03\x04").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("preflight")
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    let value = assert_cli_refusal_or_timeout(&output, "decode_failed");
    assert_eq!(value["schema"], "dutchman.preflight/1");
    assert_eq!(value["targets"].as_array().unwrap().len(), 4);
    assert!(value["scan"]["feature_count"].is_null());
    std::fs::write(&path, []).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("preflight")
        .arg(&path)
        .arg("--json")
        .output()
        .unwrap();
    assert_cli_refusal_or_timeout(&output, "empty_input");
}

#[test]
fn predictions_are_compared_with_conversion_for_every_native_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = tempfile::tempdir().unwrap();
    let options = dutchman::ConvertOptions {
        no_verify: true,
        solidcraft_version: Some(Default::default()),
        ..Default::default()
    };
    let mut folders = vec![
        root.join("crates/dutchman-core/tests/fixtures"),
        root.join("crates/dutchman-solidcraft/tests/fixtures"),
    ];
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
    let mut times = Vec::new();
    let mut conversions = Vec::new();
    let mut categories = std::collections::BTreeMap::<String, (usize, usize, usize)>::new();
    println!(
        "| Row | Fixture | Prediction | Conversion | Agreement | Preflight ms | Conversion ms |"
    );
    let mut count = 0;
    for path in files {
        if dutchman_core::Format::from_path(&path).is_none() {
            continue;
        }
        let started = Instant::now();
        let forecast = preflight(&path, test_budget());
        let elapsed = started.elapsed().as_secs_f64() * 1000.;
        let prediction = forecast
            .targets
            .iter()
            .find(|target| target.target == "sc")
            .unwrap();
        let started = Instant::now();
        let result =
            dutchman_core::convert(&path, &dir.path().join("result.sc"), &options.writer());
        let conversion_ms = started.elapsed().as_secs_f64() * 1000.;
        let mut actual_mapped = None;
        let mut actual_details = String::new();
        let actual = match result {
            Ok(report) => {
                actual_details = format!("{:?}", report.outcome());
                actual_mapped = report.outcome().map(|outcome| {
                    outcome
                        .features
                        .iter()
                        .filter(|feature| {
                            matches!(
                                feature.status,
                                dutchman_core::FeatureStatus::Mapped
                                    | dutchman_core::FeatureStatus::NotNeeded
                            )
                        })
                        .count()
                });
                match report.outcome().map(|outcome| outcome.mode) {
                    Some(dutchman_core::Mode::History) => Prediction::FullHistory,
                    Some(dutchman_core::Mode::PartialHistory) => Prediction::Partial,
                    Some(dutchman_core::Mode::Import) => Prediction::SolidOnly,
                    None => panic!("conversion has no mode: {}", path.display()),
                }
            }
            Err(_) => Prediction::Refused,
        };
        let rank = |prediction| match prediction {
            Prediction::FullHistory => 3,
            Prediction::Partial => 2,
            Prediction::SolidOnly => 1,
            Prediction::Refused => 0,
            Prediction::Unknown => -1,
        };
        assert!(
            rank(prediction.outcome) <= rank(actual),
            "optimistic forecast for {}: {:?} > {actual:?}",
            path.display(),
            prediction.outcome
        );
        if matches!(
            prediction.outcome,
            Prediction::FullHistory | Prediction::Partial
        ) {
            if prediction.mapped_features.unwrap() > actual_mapped.unwrap() {
                let mut report = dutchman_core::Report::default();
                let model = dutchman_core::read(
                    dutchman_core::Format::from_path(&path).unwrap(),
                    &path,
                    &mut report,
                )
                .unwrap();
                let full_plan = dutchman_solidcraft::SolidCraftWriter::new()
                    .verification(dutchman_solidcraft::Verification::Off)
                    .target_version(Some(Default::default()))
                    .plan(&model, &mut report);
                eprintln!("Forecast: {prediction:?}\nConversion: {actual_details}\nFull-read plan: {full_plan:?}");
            }
            assert!(
                prediction.mapped_features.unwrap() <= actual_mapped.unwrap(),
                "optimistic feature count for {}",
                path.display()
            );
        }
        assert_ne!(
            prediction.outcome,
            Prediction::Unknown,
            "fixture forecast abstained: {}: {:?}",
            path.display(),
            forecast.stop_reason
        );
        let agreement = prediction.outcome == actual;
        let category = categories.entry(format!("{actual:?}")).or_default();
        category.0 += 1;
        category.1 += usize::from(agreement);
        category.2 += usize::from(prediction.outcome == Prediction::Unknown);
        if prediction.outcome == Prediction::Partial {
            assert!(prediction.mapped_features.unwrap() < prediction.total_features.unwrap());
            assert!(prediction.first_blocking_feature.is_some());
        }
        count += 1;
        println!("| {count:02} | {} | {:?} | {actual:?} | {agreement} | {elapsed:.3} | {conversion_ms:.3} |", path.strip_prefix(&root).unwrap().display(), prediction.outcome);
        times.push(elapsed);
        conversions.push(conversion_ms);
    }
    assert!(count >= 36);
    for (category, (total, agreed, unknown)) in categories {
        println!("Category {category}: {agreed}/{total} agree, {unknown} unknown");
    }
    for (label, values) in [("preflight", &mut times), ("conversion", &mut conversions)] {
        values.sort_by(f64::total_cmp);
        let middle = values.len() / 2;
        let median = if values.len() % 2 == 0 {
            (values[middle - 1] + values[middle]) / 2.
        } else {
            values[middle]
        };
        println!(
            "{label}: median {median:.3} ms; worst {:.3} ms",
            values.last().unwrap()
        );
    }
}

#[test]
fn planning_budgets_discard_forecasts_and_never_write_output() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/onshape-plate-fillet.onshape-bundle");
    let budget = test_budget();
    let planned = preflight(&path, budget);
    assert_eq!(planned.tier, Tier::Planned);
    assert_eq!(planned.targets[0].outcome, Prediction::Partial);
    for (budget, reason) in [
        (
            Budget {
                max_time: Duration::ZERO,
                ..budget
            },
            Reason::TimeBudget,
        ),
        (
            Budget {
                max_bytes: 1,
                ..budget
            },
            Reason::ByteBudget,
        ),
        (
            Budget {
                max_features: 1,
                ..budget
            },
            Reason::FeatureBudget,
        ),
        // Enough for the prefix, insufficient for the separate decode read.
        (
            Budget {
                max_bytes: std::fs::metadata(&path).unwrap().len(),
                ..budget
            },
            Reason::ByteBudget,
        ),
    ] {
        let forecast = preflight(&path, budget);
        assert_eq!(forecast.tier, Tier::Prefix);
        assert_eq!(forecast.stop_reason, Some(reason));
        assert!(forecast
            .targets
            .iter()
            .all(|target| target.outcome == Prediction::Unknown));
    }
    let limited = preflight(
        &path,
        Budget {
            max_bytes: std::fs::metadata(&path).unwrap().len() * 2,
            ..budget
        },
    );
    assert_eq!(limited.stop_reason, Some(Reason::ResourceLimit));
    assert!(limited
        .targets
        .iter()
        .all(|target| target.outcome == Prediction::Unknown));
    let dir = tempfile::tempdir().unwrap();
    let hostile = dir.path().join("hostile.f3d");
    std::fs::write(&hostile, [255; 100_000]).unwrap();
    let forecast = preflight(&hostile, budget);
    assert_eq!(forecast.targets[0].outcome, Prediction::Refused);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    let shared = dir.path().join("unknown.prt");
    std::fs::rename(&hostile, &shared).unwrap();
    let forecast = preflight(&shared, budget);
    assert_eq!(forecast.scan.reason, Reason::ReaderUnavailable);
    assert!(forecast
        .targets
        .iter()
        .all(|target| target.outcome == Prediction::Unknown));
}

#[test]
fn freecad_planning_matches_the_build_feature() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/freecad-1.1.4-A01/P01a.FCStd");
    let forecast = preflight(&path, test_budget());
    if cfg!(feature = "freecad") {
        assert_eq!(forecast.tier, Tier::Planned);
        assert_eq!(forecast.targets[0].outcome, Prediction::FullHistory);
    } else {
        assert_eq!(forecast.tier, Tier::Prefix);
        assert_eq!(forecast.scan.reason, Reason::FormatNotSupported);
        assert!(forecast
            .targets
            .iter()
            .all(|target| target.outcome == Prediction::Refused));
    }
}

#[test]
fn definite_refusals_agree_with_conversion() {
    let dir = tempfile::tempdir().unwrap();
    let options = dutchman::ConvertOptions {
        no_verify: true,
        solidcraft_version: Some(Default::default()),
        ..Default::default()
    };
    for format in dutchman_core::Format::ALL {
        let input = dir.path().join(format!("empty.{}", format.extension()));
        std::fs::write(&input, []).unwrap();
        let prediction = scan(&input, test_budget()).predict("sc");
        assert_eq!(prediction.outcome, Prediction::Refused);
        let result =
            dutchman_core::convert(&input, &dir.path().join("result.sc"), &options.writer());
        assert!(result.is_err(), "{}", format.id());
    }
}

#[test]
fn an_unsupported_suffix_is_refused_before_decoding() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("part.iges");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
    std::fs::copy(fixture, &input).unwrap();
    let prediction = scan(&input, test_budget());
    assert_eq!(
        prediction.reason,
        dutchman_core::preflight::Reason::FormatNotSupported
    );
    assert_eq!(prediction.predict("sc").outcome, Prediction::Refused);
    let options = dutchman::ConvertOptions {
        no_verify: true,
        solidcraft_version: Some(Default::default()),
        ..Default::default()
    };
    assert!(
        dutchman_core::convert(&input, &dir.path().join("result.sc"), &options.writer()).is_err()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_dutchman"))
        .arg("preflight")
        .arg(&input)
        .arg("--json")
        .output()
        .unwrap();
    assert_cli_refusal_or_timeout(&output, "format_not_supported");
    let shared = dir.path().join("part.prt");
    std::fs::copy(&input, &shared).unwrap();
    let forecast = preflight(&shared, test_budget());
    assert!(matches!(
        forecast.targets[0].outcome,
        Prediction::Refused | Prediction::Unknown
    ));
    if forecast.scan.detected_codec.is_some() {
        assert_eq!(forecast.scan.reason, Reason::FormatMismatch);
        assert_eq!(forecast.targets[0].outcome, Prediction::Refused);
    }
    assert!(
        dutchman_core::convert(&shared, &dir.path().join("result.sc"), &options.writer()).is_err()
    );
}
