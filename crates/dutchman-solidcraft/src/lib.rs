//! SolidCraft adapter for Dutchman: writes the neutral model as a `.solidcraft` design.
//!
//! When every feature of the model's history maps onto SolidCraft features, the design is
//! that history, followed by the original geometry as a suppressed import; the history is
//! checked against the original in SolidCraft's own CLI when one is available, and dropped if
//! it does not match. Otherwise the design is exactly what SolidCraft saves when it imports a
//! STEP file: a design container holding one component, one occurrence of it, and a
//! non-parametric `import` feature whose geometry is STEP text (cadmpeg's restatement of the
//! model). The adapter writes the container itself rather than linking SolidCraft's crates,
//! so it carries none of SolidCraft's kernel or renderer.

mod assembly;
mod container;
mod document;
mod history;
mod step;
#[cfg(test)]
mod testing;
mod verify;

use std::path::PathBuf;
use std::time::Duration;

use dutchman_core::{
    Check, Error, FeatureOutcome, FeatureStatus, Mode, Model, Outcome, Report, Severity, Stage,
    Writer,
};
use serde_json::Value;

pub use verify::{
    find_cli, solidcraft_version, usable_cli, NoCli, Verification, CLI_ENV, DEFAULT_TIMEOUT,
    OLDEST_SOLIDCRAFT, VOLUME_TOLERANCE,
};

use document::Live;

/// The SolidCraft design format this adapter writes: the first in which an Import may name a
/// non-STEP source file whose geometry it keeps as restated STEP text. Every newer
/// SolidCraft upgrades it on open.
pub const FORMAT: &str = "solidcraft/23";

/// The report subject of the lines that say what became of the history.
const HISTORY: &str = "history";

/// The report subject of what SolidCraft shows of the written geometry.
const GEOMETRY: &str = "geometry";

/// Writes `.solidcraft` designs.
#[derive(Clone, Debug)]
pub struct SolidCraftWriter {
    import_only: bool,
    verification: Verification,
    replay: Verification,
    timeout: Duration,
}

impl Default for SolidCraftWriter {
    fn default() -> Self {
        SolidCraftWriter {
            import_only: false,
            verification: Verification::Auto,
            replay: Verification::Auto,
            timeout: DEFAULT_TIMEOUT,
        }
    }
}

impl SolidCraftWriter {
    /// A writer that writes history when it can, checked with the SolidCraft CLI it finds.
    pub fn new() -> Self {
        SolidCraftWriter::default()
    }

    /// Always write the geometry as one import, never the history.
    pub fn import_only(mut self, import_only: bool) -> Self {
        self.import_only = import_only;
        self
    }

    /// How a written history is checked, and which SolidCraft CLI finds the input edges of
    /// its blends (see [`SolidCraftWriter::replay`]).
    pub fn verification(mut self, verification: Verification) -> Self {
        self.replay = verification.clone();
        self.verification = verification;
        self
    }

    /// Which SolidCraft CLI replays the history before each fillet or chamfer to find the
    /// edges it blends, after [`SolidCraftWriter::verification`] has set it too. Without one
    /// ([`Verification::Off`], or none found) blends are refused, saying why.
    pub fn replay(mut self, replay: Verification) -> Self {
        self.replay = replay;
        self
    }

    /// How long each SolidCraft evaluation may run before the check gives up.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Whether `model` can be written at all: `Err` with the reason a conversion would fail
    /// with, when it has no geometry SolidCraft can import (for an assembly, when none of its
    /// parts has). Plans the STEP restatement without writing anything.
    pub fn convertible(&self, model: &Model) -> Result<(), String> {
        fn any_part(assembly: &dutchman_core::Assembly) -> bool {
            assembly.parts.iter().any(|part| match &part.assembly {
                Some(sub) => any_part(sub),
                None => step::restatable(part).is_ok(),
            })
        }
        match &model.assembly {
            Some(assembly) if any_part(assembly) || assembly.placements.is_empty() => Ok(()),
            Some(_) => {
                Err("none of the assembly's parts has geometry SolidCraft can import".to_owned())
            }
            None if parameters_only(model) => Ok(()),
            None => step::restatable(model).map_err(|err| match err {
                Error::Encode(reason) | Error::NoGeometry(reason) | Error::Restate(reason) => {
                    reason
                }
                other => other.to_string(),
            }),
        }
    }

