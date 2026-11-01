#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::cell::{Cell, RefCell};
use std::sync::Mutex;

use dutchman_core::{Assembly, Model, Placement};

use super::*;

/// A design in numbers: which part fails, from which feature of its history.
#[derive(Clone)]
struct Fake {
    parts: usize,
    failing: Vec<usize>,
    first_feature: usize,
    features: usize,
    /// The part fails only beside its neighbours.
    only_together: bool,
    /// The part converts when tried again: the ranking's failure does not repeat.
    never_again: bool,
    tried: Vec<Trial>,
}

impl Fake {
    fn design() -> Self {
        Fake {
            parts: 30,
            failing: vec![17],
            first_feature: 41,
            features: 80,
            only_together: false,
            never_again: false,
            tried: Vec::new(),
        }
    }

    fn fails(&self, trial: Trial) -> bool {
        if self.never_again {
            return false;
        }
        let Some(part) = trial.part else {
            return false;
        };
        self.failing.contains(&part)
            && (!self.only_together || trial.scope == Scope::Neighbours)
            && trial.stop_at.unwrap_or(self.features) >= self.first_feature
    }
}

fn restate() -> Failure {
    let mut run = PartRun::simple(Outcome::Failed);
    run.stage = Some("write".to_owned());
    run.kind = Some("restate".to_owned());
    Failure::from_run(&run).unwrap()
}

impl Trials for Fake {
    fn rank(&mut self, on_progress: &(dyn Fn(Progress) + Sync)) -> Result<Value, String> {
        on_progress(Progress {
            step: 1,
            text: "ranking the parts (25 of 30)".to_owned(),
        });
        let rows: Vec<Value> = (1..=self.parts)
            .map(|part| {
                let failed = self.failing.contains(&part);
                json!({"part": part, "outcome": if failed { "failed" } else { "ok" },
                    "stage": failed.then_some("write"), "error_kind": failed.then_some("restate"),
                    "findings": {}, "curve_kinds": {}})
            })
            .collect();
        Ok(json!({"status": "complete", "summary": {"parts": self.parts}, "parts": rows}))
    }

    fn attempt(&mut self, trial: Trial, _limit: &StepLimit) -> Result<Attempt, String> {
        self.tried.push(trial);
        Ok(Attempt {
            failure: self.fails(trial).then(restate),
            features: self.features,
        })
    }

    fn describe(
        &mut self,
        _part: usize,
        _feature: usize,
        _limit: &StepLimit,
    ) -> Result<Option<FeatureShape>, String> {
        Ok(Some(FeatureShape {
            kind: "extrude".to_owned(),
            ..FeatureShape::default()
        }))
    }
}

fn settings() -> NarrowSettings {
    NarrowSettings::new(
        Settings {
            memory_limit: None,
            ..Settings::default()
        },
        None,
    )
}

fn fresh() -> Checkpoint {
    Checkpoint {
        schema: STATE_SCHEMA.to_owned(),
        step: 1,
        ..Checkpoint::default()
    }
}

/// Runs the steps to the end and returns the state and what was said.
fn narrowed(fake: &mut Fake, settings: &NarrowSettings) -> (Checkpoint, Vec<String>) {
    let mut state = fresh();
    let said = Mutex::new(Vec::new());
    let on_progress = |progress: Progress| said.lock().unwrap().push(progress.to_string());
    Drive {
        state: &mut state,
        trials: fake,
        settings,
        save: &|_| Ok(()),
        on_progress: &on_progress,
        step_started: Instant::now(),
        spent_before: 0,
    }
    .run()
    .unwrap();
    (state, said.into_inner().unwrap())
}

