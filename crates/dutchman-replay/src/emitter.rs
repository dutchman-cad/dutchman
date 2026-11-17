use crate::{package, prefix, Plan, Step};
use dutchman_core::{Check, Error, FeatureStatus, Mode, Model, Outcome, Report, Severity, Writer};

/// Target-specific support checks, calls, runtime and instructions.
pub trait Emitter {
    /// Target identifier used by step records.
    fn target(&self) -> &'static str;
    /// Target program name used in instructions and diagnostics.
    fn program(&self) -> &'static str;
    /// Output format identifier.
    fn format(&self) -> &'static str;
    /// Refuse unknown kinds, unevaluated quantities, and every extra key not handled.
    fn supports(&self, step: &Step, plan: &Plan) -> Result<(), String>;
    /// Dotted untyped option paths handled for this step kind.
    fn handled_options(&self, kind: &str) -> &'static [&'static str];
    /// Lower the accepted prefix, with the digest of the serialized plan.
    fn lower(&self, plan: &Plan, upto: usize, plan_sha256: &str) -> Result<Lowered, String>;
    /// Runtime text, then its file name.
    fn runtime(&self) -> (&'static str, &'static str);
    /// Package instructions supplied by the target.
    fn instructions(&self, plan: &Plan) -> String;
}
/// Lowered TSV calls and any additional package files.
#[non_exhaustive]
pub struct Lowered {
    /// Encoded step records, beginning with the steps header.
    pub tsv: String,
    /// Additional single-name entries appended in supplied order.
    pub extra_files: Vec<(String, Vec<u8>)>,
}
impl Lowered {
    /// Construct lowered calls with no additional files.
    pub fn new(tsv: String) -> Self {
        Self {
            tsv,
            extra_files: Vec::new(),
        }
    }
}
/// Writer adapter for one script emitter.
#[non_exhaustive]
pub struct ScriptWriter<E: Emitter> {
    /// Target implementation.
    pub emitter: E,
}
impl<E: Emitter> ScriptWriter<E> {
    /// Wrap a target emitter as a package writer.
    pub fn new(emitter: E) -> Self {
        Self { emitter }
    }
}
impl<E: Emitter> Writer for ScriptWriter<E> {
    /// File extension of the generated script package.
    fn extension(&self) -> &'static str {
        "zip"
    }
    /// Map a part and write its supported history and runtime package.
    fn write(
        &self,
        model: &Model,
        out: &mut dyn std::io::Write,
        report: &mut Report,
    ) -> Result<(), Error> {
        if model.assembly.is_some() {
            return Err(Error::Encode(format!(
                "assemblies are not supported yet in {} output; convert each part",
                self.emitter.program()
            )));
        }
        let mut mapping_report = Report::default();
        let mapping = dutchman_solidcraft::mapped_history(model, &mut mapping_report);
        let mut outcomes = mapping.outcomes.clone();
        let mut plan = Plan::from_mapping(model, mapping);
        report.append(mapping_report);
        let original = dutchman_solidcraft::restate_step(model, report)?;
        let upto = prefix::supported_prefix(&self.emitter, &mut plan);
        for (o, p) in outcomes.iter_mut().zip(&plan.outcomes) {
            o.status = match p.status {
                crate::OutcomeStatus::Mapped => FeatureStatus::Mapped,
                crate::OutcomeStatus::Blocked => FeatureStatus::Blocked,
                crate::OutcomeStatus::NotNeeded => FeatureStatus::NotNeeded,
                crate::OutcomeStatus::LeftOut => FeatureStatus::LeftOut,
            };
            o.reason = p.reason.clone();
        }
        let mut plan_json = serde_json::to_vec_pretty(&plan)
            .map_err(|_| Error::Encode("feature history: cannot write the replay plan".into()))?;
        plan_json.push(b'\n');
        let digest = package::sha256(&plan_json);
        let lowered = self
            .emitter
            .lower(&plan, upto, &digest)
            .map_err(Error::Encode)?;
        let tsv = format!("dutchman-plan-sha256/1\t{digest}\r\n{}", lowered.tsv);
        let instructions = self.emitter.instructions(&plan);
        let (runtime, name) = self.emitter.runtime();
        let mut files = vec![
            ("README.txt", instructions.as_bytes()),
            (name, runtime.as_bytes()),
            ("steps.tsv", tsv.as_bytes()),
            ("plan.json", plan_json.as_slice()),
            ("original.step", original.as_bytes()),
        ];
        files.extend(
            lowered
                .extra_files
                .iter()
                .map(|(name, bytes)| (name.as_str(), bytes.as_slice())),
        );
        package::write_package(out, &files)?;
        write_report(model, &plan, upto, outcomes, self.emitter.program(), report);
        Ok(())
    }
}

/// Record the accepted prefix and its reference-history note.
pub(crate) fn write_report(
    model: &Model,
    plan: &Plan,
    upto: usize,
    outcomes: Vec<dutchman_core::FeatureOutcome>,
    program: &str,
    report: &mut Report,
) {
    if plan.stop.is_some() && (upto > 0 || !plan.sketches.is_empty()) {
        let mapped = plan
            .outcomes
            .iter()
            .filter(|o| o.reason.as_deref() == Some("rebuilt as reference in partial history"))
            .count();
        let total = plan
            .outcomes
            .iter()
            .filter(|o| {
                !matches!(
                    o.status,
                    crate::OutcomeStatus::LeftOut | crate::OutcomeStatus::NotNeeded
                )
            })
            .count();
        report.push(
            Severity::Degraded,
            &model.name,
            format!("rebuilt {mapped} of {total} features; kept as reference"),
        );
    }
    report.set_outcome(Outcome {
        mode: if upto == 0 && plan.sketches.is_empty() {
            Mode::Import
        } else if plan.stop.is_some() {
            Mode::PartialHistory
        } else {
            Mode::History
        },
        check: Check::Unverified(format!(
            "run the script in {}; it checks its own result",
            program
        )),
        features: outcomes,
    });
}