    /// What writing `model` would do, without restating its geometry, writing anything or
    /// running SolidCraft: the history's mapping, as `dutchman inspect` shows it.
    pub fn plan(&self, model: &Model, report: &mut Report) -> Outcome {
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
                check: Check::NotApplicable("--import-only".to_owned()),
                features: Vec::new(),
            };
        }
        let mut history_report = Report::default();
        let mut mapping = history::map(model, &mut history_report);
        self.plan_blends(&mut mapping);
        let undecoded = no_decoded_history(&mapping.features);
        match mapping.result {
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
                    check: Check::Unverified("not checked: inspect writes nothing".to_owned()),
                    features: mapping.features,
                }
            }
            Err(reason) if mapping.features.is_empty() => Outcome {
                mode: Mode::Import,
                check: Check::NotApplicable(reason),
                features: Vec::new(),
            },
            Err(_) if undecoded.is_some() => Outcome {
                mode: Mode::Import,
                check: Check::NotApplicable(undecoded.unwrap_or_default()),
                features: mapping.features,
            },
            Err(reason) => Outcome {
                mode: Mode::Import,
                check: Check::Flattened(reason),
                features: mapping.features,
            },
        }
    }

    fn import_design(
        &self,
        model: &Model,
        step: &str,
        references: Option<&history::History>,
        report: &mut Report,
    ) -> Result<Vec<u8>, Error> {
        document::report_unmapped(model, references, report);
        let design = container::Container::new(step.as_bytes())?;
        let mut doc = document::import_design(model, design.step_blob());
        if let Some(references) = references {
            document::keep_sketches(&mut doc, references);
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
    ) -> Result<Vec<u8>, Error> {
        let mut design = container::Container::new(step.as_bytes())?;
        let mut history = history.clone();
        history::base::store(&mut history.features, &mut design)?;
        let doc: Value = document::history_design(model, &history, design.step_blob(), live);
        design.finish(&doc)
    }

    /// What inspecting says of the blends, which it does not replay: refused when no
    /// SolidCraft CLI would find their edges, as writing would refuse them.
    fn plan_blends(&self, mapping: &mut history::Mapping) {
        if let Err(why) = verify::find_cli(&self.replay) {
            history::prove_blends(mapping, |_, _| Err(why));
        }
    }

    /// Finds the input edges of the history's blends on SolidCraft's replay of the history
    /// written so far (two runs of the CLI for the whole design, within the timeout), or
    /// refuses them, saying why.
    fn prove_blends(
        &self,
        model: &Model,
        mapping: &mut history::Mapping,
        step: &str,
    ) -> Result<(), Error> {
        let mut failure = None;
        history::prove_blends(mapping, |history, after| {
            let cli = verify::usable_cli(&self.replay).map_err(|why| why.to_string())?;
            let design = match self.history_design(model, history, step, Live::History) {
                Ok(design) => design,
                Err(err) => {
                    let why = err.to_string();
                    failure = Some(err);
                    return Err(why);
                }
            };
            verify::replay_edges(&cli, self.timeout, &design, after)
        });
        match failure {
            Some(err) => Err(err),
            None => Ok(()),
        }
    }

    fn check(
        &self,
        model: &Model,
        history: &history::History,
        step: &str,
        written: &[u8],
    ) -> Result<verify::Outcome, Error> {
        let cli: PathBuf = match verify::usable_cli(&self.verification) {
            Ok(cli) => cli,
            Err(why) => return Ok(verify::Outcome::Unverified(why.to_string())),
        };
        let original = self.history_design(model, history, step, Live::Original)?;
        Ok(verify::compare(
            &cli,
            self.timeout,
            written,
            &original,
            verify::Expected {
                solids: solid_bodies(model),
                sheets: sheet_bodies(model),
            },
        ))
    }
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
        .model
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
        .model
        .bodies
        .iter()
        .filter(|body| body.kind == cadmpeg_ir::topology::BodyKind::Solid)
        .count()
}

impl SolidCraftWriter {
    /// Opens geometry written as an import in SolidCraft, when a SolidCraft CLI is found, and
    /// reports it as Degraded when SolidCraft shows less of it than the model holds: geometry
    /// that is wrong but opens would otherwise go unnoticed.
    fn check_import(&self, model: &Model, design: &[u8], report: &mut Report) {
        let expected = verify::Expected {
            solids: solid_bodies(model),
            sheets: sheet_bodies(model),
        };
        self.check_geometry(design, expected, report);
    }