#[test]
fn one_feature_of_one_part_of_an_assembly_is_found_in_a_handful_of_trials() {
    let mut fake = Fake::design();
    let (state, said) = narrowed(&mut fake, &settings());
    assert_eq!(status(&state), "narrowed");
    let target = state.target.as_ref().unwrap();
    assert_eq!(target.part, Some(17));
    assert_eq!(target.scope, Scope::Alone);
    assert_eq!(state.first_failing, Some(41));
    // Alone, no features, then a bisection of 80.
    assert!(fake.tried.len() <= 2 + 7, "{}", fake.tried.len());
    assert!(
        said.contains(&"step 3 of 5: narrowing part 17 (feature 41 of 80)".to_owned()),
        "{said:?}"
    );
    assert!(said.iter().any(|line| line.starts_with("step 1 of 5:")));
    assert!(said.iter().any(|line| line.starts_with("step 2 of 5:")));
    assert!(said.iter().any(|line| line.starts_with("step 4 of 5:")));
    let signature = state.signature.as_ref().unwrap();
    assert_eq!(signature.error.kind.as_deref(), Some("restate"));
    assert_eq!(signature.feature.as_ref().unwrap().kind, "extrude");
    // The result the feature is claimed from: N fails, N - 1 does not.
    let found = |stop_at| {
        state
            .trials
            .iter()
            .find(|t| t.stop_at == Some(stop_at))
            .unwrap()
            .failed
    };
    assert!(found(41));
    assert!(!found(40));
    let text = summary(&findings(&state), &state);
    assert!(
        text.contains("part 17, first at feature 41 of 80"),
        "{text}"
    );
}

/// Stops at every checkpoint in turn, the way a crash would, reads the state back from its JSON
/// and carries on. Each run must come to the same answer, without repeating finished trials.
#[test]
fn a_run_stopped_at_any_checkpoint_resumes_to_the_same_answer() {
    let mut whole = Fake::design();
    let (expected, _) = narrowed(&mut whole, &settings());
    let settings = settings();
    let mut stops = 0;
    for stop_after in 1.. {
        let saved: RefCell<String> = RefCell::new(String::new());
        let saves = Cell::new(0);
        let save = |state: &Checkpoint| {
            saves.set(saves.get() + 1);
            *saved.borrow_mut() = serde_json::to_string(state).unwrap();
            if saves.get() == stop_after {
                Err("stopped".to_owned())
            } else {
                Ok(())
            }
        };
        let mut first = Fake::design();
        let mut state = fresh();
        let stopped = Drive {
            state: &mut state,
            trials: &mut first,
            settings: &settings,
            save: &save,
            on_progress: &|_| {},
            step_started: Instant::now(),
            spent_before: 0,
        }
        .run();
        if stopped.is_ok() {
            break;
        }
        stops += 1;
        let mut state: Checkpoint = serde_json::from_str(&saved.borrow()).unwrap();
        let done = state.trials.len();
        let mut second = Fake::design();
        Drive {
            state: &mut state,
            trials: &mut second,
            settings: &settings,
            save: &|_| Ok(()),
            on_progress: &|_| {},
            step_started: Instant::now(),
            spent_before: 0,
        }
        .run()
        .unwrap();
        assert_eq!(state.first_failing, expected.first_failing, "{stop_after}");
        assert_eq!(state.signature, expected.signature, "{stop_after}");
        assert_eq!(state.trials.len(), expected.trials.len(), "{stop_after}");
        assert_eq!(second.tried.len(), expected.trials.len() - done);
    }
    assert!(stops >= 6, "{stops}");
}

#[test]
fn a_state_file_is_refused_for_another_design_and_holds_no_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(STATE_FILE);
    let first = dir.path().join("Secret Design.f3d");
    std::fs::write(&first, b"one").unwrap();
    let other = dir.path().join("other.f3d");
    std::fs::write(&other, b"two").unwrap();
    let file = Fingerprint::of(&first).unwrap();
    assert_eq!(file.size, 3);
    let mut state = load_state(&path, &file).unwrap();
    state.step = 3;
    write_file(&path, &serde_json::to_vec(&state).unwrap(), true).unwrap();
    assert_eq!(load_state(&path, &file).unwrap().step, 3);
    let refused = load_state(&path, &Fingerprint::of(&other).unwrap()).unwrap_err();
    assert!(refused.contains("another file"), "{refused}");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("Secret"), "{text}");
    std::fs::write(&path, b"not json").unwrap();
    assert!(load_state(&path, &file).is_err());
}

