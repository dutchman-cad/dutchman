//! SolidCraft adapter for Dutchman: writes the neutral model as a `.sc` design.
//!
//! When every feature of the model's history maps onto SolidCraft features, the design is
//! that history, followed by the original geometry as a suppressed import; the history is
//! checked against the original in SolidCraft's own CLI when one is available. A blocked or
//! differing history keeps its editable prefix hidden beside the visible original. Without
//! a body-building prefix, the design is what SolidCraft saves when it imports a
//! STEP file: a design container holding one component, one occurrence of it, and a
//! non-parametric `import` feature whose geometry is STEP text (cadmpeg's restatement of the
//! model). The adapter writes the container itself rather than linking SolidCraft's crates,
//! so it carries none of SolidCraft's kernel or renderer.

mod assembly;
// Advisory comparisons include native CLI snapshots; conversion checks use the evaluator.
#[cfg(not(target_family = "wasm"))]
pub mod compare;
mod container;
mod document;
mod evaluator;
mod linked;
mod opening;
mod output_links;
#[cfg(not(target_family = "wasm"))]
pub use evaluator::CliEvaluator;
pub use evaluator::SolidCraftEvaluator;
mod history;
mod placements;
mod repeats;
mod replay;
mod step;
mod tolerances;
pub use tolerances::report as verification_tolerances;
#[cfg(test)]
mod testing;
mod verdicts;
mod verify;
pub use verdicts::{FeatureVerdict, PartVerdicts, Verdict};

use std::sync::OnceLock;
use std::time::Duration;

use dutchman_core::{
    Check, Error, FeatureOutcome, FeatureStatus, Mode, Model, Outcome, Report, Severity, Stage,
    Writer,
};
use serde_json::{json, Value};

pub use verify::{
    cached_solidcraft_version, chosen_cli, discover, find_cli, search_report, solidcraft_minor,
    solidcraft_status, solidcraft_version, target_loses_history, usable_cli, Discovery, NoCli,
    Place, SolidCraftVersion, Verification, VersionGate, CLI_ENV, DEFAULT_TIMEOUT,
    NEWEST_SOLIDCRAFT, OLDEST_SOLIDCRAFT, VOLUME_TOLERANCE, WINDOW_OLDEST,
};

#[cfg(not(target_family = "wasm"))]
pub use verify::{hide_console, watch_check_memory};

use document::Live;
pub use replay::replay_script;

/// Debug-only fixture hook; release builds never read this variable.
#[cfg(all(debug_assertions, not(target_family = "wasm")))]
fn slow_part(name: &str) {
    if std::env::var("DUTCHMAN_TEST_SLOW_PART").ok().as_deref() == Some(name) {
        if let Some(release) = std::env::var_os("DUTCHMAN_TEST_SLOW_PART_RELEASE") {
            // Let integration tests keep the step alive until they observe a heartbeat.
            // Bound the wait even if the test process exits before releasing it.
            let started = std::time::Instant::now();
            while !std::path::Path::new(&release).exists()
                && started.elapsed() < Duration::from_secs(60)
            {
                std::thread::sleep(Duration::from_millis(10));
            }
        } else {
            std::thread::sleep(Duration::from_secs(6));
        }
    }
}

/// The SolidCraft design format this adapter writes: the first in which an Import may name a
/// non-STEP source file whose geometry it keeps as restated STEP text. Every newer
/// SolidCraft upgrades it on open. Keep this fixed for every target, including 0.28:
/// its format 45 adds nested Browser groups, point cloud properties/filters and print
/// faces, which this writer does not use. Likewise, 0.45 saves format 63 for drawings,
/// which this writer does not emit. Format 23 also keeps older targets readable.
pub const FORMAT: &str = "solidcraft/23";

/// The report subject of the lines that say what became of the history.
const HISTORY: &str = "history";

/// The report subject of what SolidCraft shows of the written geometry.
const GEOMETRY: &str = "geometry";

/// What the report says turned history off, unless a front end names its own choice.
const IMPORT_ONLY_REASON: &str = "--import-only";

/// The least time the final opening check gets: a large assembly takes minutes to open.
pub const OPENING_CHECK_FLOOR: Duration = Duration::from_secs(15 * 60);

/// Writes `.sc` designs.
#[derive(Clone, Debug)]
pub struct SolidCraftWriter {
    evaluator: Option<std::sync::Arc<dyn SolidCraftEvaluator>>,
    threads: usize,
    #[cfg(test)]
    reuse_evaluations: bool,
    #[cfg(test)]
    prepare_hook: Option<fn(&Model)>,
    #[cfg(test)]
    prepare_available: Option<u64>,
    plan_deadline: Option<std::time::Instant>,
    import_only: bool,
    import_only_reason: String,
    sketches: bool,
    joints: bool,
    target_version: Option<SolidCraftVersion>,
    detected_version: OnceLock<SolidCraftVersion>,
    verification: Verification,
    replay: Verification,
    timeout: Duration,
    opening_timeout: Option<Duration>,
    hide_unverified: bool,
    stand_up: Option<bool>,
    memory_budget: Option<u64>,
    memory_skip: OnceLock<(u64, u64)>,
}

impl Default for SolidCraftWriter {
    fn default() -> Self {
        SolidCraftWriter {
            evaluator: None,
            threads: 0,
            #[cfg(test)]
            reuse_evaluations: true,
            #[cfg(test)]
            prepare_hook: None,
            #[cfg(test)]
            prepare_available: None,
            plan_deadline: None,
            import_only: false,
            import_only_reason: IMPORT_ONLY_REASON.to_owned(),
            sketches: true,
            joints: true,
            target_version: None,
            detected_version: OnceLock::new(),
            verification: Verification::Auto,
            replay: Verification::Auto,
            timeout: DEFAULT_TIMEOUT,
            opening_timeout: None,
            hide_unverified: false,
            stand_up: None,
            memory_budget: None,
            memory_skip: OnceLock::new(),
        }
    }
}

impl SolidCraftWriter {
    /// A writer that writes history when it can, checked with the SolidCraft CLI it finds.
    pub fn new() -> Self {
        SolidCraftWriter::default()
    }

    /// Supply the host evaluator for all proofs and final checks. The existing
    /// verification and replay switches still apply.
    /// Set target_version explicitly to the host's SolidCraft release.
    pub fn evaluator(mut self, evaluator: std::sync::Arc<dyn SolidCraftEvaluator>) -> Self {
        self.evaluator = Some(evaluator);
        self.detected_version = OnceLock::new();
        self
    }

    fn selected_evaluator(
        &self,
        verification: &Verification,
    ) -> Result<std::sync::Arc<dyn SolidCraftEvaluator>, NoCli> {
        if !matches!(verification, Verification::Off) {
            if let Some(evaluator) = &self.evaluator {
                return Ok(evaluator.clone());
            }
        }
        #[cfg(not(target_family = "wasm"))]
        {
            verify::usable_cli(verification)
                .map(|path| std::sync::Arc::new(CliEvaluator::new(path)) as _)
        }
        #[cfg(target_family = "wasm")]
        {
            Err(NoCli::NotFound(
                if matches!(verification, Verification::Off) {
                    "verification turned off"
                } else {
                    "no SolidCraft evaluator supplied"
                }
                .to_owned(),
            ))
        }
    }

    /// Share only plain evaluation answers between checks of this written design.
    /// The selected host (and therefore binary, version and evaluation options) is fixed
    /// for this scope. Replay scripts and other proofs keep their own evaluator.
    fn final_evaluator(&self) -> Option<std::sync::Arc<dyn SolidCraftEvaluator>> {
        let evaluator = self.selected_evaluator(&self.verification).ok()?;
        #[cfg(test)]
        if !self.reuse_evaluations {
            return Some(evaluator);
        }
        Some(std::sync::Arc::new(evaluator::Reusing::new(evaluator)))
    }