    fn check_geometry(&self, design: &[u8], expected: verify::Expected, report: &mut Report) {
        let cli = match verify::usable_cli(&self.verification) {
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
        let shown = verify::check_import(&cli, self.timeout, design, expected);
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
    fn extension(&self) -> &'static str {
        "solidcraft"
    }

    fn write(
        &self,
        model: &Model,
        out: &mut dyn std::io::Write,
        report: &mut Report,
    ) -> Result<(), Error> {
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
        if let Some(assembly) = &model.assembly {
            let bytes = assembly::write(self, model, assembly, report)?;
            // Each part is placed any number of times, so only SolidCraft's own warnings count.
            let expected = verify::Expected {
                sheets: placed_sheets(assembly, 0),
                ..verify::Expected::default()
            };
            self.check_geometry(&bytes, expected, report);
            return out
                .write_all(&bytes)
                .map_err(|err| Error::Encode(format!("cannot write the design: {err}")));
        }
        if parameters_only(model) {
            let (params, _) = history::params::map(&model.ir.model.parameters, report);
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
            report.push(Severity::Info, HISTORY, "not written (--import-only)");
            let outcome = Outcome {
                mode: Mode::Import,
                check: Check::NotApplicable("--import-only".to_owned()),
                features: Vec::new(),
            };
            (self.import_design(model, &step, None, report)?, outcome)
        } else {
            let mut history_report = Report::default();
            let mut mapping = history::map(model, &mut history_report);
            if let Ok(history) = &mut mapping.result {
                if let Err(why) = history::base::restate(history, model) {
                    mapping.result = Err(why);
                }
            }
            self.prove_blends(model, &mut mapping, &step)?;
            let flat = |reason: String| Outcome {
                mode: Mode::Import,
                check: Check::Flattened(reason),
                features: mapping.features.clone(),
            };
            let helper_notes = history::helper_notes(&history_report);
            let written = match (&mapping.result, no_decoded_history(&mapping.features)) {
                (Err(_), Some(why)) => {
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
                (Ok(history), _) if only_stored_geometry(&mapping.features) => {
                    report.append(history_report);
                    let written = self.history_design(model, history, &step, Live::History)?;
                    let outcome = Outcome {
                        mode: Mode::Import,
                        check: Check::NotApplicable(history::base::STORED_SOLID.to_owned()),
                        features: mapping.features.clone(),
                    };
                    (written, outcome)
                }
                (Ok(history), _) => {
                    let written = self.history_design(model, history, &step, Live::History)?;
                    let summary =
                        format!("{} of {} features mapped", history.mapped, history.total);
                    let written_as = |check| Outcome {
                        mode: Mode::History,
                        check,
                        features: mapping.features.clone(),
                    };
                    match self.check(model, history, &step, &written)? {
                        verify::Outcome::Matches => {
                            report.append(history_report);
                            report.push(
                                Severity::Info,
                                HISTORY,
                                format!("{summary}; verified against the original in SolidCraft"),
                            );
                            (written, written_as(Check::Verified))
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
            if written.1.mode == Mode::Import {
                report.append(helper_notes);
            }
            written
        };
        if outcome.mode == Mode::Import {
            self.check_import(model, &bytes, report);
        }
        report.set_outcome(outcome);
        out.write_all(&bytes)
            .map_err(|err| Error::Encode(format!("cannot write the design: {err}")))
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
}

/// Maps history without writing a container or running SolidCraft.
pub fn mapped_history(model: &Model, report: &mut Report) -> MappedHistory {
    let mut mapping = history::map(model, report);
    // Blends are proven only on SolidCraft's replay of the history before them.
    history::prove_blends(&mut mapping, |_, _| {
        Err("only a SolidCraft design is replayed to find them".to_owned())
    });
    MappedHistory {
        features: mapping.result.map(|history| history.features),
        sketches: mapping.references.features,
        outcomes: mapping.features,
    }
}

/// Restates geometry with the existing guarded STEP encoder, for output adapters that
/// preserve the original beside rebuilt features.
pub fn restate_step(model: &Model, report: &mut Report) -> Result<String, Error> {
    step::restate(model, report)
}
