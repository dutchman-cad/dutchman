//! `dutchman diagnose --narrow`: the whole narrowing, with nobody at the keyboard.
//!
//! It runs in five steps, each of which a stopped run picks up where it left off:
//!
//! 1. rank the parts, with the ordinary [`run`](super::run);
//! 2. isolate a failing part: alone, then with the parts either side of it;
//! 3. bisect that part's history down to the first feature it fails at, by cutting the history
//!    short ([`parts::stop_at_feature`]) and trying again;
//! 4. record the failure's [`Signature`];
//! 5. write the results: `signature.json`, `narrow.json` and a kit.
//!
//! The design is read once, in a child process that then takes one trial at a time on its
//! standard input, so a bisection does not read the design for every trial. A trial that runs
//! past its time or memory limit, or crashes the child, costs that trial: the child is stopped,
//! the trial is counted as ending that way, and a new child reads the design for the next one.
//!
//! `diagnose/narrow-state.json` is written after every step and every trial, by replacing the
//! file whole. It holds part and feature numbers, fixed categories and a hash of the design,
//! and nothing else, so a run stopped at any moment, by a crash or a reboot, resumes by running
//! the same command again, and loses at most the trial it was on.
//!
//! The bisection trusts only what it tries: it ends with a feature N where the history up to
//! N failed the same way and the history up to N - 1 did not.
//!
//! [`same_failure`] compares a small rebuilt file with the signature, and [`make_report`]
//! gathers the one zip a person sends. Neither sends anything.

use std::io::{BufRead, Read, Write};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::signature::{feature_shape, Comparison, ErrorCode, FeatureShape, Signature};
use super::*;
use crate::repro;

/// The state file, in the `diagnose` folder.
pub const STATE_FILE: &str = "narrow-state.json";
/// The result file, in the `diagnose` folder. Safe to share.
pub const RESULT_FILE: &str = "narrow.json";
/// The signature, in the `diagnose` folder. Safe to share.
pub const SIGNATURE_FILE: &str = "signature.json";
/// The last `--same-failure` verdict, in the `diagnose` folder.
pub const VERDICT_FILE: &str = "same-failure.json";
/// The report zip, in the `diagnose` folder.
pub const REPORT_ZIP: &str = "dutchman-report.zip";
/// The prefilled issue link, in the `diagnose` folder.
pub const ISSUE_LINK: &str = "issue-link.txt";

const STATE_SCHEMA: &str = "dutchman.narrow-state/1";

/// How many steps a run has.
pub const STEPS: usize = 5;

/// How many failing parts are tried before the run says none repeats.
const MAX_CANDIDATES: usize = 5;

/// How long isolating or bisecting may take in all, unless set otherwise.
pub const DEFAULT_STEP_BUDGET: Duration = Duration::from_secs(4 * 3600);

/// The time the child gets to describe one feature.
const DESCRIBE_LIMIT: Duration = Duration::from_secs(600);

/// How people without GitHub send the report.
pub const PRIVATE_SEND: &str =
    "No GitHub account, or the small file must stay private? Join the Dutchman Discord and send \
     the zip to Dutchy in a direct message:";

/// What one step may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepLimit {
    /// How long one trial may take.
    pub time: Duration,
    /// How long the whole step may take; `None` for no limit. A step past it stops, and the run
    /// reports what it knows.
    pub budget: Option<Duration>,
    /// How much memory the converter may hold during a trial, in bytes; `None` for no limit.
    pub memory: Option<u64>,
}

/// How a narrowing run is set up.
#[derive(Clone, Debug)]
pub struct NarrowSettings {
    /// Step 1, ranking the parts: the limits are per part.
    pub rank: Settings,
    /// Step 2, isolating the part.
    pub isolate: StepLimit,
    /// Step 3, bisecting its history.
    pub bisect: StepLimit,
    /// Step 4, describing the feature.
    pub describe: StepLimit,
}

impl NarrowSettings {
    /// Each step's limits for a run ranked under `rank`. A trial gets the time and memory a
    /// part got in the ranking, so a part that ran past them there does so again.
    pub fn new(rank: Settings, step_budget: Option<Duration>) -> Self {
        let trial = StepLimit {
            time: rank.time_limit,
            budget: Some(step_budget.unwrap_or(DEFAULT_STEP_BUDGET)),
            memory: rank.memory_limit,
        };
        NarrowSettings {
            isolate: trial,
            bisect: trial,
            describe: StepLimit {
                time: DESCRIBE_LIMIT,
                budget: None,
                memory: rank.memory_limit,
            },
            rank,
        }
    }
}

/// Where a run has got to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    /// The step, from 1.
    pub step: usize,
    /// What it is doing.
    pub text: String,
}

impl std::fmt::Display for Progress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "step {} of {STEPS}: {}", self.step, self.text)
    }
}

/// Where one trial looks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Scope {
    /// The part alone.
    #[default]
    Alone,
    /// The part with the parts either side of it.
    Neighbours,
    /// The whole design.
    Design,
}

impl Scope {
    fn id(self) -> &'static str {
        match self {
            Self::Alone => "alone",
            Self::Neighbours => "neighbours",
            Self::Design => "design",
        }
    }
}

/// One trial to run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Trial {
    /// The part; `None` for the whole design.
    part: Option<usize>,
    scope: Scope,
    /// Cut the part's history after this feature.
    stop_at: Option<usize>,
}

/// How a trial ended.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Failure {
    outcome: String,
    /// `stage`, `kind`, `findings` and `curve_kinds`, as in a reproduction kit: fixed categories
    /// and counts.
    facts: Value,
}

impl Failure {
    fn from_run(run: &PartRun) -> Option<Self> {
        matches!(
            run.outcome,
            Outcome::Failed | Outcome::TimedOut | Outcome::OutOfMemory
        )
        .then(|| Failure {
            outcome: run.outcome.id().to_owned(),
            facts: json!({
                "stage": run.stage,
                "kind": run.kind.clone().unwrap_or_else(|| run.outcome.id().to_owned()),
                "findings": run.findings,
                "curve_kinds": run.curve_kinds,
            }),
        })
    }