    /// A design of sketches alone has no source bodies to compare with. Replaying it shows
    /// whether SolidCraft rebuilds every sketch: it must run without a feature error and,
    /// like its source, build no body.
    fn check_sketches(
        &self,
        design: &[u8],
        evaluator: Option<&std::sync::Arc<dyn SolidCraftEvaluator>>,
    ) -> Check {
        const NO_BODIES: &str = "the design contains sketches and no source bodies to compare";
        let cli = match evaluator
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| self.selected_evaluator(&self.verification))
        {
            Ok(cli) => cli,
            Err(why) => return Check::NotApplicable(format!("{NO_BODIES}; not replayed: {why}")),
        };
        if let Some(why) = self.memory_refusal() {
            return Check::NotApplicable(format!("{NO_BODIES}; not replayed: {why}"));
        }
        match verify::replay(cli.as_ref(), self.timeout, design) {
            (verify::Outcome::Matches, Some(bodies)) if bodies.is_empty() => Check::Verified,
            (verify::Outcome::Matches, _) => Check::Differs(
                "SolidCraft built a body from a design whose source has only sketches".to_owned(),
            ),
            (verify::Outcome::Differs(why), _) => {
                Check::Differs(format!("SolidCraft cannot replay the sketches: {why}"))
            }
            (verify::Outcome::Unverified(why), _) => Check::Unverified(why),
        }
    }

    /// Maximum simultaneous assembly part preparations (0: automatic, 1: serial).
    /// Available memory and the number of parts may reduce this limit.
    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

    fn preparation_memory(&self) -> Option<u64> {
        #[cfg(test)]
        if let Some(available) = self.prepare_available {
            return Some(available);
        }
        dutchman_core::available_memory()
    }

    /// Target this SolidCraft release, or detect the installed CLI's version when absent.
    /// Without a known CLI version, use the newest release known at build time.
    pub fn target_version(mut self, version: Option<SolidCraftVersion>) -> Self {
        self.target_version = version;
        self
    }

    /// The release the written features must open in.
    pub fn resolved_version(&self) -> SolidCraftVersion {
        self.target_version.unwrap_or_else(|| {
            *self.detected_version.get_or_init(|| {
                if self.evaluator.is_some() {
                    SolidCraftVersion::default()
                } else {
                    SolidCraftVersion::detected(&self.verification)
                }
            })
        })
    }

    /// Always write the geometry as one import, never the history.
    pub fn import_only(mut self, import_only: bool) -> Self {
        self.import_only = import_only;
        self
    }

    /// How the report names the choice that turned history off, in the words of the front
    /// end that offers it: `--import-only` (the command line's flag) by default.
    pub fn import_only_reason(mut self, reason: impl Into<String>) -> Self {
        self.import_only_reason = reason.into();
        self
    }

    /// Keep reference sketches, including those beside imported geometry.
    pub fn sketches(mut self, enabled: bool) -> Self {
        self.sketches = enabled;
        self
    }

    /// Keep assembly joints and their solved placements.
    pub fn joints(mut self, enabled: bool) -> Self {
        self.joints = enabled;
        self
    }

    /// How a written history is checked, and which SolidCraft CLI finds the input edges of
    /// its blends and body-edge sweep paths (see [`SolidCraftWriter::replay`]).
    pub fn verification(mut self, verification: Verification) -> Self {
        self.replay = verification.clone();
        self.verification = verification;
        self.detected_version = OnceLock::new();
        self
    }

    /// Which SolidCraft CLI replays the history to prove blend and body-edge sweep-path input edges,
    /// after [`SolidCraftWriter::verification`] has set it too. Without one
    /// ([`Verification::Off`], or none found) those selections are refused, saying why.
    pub fn replay(mut self, replay: Verification) -> Self {
        self.replay = replay;
        self
    }

    /// Whether a history that could not be checked against the original is kept hidden:
    /// the original geometry stays the visible model and the mapped history goes into a
    /// hidden "Editable history (unverified)" part, as a history that fails the check is
    /// kept. For a writer with no SolidCraft CLI at all, such as one running in a sandbox.
    pub fn hide_unverified_history(mut self, hide: bool) -> Self {
        self.hide_unverified = hide;
        self
    }

    /// Whether to orient Y-up models (such as Fusion 360 designs) to SolidCraft's Z-up
    /// coordinate system. By default (`None` or `false`), source axes are preserved.
    pub fn stand_up(mut self, stand_up: Option<bool>) -> Self {
        self.stand_up = stand_up;
        self
    }

    pub(crate) fn should_stand_up(&self) -> bool {
        self.stand_up.unwrap_or(false)
    }

    /// Additional memory estimated after reading the design. Recheck current free memory
    /// immediately before every geometry replay; a skipped check stays skipped for this design.
    pub fn history_check_memory(mut self, needed: u64, available: Option<u64>) -> Self {
        self.memory_budget = Some(needed);
        self.memory_skip = OnceLock::new();
        if let Some(available) = available.filter(|free| needed > *free) {
            let _ = self.memory_skip.set((needed, available));
        }
        self
    }

    /// Bytes needed and free when replay was skipped for memory.
    pub fn history_check_skipped(&self) -> Option<(u64, u64)> {
        self.memory_skip.get().copied()
    }

    fn memory_refusal(&self) -> Option<String> {
        let needed = self.memory_budget?;
        if self.memory_skip.get().is_none() {
            let available = dutchman_core::available_memory()?;
            if needed <= available {
                return None;
            }
            let _ = self.memory_skip.set((needed, available));
        }
        self.memory_skip.get().map(|(needed, available)| {
            format!(
                "History check skipped: needs about {:.1} GB, {:.1} GB free",
                *needed as f64 / 1_000_000_000.0,
                *available as f64 / 1_000_000_000.0,
            )
        })
    }

    fn keep_unverified_hidden(&self) -> bool {
        self.hide_unverified || self.memory_skip.get().is_some()
    }

    fn keep_unverified_history_hidden(&self, history: &history::History) -> bool {
        self.keep_unverified_hidden() || history.topological_replay_required
    }

    /// How long each SolidCraft evaluation may run before the check gives up.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The whole time the final opening check of the written design may take. By default it is
    /// its own budget, at least [`OPENING_CHECK_FLOOR`], never what the replay checks left over.
    pub fn opening_timeout(mut self, timeout: Duration) -> Self {
        self.opening_timeout = Some(timeout);
        self
    }

    fn opening_budget(&self) -> Duration {
        self.opening_timeout
            .unwrap_or_else(|| self.timeout.max(OPENING_CHECK_FLOOR))
    }

    /// Whether `model` can be written at all: `Err` with the reason a conversion would fail
    /// with, when it has no geometry SolidCraft can import (for an assembly, when none of its
    /// parts has). Plans the STEP restatement without writing anything.
    pub fn convertible(&self, model: &Model) -> Result<(), String> {
        if let Some(flat) = flattened_assembly(model) {
            return self.convertible(&flat);
        }
        match &model.assembly {
            Some(assembly) if any_part(assembly) || assembly.placements.is_empty() => Ok(()),
            Some(_) => {
                Err("none of the assembly's parts has geometry SolidCraft can import".to_owned())
            }
            None if parameters_only(model)
                || (sketches_only(model) && !self.import_only && self.sketches) =>
            {
                Ok(())
            }
            None => step::restatable(model).map_err(|err| match err {
                Error::Encode(reason) | Error::NoGeometry(reason) | Error::Restate(reason) => {
                    reason
                }
                other => other.to_string(),
            }),
        }
    }

    /// Cooperatively stop planning at the deadline; an expired plan supplies no forecast.
    pub fn plan_until(
        &self,
        model: &Model,
        report: &mut Report,
        deadline: std::time::Instant,
    ) -> Option<Outcome> {
        let mut bounded = self.clone();
        bounded.plan_deadline = Some(deadline);
        let outcome = bounded.plan(model, report);
        (std::time::Instant::now() < deadline).then_some(outcome)
    }

    /// Maps history without writing output or replaying geometry.
    pub fn plan(&self, model: &Model, report: &mut Report) -> Outcome {
        if self
            .plan_deadline
            .is_some_and(|deadline| std::time::Instant::now() >= deadline)
        {
            return empty_outcome();
        }
        document::report_scoped_attributes(model, report);
        if let Some(flat) = flattened_assembly(model) {
            report.push(Severity::Degraded, GEOMETRY, FLATTENED_ASSEMBLY);
            return self.plan(&flat, report);
        }
        if empty_assembly(model) {
            return empty_outcome();
        }
        if let Some(assembly) = &model.assembly {
            return assembly::plan(self, assembly, report);
        }
        if parameters_only(model) {
            return parameters_outcome(model);
        }
        if self.import_only {
            return Outcome {
                mode: Mode::Import,
                check: Check::NotApplicable(self.import_only_reason.clone()),
                features: Vec::new(),
            };
        }
        let mut history_report = Report::default();
        let mut mapping = history::map_until(
            model,
            &mut history_report,
            self.resolved_version(),
            self.sketches,
            report.losses(),
            self.plan_deadline,
        );
        self.plan_blends(&mut mapping);
        output_links::not_written(&mut mapping.features);
        let undecoded = no_decoded_history(&mapping.features);
        match mapping.result {
            Ok(ref history) if history.stop.is_some() => {
                report.append(history_report);
                report.push(Severity::Degraded, HISTORY, partial_summary(history));
                Outcome {
                    mode: Mode::PartialHistory,
                    check: Check::Partial(format!(
                        "{}; prefix unverified: inspect writes nothing",
                        partial_summary(history)
                    )),
                    features: mapping.features,
                }
            }
            Ok(_) if only_stored_geometry(&mapping.features) => {
                report.append(history_report);
                Outcome {
                    mode: Mode::Import,
                    check: Check::NotApplicable(history::base::STORED_SOLID.to_owned()),
                    features: mapping.features,
                }
            }
            Ok(_) => {
                report.append(history_report);
                Outcome {
                    mode: Mode::History,
                    check: if sketches_only(model) {
                        Check::NotApplicable(
                            "the design contains sketches and no source bodies to compare"
                                .to_owned(),
                        )
                    } else {
                        Check::Unverified("not checked: inspect writes nothing".to_owned())
                    },
                    features: mapping.features,
                }
            }
            Err(reason) if mapping.features.is_empty() => {
                report.append(history_report);
                Outcome {
                    mode: Mode::Import,
                    check: Check::NotApplicable(reason),
                    features: Vec::new(),
                }
            }
            Err(_) if undecoded.is_some() => {
                report.append(history_report);
                Outcome {
                    mode: Mode::Import,
                    check: Check::NotApplicable(undecoded.unwrap_or_default()),
                    features: mapping.features,
                }
            }
            Err(reason) => {
                report.append(history_report);
                Outcome {
                    mode: Mode::Import,
                    check: Check::Flattened(reason),
                    features: mapping.features,
                }
            }
        }
    }

    fn history_summary(&self, history: &history::History) -> String {
        let summary = format!(
            "{} of {} features rebuilt",
            history.rebuilt(),
            history.needing()
        );
        let uses_target = history.features.iter().any(|feature| {
            (feature.get("type").and_then(Value::as_str) == Some("coil")
                && feature.get("sketch").is_some())
                || feature.pointer("/hole/tip_angle").and_then(Value::as_str) == Some("180")
        });
        if uses_target {
            format!(
                "{summary}; target SolidCraft {}",
                self.resolved_version().minor()
            )
        } else {
            summary
        }
    }

    fn import_design(
        &self,
        model: &Model,
        step: &str,
        references: Option<&history::History>,
        report: &mut Report,
    ) -> Result<Vec<u8>, Error> {
        let references = references.filter(|_| self.sketches);
        document::report_unmapped(model, references, report);
        let design = container::Container::new(step.as_bytes())?;
        let mut doc = document::import_design(model, design.step_blob(), self.should_stand_up());
        let looks = history::imported(model, step, report, false)?;
        document::import_looks(&mut doc, &looks);
        if let Some(references) = references {
            document::keep_sketches(&mut doc, references, model);
            let kept = references.features.len();
            let left_out = model.ir.model.sketches.len().saturating_sub(kept);
            if kept > 0 || left_out > 0 {
                let noun = if kept == 1 { "sketch" } else { "sketches" };
                let detail = if left_out > 0 {
                    format!("kept {kept} {noun} for reference; {left_out} left out")
                } else {
                    format!("kept {kept} {noun} for reference")
                };
                report.push(Severity::Info, "sketches", detail);
            }
        }
        design.finish(&doc)
    }

    fn history_design(
        &self,
        model: &Model,
        history: &history::History,
        step: &str,
        live: Live,
        stand_up: bool,
    ) -> Result<Vec<u8>, Error> {
        self.history_design_report(model, history, step, live, stand_up, &mut Report::default())
    }

    fn history_design_report(
        &self,
        model: &Model,
        history: &history::History,
        step: &str,
        live: Live,
        stand_up: bool,
        report: &mut Report,
    ) -> Result<Vec<u8>, Error> {
        let mut design = if live == Live::Whole {
            container::Container::empty()
        } else {
            container::Container::new(step.as_bytes())?
        };
        let mut history = history.clone();
        history.preserve_import_names();
        history::base::store(&mut history.features, &mut design)?;
        let mut doc: Value = document::history_design(
            model,
            &history,
            design.step_blob(),
            live,
            self.resolved_version(),
            stand_up,
        );
        if matches!(live, Live::Partial | Live::Unverified) {
            let mut looks = history::imported(model, step, report, false)?;
            let mut taken: history::BodyNames = history
                .features
                .iter()
                .filter_map(|feature| feature.get("body_names").and_then(Value::as_array))
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            taken.add_defaults(history.default_bodies.keys().cloned());
            looks.rename_bodies("Original geometry", &mut taken);
            document::original_looks(&mut doc, &looks);
        }
        linked::store(&mut doc, &mut design)?;
        design.finish(&doc)
    }

    /// What inspecting says of the blends, which it does not replay: refused when no
    /// SolidCraft CLI would find their edges, as writing would refuse them.
    fn plan_blends(&self, mapping: &mut history::Mapping) {
        if self.evaluator.is_none() || matches!(self.replay, Verification::Off) {
            if let Err(why) = verify::find_cli(&self.replay) {
                history::prove_blends(
                    mapping,
                    |_, _, _| Err(why.clone()),
                    |_, _| history::infer::Fit::Unknown,
                );
            }
        }
    }

    /// Finds the input edges of the history's blends on SolidCraft's replay of the history
    /// written so far (two base CLI runs, plus a carrier pass for closed circles, within
    /// the timeout), or
    /// refuses them, saying why.
    fn prove_blends(
        &self,
        model: &Model,
        mapping: &mut history::Mapping,
        step: &str,
        progress: Option<&dutchman_core::Progress<'_>>,
    ) -> Result<(), Error> {
        let mut remaining = self.timeout;
        let mut failure = None;
        let mut circles = false;
        history::prove_blends(
            mapping,
            |history, after, faces| {
                circles |= history
                    .blends
                    .iter()
                    .any(|pending| pending.needs_circle_carriers());
                if let Some(why) = self.memory_refusal() {
                    return Err(why);
                }
                let cli = self
                    .selected_evaluator(&self.replay)
                    .map_err(|why| why.to_string())?;
                // SolidCraft evaluates a whole design when it opens it, rolled back or not, so
                // the replay gets the history only up to the last step it inspects: a slow later
                // feature would otherwise cost every proof run its full time.
                let prefix = through_last(history, after);
                let history = prefix.as_ref().unwrap_or(history);
                let design = match self.history_design(model, history, step, Live::History, false) {
                    Ok(design) => design,
                    Err(err) => {
                        let why = err.to_string();
                        failure = Some(err);
                        return Err(why);
                    }
                };
                let _checking = progress.map(|p| {
                    p.step(
                        dutchman_core::StepKind::CheckingBlends,
                        Some(&model.name),
                        Some(remaining),
                        Some(dutchman_core::progress_words::blend_limit()),
                    )
                });
                let started = dutchman_core::time::Instant::now();
                let replayed =
                    verify::replay_edges(cli.as_ref(), remaining, &design, after, faces, circles);
                remaining = remaining.saturating_sub(started.elapsed());
                replayed
            },
            |history, budget| {
                self.bounded_profile_check(model, history, step, None, true, budget, progress)
            },
        );
        match failure {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    /// Share the feature's remaining budget across original, replay and surface measurements.
    #[allow(clippy::too_many_arguments)]
    fn bounded_profile_check(
        &self,
        model: &Model,
        prefix: &history::History,
        step: &str,
        stored: Option<&[verify::StoredBody]>,
        final_state: bool,
        budget: Duration,
        progress: Option<&dutchman_core::Progress<'_>>,
    ) -> history::infer::Fit {
        let started = dutchman_core::time::Instant::now();
        let Ok(evaluator) = self.selected_evaluator(&self.verification) else {
            return history::infer::Fit::Unknown;
        };
        let mut writer = self.clone();
        writer.evaluator = Some(std::sync::Arc::new(evaluator::Budgeted::new(
            evaluator,
            budget.saturating_sub(started.elapsed()),
        )));
        writer.profile_check(model, prefix, step, stored, final_state, true, progress)
    }

    /// Measure an inferred profile with the same comparison used for unconfirmed whole-sketch
    /// profiles. Unknown measurements never confirm an inference. At the final body-building
    /// feature, compare the complete replay with the source as well; with `compare`, also load
    /// the source's solid beside a one-body replay for the distance between their surfaces.
    #[allow(clippy::too_many_arguments)]
    fn profile_check(
        &self,
        model: &Model,
        prefix: &history::History,
        step: &str,
        stored: Option<&[verify::StoredBody]>,
        final_state: bool,
        compare: bool,
        progress: Option<&dutchman_core::Progress<'_>>,
    ) -> history::infer::Fit {
        use dutchman_core::InferredAgainst;
        use history::infer::{Fit, Measure};
        if self.memory_refusal().is_some() {
            return Fit::Unknown;
        }
        let Ok(cli) = self.selected_evaluator(&self.verification) else {
            return Fit::Unknown;
        };
        let _measuring = progress.map(|p| p.measuring());
        let Ok(design) = self.history_design(model, prefix, step, Live::History, false) else {
            return Fit::Unknown;
        };
        if final_state {
            return match self.check_measurements(model, prefix, step, &design, progress, compare) {
                Ok(verify::Outcome::Matches) => {
                    let one_body = solid_bodies(model) == 1 && sheet_bodies(model) == 0;
                    let deviation = if compare {
                        if !one_body {
                            return Fit::Unknown;
                        }
                        let Some(deviation) =
                            verify::deviation(cli.as_ref(), self.timeout, &design, prefix.next_id)
                                .filter(|deviation| deviation.faces.is_some())
                        else {
                            return Fit::Unknown;
                        };
                        Some(deviation)
                    } else {
                        None
                    };
                    Fit::Matches(Measure {
                        against: InferredAgainst::OriginalShape,
                        deviation,
                    })
                }
                Ok(verify::Outcome::Differs(why)) => Fit::Differs(why),
                Ok(verify::Outcome::Unverified(_)) | Err(_) => Fit::Unknown,
            };
        }
        let Some(stored) = stored.filter(|stored| !stored.is_empty()) else {
            return Fit::Unknown;
        };
        let (outcome, bodies) = verify::replay(cli.as_ref(), self.timeout, &design);
        match (outcome, bodies) {
            (verify::Outcome::Matches, Some(bodies))
                if bodies
                    .iter()
                    .all(|body| body.center.is_some() && body.size.is_some()) =>
            {
                match verify::bodies_differ(Some(stored), Some(&bodies)) {
                    None => Fit::Matches(Measure {
                        against: InferredAgainst::StoredBodies,
                        deviation: None,
                    }),
                    Some(why) => Fit::Differs(why),
                }
            }
            (verify::Outcome::Differs(why), _) => Fit::Differs(why),
            _ => Fit::Unknown,
        }
    }

    /// Count the regions the written sketch has. `sketch.inspect` supplies centroids,
    /// which do not prove interior selections, so these regions cannot yet be inferred.
    fn sketch_regions(
        &self,
        model: &Model,
        prefix: &history::History,
        step: &str,
        sketch: u64,
        budget: Duration,
    ) -> Option<usize> {
        let started = dutchman_core::time::Instant::now();
        if self.memory_refusal().is_some() {
            return None;
        }
        let cli = self.selected_evaluator(&self.verification).ok()?;
        let cli = evaluator::Budgeted::new(cli, budget.saturating_sub(started.elapsed()));
        let design = self
            .history_design(model, prefix, step, Live::History, false)
            .ok()?;
        let script = [json!({"command": "sketch.inspect", "params": {"sketch": sketch}})];
        let results = cli
            .run(&design, &script, self.timeout, "sketch regions")
            .ok()?;
        Some(results.first()?.get("profiles")?.as_array()?.len())
    }

    /// Keeps the prefix before SolidCraft's first error or a measured geometry difference.
    /// Replaying successfully cannot establish that a differing body is correct.
    fn partial(
        &self,
        model: &Model,
        mapping: &mut history::Mapping,
        step: &str,
        why: Option<String>,
        report: &mut Report,
        progress: Option<&dutchman_core::Progress<'_>>,
    ) -> Result<Check, Error> {
        if let Some(why) = why {
            let error = why
                .strip_prefix("replay differs (")
                .and_then(|s| s.strip_suffix(')'))
                .unwrap_or(&why);
            let identified = self.stop_replay(mapping, error);
            // An unverified pattern is represented as a replay difference so its
            // original stays visible; it is not a measured geometry mismatch.
            let measured = why.starts_with("replay differs (")
                && !error.starts_with("the pattern instance set could not be verified:");
            if !identified && measured {
                Self::stop_geometry_difference(mapping, why);
            } else if !identified && !unconfirmed_stop(mapping) {
                if let Ok(history) = &mut mapping.result {
                    history.stop = Some(why);
                }
            }
        }
        let cli = self.selected_evaluator(&self.verification);
        let started = dutchman_core::time::Instant::now();
        let _checking = progress.map(|p| {
            p.step(
                dutchman_core::StepKind::CheckingPrefix,
                Some(&model.name),
                Some(self.timeout),
                Some(dutchman_core::progress_words::prefix_limit()),
            )
        });
        for _ in 0..64 {
            let Ok(history) = &mapping.result else { break };
            let summary = partial_summary(history);
            if let Some(why) = self.memory_refusal() {
                return Ok(Check::Partial(format!(
                    "{summary}; prefix unverified: {why}"
                )));
            }
            let cli = match &cli {
                Ok(cli) => cli,
                Err(why) => {
                    return Ok(Check::Partial(format!(
                        "{summary}; prefix unverified: {why}"
                    )))
                }
            };
            let Some(remaining) = self.timeout.checked_sub(started.elapsed()) else {
                return Ok(Check::Partial(format!(
                    "{summary}; prefix unverified: replay timed out"
                )));
            };
            let design = self.history_design(model, history, step, Live::History, false)?;
            if let Some(why) = self.memory_refusal() {
                return Ok(Check::Partial(format!("{summary}; {why}")));
            }
            let (outcome, bodies) = verify::replay(cli.as_ref(), remaining, &design);
            match outcome {
                verify::Outcome::Matches => {
                    let stored = history.stored_bodies_at_end();
                    let differs = verify::bodies_differ(stored, bodies.as_deref());
                    // A whole-sketch profile of several stays only when the source's bodies
                    // at this point are known and the replay holds them.
                    let confirmed = differs.is_none() && stored.is_some() && bodies.is_some();
                    // The centres the source stores cannot tell a region chosen without proof
                    // from the right one when both are centred alike. The final shape can,
                    // where nothing after the prefix builds material.
                    let conflict = if differs.is_none() {
                        let budget = self.timeout.saturating_sub(started.elapsed());
                        self.prefix_conflict(model, history, step, cli.as_ref(), budget)
                    } else {
                        None
                    };
                    if !confirmed && unconfirmed_stop(mapping) {
                        continue;
                    }
                    if let Some(why) = differs {
                        report.push_coded(
                            Severity::Degraded,
                            model.name.as_str(),
                            "history/body-mismatch",
                            &why,
                        );
                        Self::stop_geometry_difference(mapping, format!("replay differs ({why})"));
                        continue;
                    }
                    if let Some(why) = conflict {
                        report.push_coded(
                            Severity::Degraded,
                            model.name.as_str(),
                            "history/body-mismatch",
                            &why,
                        );
                        Self::stop_geometry_difference(mapping, format!("replay differs ({why})"));
                        continue;
                    }
                    return Ok(Check::Partial(format!("{summary}; retained features replay in SolidCraft; the full history is not verified")));
                }
                verify::Outcome::Unverified(why) => {
                    return Ok(Check::Partial(format!(
                        "{summary}; prefix unverified: {why}"
                    )))
                }
                verify::Outcome::Differs(why) => {
                    if !self.stop_replay(mapping, &why) {
                        mapping.result = Err(format!("cannot replay retained history: {why}"));
                        break;
                    }
                }
            }
        }
        if mapping.result.is_ok() {
            mapping.result = Err("retained history exceeded the replay repair limit".to_owned());
        }
        Ok(Check::Flattened("no replayable prefix remains".to_owned()))
    }

    /// Why the replay of the retained `history` cannot be the state before the source features
    /// after it: its box or material falls short of the source's final shape, though none of
    /// those features builds material. `None` when they might, or when it cannot be measured.
    fn prefix_conflict(
        &self,
        model: &Model,
        history: &history::History,
        step: &str,
        cli: &(impl SolidCraftEvaluator + ?Sized),
        budget: Duration,
    ) -> Option<String> {
        let removes = history.unreachable_prefix()?;
        if budget.is_zero() || self.memory_refusal().is_some() {
            return None;
        }
        let source = self
            .history_design(model, history, step, Live::Original, false)
            .ok()?;
        let prefix = self
            .history_design(model, history, step, Live::History, false)
            .ok()?;
        let source = verify::extents(cli, budget, "original", &source)?;
        let prefix = verify::extents(cli, budget, "prefix", &prefix)?;
        verify::prefix_misses_source(&prefix, &source, removes)
    }

    /// A final-body difference does not identify the operation that introduced it.
    /// Keep source imports before the first unconfirmed body edit, and independent
    /// sketches; a replayable but unproven edited body must not survive in the file.
    fn stop_geometry_difference(mapping: &mut history::Mapping, reason: String) {
        let source = mapping.result.as_ref().ok().and_then(|history| {
            history
                .features
                .iter()
                .find_map(|feature| {
                    if matches!(
                        feature.get("type").and_then(Value::as_str),
                        Some("sketch" | "import")
                    ) {
                        return None;
                    }
                    history.sources.get(&feature.get("id")?.as_u64()?).copied()
                })
                .or_else(|| {
                    history.features.iter().find_map(|feature| {
                        if feature.get("type").and_then(Value::as_str) == Some("sketch") {
                            return None;
                        }
                        history.sources.get(&feature.get("id")?.as_u64()?).copied()
                    })
                })
        });
        if let Some(source) = source {
            history::shorten(mapping, source, reason);
        } else {
            // Even a stored body must be discarded if it is itself the differing
            // result. The final import is the only confirmed geometry in this case.
            mapping.result = Err(reason);
        }
    }

    fn stop_replay(&self, mapping: &mut history::Mapping, why: &str) -> bool {
        let Ok(history) = &mut mapping.result else {
            return false;
        };
        let failed = history
            .features
            .iter()
            .enumerate()
            .find(|(_, feature)| {
                feature
                    .get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| why.starts_with(&format!("feature \"{name}\" failed:")))
            })
            .map(|(at, _)| at);
        if let Some(at) = failed {
            let source = history
                .features
                .get(at)
                .and_then(|f| f.get("id"))
                .and_then(Value::as_u64)
                .and_then(|id| history.sources.get(&id))
                .copied();
            let mut repaired = false;
            let mut dropped = 0usize;
            if history
                .features
                .get(at)
                .and_then(|f| f.get("type"))
                .and_then(Value::as_str)
                == Some("sketch")
            {
                // Retry the solved geometry without constraints before giving up its sketch.
                for key in ["constraints", "wire_constraints"] {
                    if let Some(constraints) = history
                        .features
                        .get_mut(at)
                        .and_then(|f| f.get_mut("sketch"))
                        .and_then(|s| s.get_mut(key))
                        .and_then(Value::as_array_mut)
                        .filter(|constraints| !constraints.is_empty())
                    {
                        dropped += constraints.len();
                        constraints.clear();
                        repaired = true;
                        if key == "wire_constraints" {
                            if let Some(sketch) = history
                                .features
                                .get_mut(at)
                                .and_then(|f| f.get_mut("sketch"))
                                .and_then(Value::as_object_mut)
                            {
                                sketch.insert("wires_joined".to_owned(), Value::Bool(false));
                            }
                        }
                    }
                }
                if repaired {
                    if let Some(sketch) = history.features.get(at) {
                        if let Some(reference) = mapping
                            .references
                            .features
                            .iter_mut()
                            .find(|f| f.get("id") == sketch.get("id"))
                        {
                            *reference = sketch.clone();
                        }
                    }
                    if let Some(feature) =
                        source.and_then(|source| mapping.features.get_mut(source))
                    {
                        feature.reason = Some(format!("{} not carried after replay failed: {why}; its solved geometry is retained", count(dropped, "constraint", "constraints")));
                    }
                } else {
                    let removed = history.features.remove(at);
                    mapping
                        .references
                        .features
                        .retain(|feature| feature.get("id") != removed.get("id"));
                    if let Some(feature) =
                        source.and_then(|source| mapping.features.get_mut(source))
                    {
                        feature.status = FeatureStatus::Blocked;
                        feature.reason = Some(why.to_owned());
                    }
                }
            }
            if let Some(source) = source {
                history::shorten(mapping, source, why.to_owned());
            } else {
                let detail = if repaired {
                    format!(
                        "a sketch's {} not carried after replay failed: {why}",
                        count(dropped, "constraint", "constraints")
                    )
                } else {
                    format!("a sketch was left out: {why}")
                };
                history.stop = Some(match history.stop.take() {
                    Some(stop) => format!("{stop}; {detail}"),
                    None => detail,
                });
            }
            true
        } else {
            history.stop = Some(why.to_owned());
            false
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn check(
        &self,
        model: &Model,
        history: &history::History,
        step: &str,
        written: &[u8],
        progress: Option<&dutchman_core::Progress<'_>>,
        report: &mut Report,
    ) -> Result<verify::Outcome, Error> {
        let unavailable = |reason: String, report: &mut Report| {
            verify::record_unavailable(report, &model.name, &reason);
            verify::Outcome::Unverified(reason)
        };
        if let Some(why) = self.memory_refusal() {
            return Ok(unavailable(why, report));
        }
        let cli = match self.selected_evaluator(&self.verification) {
            Ok(cli) => cli,
            Err(why) => return Ok(unavailable(why.to_string(), report)),
        };
        let original = self.history_design(model, history, step, Live::Original, false)?;
        if let Some(why) = self.memory_refusal() {
            return Ok(unavailable(why, report));
        }
        let result = verify::compare_report(
            cli.as_ref(),
            self.timeout,
            written,
            &original,
            verify::Expected {
                solids: solid_bodies(model),
                sheets: sheet_bodies(model),
                single_body_regions: single_body_regions(model),
            },
            progress,
            model,
            dutchman_core::progress_words::history_limit(
                history.pattern_replay_required,
                self.keep_unverified_hidden(),
            ),
            report,
        );
        Ok(match result {
            verify::Outcome::Unverified(why) if history.pattern_replay_required => {
                verify::Outcome::Differs(format!(
                    "the pattern instance set could not be verified: {why}"
                ))
            }
            result => result,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn check_measurements(
        &self,
        model: &Model,
        history: &history::History,
        step: &str,
        written: &[u8],
        progress: Option<&dutchman_core::Progress<'_>>,
        complete: bool,
    ) -> Result<verify::Outcome, Error> {
        if let Some(why) = self.memory_refusal() {
            return Ok(verify::Outcome::Unverified(why));
        }
        let cli = match self.selected_evaluator(&self.verification) {
            Ok(cli) => cli,
            Err(why) => return Ok(verify::Outcome::Unverified(why.to_string())),
        };
        let original = self.history_design(model, history, step, Live::Original, false)?;
        if let Some(why) = self.memory_refusal() {
            return Ok(verify::Outcome::Unverified(why));
        }
        let compare = if complete {
            verify::compare_complete
        } else {
            verify::compare_with_progress
        };
        let result = compare(
            cli.as_ref(),
            self.timeout,
            written,
            &original,
            verify::Expected {
                solids: solid_bodies(model),
                sheets: sheet_bodies(model),
                single_body_regions: single_body_regions(model),
            },
            progress,
            &model.name,
            dutchman_core::progress_words::history_limit(
                history.pattern_replay_required,
                self.keep_unverified_history_hidden(history),
            ),
        );
        Ok(match result {
            // New inference must retain Unknown so an unmeasured candidate cannot be
            // treated as a nonmatch when proving uniqueness.
            verify::Outcome::Unverified(why) if history.pattern_replay_required && !complete => {
                verify::Outcome::Differs(format!(
                    "the pattern instance set could not be verified: {why}"
                ))
            }
            result => result,
        })
    }
}

/// The writer's SolidCraft measurements, as measured inference asks for them.
pub(crate) struct Measuring<'a, 'p> {
    pub(crate) writer: &'a SolidCraftWriter,
    pub(crate) model: &'a Model,
    pub(crate) step: &'a str,
    pub(crate) progress: Option<&'a dutchman_core::Progress<'p>>,
}

impl history::infer::Host for Measuring<'_, '_> {
    fn fit(
        &mut self,
        prefix: &history::History,
        stored: Option<&[verify::StoredBody]>,
        final_state: bool,
        compare: bool,
    ) -> history::infer::Fit {
        self.writer.profile_check(
            self.model,
            prefix,
            self.step,
            stored,
            final_state,
            compare,
            self.progress,
        )
    }

    fn fit_bounded(
        &mut self,
        prefix: &history::History,
        stored: Option<&[verify::StoredBody]>,
        final_state: bool,
        budget: Duration,
    ) -> history::infer::Fit {
        self.writer.bounded_profile_check(
            self.model,
            prefix,
            self.step,
            stored,
            final_state,
            budget,
            self.progress,
        )
    }

    fn regions(
        &mut self,
        prefix: &history::History,
        sketch: u64,
        budget: Duration,
    ) -> Option<usize> {
        self.writer
            .sketch_regions(self.model, prefix, self.step, sketch, budget)
    }
}

fn report_sketches(features: &[FeatureOutcome], report: &mut Report) {
    for feature in features.iter().filter(|f| {
        (f.kind == "sketch"
            || (f.kind == "spatial_sketch"
                && f.reason
                    .as_deref()
                    .is_some_and(|r| r.contains("not carried after replay failed"))))
            && (f.status == FeatureStatus::Blocked || f.reason.is_some())
    }) {
        report.push(
            Severity::Degraded,
            format!(
                "sketch \"{}\"",
                feature.name.as_deref().unwrap_or("Unnamed")
            ),
            if feature.status == FeatureStatus::Blocked {
                format!(
                    "sketch left out: {}",
                    feature
                        .reason
                        .as_deref()
                        .unwrap_or("its geometry is not resolved")
                )
            } else {
                feature.reason.clone().unwrap_or_default()
            },
        );
    }
}

/// How a design whose history is live ends: with the original geometry, unless the history
/// already is that geometry.
fn final_live(history: &history::History, model: &Model) -> Live {
    if history::base::whole(history, model) {
        Live::Whole
    } else {
        Live::History
    }
}

/// `history` ending at the last of its features whose id is in `after`, or `None` when that
/// is its last feature already (or none of them is in it).
fn through_last(history: &history::History, after: &[u64]) -> Option<history::History> {
    let last = history.features.iter().rposition(|feature| {
        feature
            .get("id")
            .and_then(Value::as_u64)
            .is_some_and(|id| after.contains(&id))
    })?;
    if last.saturating_add(1) >= history.features.len() {
        return None;
    }
    let mut prefix = history.clone();
    prefix.features.truncate(last.saturating_add(1));
    Some(prefix)
}

/// Cuts the history before its first feature whose whole-sketch profile nothing confirmed.
/// False when it has none.
fn unconfirmed_stop(mapping: &mut history::Mapping) -> bool {
    let Some((source, why)) = mapping
        .result
        .as_ref()
        .ok()
        .and_then(history::History::first_unconfirmed)
    else {
        return false;
    };
    history::shorten(mapping, source, why);
    true
}

fn partial_summary(history: &history::History) -> String {
    format!(
        "history stopped at {} ({} of {} features rebuilt); the rest is the imported final shape",
        history.stop.as_deref().unwrap_or("a replay difference"),
        history.rebuilt(),
        history.needing()
    )
}

/// How many sheet bodies an assembly places, through its sub-assemblies, each as often as it
/// is placed.
fn placed_sheets(assembly: &dutchman_core::Assembly, depth: usize) -> usize {
    if depth > 64 {
        return 0;
    }
    assembly
        .placements
        .iter()
        .filter_map(|placement| assembly.parts.get(placement.part))
        .map(|part| {
            sheet_bodies(part).saturating_add(
                part.assembly
                    .as_ref()
                    .map_or(0, |inner| placed_sheets(inner, depth + 1)),
            )
        })
        .fold(0, usize::saturating_add)
}

/// How many of the model's bodies are sheets: surfaces, open by design.
fn sheet_bodies(model: &Model) -> usize {
    use cadmpeg_ir::topology::BodyKind;
    model
        .ir
        .geometry
        .bodies
        .iter()
        .filter(|body| matches!(body.kind, BodyKind::Sheet | BodyKind::General))
        .count()
}

/// Whether the mapped timeline only retains bodies without a construction recipe.
fn only_stored_geometry(features: &[FeatureOutcome]) -> bool {
    features.iter().any(|f| f.status == FeatureStatus::Mapped)
        && features
            .iter()
            .all(|f| f.status != FeatureStatus::Mapped || f.kind == "stored_geometry")
}

/// Why a design has no history to write, when cadmpeg decodes none of its features: every one
/// it keeps (suppressed ones and those SolidCraft needs none for aside) is in its native form,
/// as a part made of imported or base bodies is. Such a design converts as geometry with no
/// history lost, rather than as a history flattened.
fn no_decoded_history(features: &[FeatureOutcome]) -> Option<String> {
    let kept: Vec<&FeatureOutcome> = features
        .iter()
        .filter(|f| matches!(f.status, FeatureStatus::Mapped | FeatureStatus::Blocked))
        .collect();
    let native = kept
        .iter()
        .all(|f| f.status == FeatureStatus::Blocked && f.kind == "native");
    (!kept.is_empty() && native).then(|| {
        format!(
            "no feature history cadmpeg decodes: {}",
            if kept.len() == 1 {
                "its one feature is kept in its native form".to_owned()
            } else {
                format!("its {} features are kept in their native form", kept.len())
            }
        )
    })
}

/// Whether `model` is a part holding parameters and nothing else: no features and no
/// geometry SolidCraft can import. It is a design of parameters other designs use, written
/// as a design of parameters alone.
fn parameters_only(model: &Model) -> bool {
    model.assembly.is_none()
        && !model.ir.model.parameters.is_empty()
        && model.ir.model.features.is_empty()
        && matches!(step::restatable(model), Err(Error::NoGeometry(_)))
}

/// A neutral timeline that builds sketches and no bodies needs no STEP fallback.
fn sketches_only(model: &Model) -> bool {
    use cadmpeg_ir::features::FeatureOperation;
    model.assembly.is_none()
        && model.source_step.is_none()
        && model.ir.geometry.bodies.is_empty()
        && model.ir.geometry.faces.is_empty()
        && !model.ir.model.sketches.is_empty()
        && model.ir.model.features.iter().any(|feature| {
            feature.suppressed != Some(true)
                && matches!(
                    feature.evaluation.definition().operation(),
                    FeatureOperation::Sketch { .. }
                )
        })
        && model.ir.model.features.iter().all(|feature| {
            feature.suppressed == Some(true)
                || matches!(
                    feature.evaluation.definition().operation(),
                    FeatureOperation::Sketch { .. } | FeatureOperation::TreeNode { .. }
                )
        })
}

/// Whether any part `assembly` places, at any depth, has geometry SolidCraft can import.
fn any_part(assembly: &dutchman_core::Assembly) -> bool {
    assembly.parts.iter().any(|part| match &part.assembly {
        Some(sub) => any_part(sub),
        None => step::restatable(part).is_ok(),
    })
}

/// What the report says when an assembly is written as one part by [`flattened_assembly`].
const FLATTENED_ASSEMBLY: &str = "the assembly's placed parts hold no geometry SolidCraft can \
    import, so the design's own bodies are written as one part without component placements";

/// `model` without its assembly, when the assembly places parts but none of them has geometry
/// SolidCraft can import while the design's own bodies do: those bodies are then written as
/// one part instead of failing the conversion.
fn flattened_assembly(model: &Model) -> Option<Model> {
    let assembly = model.assembly.as_ref()?;
    if assembly.placements.is_empty() || model.ir.geometry.bodies.is_empty() || any_part(assembly) {
        return None;
    }
    // Restating reads only the design's own geometry, never its assembly.
    step::restatable(model).ok()?;
    Some(Model {
        assembly: None,
        ..model.clone()
    })
}

/// Whether `model` is an assembly that places no parts, written as an empty design.
fn empty_assembly(model: &Model) -> bool {
    model
        .assembly
        .as_ref()
        .is_some_and(|assembly| assembly.placements.is_empty())
}

/// What writing an assembly that places no parts does.
fn empty_outcome() -> Outcome {
    Outcome {
        mode: Mode::Import,
        check: Check::NotApplicable("the assembly places no parts".to_owned()),
        features: Vec::new(),
    }
}

/// What writing a design of parameters alone does.
fn parameters_outcome(model: &Model) -> Outcome {
    Outcome {
        mode: Mode::Import,
        check: Check::NotApplicable(format!(
            "the design holds {} and no geometry",
            count(model.ir.model.parameters.len(), "parameter", "parameters")
        )),
        features: Vec::new(),
    }
}

/// `n` and the noun for one or for many.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// How many of the model's bodies are solids, each of which should have a volume.
fn solid_bodies(model: &Model) -> usize {
    model
        .ir
        .geometry
        .bodies
        .iter()
        .filter(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid)
        .count()
}

/// Only one solid body gives an unambiguous owner for all imported regions.
/// Keep the ordinary per-piece comparison for mixed or multiple source bodies.
fn single_body_regions(model: &Model) -> usize {
    let geometry = &model.ir.geometry;
    match geometry.bodies.as_slice() {
        [body] if body.kind == cadmpeg_ir::topology::BodyKind::Solid => geometry
            .regions
            .iter()
            .filter(|region| region.body == body.id)
            .count(),
        _ => 0,
    }
}

impl SolidCraftWriter {
    /// Opens geometry written as an import in SolidCraft, when a SolidCraft CLI is found, and
    /// reports it as Degraded when SolidCraft shows less of it than the model holds: geometry
    /// that is wrong but opens would otherwise go unnoticed.
    fn check_import(
        &self,
        model: &Model,
        design: &[u8],
        report: &mut Report,
        progress: Option<&dutchman_core::Progress<'_>>,
        evaluator: Option<&std::sync::Arc<dyn SolidCraftEvaluator>>,
    ) {
        let expected = verify::Expected {
            solids: solid_bodies(model).max(single_body_regions(model)),
            sheets: sheet_bodies(model),
            single_body_regions: single_body_regions(model),
        };
        self.check_geometry(design, expected, report, progress, &model.name, evaluator);
    }

    #[allow(clippy::too_many_arguments)]
    fn check_geometry(
        &self,
        design: &[u8],
        expected: verify::Expected,
        report: &mut Report,
        progress: Option<&dutchman_core::Progress<'_>>,
        name: &str,
        evaluator: Option<&std::sync::Arc<dyn SolidCraftEvaluator>>,
    ) {
        if let Some(why) = self.memory_refusal() {
            report.push(Severity::Info, HISTORY, why);
            return;
        }
        let cli = match evaluator
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| self.selected_evaluator(&self.verification))
        {
            Ok(cli) => cli,
            // Report a CLI that was found but cannot check the design.
            Err(verify::NoCli::TooOld(why) | verify::NoCli::Blocked(why)) => {
                report.push_at(
                    Stage::SolidCraft,
                    Severity::Info,
                    GEOMETRY,
                    format!("not checked: {why}"),
                );
                return;
            }
            Err(verify::NoCli::NotFound(_)) => return,
        };
        if let Some(why) = self.memory_refusal() {
            report.push(Severity::Info, HISTORY, why);
            return;
        }
        let _checking = progress.map(|p| {
            p.step(
                dutchman_core::StepKind::CheckingGeometry,
                Some(name),
                Some(self.timeout),
                Some(dutchman_core::progress_words::geometry_limit()),
            )
        });
        let shown = verify::check_import(cli.as_ref(), self.timeout, design, expected);
        if expected.sheets > 0 && !matches!(shown, verify::Shown::Unchecked(_)) {
            report.push_at(
                Stage::SolidCraft,
                Severity::Info,
                GEOMETRY,
                format!(
                    "{} surface bod{} (sheets, open by design) not checked for a volume or a \
                     closed shell",
                    expected.sheets,
                    if expected.sheets == 1 { "y" } else { "ies" }
                ),
            );
        }
        match shown {
            verify::Shown::Whole => {}
            verify::Shown::Short(what) => {
                report.push_at(Stage::SolidCraft, Severity::Degraded, GEOMETRY, what);
            }
            verify::Shown::Unchecked(why) => {
                report.push_at(
                    Stage::SolidCraft,
                    Severity::Info,
                    GEOMETRY,
                    format!("not checked: {why}"),
                );
            }
        }
    }
}

impl Writer for SolidCraftWriter {
    fn companion_files(&self, encoded: &[u8]) -> Result<Vec<(String, Vec<u8>)>, Error> {
        linked::companions(encoded)
    }

    fn extension(&self) -> &'static str {
        self.resolved_version().extension()
    }

    fn accepts_extension(&self, extension: &str) -> bool {
        ["sc", "solidcraft", "solvecraft"]
            .iter()
            .any(|suffix| extension.eq_ignore_ascii_case(suffix))
    }

    fn write(
        &self,
        model: &Model,
        out: &mut dyn std::io::Write,
        report: &mut Report,
    ) -> Result<(), Error> {
        self.write_with_progress(model, out, report, None)
    }

    fn write_with_progress(
        &self,
        model: &Model,
        out: &mut dyn std::io::Write,
        report: &mut Report,
        progress: Option<&dutchman_core::Progress<'_>>,
    ) -> Result<(), Error> {
        document::report_scoped_attributes(model, report);
        let _preparing = progress
            .filter(|_| model.assembly.is_none())
            .map(|p| p.stage_step(dutchman_core::StepKind::Preparing, Some(&model.name)));
        #[cfg(all(debug_assertions, not(target_family = "wasm")))]
        if model.assembly.is_none() {
            slow_part(&model.name);
        }
        if let Some(flat) = flattened_assembly(model) {
            report.push(Severity::Degraded, GEOMETRY, FLATTENED_ASSEMBLY);
            return self.write_with_progress(&flat, out, report, progress);
        }
        report.set_threads(self.threads, 1);
        if empty_assembly(model) {
            report.push(
                Severity::Info,
                GEOMETRY,
                "none: the assembly places no parts",
            );
            let bytes =
                container::Container::empty().finish(&document::parameters_design(model, &[]))?;
            report.set_outcome(empty_outcome());
            return out
                .write_all(&bytes)
                .map_err(|err| Error::Encode(format!("cannot write the design: {err}")));
        }
        let final_evaluator = self.final_evaluator();
        if let Some(assembly) = &model.assembly {
            let bytes = assembly::write(self, model, assembly, report, progress)?;
            let bytes = self.check_opening(bytes, report, final_evaluator.as_ref())?;
            // Each part is placed any number of times, so only SolidCraft's own warnings count.
            let expected = verify::Expected {
                sheets: placed_sheets(assembly, 0),
                ..verify::Expected::default()
            };
            self.check_geometry(
                &bytes,
                expected,
                report,
                progress,
                &model.name,
                final_evaluator.as_ref(),
            );
            return out
                .write_all(&bytes)
                .map_err(|err| Error::Encode(format!("cannot write the design: {err}")));
        }
        if parameters_only(model) {
            let (params, _) = history::params::map(model, report);
            report.push(
                Severity::Info,
                GEOMETRY,
                format!(
                    "none: the design holds {} and no geometry",
                    count(params.len(), "parameter", "parameters")
                ),
            );
            let bytes = container::Container::empty()
                .finish(&document::parameters_design(model, &params))?;
            report.set_outcome(parameters_outcome(model));
            return out
                .write_all(&bytes)
                .map_err(|err| Error::Encode(format!("cannot write the design: {err}")));
        }
        if sketches_only(model) {
            if self.import_only || !self.sketches {
                return Err(Error::NoGeometry("the design contains only sketches; enable sketches and editable history to convert it".to_owned()));
            }
            let mapping = history::map_with_sketches(model, report, self.resolved_version(), true);
            let history = mapping.result.map_err(Error::Encode)?;
            if let Some(reason) = &history.stop {
                return Err(Error::Encode(format!(
                    "cannot write the sketch-only design: {reason}"
                )));
            }
            let bytes = self.history_design_report(
                model,
                &history,
                "",
                Live::Whole,
                self.should_stand_up(),
                report,
            )?;
            report.set_outcome(Outcome {
                mode: Mode::History,
                check: self.check_sketches(&bytes, final_evaluator.as_ref()),
                features: mapping.features,
            });
            let bytes = self.check_opening(bytes, report, final_evaluator.as_ref())?;
            return out
                .write_all(&bytes)
                .map_err(|err| Error::Encode(format!("cannot write the design: {err}")));
        }
        let step = match &model.source_step {
            // A STEP input goes to SolidCraft as it is: restating it would only lose detail.
            Some(text) => {
                report.push(
                    Severity::Info,
                    GEOMETRY,
                    "the STEP file is passed to SolidCraft as it is",
                );
                text.clone()
            }
            None => step::restate(model, report)?,
        };
        let (bytes, outcome) = if self.import_only {
            report.push(
                Severity::Info,
                HISTORY,
                format!("not written ({})", self.import_only_reason),
            );
            let outcome = Outcome {
                mode: Mode::Import,
                check: Check::NotApplicable(self.import_only_reason.clone()),
                features: Vec::new(),
            };
            let references = self.sketches.then(|| history::sketches(model, report).0);
            (
                self.import_design(model, &step, references.as_ref(), report)?,
                outcome,
            )
        } else {
            let mut history_report = Report::default();
            let mut mapping = history::map_with_source_profile_check(
                model,
                &mut history_report,
                self.resolved_version(),
                self.sketches,
                &mut Measuring {
                    writer: self,
                    model,
                    step: &step,
                    progress,
                },
                progress,
                report.losses(),
            );
            if let Ok(history) = &mut mapping.result {
                if let Err(why) = history::base::restate(history, model) {
                    mapping.result = Err(why);
                }
            }
            self.prove_blends(model, &mut mapping, &step, progress)?;
            let mut checked = None;
            if let Ok(history) = &mapping.result {
                if history.stop.is_none() && !only_stored_geometry(&mapping.features) {
                    let written =
                        self.history_design(model, history, &step, Live::History, false)?;
                    checked = Some(self.check(model, history, &step, &written, progress, report)?);
                }
            }
            let partial_check = if let Some(verify::Outcome::Differs(why)) = &checked {
                Some(self.partial(
                    model,
                    &mut mapping,
                    &step,
                    Some(format!("replay differs ({why})")),
                    report,
                    progress,
                )?)
            } else if mapping.result.as_ref().is_ok_and(|h| h.stop.is_some()) {
                Some(self.partial(model, &mut mapping, &step, None, report, progress)?)
            } else {
                None
            };

            report_sketches(&mapping.features, report);
            let flat = |reason: String| Outcome {
                mode: Mode::Import,
                check: Check::Flattened(reason),
                features: mapping.features.clone(),
            };
            let helper_notes = history::helper_notes(&history_report);
            let written = match (&mapping.result, no_decoded_history(&mapping.features)) {
                (Err(_), Some(why)) => {
                    report.append(history_report);
                    report.push(Severity::Info, HISTORY, format!("not written: {why}"));
                    let outcome = Outcome {
                        mode: Mode::Import,
                        check: Check::NotApplicable(why),
                        features: mapping.features.clone(),
                    };
                    (
                        self.import_design(model, &step, Some(&mapping.references), report)?,
                        outcome,
                    )
                }
                (Err(reason), None) => {
                    report.append(history_report);
                    report.push(Severity::Info, HISTORY, format!("flattened: {reason}"));
                    let outcome = if mapping.features.is_empty() {
                        Outcome {
                            mode: Mode::Import,
                            check: Check::NotApplicable(reason.clone()),
                            features: Vec::new(),
                        }
                    } else {
                        flat(reason.clone())
                    };
                    (
                        self.import_design(model, &step, Some(&mapping.references), report)?,
                        outcome,
                    )
                }
                (Ok(history), _) if history.stop.is_some() => {
                    report.append(history_report);
                    let check =
                        partial_check.unwrap_or_else(|| Check::Partial(partial_summary(history)));
                    if let Check::Partial(message) = &check {
                        report.push(Severity::Degraded, HISTORY, message);
                    }
                    let written = self.history_design_report(
                        model,
                        history,
                        &step,
                        Live::Partial,
                        self.should_stand_up(),
                        report,
                    )?;
                    // Check the visible original separately: hidden editable bodies also
                    // appear in SolidCraft's numerical eval result.
                    let original =
                        self.history_design(model, history, &step, Live::Original, false)?;
                    self.check_import(model, &original, report, progress, final_evaluator.as_ref());
                    (
                        written,
                        Outcome {
                            mode: Mode::PartialHistory,
                            check,
                            features: mapping.features.clone(),
                        },
                    )
                }
                (Ok(history), _) if only_stored_geometry(&mapping.features) => {
                    report.append(history_report);
                    let written = self.history_design(
                        model,
                        history,
                        &step,
                        final_live(history, model),
                        self.should_stand_up(),
                    )?;
                    let outcome = Outcome {
                        mode: Mode::Import,
                        check: Check::NotApplicable(history::base::STORED_SOLID.to_owned()),
                        features: mapping.features.clone(),
                    };
                    (written, outcome)
                }
                (Ok(history), _) => {
                    let written = self.history_design(
                        model,
                        history,
                        &step,
                        final_live(history, model),
                        self.should_stand_up(),
                    )?;
                    let summary = self.history_summary(history);
                    let written_as = |check| Outcome {
                        mode: Mode::History,
                        check,
                        features: mapping.features.clone(),
                    };
                    match checked.unwrap_or(verify::Outcome::Unverified("not checked".to_owned())) {
                        verify::Outcome::Matches => {
                            report.append(history_report);
                            report.push(
                                Severity::Info,
                                HISTORY,
                                format!("{summary}; verified against the original in SolidCraft"),
                            );
                            (written, written_as(Check::Verified))
                        }
                        verify::Outcome::Unverified(why)
                            if self.keep_unverified_history_hidden(history) =>
                        {
                            report.append(history_report);
                            let message = format!(
                                "{summary}; unverified: {why}; kept hidden beside the original, which stays the visible model"
                            );
                            report.push_at(
                                Stage::SolidCraft,
                                Severity::Degraded,
                                HISTORY,
                                &message,
                            );
                            let written = self.history_design_report(
                                model,
                                history,
                                &step,
                                Live::Unverified,
                                self.should_stand_up(),
                                report,
                            )?;
                            (
                                written,
                                Outcome {
                                    mode: Mode::PartialHistory,
                                    check: Check::Unverified(why),
                                    features: mapping.features.clone(),
                                },
                            )
                        }
                        verify::Outcome::Unverified(why) => {
                            report.append(history_report);
                            report.push_at(
                                Stage::SolidCraft,
                                Severity::Info,
                                HISTORY,
                                format!("{summary}; unverified: {why}"),
                            );
                            (written, written_as(Check::Unverified(why)))
                        }
                        verify::Outcome::Differs(why) => {
                            report.append(helper_notes);
                            let reason = format!("replay differs ({why})");
                            report.push(Severity::Info, HISTORY, format!("flattened: {reason}"));
                            (
                                self.import_design(
                                    model,
                                    &step,
                                    Some(&mapping.references),
                                    report,
                                )?,
                                flat(reason),
                            )
                        }
                    }
                }
            };
            let (bytes, mut outcome) = written;
            let flattened = mapping.result.is_err() || matches!(outcome.check, Check::Flattened(_));
            if flattened {
                output_links::retain(
                    &mut outcome.features,
                    &mapping.references.features,
                    document::FEATURE,
                );
            } else if let Ok(history) = &mapping.result {
                output_links::history(&mut outcome.features, history, 0);
            }
            (bytes, outcome)
        };
        if outcome.mode == Mode::Import {
            self.check_import(model, &bytes, report, progress, final_evaluator.as_ref());
        }
        state_tolerances(report, &outcome);
        report.set_outcome(outcome);
        let bytes = self.check_opening(bytes, report, final_evaluator.as_ref())?;
        out.write_all(&bytes)
            .map_err(|err| Error::Encode(format!("cannot write the design: {err}")))
    }
}

/// Records the tolerances of a history check that ran: the history verified, partly kept, or
/// rebuilt and found to differ. Nothing is recorded when no replay was compared.
pub(crate) fn state_tolerances(report: &mut Report, outcome: &Outcome) {
    let compared = match &outcome.check {
        Check::Verified | Check::Partial(_) => true,
        Check::Flattened(reason) => reason.starts_with("replay differs ("),
        _ => false,
    };
    if outcome.mode != Mode::Import && compared {
        report.set_verification_tolerances(verification_tolerances());
    }
}

/// Mapped feature data shared with other output adapters. This is an internal workspace
/// interface, not the SolidCraft document format or a stable public API.
pub struct MappedHistory {
    /// Features in timeline order, or the first reason the history cannot be rebuilt.
    pub features: Result<Vec<Value>, String>,
    /// Resolved sketch rows, also when a later feature blocks the history.
    pub sketches: Vec<Value>,
    /// One mapping result per source feature.
    pub outcomes: Vec<FeatureOutcome>,
    /// The value of each parameter the features may name, as a quantity, by parameter name.
    pub parameter_values: std::collections::BTreeMap<String, String>,
}

/// Maps history without writing a container or running SolidCraft.
pub fn mapped_history(model: &Model, report: &mut Report) -> MappedHistory {
    let mut mapping = history::map(model, report, SolidCraftVersion::default());
    // Blends are proven only on SolidCraft's replay of the history before them.
    history::prove_blends(
        &mut mapping,
        |_, _, _| Err("only a SolidCraft design is replayed to find them".to_owned()),
        |_, _| history::infer::Fit::Unknown,
    );
    for feature in &mut mapping.features {
        feature.output = Default::default();
    }
    MappedHistory {
        features: mapping.result.and_then(|history| match history.stop {
            Some(why) => Err(why),
            None => Ok(history.features),
        }),
        sketches: mapping.references.features,
        outcomes: mapping.features,
        parameter_values: parameter_values(model, &mapping.references.params),
    }
}

/// The value of each parameter in `params` (the rows the mapped history names), as a quantity:
/// a source parameter's solved value, a literal expression as written, and a negated parameter
/// as the negation of its value.
fn parameter_values(
    model: &Model,
    params: &[serde_json::Value],
) -> std::collections::BTreeMap<String, String> {
    let mut values = history::params::values(model);
    for row in params {
        let (Some(name), Some(expr)) = (
            row.get("name").and_then(serde_json::Value::as_str),
            row.get("expr").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        if values.contains_key(name) {
            continue;
        }
        let expr = expr.trim();
        let value = match expr.strip_prefix('-').map(str::trim) {
            Some(negated) => {
                values
                    .get(negated)
                    .map(|value| match value.trim().strip_prefix('-') {
                        Some(positive) => positive.trim().to_owned(),
                        None => format!("-{}", value.trim()),
                    })
            }
            None if expr.starts_with(|c: char| c.is_ascii_digit() || c == '.') => {
                Some(expr.to_owned())
            }
            None => None,
        };
        if let Some(value) = value {
            values.insert(name.to_owned(), value);
        }
    }
    values
}

/// Restates geometry with the existing guarded STEP encoder, for output adapters that
/// preserve the original beside rebuilt features.
pub fn restate_step(model: &Model, report: &mut Report) -> Result<String, Error> {
    step::restate(model, report)
}

#[cfg(test)]
mod extension_tests {
    use super::*;

    #[test]
    fn target_release_selects_the_default_but_accepts_all_design_suffixes() {
        for (target, extension) in [
            ("0.18.2", "solidcraft"),
            ("0.25.0", "solidcraft"),
            ("0.25.1", "sc"),
        ] {
            let writer = SolidCraftWriter::new().target_version(Some(target.parse().unwrap()));
            assert_eq!(writer.extension(), extension);
            for suffix in ["sc", "SC", "solidcraft", "SOLVECRAFT"] {
                assert!(writer.accepts_extension(suffix));
            }
            assert!(!writer.accepts_extension("step"));
        }
        let newest = SolidCraftWriter::new().target_version(Some(SolidCraftVersion::default()));
        assert_eq!(newest.extension(), "sc");
    }
}

#[cfg(test)]
mod memory_tests {
    use super::*;

    #[test]
    fn a_post_read_skip_is_sticky_and_keeps_unverified_history_hidden() {
        let writer =
            SolidCraftWriter::new().history_check_memory(9_000_000_000, Some(2_000_000_000));
        assert_eq!(
            writer.history_check_skipped(),
            Some((9_000_000_000, 2_000_000_000))
        );
        assert_eq!(
            writer.memory_refusal().as_deref(),
            Some("History check skipped: needs about 9.0 GB, 2.0 GB free")
        );
        assert!(writer.keep_unverified_hidden());
        // Current host headroom cannot undo the decision made for this design.
        assert!(writer.memory_refusal().is_some());
    }

    #[test]
    fn a_check_rechecks_free_memory_before_replay() {
        assert!(SolidCraftWriter::new().memory_refusal().is_none());
        let writer = SolidCraftWriter::new().history_check_memory(u64::MAX, None);
        if dutchman_core::available_memory().is_some() {
            assert!(writer.memory_refusal().is_some());
            assert!(writer.history_check_skipped().is_some());
        }
        let fits = SolidCraftWriter::new().history_check_memory(0, Some(0));
        assert!(fits.memory_refusal().is_none());
        assert!(!fits.keep_unverified_hidden());
    }
}

#[cfg(test)]
mod proof_prefix_tests {
    use super::*;

    #[test]
    fn a_blend_proof_replays_only_through_its_last_step() {
        let history = history::History {
            features: (1..=4)
                .map(|id| serde_json::json!({"id": id, "type": "extrude"}))
                .collect(),
            ..history::History::default()
        };
        let ids = |history: &history::History| -> Vec<u64> {
            history
                .features
                .iter()
                .filter_map(|feature| feature.get("id").and_then(Value::as_u64))
                .collect()
        };
        assert_eq!(ids(&through_last(&history, &[1, 2]).unwrap()), [1, 2]);
        assert_eq!(ids(&through_last(&history, &[3]).unwrap()), [1, 2, 3]);
        // Already the whole history, or no step in it: nothing to trim.
        assert!(through_last(&history, &[4, 1]).is_none());
        assert!(through_last(&history, &[9]).is_none());
    }
}

#[cfg(test)]
mod mesh_only_tests {
    use super::*;

    /// A model whose only geometry is a mesh has nothing SolidCraft can take.
    #[test]
    fn a_mesh_only_model_writes_no_design() {
        use cadmpeg_ir::math::Point3;
        use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};
        let point = |x: f64, y: f64, z: f64| Point3 { x, y, z };
        let mesh = TessellationMesh::List {
            vertices: vec![
                point(0.0, 0.0, 0.0),
                point(10.0, 0.0, 0.0),
                point(0.0, 10.0, 0.0),
                point(0.0, 0.0, 10.0),
            ],
            triangles: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
        };
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.tessellations.push(
            Tessellation::new(
                TessellationId::mint("f3d:model:tessellation#1").unwrap(),
                mesh,
                Vec::new(),
            )
            .unwrap(),
        );
        let model = Model {
            ir,
            fidelity: None,
            name: "mesh".to_owned(),
            source_file: Some("mesh.f3d".to_owned()),
            assembly: None,
            source_step: None,
        };
        let mut out = Vec::new();
        let result = SolidCraftWriter::default().write(&model, &mut out, &mut Report::default());
        assert!(matches!(result, Err(Error::NoGeometry(_))), "{result:?}");
        assert!(out.is_empty());
    }
}

#[cfg(test)]
mod reuse_tests;