#[test]
fn a_part_that_fails_only_beside_its_neighbours_is_narrowed_there() {
    let mut fake = Fake {
        only_together: true,
        ..Fake::design()
    };
    let (state, _) = narrowed(&mut fake, &settings());
    assert_eq!(state.target.as_ref().unwrap().scope, Scope::Neighbours);
    assert_eq!(state.first_failing, Some(41));
    assert!(fake.tried[2..]
        .iter()
        .all(|trial| trial.scope == Scope::Neighbours));
    let text = summary(&findings(&state), &state);
    assert!(text.contains("only with its neighbours"), "{text}");
}

#[test]
fn a_failure_with_no_history_is_in_the_stored_shape_and_names_no_feature() {
    let mut fake = Fake {
        first_feature: 0,
        ..Fake::design()
    };
    let (state, _) = narrowed(&mut fake, &settings());
    assert!(state.stored_shape);
    assert_eq!(state.first_failing, None);
    assert_eq!(state.signature.as_ref().unwrap().feature, None);
    let text = summary(&findings(&state), &state);
    assert!(text.contains("stored shape"), "{text}");
}

#[test]
fn a_part_with_no_history_stops_at_the_part() {
    let mut fake = Fake {
        features: 0,
        first_feature: 0,
        ..Fake::design()
    };
    let (state, _) = narrowed(&mut fake, &settings());
    assert_eq!(state.target.as_ref().unwrap().part, Some(17));
    assert_eq!(state.signature.as_ref().unwrap().feature, None);
    assert!(fake.tried.iter().all(|trial| trial.stop_at.is_none()));
}

#[test]
fn a_failure_that_does_not_repeat_is_said_to_and_gets_no_signature() {
    let mut fake = Fake {
        failing: vec![3, 9],
        never_again: true,
        ..Fake::design()
    };
    let (state, _) = narrowed(&mut fake, &settings());
    assert_eq!(status(&state), "not_repeated");
    assert_eq!(state.signature, None);
    // Each failing part, alone and with its neighbours.
    assert_eq!(fake.tried.len(), 4);
}

#[test]
fn every_part_converting_tries_the_whole_design() {
    let mut fake = Fake {
        failing: Vec::new(),
        ..Fake::design()
    };
    let (state, _) = narrowed(&mut fake, &settings());
    assert_eq!(status(&state), "nothing_to_narrow");
    assert_eq!(fake.tried.len(), 1);
    assert_eq!(fake.tried[0].scope, Scope::Design);
}

#[test]
fn a_step_past_its_budget_stops_and_keeps_what_it_knows() {
    let mut limits = settings();
    limits.bisect.budget = Some(Duration::ZERO);
    let mut fake = Fake::design();
    let (state, _) = narrowed(&mut fake, &limits);
    assert_eq!(status(&state), "stopped");
    assert_eq!(state.stopped_at_step, Some(3));
    assert_eq!(state.first_failing, None);
    assert_eq!(state.target.as_ref().unwrap().part, Some(17));
    assert_eq!(state.signature.as_ref().unwrap().feature, None);
    assert_eq!(findings(&state)["bracket"]["fails"], 80);
}

#[test]
fn each_step_has_its_own_limits() {
    let settings = NarrowSettings::new(
        Settings {
            time_limit: Duration::from_secs(7),
            memory_limit: Some(1 << 30),
            ..Settings::default()
        },
        Some(Duration::from_secs(60)),
    );
    assert_eq!(settings.isolate.time, Duration::from_secs(7));
    assert_eq!(settings.bisect.memory, Some(1 << 30));
    assert_eq!(settings.bisect.budget, Some(Duration::from_secs(60)));
    assert_eq!(settings.describe.time, DESCRIBE_LIMIT);
    assert_eq!(settings.describe.budget, None);
}

// ---- a real model, in memory ----

/// The synthetic unit cube with `features` rows of history, named `name`.
fn part(name: &str, features: u64) -> Model {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
    let mut model =
        dutchman_core::read(Format::FusionArchive, &path, &mut Report::default()).unwrap();
    let mut ir = serde_json::to_value(&model.ir).unwrap();
    let rows: Vec<Value> = (1..=features)
        .map(|at| {
            json!({
                "id": format!("f3d:model:feature#{}", at + 1000),
                "ordinal": at,
                "name": format!("SecretFeature{at}"),
                "suppressed": false,
                "definition": {"definition": "native", "kind": "Mystery", "parameters": {}},
            })
        })
        .collect();
    ir["model"]["features"] = Value::Array(rows);
    model.ir = serde_json::from_value(ir).unwrap();
    model.name = name.to_owned();
    model
}