    fn code(&self) -> ErrorCode {
        ErrorCode {
            outcome: self.outcome.clone(),
            stage: self.facts["stage"].as_str().map(str::to_owned),
            kind: self.facts["kind"].as_str().map(str::to_owned),
            codes: self.facts["findings"]
                .as_object()
                .map(|findings| findings.keys().cloned().collect())
                .unwrap_or_default(),
        }
    }

    /// Whether `self` and `other` are the same failure: the counts of a finding may differ with
    /// the history that is left, its code may not.
    fn same(&self, other: &Failure) -> bool {
        self.code() == other.code()
    }
}

/// What a trial found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Attempt {
    /// How it failed; `None` when it converted.
    failure: Option<Failure>,
    /// How many features the part's history has.
    features: usize,
}

/// One recorded trial.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Tried {
    part: Option<usize>,
    scope: Scope,
    stop_at: Option<usize>,
    /// Whether it failed.
    failed: bool,
}

/// A part the ranking found failing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Candidate {
    part: usize,
    failure: Failure,
}

/// The failure isolated in step 2.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Target {
    part: Option<usize>,
    scope: Scope,
    failure: Failure,
    /// How many features the part's history has.
    features: usize,
}

/// A design's identity: enough to refuse to resume on another file, and no path.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Fingerprint {
    size: u64,
    sha256: String,
}

impl Fingerprint {
    fn of(path: &Path) -> Result<Self, String> {
        let mut file = std::fs::File::open(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let mut hash = Sha256::new();
        let mut size = 0u64;
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
            let Some(chunk) = buffer.get(..read).filter(|_| read > 0) else {
                break;
            };
            hash.update(chunk);
            size += read as u64;
        }
        Ok(Fingerprint {
            size,
            sha256: hash
                .finalize()
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        })
    }
}

/// What `narrow-state.json` holds: everything a stopped run needs to carry on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
struct Checkpoint {
    schema: String,
    file: Fingerprint,
    /// The step in progress, or `STEPS + 1` when finished.
    step: usize,
    /// How many parts the design has.
    parts: usize,
    /// The design could not be read: how.
    read_failure: Option<Failure>,
    /// The failing parts, ranked, at most [`MAX_CANDIDATES`].
    candidates: Vec<Candidate>,
    /// How many failing parts the ranking found.
    failing_parts: usize,
    /// Which candidate is being tried.
    at: usize,
    /// Every trial so far, in order.
    trials: Vec<Tried>,
    target: Option<Target>,
    /// The bisection's bracket: the history up to `low` does not fail, up to `high` does.
    low: usize,
    high: usize,
    /// Whether the history with no features was tried.
    tried_none: bool,
    /// The history with no features fails too: the failure is in the stored shape.
    stored_shape: bool,
    first_failing: Option<usize>,
    signature: Option<Signature>,
    #[serde(default)]
    known_failure: Option<super::known::KnownFailure>,
    /// Milliseconds each step has taken, across resumes, by step.
    spent_ms: BTreeMap<usize, u64>,
    /// A step stopped at its limit: which.
    stopped_at_step: Option<usize>,
}

/// What the steps ask of the converter, so they can be tested without one.
trait Trials {
    /// Ranks the parts and returns the report (step 1).
    fn rank(&mut self, on_progress: &(dyn Fn(Progress) + Sync)) -> Result<Value, String>;
    /// Runs one trial.
    fn attempt(&mut self, trial: Trial, limit: &StepLimit) -> Result<Attempt, String>;
    /// Describes feature `feature` of `part`.
    fn describe(
        &mut self,
        part: usize,
        feature: usize,
        limit: &StepLimit,
    ) -> Result<Option<FeatureShape>, String>;
}

/// Runs the steps from wherever `state` got to, saving through `save` after each change.
struct Drive<'a, T: Trials> {
    state: &'a mut Checkpoint,
    trials: &'a mut T,
    settings: &'a NarrowSettings,
    save: &'a dyn Fn(&Checkpoint) -> Result<(), String>,
    on_progress: &'a (dyn Fn(Progress) + Sync),
    step_started: Instant,
    spent_before: u64,
}

