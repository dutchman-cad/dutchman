#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use super::*;

const PRIVATE_NAME: &str = "SecretPartName";
const PRIVATE_PATH: &str = "/home/secret-user/Designs/Private.f3z";

fn listed(number: usize, entities: u64, assembly: bool, parent: Option<usize>) -> Listed {
    Listed {
        number,
        parent,
        depth: usize::from(parent.is_some()),
        assembly,
        bodies: 1,
        entities,
        name: format!("{PRIVATE_NAME} {number}"),
    }
}

fn run(outcome: Outcome, time_ms: u64) -> PartRun {
    let mut run = PartRun::simple(outcome);
    run.time_ms = time_ms;
    run.peak_bytes = 3 << 30;
    run
}

fn state(dir: &Path) -> State {
    State {
        out: dir.to_path_buf(),
        input: PathBuf::from(PRIVATE_PATH),
        settings: Settings {
            memory_limit: Some(2 << 30),
            ..Settings::default()
        },
        listed: vec![
            listed(1, 50, false, None),
            listed(2, 9, true, None),
            listed(3, 700, false, Some(2)),
            listed(4, 20, false, Some(2)),
            listed(5, 80, false, None),
            listed(6, 10, false, None),
        ],
        results: BTreeMap::new(),
        read: None,
        read_ms: 1234,
        read_peak: 5 << 30,
        restarts: 1,
        started: Instant::now(),
        written: None,
    }
}

#[test]
fn parts_are_ranked_failures_first_then_slowest_then_biggest() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state(dir.path());
    let mut failed = run(Outcome::Failed, 100);
    failed.stage = Some("write".into());
    failed.kind = Some("restate".into());
    failed.findings.insert("parameter_domain".into(), json!(88));
    state.results.insert(1, run(Outcome::Ok, 5_000));
    state.results.insert(2, run(Outcome::Container, 0));
    state.results.insert(3, run(Outcome::Ok, 5_000));
    state.results.insert(4, failed);
    state.results.insert(5, run(Outcome::TimedOut, 300_000));
    // Part 6 is not run.
    let report = state.report("running");
    let order: Vec<u64> = report["parts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["part"].as_u64().unwrap())
        .collect();
    // Failures (the slowest, the timed out one, first), the part not run, then the parts that
    // converted slowest first with the bigger of two equal times first, then the container.
    assert_eq!(order, [5, 4, 6, 3, 1, 2]);
    assert_eq!(report["parts"][0]["rank"], 1);
    assert_eq!(report["parts"][1]["findings"]["parameter_domain"], 88);
    assert_eq!(report["parts"][1]["stage"], "write");
    assert_eq!(report["summary"]["failed"], 1);
    assert_eq!(report["summary"]["timed_out"], 1);
    assert_eq!(report["summary"]["not_run"], 1);
    assert_eq!(report["schema"], "dutchman.diagnose/1");
    assert_eq!(
        failing_part(&report),
        Some(FailingPart {
            number: 5,
            total: 6,
            outcome: "timed_out".into()
        })
    );
}