/// An assembly of `names`, each with 8 features.
fn assembly(names: &[&str]) -> Model {
    let mut model = part("SecretAssembly", 0);
    let parts: Vec<Model> = names.iter().map(|name| part(name, 8)).collect();
    let placements = (0..parts.len())
        .map(|at| Placement {
            name: None,
            part: at,
            transform: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            hidden: false,
        })
        .collect();
    model.assembly = Some(Assembly {
        own_part: None,
        parts,
        definitions: Vec::new(),
        identities: Vec::new(),
        placements,
        joints: Vec::new(),
        left_out: Vec::new(),
        held: Vec::new(),
        joint_notes: Vec::new(),
        stored_placements: Vec::new(),
    });
    model
}

/// A writer that fails on one part once its history reaches a feature, as a mapper would.
struct FailsAt {
    name: &'static str,
    feature: usize,
}

impl Writer for FailsAt {
    fn extension(&self) -> &'static str {
        "sc"
    }

    fn write(
        &self,
        model: &Model,
        _out: &mut dyn std::io::Write,
        _report: &mut Report,
    ) -> Result<(), dutchman_core::Error> {
        let part =
            |model: &Model| model.name == self.name && parts::history_length(model) >= self.feature;
        let fails = part(model)
            || model
                .assembly
                .as_ref()
                .is_some_and(|assembly| assembly.parts.iter().any(part));
        if fails {
            Err(dutchman_core::Error::Restate(format!(
                "{} at /home/secret/{} cannot be restated",
                model.name, model.name
            )))
        } else {
            Ok(())
        }
    }
}

/// The trials, run on a model in this process.
struct InMemory {
    model: Model,
    writer: FailsAt,
}

impl Trials for InMemory {
    fn rank(&mut self, _on_progress: &(dyn Fn(Progress) + Sync)) -> Result<Value, String> {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::default();
        let mut state = State::new(dir.path().to_path_buf(), Path::new("x.f3d"), &settings);
        for part in parts::catalog(&self.model) {
            state.listed.push(Listed {
                number: part.number,
                parent: part.parent,
                depth: part.depth,
                assembly: part.assembly,
                bodies: part.bodies,
                entities: part.entities,
                name: part.name.clone(),
            });
            let error = convert_part(&self.model, part.number, &self.writer);
            state.results.insert(part.number, classify(&error));
        }
        Ok(state.report("complete"))
    }

    fn attempt(&mut self, trial: Trial, _limit: &StepLimit) -> Result<Attempt, String> {
        let (selected, features) =
            trial_model(&self.model, trial.part, trial.scope.id(), trial.stop_at)?;
        let error = trial_error(&selected, trial.part, &self.writer);
        Ok(Attempt {
            failure: Failure::from_run(&classify(&error)),
            features,
        })
    }

    fn describe(
        &mut self,
        part: usize,
        feature: usize,
        _limit: &StepLimit,
    ) -> Result<Option<FeatureShape>, String> {
        Ok(describe_feature(&self.model, part, feature).0)
    }
}

#[test]
fn a_design_failing_in_one_feature_of_one_part_of_a_real_model_is_narrowed() {
    let mut trials = InMemory {
        model: assembly(&["SecretOne", "SecretTwo", "SecretThree", "SecretFour"]),
        writer: FailsAt {
            name: "SecretThree",
            feature: 5,
        },
    };
    let mut state = fresh();
    Drive {
        state: &mut state,
        trials: &mut trials,
        settings: &settings(),
        save: &|_| Ok(()),
        on_progress: &|_| {},
        step_started: Instant::now(),
        spent_before: 0,
    }
    .run()
    .unwrap();
    assert_eq!(status(&state), "narrowed");
    assert_eq!(state.parts, 4);
    assert_eq!(state.target.as_ref().unwrap().part, Some(3));
    assert_eq!(state.first_failing, Some(5));
    let signature = state.signature.as_ref().unwrap();
    assert_eq!(signature.feature.as_ref().unwrap().kind, "native");
    let shared =
        serde_json::to_string(&findings(&state)).unwrap() + &serde_json::to_string(&state).unwrap();
    assert!(!shared.contains("Secret"), "{shared}");
    assert!(!shared.contains("secret"), "{shared}");
}