impl<T: Trials> Drive<'_, T> {
    fn say(&self, step: usize, text: String) {
        (self.on_progress)(Progress { step, text });
    }

    fn begin_step(&mut self, step: usize) {
        self.state.step = step;
        self.step_started = Instant::now();
        self.spent_before = self.state.spent_ms.get(&step).copied().unwrap_or(0);
    }

    fn checkpoint(&mut self) -> Result<(), String> {
        let spent = u64::try_from(self.step_started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.state
            .spent_ms
            .insert(self.state.step, self.spent_before.saturating_add(spent));
        (self.save)(self.state)
    }

    /// Whether this step has used up `limit`'s budget; if so, the run stops here.
    fn out_of_budget(&mut self, limit: &StepLimit) -> bool {
        let spent = self.state.spent_ms.get(&self.state.step).copied();
        let over = limit
            .budget
            .is_some_and(|budget| spent.unwrap_or(0) as u128 >= budget.as_millis());
        if over {
            self.state.stopped_at_step = Some(self.state.step);
        }
        over
    }

    fn attempt(&mut self, trial: Trial, limit: &StepLimit) -> Result<Attempt, String> {
        let attempt = self.trials.attempt(trial, limit)?;
        self.state.trials.push(Tried {
            part: trial.part,
            scope: trial.scope,
            stop_at: trial.stop_at,
            failed: attempt.failure.is_some(),
        });
        Ok(attempt)
    }

    fn run(&mut self) -> Result<(), String> {
        // A resumed run goes on counting the time its step has taken.
        self.begin_step(self.state.step);
        if self.state.step <= 1 {
            self.rank()?;
        }
        if self.state.step == 2 {
            self.isolate()?;
        }
        if self.state.step == 3 {
            self.bisect()?;
        }
        if self.state.step == 4 {
            self.record()?;
        }
        Ok(())
    }

    fn rank(&mut self) -> Result<(), String> {
        self.begin_step(1);
        self.say(1, "ranking the parts".to_owned());
        let report = self.trials.rank(self.on_progress)?;
        self.state.parts = report["summary"]["parts"].as_u64().unwrap_or(0) as usize;
        if report["status"] == "read_failed" {
            let mut run = PartRun::simple(Outcome::Failed);
            let read = &report["read"];
            run.stage = read["stage"].as_str().map(str::to_owned);
            run.kind = read["error_kind"].as_str().map(str::to_owned);
            run.findings = read["findings"].as_object().cloned().unwrap_or_default();
            self.state.read_failure = Failure::from_run(&run);
        }
        let failing: Vec<Candidate> = report["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|row| {
                let outcome = Outcome::from_id(row["outcome"].as_str()?)?;
                let mut run = PartRun::simple(outcome);
                run.stage = row["stage"].as_str().map(str::to_owned);
                run.kind = row["error_kind"].as_str().map(str::to_owned);
                run.findings = row["findings"].as_object().cloned().unwrap_or_default();
                run.curve_kinds = row["curve_kinds"].as_object().cloned().unwrap_or_default();
                Some(Candidate {
                    part: row["part"].as_u64()? as usize,
                    failure: Failure::from_run(&run)?,
                })
            })
            .collect();
        self.state.failing_parts = failing.len();
        self.state.candidates = failing.into_iter().take(MAX_CANDIDATES).collect();
        // Ranking already supplies a signature for stored-shape/check failures. Check it
        // before any isolation or history trials. Feature-specific matches are checked
        // when a signature exists on resume and after recording the feature.
        if known_before_narrowing(self.state, &super::known::lookup) {
            return self.checkpoint();
        }
        self.begin_step(2);
        self.checkpoint()
    }

    fn isolate(&mut self) -> Result<(), String> {
        let limit = self.settings.isolate;
        if self.state.read_failure.is_some() {
            self.begin_step(4);
            return self.checkpoint();
        }
        if self.state.candidates.is_empty() {
            if !self.state.trials.iter().any(|t| t.scope == Scope::Design) {
                self.say(
                    2,
                    "every part converts alone: trying the whole design".to_owned(),
                );
                let trial = Trial {
                    part: None,
                    scope: Scope::Design,
                    stop_at: None,
                };
                let attempt = self.attempt(trial, &limit)?;
                if let Some(failure) = attempt.failure {
                    self.state.target = Some(Target {
                        part: None,
                        scope: Scope::Design,
                        failure,
                        features: 0,
                    });
                }
            }
            self.begin_step(4);
            return self.checkpoint();
        }
        while self.state.target.is_none() {
            let Some(part) = self.state.candidates.get(self.state.at).map(|c| c.part) else {
                break;
            };
            for scope in [Scope::Alone, Scope::Neighbours] {
                let done = self
                    .state
                    .trials
                    .iter()
                    .any(|t| t.part == Some(part) && t.scope == scope && t.stop_at.is_none());
                if done {
                    continue;
                }
                if self.out_of_budget(&limit) {
                    return self.finish_early();
                }
                self.say(
                    2,
                    match scope {
                        Scope::Neighbours => format!("isolating part {part} with its neighbours"),
                        _ => format!("isolating part {part}, alone"),
                    },
                );
                let trial = Trial {
                    part: Some(part),
                    scope,
                    stop_at: None,
                };
                let attempt = self.attempt(trial, &limit)?;
                if let Some(failure) = attempt.failure {
                    self.state.target = Some(Target {
                        part: Some(part),
                        scope,
                        failure,
                        features: attempt.features,
                    });
                    break;
                }
                self.checkpoint()?;
            }
            if self.state.target.is_none() {
                self.state.at += 1;
            }
            self.checkpoint()?;
        }
        self.begin_step(if self.state.target.is_some() { 3 } else { 4 });
        self.checkpoint()
    }

    /// A step ran out of its budget: go on to record what is known.
    fn finish_early(&mut self) -> Result<(), String> {
        self.begin_step(4);
        self.checkpoint()
    }

    fn bisect(&mut self) -> Result<(), String> {
        let limit = self.settings.bisect;
        let Some(target) = self.state.target.clone() else {
            self.begin_step(4);
            return self.checkpoint();
        };
        let Some(part) = target.part.filter(|_| target.features > 0) else {
            self.begin_step(4);
            return self.checkpoint();
        };
        let total = target.features;
        if self.state.high == 0 {
            self.state.high = total;
            self.state.low = 0;
        }
        let trial = |stop_at| Trial {
            part: Some(part),
            scope: target.scope,
            stop_at: Some(stop_at),
        };
        if !self.state.tried_none {
            self.say(
                3,
                format!("narrowing part {part} (no features, {total} in all)"),
            );
            let attempt = self.attempt(trial(0), &limit)?;
            self.state.tried_none = true;
            if attempt.failure.is_some_and(|f| f.same(&target.failure)) {
                self.state.stored_shape = true;
                self.begin_step(4);
                return self.checkpoint();
            }
            self.checkpoint()?;
        }
        while self.state.high - self.state.low > 1 {
            if self.out_of_budget(&limit) {
                return self.finish_early();
            }
            let middle = self.state.low + (self.state.high - self.state.low) / 2;
            self.say(
                3,
                format!("narrowing part {part} (feature {middle} of {total})"),
            );
            let attempt = self.attempt(trial(middle), &limit)?;
            if attempt.failure.is_some_and(|f| f.same(&target.failure)) {
                self.state.high = middle;
            } else {
                self.state.low = middle;
            }
            self.checkpoint()?;
        }
        self.state.first_failing = Some(self.state.high);
        self.begin_step(4);
        self.checkpoint()
    }

    fn record(&mut self) -> Result<(), String> {
        let limit = self.settings.describe;
        self.say(4, "recording the failure signature".to_owned());
        let failure = self
            .state
            .read_failure
            .clone()
            .or_else(|| self.state.target.as_ref().map(|t| t.failure.clone()));
        if let Some(failure) = failure {
            let shape = match (
                self.state.target.as_ref().and_then(|t| t.part),
                self.state.first_failing,
            ) {
                (Some(part), Some(feature)) => self.trials.describe(part, feature, &limit)?,
                _ => None,
            };
            self.state.signature = Some(Signature::new(failure.code(), shape));
        }
        self.begin_step(5);
        self.checkpoint()
    }
}