#[test]
fn the_report_and_its_text_never_hold_a_name_or_a_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state(dir.path());
    // A failure whose message names the user's file, folder and part, as real ones can.
    let error = wire::error(
        wire::CONVERT,
        wire::input(Path::new(PRIVATE_PATH)),
        &dutchman_core::Error::Encode(format!("{PRIVATE_NAME} 4 at {PRIVATE_PATH} has a bad edge")),
    );
    state.results.insert(4, classify(&error));
    state.results.insert(5, run(Outcome::TimedOut, 1000));
    state.save_key().unwrap();
    state.save("complete", true).unwrap();
    let json = std::fs::read_to_string(dir.path().join("report.json")).unwrap();
    let text = std::fs::read_to_string(dir.path().join("report.txt")).unwrap();
    for shared in [&json, &text] {
        for private in [
            PRIVATE_NAME,
            "secret-user",
            "Private",
            "Designs",
            ".f3z",
            "bad edge",
        ] {
            assert!(!shared.contains(private), "private text in a shared report");
        }
    }
    // The key is the one file that names parts, and it is private.
    let key = std::fs::read_to_string(dir.path().join("local-key.json")).unwrap();
    assert!(key.contains(&format!("{PRIVATE_NAME} 4")) && key.contains("Do not share"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.path().join("local-key.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o077, 0, "{mode:o}");
    }
    let report: Value = serde_json::from_str(&json).unwrap();
    let four = report["parts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["part"] == 4)
        .unwrap();
    assert_eq!(four["error_kind"], "encode");
}

#[test]
fn a_report_is_written_as_the_run_goes() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state(dir.path());
    state.save("running", true).unwrap();
    let report = report_from(dir.path()).unwrap();
    assert_eq!(report["status"], "running");
    assert_eq!(report["summary"]["finished"], 0);
    state.finished(3, run(Outcome::Ok, 10), &|_| {});
    state.save("running", true).unwrap();
    let report = report_from(dir.path()).unwrap();
    assert_eq!(report["summary"]["finished"], 1);
    assert_eq!(report["status"], "running");
    assert!(report_from(&dir.path().join("missing")).is_err());
}

#[test]
fn time_limits_read_as_people_type_them() {
    assert_eq!(parse_duration("90").unwrap(), Duration::from_secs(90));
    assert_eq!(parse_duration("90s").unwrap(), Duration::from_secs(90));
    assert_eq!(parse_duration("5m").unwrap(), Duration::from_secs(300));
    assert_eq!(parse_duration("1.5h").unwrap(), Duration::from_secs(5400));
    assert_eq!(parse_duration("0.5").unwrap(), Duration::from_millis(500));
    for bad in ["", "0", "-3", "abc", "5x", "nan"] {
        assert!(parse_duration(bad).is_err(), "{bad}");
    }
}

#[test]
fn the_text_names_the_part_to_convert_first() {
    let dir = tempfile::tempdir().unwrap();
    let mut state = state(dir.path());
    state.results.insert(5, run(Outcome::OutOfMemory, 9000));
    let text = text(&state.report("running"));
    assert!(
        text.contains("dutchman convert <your file> --part 5"),
        "{text}"
    );
    assert!(text.contains("out_of_memory"), "{text}");
    assert!(text.contains("Safe to share"), "{text}");
}

/// A writer that fails for one part of a design, the way a real writer fails on a body it
/// cannot restate.
struct FailsFor(String);

impl Writer for FailsFor {
    fn extension(&self) -> &'static str {
        "sc"
    }
    fn write(
        &self,
        model: &dutchman_core::Model,
        _out: &mut dyn std::io::Write,
        _report: &mut Report,
    ) -> Result<(), dutchman_core::Error> {
        if model.name == self.0 {
            Err(dutchman_core::Error::Restate(format!(
                "{} cannot be restated",
                model.name
            )))
        } else {
            Ok(())
        }
    }
}

#[cfg(feature = "freecad")]
fn assembly_fixture() -> dutchman_core::Model {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/product_assembly.FCStd");
    dutchman_core::read_in(
        Format::FreeCadDocument,
        &path,
        &dutchman_core::PartSearch::default(),
        &mut Report::default(),
    )
    .unwrap()
}

#[cfg(feature = "freecad")]
#[test]
fn one_failing_part_of_a_multi_part_design_fails_alone() {
    let model = assembly_fixture();
    let catalog = parts::catalog(&model);
    assert!(catalog.len() >= 3, "{catalog:?}");
    let target = catalog
        .iter()
        .find(|p| !p.assembly && p.entities > 0)
        .unwrap();
    let writer = FailsFor(target.name.clone());
    for part in &catalog {
        let run = classify(&convert_part(&model, part.number, &writer));
        if part.name == target.name && part.entities > 0 {
            assert_eq!(run.outcome, Outcome::Failed);
            assert_eq!(run.kind.as_deref(), Some("restate"));
            assert_eq!(run.stage.as_deref(), Some("write"));
            // The failure carries no name.
            assert!(!format!("{run:?}").contains(&target.name));
        } else if part.assembly {
            assert!(matches!(run.outcome, Outcome::Ok | Outcome::Container));
        } else {
            assert!(
                matches!(run.outcome, Outcome::Ok | Outcome::Container),
                "part {}",
                part.number
            );
        }
    }
}

#[cfg(feature = "freecad")]
#[test]
fn a_missing_part_is_refused_with_the_range() {
    let model = assembly_fixture();
    let total = parts::catalog(&model).len();
    let error = parts::select(&model, total + 1, false)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains(&format!("there is no part {}", total + 1)),
        "{error}"
    );
    assert!(error.contains(&format!("numbered 1 to {total}")), "{error}");
    assert!(parts::select(&model, 0, false).is_err());
}

#[cfg(feature = "freecad")]
#[test]
fn a_part_selected_alone_holds_only_its_own_geometry() {
    let model = assembly_fixture();
    let catalog = parts::catalog(&model);
    let leaf = catalog
        .iter()
        .find(|p| !p.assembly && p.entities > 0)
        .unwrap();
    let alone = parts::select(&model, leaf.number, false).unwrap();
    assert!(alone.assembly.is_none());
    assert_eq!(parts::entity_count(&alone), leaf.entities);
}