#[test]
fn describing_a_feature_keeps_its_name_out_of_the_shape_and_gives_it_for_the_key() {
    let model = assembly(&["SecretOne", "SecretTwo"]);
    let (shape, name) = describe_feature(&model, 2, 3);
    assert_eq!(name.as_deref(), Some("SecretFeature3"));
    assert!(!format!("{shape:?}").contains("Secret"));
    assert_eq!(describe_feature(&model, 2, 9), (None, None));
    assert_eq!(describe_feature(&model, 9, 1), (None, None));
}

#[test]
fn neighbours_are_the_parts_either_side_placed_as_they_were() {
    let model = assembly(&["One", "Two", "Three", "Four"]);
    let (view, features) = trial_model(&model, Some(3), "neighbours", Some(2)).unwrap();
    assert_eq!(features, 8);
    let parts: Vec<_> = view
        .assembly
        .as_ref()
        .unwrap()
        .parts
        .iter()
        .map(|part| (part.name.as_str(), parts::history_length(part)))
        .collect();
    assert_eq!(parts, [("Two", 8), ("Three", 2), ("Four", 8)]);
    let placed: Vec<_> = view
        .assembly
        .as_ref()
        .unwrap()
        .placements
        .iter()
        .map(|placement| placement.part)
        .collect();
    assert_eq!(placed, [0, 1, 2]);
}

/// The whole path a person takes after a run, on files in a folder: a verdict stands only for
/// the file it was given about, and the original is never gathered up.
#[test]
fn only_a_checked_small_file_goes_in_the_report_and_never_the_original() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join(FOLDER);
    std::fs::create_dir_all(folder.join("kit")).unwrap();
    let original = dir.path().join("Secret Design.f3d");
    std::fs::write(&original, b"the private design").unwrap();
    let small = dir.path().join("Secret small.f3d");
    std::fs::write(&small, b"a small rebuild").unwrap();
    let signature = Signature::new(restate().code(), None);
    write_file(
        &folder.join(SIGNATURE_FILE),
        &serde_json::to_vec(&signature).unwrap(),
        false,
    )
    .unwrap();
    std::fs::write(folder.join("kit").join("anonymous.json"), b"{}").unwrap();
    let state = Checkpoint {
        schema: STATE_SCHEMA.to_owned(),
        file: Fingerprint::of(&original).unwrap(),
        ..Checkpoint::default()
    };
    save_state(&folder.join(STATE_FILE), &state).unwrap();

    let refused = make_report(&original, dir.path()).unwrap_err();
    assert!(refused.contains("not a small rebuild"), "{refused}");
    let refused = make_report(&small, dir.path()).unwrap_err();
    assert!(refused.contains("--same-failure"), "{refused}");
    assert!(same_failure(&original, dir.path(), &settings(), &|_| {})
        .unwrap_err()
        .contains("not a small rebuild"));

    // A verdict about another file does not count; one about this file does.
    let verdict = |file: &Path| {
        let record = json!({"verdict": "same", "small_file": Fingerprint::of(file).unwrap()});
        write_file(
            &folder.join(VERDICT_FILE),
            record.to_string().as_bytes(),
            true,
        )
        .unwrap();
    };
    let another = dir.path().join("another.f3d");
    std::fs::write(&another, b"another rebuild").unwrap();
    verdict(&another);
    assert!(make_report(&small, dir.path()).is_err());
    verdict(&small);
    let made = make_report(&small, dir.path()).unwrap();
    assert_eq!(made["schema"], wire::MADE_REPORT);
    let zip_path = PathBuf::from(made["paths"]["zip"].as_str().unwrap());
    let mut zip = zip::ZipArchive::new(std::fs::File::open(&zip_path).unwrap()).unwrap();
    let mut names: Vec<String> = zip
        .file_names()
        .map(|name| name.unwrap().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["anonymous.json", "signature.json", "small.f3d"]);
    let mut held = Vec::new();
    zip.by_name("small.f3d")
        .unwrap()
        .read_to_end(&mut held)
        .unwrap();
    assert_eq!(held, b"a small rebuild");
    assert!(made["issue_url"]
        .as_str()
        .unwrap()
        .starts_with(crate::issue::NEW_ISSUE));
    let link = std::fs::read_to_string(folder.join(ISSUE_LINK)).unwrap();
    assert!(link.starts_with(crate::issue::NEW_ISSUE));
    assert!(!link.contains("Secret"));
    assert!(made["private_send"]["url"]
        .as_str()
        .unwrap()
        .contains("discord"));
    // No email address anywhere a person is told to send to.
    assert!(!made.to_string().contains('@'), "{made}");
}