/// Prefer an already recorded feature signature, then the ranking's read/part failures.
/// Matching both keys happens before the first isolation or bisection trial.
fn known_before_narrowing(
    state: &mut Checkpoint,
    lookup: &dyn Fn(&Signature) -> Option<super::known::KnownFailure>,
) -> bool {
    let signatures = state
        .signature
        .clone()
        .into_iter()
        .chain(
            state
                .read_failure
                .as_ref()
                .map(|failure| Signature::new(failure.code(), None)),
        )
        .chain(
            state
                .candidates
                .iter()
                .map(|candidate| Signature::new(candidate.failure.code(), None)),
        );
    for signature in signatures {
        if let Some(known) = lookup(&signature) {
            state.known_failure = Some(known);
            state.signature = Some(signature);
            state.step = 5;
            return true;
        }
    }
    false
}

/// How a run came out.
fn status(state: &Checkpoint) -> &'static str {
    if state.known_failure.is_some() {
        "known_failure"
    } else if state.read_failure.is_some() {
        "read_failed"
    } else if state.stopped_at_step.is_some() {
        "stopped"
    } else if state.target.is_some() {
        "narrowed"
    } else if state.failing_parts > 0 {
        "not_repeated"
    } else {
        "nothing_to_narrow"
    }
}

/// What the run found, for `narrow.json`: numbers and fixed categories only.
fn findings(state: &Checkpoint) -> Value {
    let target = state.target.as_ref();
    json!({
        "schema": wire::NARROW,
        "versions": {"dutchman": env!("CARGO_PKG_VERSION"), "cadmpeg": crate::anon::CADMPEG_VERSION},
        "status": status(state),
        "parts": state.parts,
        "failing_parts": state.failing_parts,
        "part": target.and_then(|t| t.part),
        "scope": target.map(|t| t.scope.id()),
        "features": target.map(|t| t.features),
        "first_failing_feature": state.first_failing,
        "bracket": (state.stopped_at_step == Some(3))
            .then(|| json!({"passes": state.low, "fails": state.high})),
        "stored_shape": state.stored_shape,
        "stopped_at_step": state.stopped_at_step,
        "trials": state.trials.len(),
        "signature": state.signature,
        "known_failure": state.known_failure,
        "recipe": state.signature.as_ref().map(super::recipes::render),
    })
}

/// What the run found, in plain words.
fn summary(found: &Value, state: &Checkpoint) -> String {
    if let Some(known) = &state.known_failure {
        return format!("Known problem: {}", known.message);
    }
    let signature = state.signature.as_ref();
    let said = signature.map_or(String::new(), |s| format!(" It {}.", s.describe()));
    match found["status"].as_str() {
        Some("read_failed") => format!(
            "The design could not be read, so no part was tried.{said} Failure signature: {}.",
            signature.map_or("none", Signature::short)
        ),
        Some("not_repeated") => format!(
            "{} parts failed in the ranking, but none failed again alone or with its neighbours: \
             see report.txt for the limits they ran into.",
            state.failing_parts
        ),
        Some("nothing_to_narrow") => {
            "Every part converted, alone and in the whole design: there is no failure to narrow."
                .to_owned()
        }
        Some("stopped") => format!(
            "Step {} ran out of its time; the result holds what was found.{said} Run the same \
             command with a longer --step-time-limit in a fresh --out folder to go further.",
            state.stopped_at_step.unwrap_or(0)
        ),
        _ => {
            let place = match (found["part"].as_u64(), found["scope"].as_str()) {
                (None, _) => "the whole design".to_owned(),
                (Some(part), Some("neighbours")) => {
                    format!("part {part}, only with its neighbours")
                }
                (Some(part), _) => format!("part {part}"),
            };
            let feature = match (state.first_failing, state.stored_shape) {
                (Some(feature), _) => format!(
                    ", first at feature {feature} of {}",
                    found["features"].as_u64().unwrap_or(0)
                ),
                (None, true) => ", in its stored shape: it fails with no history at all".to_owned(),
                _ => String::new(),
            };
            format!(
                "The failure is in {place}{feature}.{said} Failure signature: {}. Next: follow \
                 kit/next-steps.md to rebuild it as a small file.",
                signature.map_or("none", Signature::short)
            )
        }
    }
}

fn save_state(path: &Path, state: &Checkpoint) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(state).map_err(|e| e.to_string())?;
    write_file(path, &bytes, true)?;
    // Tests stop a run the way a crash does, after this many trials; release builds cannot.
    #[cfg(debug_assertions)]
    if std::env::var("DUTCHMAN_NARROW_TEST_ABORT_AFTER")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .is_some_and(|after| state.trials.len() >= after)
    {
        std::process::abort();
    }
    Ok(())
}

/// Reads a design's checkpoint: a fresh one when `path` is not there. Another design's, or a
/// broken one, is refused: the folder has results that must not be mixed with this design's.
fn load_state(path: &Path, file: &Fingerprint) -> Result<Checkpoint, String> {
    let Ok(bytes) = std::fs::read(path) else {
        return Ok(Checkpoint {
            schema: STATE_SCHEMA.to_owned(),
            file: file.clone(),
            step: 1,
            ..Checkpoint::default()
        });
    };
    let state: Checkpoint = serde_json::from_slice(&bytes).map_err(|e| {
        format!(
            "{} cannot be read: {e}; choose a fresh --out folder",
            path.display()
        )
    })?;
    if state.schema != STATE_SCHEMA {
        return Err(format!(
            "{} is from another version of Dutchman; choose a fresh --out folder",
            path.display()
        ));
    }
    if state.file != *file {
        return Err(format!(
            "{} is about another file; choose a fresh --out folder",
            path.display()
        ));
    }
    Ok(state)
}

/// Runs the narrowing on `input`, or carries on with the run that stopped in `out`, and returns
/// the result (schema `dutchman.narrow/1`).
pub fn run(
    input: &Path,
    out: &Path,
    settings: &NarrowSettings,
    on_progress: &(dyn Fn(Progress) + Sync),
) -> Result<Value, String> {
    let folder = out.join(FOLDER);
    let file = Fingerprint::of(input)?;
    std::fs::create_dir_all(&folder)
        .map_err(|e| format!("cannot create {}: {e}", folder.display()))?;
    let path = folder.join(STATE_FILE);
    let fresh = !path.exists();
    if fresh && folder.join("report.json").exists() {
        check_same_design(&folder, input)?;
    }
    let mut state = load_state(&path, &file)?;
    if !fresh && state.step > 1 {
        on_progress(Progress {
            step: state.step.min(STEPS),
            text: format!("carrying on from {STATE_FILE}"),
        });
    }
    if fresh {
        save_state(&path, &state)?;
    }
    known_before_narrowing(&mut state, &super::known::lookup);
    let mut trials = Children::new(input, out, settings)?;
    let save = |state: &Checkpoint| save_state(&path, state);
    Drive {
        state: &mut state,
        trials: &mut trials,
        settings,
        save: &save,
        on_progress,
        step_started: Instant::now(),
        spent_before: 0,
    }
    .run()?;
    drop(trials);
    on_progress(Progress {
        step: 5,
        text: "writing the results".to_owned(),
    });
    if state.known_failure.is_none() {
        state.known_failure = state.signature.as_ref().and_then(super::known::lookup);
    }
    let result = write_results(input, &folder, &mut state)?;
    save_state(&path, &state)?;
    Ok(result)
}

/// Converts a numbered part (optionally with neighbours), discarding geometry. A history
/// prefix uses the same guarded worker and limits as automatic narrowing.
pub fn trial(
    input: &Path,
    part: usize,
    neighbours: bool,
    stop_at: Option<usize>,
    settings: &NarrowSettings,
) -> Result<Value, String> {
    if part == 0 {
        return Err("part numbers start at 1".to_owned());
    }
    let scratch = tempfile::tempdir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(scratch.path().join(FOLDER)).map_err(|e| e.to_string())?;
    let mut children = Children::new(input, scratch.path(), settings)?;
    let attempt = children.attempt(
        Trial {
            part: Some(part),
            scope: if neighbours {
                Scope::Neighbours
            } else {
                Scope::Alone
            },
            stop_at,
        },
        &settings.isolate,
    )?;
    if stop_at.is_some_and(|n| n > attempt.features) {
        return Err("feature number is beyond this part's history".to_owned());
    }
    let shape = match stop_at.filter(|n| *n > 0) {
        Some(feature) if attempt.failure.is_some() => {
            children.describe(part, feature, &settings.describe)?
        }
        _ => None,
    };
    let signature = attempt
        .failure
        .map(|failure| Signature::new(failure.code(), shape));
    Ok(json!({"schema": wire::PART_TRIAL, "ok": true, "part": part,
        "neighbours": neighbours, "stop_at_feature": stop_at, "features": attempt.features,
        "converted": signature.is_none(), "signature": signature}))
}