#[test]
fn the_kit_of_a_narrowed_failure_names_numbers_and_keeps_names_in_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let kit = dir.path().join("kit");
    let design = dir.path().join("Secret Design.f3d");
    std::fs::write(&design, b"design").unwrap();
    let signature = Signature::new(
        restate().code(),
        Some(FeatureShape {
            kind: "extrude".to_owned(),
            ..FeatureShape::default()
        }),
    );
    let failure = restate().facts;
    repro::narrowed_kit(
        &repro::NarrowedFailure {
            input: &design,
            failure: &failure,
            part: Some((17, 30, "SecretPart".to_owned())),
            feature: Some((
                41,
                80,
                Some("SecretFeature".to_owned()),
                "extrude".to_owned(),
            )),
            signature: &signature,
            limits: Vec::new(),
        },
        &kit,
    )
    .unwrap();
    let anonymous = std::fs::read_to_string(kit.join("anonymous.json")).unwrap();
    let steps = std::fs::read_to_string(kit.join("next-steps.md")).unwrap();
    for shared in [&anonymous, &steps] {
        assert!(!shared.contains("Secret"), "{shared}");
    }
    let anonymous: Value = serde_json::from_str(&anonymous).unwrap();
    assert_eq!(anonymous["narrowing"]["path"][1]["unit"], "feature");
    assert_eq!(anonymous["narrowing"]["path"][1]["ordinal"], 41);
    assert_eq!(
        anonymous["narrowing"]["signature"]["hash"],
        json!(signature.hash)
    );
    assert!(steps.contains("--same-failure"), "{steps}");
    assert!(steps.contains("--make-report"), "{steps}");
    assert!(steps.contains("part 17 of 30"), "{steps}");
    assert!(steps.contains("feature #41 of 80"), "{steps}");
    let key = std::fs::read_to_string(kit.join("local-key.json")).unwrap();
    assert!(key.contains("SecretFeature") && key.contains("SecretPart"));
}

#[test]
fn known_failure_stops_before_any_isolation_or_bisection() {
    let mut state = Checkpoint {
        step: 2,
        candidates: vec![Candidate {
            part: 17,
            failure: restate(),
        }],
        ..Default::default()
    };
    let signature = Signature::new(restate().code(), None);
    let expected = signature.clone();
    assert!(known_before_narrowing(&mut state, &|got| {
        (got == &expected).then(|| super::super::known::KnownFailure {
            hash: got.hash.clone(),
            class: "generic".into(),
            status: "fixed_next_version".into(),
            message: "This known problem is fixed in the next version.".into(),
        })
    }));
    assert_eq!(state.step, 5);
    assert_eq!(state.signature, Some(signature));
    assert_eq!(status(&state), "known_failure");
    let mut fake = Fake::design();
    let settings = NarrowSettings::new(Settings::default(), None);
    Drive {
        state: &mut state,
        trials: &mut fake,
        settings: &settings,
        save: &|_| Ok(()),
        on_progress: &|_| {},
        step_started: Instant::now(),
        spent_before: 0,
    }
    .run()
    .unwrap();
    assert!(fake.tried.is_empty());
}