/// A plain `dutchman diagnose` report in the folder is used only if it is of this design.
fn check_same_design(folder: &Path, input: &Path) -> Result<(), String> {
    let key: Value = std::fs::read(folder.join("local-key.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null);
    if key["input"].as_str() == input.to_str() {
        return Ok(());
    }
    Err(format!(
        "{} holds a report on another file; choose a fresh --out folder",
        folder.display()
    ))
}

/// Writes `signature.json`, `narrow.json` and the kit, and marks the run finished. Writing them
/// again after a stop gives the same files.
fn write_results(input: &Path, folder: &Path, state: &mut Checkpoint) -> Result<Value, String> {
    let found = findings(state);
    let kit = folder.join("kit");
    if let Some(signature) = &state.signature {
        write_file(
            &folder.join(SIGNATURE_FILE),
            &serde_json::to_vec_pretty(signature).map_err(|e| e.to_string())?,
            false,
        )?;
        // The folder is this run's own, and the kit in it was started by this run.
        if kit.exists() {
            std::fs::remove_dir_all(&kit)
                .map_err(|e| format!("cannot write {}: {e}", kit.display()))?;
        }
        write_kit(input, folder, &kit, state, signature)?;
    }
    write_file(
        &folder.join(RESULT_FILE),
        &serde_json::to_vec_pretty(&found).map_err(|e| e.to_string())?,
        false,
    )?;
    state.step = STEPS + 1;
    let mut paths = json!({
        "result": folder.join(RESULT_FILE),
        "state": folder.join(STATE_FILE),
        "report": folder.join("report.json"),
    });
    if state.signature.is_some() {
        paths["signature"] = json!(folder.join(SIGNATURE_FILE));
        paths["anonymous"] = json!(kit.join("anonymous.json"));
        paths["next_steps"] = json!(kit.join("next-steps.md"));
    }
    Ok(json!({
        "schema": wire::NARROW, "ok": true, "status": found["status"],
        "summary": summary(&found, state), "paths": paths, "result": found,
    }))
}

fn write_kit(
    input: &Path,
    folder: &Path,
    kit: &Path,
    state: &Checkpoint,
    signature: &Signature,
) -> Result<(), String> {
    let key: Value = std::fs::read(folder.join("local-key.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null);
    let name = |part: usize| -> String {
        key["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row["part"].as_u64() == Some(part as u64))
            .and_then(|row| row["name"].as_str())
            .unwrap_or_default()
            .to_owned()
    };
    let target = state.target.as_ref();
    let part = target
        .and_then(|t| t.part)
        .map(|part| (part, state.parts, name(part)));
    let feature = match (target, state.first_failing) {
        (Some(target), Some(number)) => Some((
            number,
            target.features,
            key["feature"]["name"].as_str().map(str::to_owned),
            signature
                .feature
                .as_ref()
                .map_or_else(|| "other".to_owned(), |f| f.kind.clone()),
        )),
        _ => None,
    };
    let mut limits = Vec::new();
    if state.stored_shape {
        limits.push(
            "The part fails with no history at all, so the failure is in its stored shape and no \
             feature is named.",
        );
    }
    if target.is_some_and(|t| t.scope == Scope::Neighbours) {
        limits.push("The part fails only with the parts beside it.");
    }
    if state.stopped_at_step.is_some() {
        limits.push("A step ran out of its time, so the narrowing is not complete.");
    }
    let failure = state
        .read_failure
        .as_ref()
        .or(target.map(|t| &t.failure))
        .map_or(Value::Null, |failure| failure.facts.clone());
    repro::narrowed_kit(
        &repro::NarrowedFailure {
            input,
            failure: &failure,
            part,
            feature,
            signature,
            limits,
        },
        kit,
    )
    .map(|_| ())
}

// ---- trials in a child process ----

/// The ranking and the trials, run for real.
struct Children<'a> {
    input: &'a Path,
    out: &'a Path,
    settings: &'a NarrowSettings,
    program: PathBuf,
    child: Option<Child>,
}

/// A child that has read the design and takes trials.
struct Child {
    process: std::process::Child,
    stdin: std::process::ChildStdin,
    messages: mpsc::Receiver<Value>,
}

impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// How a question to the child ended.
enum Answer {
    Said(Value),
    Stopped(Outcome),
    Ended(Option<std::process::ExitStatus>),
}

impl<'a> Children<'a> {
    fn new(input: &'a Path, out: &'a Path, settings: &'a NarrowSettings) -> Result<Self, String> {
        let program = match &settings.rank.program {
            Some(program) => program.clone(),
            None => std::env::current_exe()
                .map_err(|err| format!("cannot find the running program: {err}"))?,
        };
        Ok(Children {
            input,
            out,
            settings,
            program,
            child: None,
        })
    }

    /// The child, started and through reading the design.
    fn child(&mut self, memory: Option<u64>) -> Result<&mut Child, String> {
        if self.child.is_none() {
            self.child = Some(self.start(memory)?);
        }
        self.child
            .as_mut()
            .ok_or_else(|| "the converter did not start".to_owned())
    }

    fn start(&self, memory: Option<u64>) -> Result<Child, String> {
        let options = &self.settings.rank.options;
        let mut process = Command::new(&self.program)
            .arg(WORKER_ARG)
            .arg(self.input)
            .arg("--serve")
            .args(options.to_args())
            .args(worker::search_args(&options.part_search()))
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|err| format!("cannot start the converter: {err}"))?;
        let stderr = process.stderr.take();
        let stdin = process.stdin.take().ok_or("cannot talk to the converter")?;
        let (send, messages) = mpsc::channel::<Value>();
        std::thread::spawn(move || {
            let Some(stderr) = stderr else { return };
            for line in std::io::BufReader::new(stderr).lines() {
                let Ok(line) = line else { break };
                let Ok(value) = serde_json::from_str::<Value>(&line) else {
                    continue;
                };
                if value.get("dutchman_diagnose").is_some() && send.send(value).is_err() {
                    break;
                }
            }
        });
        let mut child = Child {
            process,
            stdin,
            messages,
        };
        let limit = (self.settings.rank.time_limit * READ_FACTOR).max(MIN_READ_LIMIT);
        match wait(&mut child, "catalog", limit, memory)? {
            Answer::Said(_) => Ok(child),
            _ => Err("the design could not be read again for the next trial".to_owned()),
        }
    }

    /// Asks the child and waits, within `limit`; a child stopped or ended is gone for good.
    fn ask(&mut self, question: &Value, limit: &StepLimit) -> Result<Answer, String> {
        let child = self.child(limit.memory)?;
        let sent = writeln!(child.stdin, "{question}").and_then(|()| child.stdin.flush());
        let answer = match sent {
            Ok(()) => wait(child, "answer", limit.time, limit.memory)?,
            Err(_) => Answer::Ended(None),
        };
        if !matches!(answer, Answer::Said(_)) {
            self.child = None;
        }
        Ok(answer)
    }
}

/// Waits for the child's message `event`, holding it to `time` and `memory`.
fn wait(
    child: &mut Child,
    event: &str,
    time: Duration,
    memory: Option<u64>,
) -> Result<Answer, String> {
    let started = Instant::now();
    let mut measured = Instant::now();
    loop {
        match child.messages.recv_timeout(POLL) {
            Ok(message) if message["dutchman_diagnose"] == event => {
                return Ok(Answer::Said(message))
            }
            Ok(message) if message["dutchman_diagnose"] == "read_failed" => {
                return Err("the design could not be read again for the next trial".to_owned())
            }
            Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                let status = child.process.wait().map_err(|e| e.to_string())?;
                return Ok(Answer::Ended(Some(status)));
            }
        }
        if started.elapsed() > time {
            return Ok(Answer::Stopped(Outcome::TimedOut));
        }
        if measured.elapsed() >= MEMORY_POLL {
            measured = Instant::now();
            let held = worker::resident_bytes(child.process.id());
            if held.is_some_and(|held| memory.is_some_and(|limit| held > limit)) {
                return Ok(Answer::Stopped(Outcome::OutOfMemory));
            }
        }
    }
}

impl Trials for Children<'_> {
    fn rank(&mut self, on_progress: &(dyn Fn(Progress) + Sync)) -> Result<Value, String> {
        let on_event = |event: Event| {
            let text = match event {
                Event::Reading => "ranking the parts: reading the design".to_owned(),
                Event::Part {
                    done,
                    total,
                    number,
                    outcome,
                    ..
                } if !matches!(outcome, "ok" | "container") => format!(
                    "ranking the parts ({done} of {total}): part {number} {}",
                    outcome.replace('_', " ")
                ),
                Event::Part { done, total, .. } if done % 25 == 0 => {
                    format!("ranking the parts ({done} of {total})")
                }
                _ => return,
            };
            on_progress(Progress { step: 1, text });
        };
        let result = run_resuming(self.input, self.out, &self.settings.rank, &on_event)?;
        Ok(result["report"].clone())
    }

    fn attempt(&mut self, trial: Trial, limit: &StepLimit) -> Result<Attempt, String> {
        let question = json!({"try": {
            "part": trial.part, "scope": trial.scope.id(), "stop_at": trial.stop_at,
        }});
        let run = match self.ask(&question, limit)? {
            Answer::Said(answer) => {
                let run = classify(&answer["error"]);
                return Ok(Attempt {
                    failure: Failure::from_run(&run),
                    features: answer["features"].as_u64().unwrap_or(0) as usize,
                });
            }
            Answer::Stopped(outcome) => {
                let mut run = PartRun::simple(outcome);
                run.stage = Some("map".to_owned());
                run
            }
            Answer::Ended(status) => {
                let mut run = PartRun::simple(if killed_by_system(status) {
                    Outcome::OutOfMemory
                } else {
                    Outcome::Failed
                });
                run.stage = Some("map".to_owned());
                if run.outcome == Outcome::Failed {
                    run.kind = Some("crash".to_owned());
                }
                run
            }
        };
        // The child is gone. Only the first try at a part needs the count of its features.
        let features = match trial.part.filter(|_| trial.stop_at.is_none()) {
            Some(part) => self.features(part, limit).unwrap_or(0),
            None => 0,
        };
        Ok(Attempt {
            failure: Failure::from_run(&run),
            features,
        })
    }

    fn describe(
        &mut self,
        part: usize,
        feature: usize,
        limit: &StepLimit,
    ) -> Result<Option<FeatureShape>, String> {
        let question = json!({"describe": {"part": part, "feature": feature}});
        let Answer::Said(answer) = self.ask(&question, limit)? else {
            return Ok(None);
        };
        if let Some(name) = answer["name"].as_str() {
            self.keep_feature_name(part, feature, name)?;
        }
        Ok(serde_json::from_value(answer["shape"].clone()).ok())
    }
}

impl Children<'_> {
    /// How many features `part`'s history has.
    fn features(&mut self, part: usize, limit: &StepLimit) -> Option<usize> {
        let question = json!({"count": {"part": part}});
        let describe = StepLimit {
            time: DESCRIBE_LIMIT,
            ..*limit
        };
        match self.ask(&question, &describe) {
            Ok(Answer::Said(answer)) => answer["features"].as_u64().map(|n| n as usize),
            _ => None,
        }
    }

    /// The feature's name goes in the private key, the one file that holds names.
    fn keep_feature_name(&self, part: usize, feature: usize, name: &str) -> Result<(), String> {
        let path = self.out.join(FOLDER).join("local-key.json");
        let mut key: Value = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_else(|| json!({"schema": LOCAL_SCHEMA}));
        key["feature"] = json!({"part": part, "number": feature, "name": name});
        write_file(
            &path,
            &serde_json::to_vec_pretty(&key).map_err(|e| e.to_string())?,
            true,
        )
    }
}

// ---- the child's side ----

/// The model a trial converts, and how many features the part it is about has.
fn trial_model(
    model: &dutchman_core::Model,
    part: Option<usize>,
    scope: &str,
    stop_at: Option<usize>,
) -> Result<(dutchman_core::Model, usize), String> {
    let Some(number) = part else {
        return Ok((model.clone(), 0));
    };
    let alone = parts::select(model, number, false);
    let features = alone.as_ref().map_or(0, parts::history_length);
    if scope == "neighbours" {
        let view = parts::with_neighbours(model, number, stop_at).map_err(|e| e.to_string())?;
        return Ok((view, features));
    }
    let alone = alone.map_err(|e| e.to_string())?;
    match stop_at {
        Some(stop_at) => parts::stop_at_feature(&alone, stop_at)
            .map(|cut| (cut, features))
            .map_err(|e| e.to_string()),
        None => Ok((alone, features)),
    }
}

/// Takes trials on standard input until it closes: one JSON line each, one answer each.
pub(super) fn serve(model: &dutchman_core::Model, writer: &dyn Writer) -> ExitCode {
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(question) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        let number = |v: &Value| v.as_u64().and_then(|n| usize::try_from(n).ok());
        let answer = if let Some(trial) = question.get("try") {
            let part = number(&trial["part"]);
            let scope = trial["scope"].as_str().unwrap_or("alone");
            match trial_model(model, part, scope, number(&trial["stop_at"])) {
                Ok((selected, features)) => {
                    let error = trial_error(&selected, part, writer);
                    json!({"dutchman_diagnose": "answer", "error": error, "features": features})
                }
                Err(why) => json!({"dutchman_diagnose": "answer", "features": 0,
                    "error": wire::failure(wire::CONVERT, Value::Null, &why)}),
            }
        } else if let Some(count) = question.get("count") {
            let features = number(&count["part"])
                .and_then(|part| parts::select(model, part, false).ok())
                .map_or(0, |part| parts::history_length(&part));
            json!({"dutchman_diagnose": "answer", "features": features})
        } else if let Some(describe) = question.get("describe") {
            let (shape, name) = number(&describe["part"])
                .zip(number(&describe["feature"]))
                .map_or((None, None), |(part, feature)| {
                    describe_feature(model, part, feature)
                });
            json!({"dutchman_diagnose": "answer", "shape": shape, "name": name})
        } else {
            continue;
        };
        emit(&answer);
    }
    ExitCode::SUCCESS
}

fn trial_error(selected: &dutchman_core::Model, part: Option<usize>, writer: &dyn Writer) -> Value {
    #[cfg(debug_assertions)]
    if let Some(part) = part {
        test_fault(part);
    }
    if selected.assembly.is_none() && parts::entity_count(selected) == 0 {
        return json!({"container": true});
    }
    write_model(selected, part.unwrap_or(0), writer)
}

/// The shape of feature `feature` of `part`, and its private name.
fn describe_feature(
    model: &dutchman_core::Model,
    part: usize,
    feature: usize,
) -> (Option<FeatureShape>, Option<String>) {
    let Ok(part) = parts::select(model, part, false) else {
        return (None, None);
    };
    let Some(row) = parts::feature_position(&part, feature)
        .and_then(|position| part.ir.model.features.get(position))
    else {
        return (None, None);
    };
    let name = row.name.clone();
    let id = row.id.as_str().to_owned();
    let Ok(wire) = serde_json::to_value(&part.ir) else {
        return (None, name);
    };
    let wire = wire.get("model").unwrap_or(&Value::Null);
    let shape = wire["features"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["id"].as_str() == Some(id.as_str()))
        .map(|row| feature_shape(row, wire));
    (shape, name)
}

// ---- checking a rebuilt file, and the report ----

fn read_signature(folder: &Path) -> Result<Signature, String> {
    let path = folder.join(SIGNATURE_FILE);
    let text = std::fs::read_to_string(&path).map_err(|_| {
        format!(
            "no failure signature in {}; run `dutchman diagnose --narrow` with this --out first",
            folder.display()
        )
    })?;
    Signature::from_json(&text)
}

/// The design's identity from the run's checkpoint, to keep it out of the report.
fn original_of(folder: &Path) -> Option<Fingerprint> {
    let state: Checkpoint =
        serde_json::from_slice(&std::fs::read(folder.join(STATE_FILE)).ok()?).ok()?;
    Some(state.file)
}

fn refuse_original(small: &Fingerprint, folder: &Path) -> Result<(), String> {
    if original_of(folder).is_some_and(|original| original == *small) {
        return Err(
            "this is the design that was narrowed, not a small rebuild of it; rebuild the \
             failure in a new file as kit/next-steps.md says"
                .to_owned(),
        );
    }
    Ok(())
}

/// Converts the rebuilt file `small` and says whether it fails as the design narrowed into
/// `out` did (schema `dutchman.same-failure/1`).
pub fn same_failure(
    small: &Path,
    out: &Path,
    settings: &NarrowSettings,
    on_progress: &(dyn Fn(Progress) + Sync),
) -> Result<Value, String> {
    let folder = out.join(FOLDER);
    let expected = read_signature(&folder)?;
    let file = Fingerprint::of(small)?;
    refuse_original(&file, &folder)?;
    let scratch = tempfile::Builder::new()
        .prefix(".same-failure-")
        .tempdir_in(&folder)
        .map_err(|e| format!("cannot create a local working folder: {e}"))?;
    let result = run(small, scratch.path(), settings, on_progress)?;
    let got: Option<Signature> =
        serde_json::from_value(result["result"]["signature"].clone()).unwrap_or(None);
    let (verdict, differs, text): (&str, Vec<&str>, String) = match &got {
        None => (
            "converts_fine",
            Vec::new(),
            format!(
                "It converts fine, so it does not show the failure. Try again: the failure {}.",
                expected.describe()
            ),
        ),
        Some(got) => match expected.compare(got) {
            Comparison::Same => (
                "same",
                Vec::new(),
                format!(
                    "It fails the same way ({}): this is the one to send. Next: dutchman \
                     diagnose --make-report <this file> --out <this folder>.",
                    got.short()
                ),
            ),
            Comparison::TooBroad => (
                "too_broad",
                Vec::new(),
                "It stops at the same stage with the same error category, but this category is \
                 too broad to prove it is the same failure. You can still make the report."
                    .to_owned(),
            ),
            Comparison::Different(fields) => (
                "different",
                fields.clone(),
                format!(
                    "It fails, but differently ({} differ): it {}; the failure to show {}.",
                    fields.join(", "),
                    got.describe(),
                    expected.describe()
                ),
            ),
        },
    };
    let record = json!({
        "schema": wire::SAME_FAILURE, "verdict": verdict, "differs": differs,
        "expected": expected, "got": got,
    });
    let mut local = record.clone();
    local["small_file"] = json!(file);
    write_file(
        &folder.join(VERDICT_FILE),
        &serde_json::to_vec_pretty(&local).map_err(|e| e.to_string())?,
        true,
    )?;
    let mut answer = record;
    answer["ok"] = json!(true);
    answer["same"] = json!(verdict == "same");
    answer["summary"] = json!(text);
    Ok(answer)
}

/// Gathers the report into one zip in `out`'s `diagnose` folder, and the issue link beside it
/// (schema `dutchman.made-report/1`). `small` must be the file `--same-failure` passed. Nothing
/// is sent.
pub fn make_report(small: &Path, out: &Path) -> Result<Value, String> {
    let folder = out.join(FOLDER);
    let signature = read_signature(&folder)?;
    let file = Fingerprint::of(small)?;
    refuse_original(&file, &folder)?;
    let verdict: Value = std::fs::read(folder.join(VERDICT_FILE))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or(Value::Null);
    let checked = serde_json::from_value::<Fingerprint>(verdict["small_file"].clone())
        .is_ok_and(|checked| checked == file);
    if !checked || !matches!(verdict["verdict"].as_str(), Some("same" | "too_broad")) {
        return Err(
            "this file has not passed the check; run `dutchman diagnose --same-failure` on it \
             with this --out first"
                .to_owned(),
        );
    }
    let anonymous = std::fs::read(folder.join("kit").join("anonymous.json"))
        .map_err(|e| format!("cannot read the kit's anonymous.json: {e}"))?;
    let small_bytes =
        std::fs::read(small).map_err(|e| format!("cannot read {}: {e}", small.display()))?;
    let extension = small
        .extension()
        .and_then(|extension| extension.to_str())
        .filter(|extension| extension.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("bin");
    let zip_path = folder.join(REPORT_ZIP);
    let temporary = folder.join(format!("{REPORT_ZIP}.tmp"));
    let failed = |e: &dyn std::fmt::Display| format!("cannot write the report zip: {e}");
    let mut zip = zip::ZipWriter::new(std::fs::File::create(&temporary).map_err(|e| failed(&e))?);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let signature_bytes = serde_json::to_vec_pretty(&signature).map_err(|e| e.to_string())?;
    for (name, bytes) in [
        ("anonymous.json".to_owned(), &anonymous),
        ("signature.json".to_owned(), &signature_bytes),
        (format!("small.{extension}"), &small_bytes),
    ] {
        zip.start_file(name, options).map_err(|e| failed(&e))?;
        zip.write_all(bytes).map_err(|e| failed(&e))?;
    }
    zip.finish().map_err(|e| failed(&e))?;
    std::fs::rename(&temporary, &zip_path).map_err(|e| failed(&e))?;
    let format = Format::from_path(small).map_or("Other", |format| format.name());
    let link = crate::issue::narrowed(format, &signature);
    let link_path = folder.join(ISSUE_LINK);
    write_file(&link_path, format!("{link}\n").as_bytes(), false)?;
    Ok(json!({
        "schema": wire::MADE_REPORT, "ok": true,
        "summary": format!(
            "The report is ready and nothing was sent. It holds anonymous.json, signature.json \
             and your small file, under the name small.{extension}: send it only if you are \
             free to share that file."
        ),
        "paths": {"zip": zip_path, "issue_link": link_path},
        "issue_url": link,
        "private_send": {"text": PRIVATE_SEND, "url": wire::DISCORD_URL},
        "signature": signature.hash,
    }))
}

#[cfg(test)]
mod tests;
