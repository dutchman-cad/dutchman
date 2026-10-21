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

use dutchman_core::{Check, Error, Mode, Model, Outcome, Report, Severity, Stage, Writer};
use serde_json::Value;

pub use verify::{
    find_cli, solidcraft_version, Verification, CLI_ENV, DEFAULT_TIMEOUT, VOLUME_TOLERANCE,
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
    timeout: Duration,
}

impl Default for SolidCraftWriter {
    fn default() -> Self {
        SolidCraftWriter {
            import_only: false,
            verification: Verification::Auto,
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

    /// How a written history is checked.
    pub fn verification(mut self, verification: Verification) -> Self {
        self.verification = verification;
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
            Some(assembly) if any_part(assembly) => Ok(()),
            Some(_) => {
                Err("none of the assembly's parts has geometry SolidCraft can import".to_owned())
            }
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
        if let Some(assembly) = &model.assembly {
            return assembly::plan(self, assembly, report);
        }
        if self.import_only {
            return Outcome {
                mode: Mode::Import,
                check: Check::NotApplicable("--import-only".to_owned()),
                features: Vec::new(),
            };
        }
        let mut history_report = Report::default();
        let mapping = history::map(model, &mut history_report);
        match mapping.result {
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
        report: &mut Report,
    ) -> Result<Vec<u8>, Error> {
        document::report_unmapped(model, report);
        let design = container::Container::new(step.as_bytes())?;
        let doc = document::import_design(model, design.step_blob());
        design.finish(&doc)
    }

    fn history_design(
        &self,
        model: &Model,
        history: &history::History,
        step: &str,
        live: Live,
    ) -> Result<Vec<u8>, Error> {
        let design = container::Container::new(step.as_bytes())?;
        let doc: Value = document::history_design(model, history, design.step_blob(), live);
        design.finish(&doc)
    }

    fn check(
        &self,
        model: &Model,
        history: &history::History,
        step: &str,
        written: &[u8],
    ) -> Result<verify::Outcome, Error> {
        let cli: PathBuf = match verify::find_cli(&self.verification) {
            Ok(cli) => cli,
            Err(why) => return Ok(verify::Outcome::Unverified(why)),
        };
        let original = self.history_design(model, history, step, Live::Original)?;
        Ok(verify::compare(&cli, self.timeout, written, &original))
    }
}

impl SolidCraftWriter {
    /// Opens geometry written as an import in SolidCraft, when a SolidCraft CLI is found, and
    /// reports it as Degraded when SolidCraft shows less of it than the model holds: geometry
    /// that is wrong but opens would otherwise go unnoticed.
    fn check_import(&self, model: &Model, design: &[u8], report: &mut Report) {
        self.check_geometry(design, model.ir.model.faces.len(), report);
    }

    fn check_geometry(&self, design: &[u8], faces: usize, report: &mut Report) {
        let Ok(cli) = verify::find_cli(&self.verification) else {
            return;
        };
        match verify::check_import(&cli, self.timeout, design, faces) {
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
        if let Some(assembly) = &model.assembly {
            let bytes = assembly::write(self, model, assembly, report)?;
            // Each part is placed any number of times, so only SolidCraft's own warnings count.
            self.check_geometry(&bytes, 0, report);
            return out
                .write_all(&bytes)
                .map_err(|err| Error::Encode(format!("cannot write the design: {err}")));
        }
        let step = step::restate(model, report)?;
        let (bytes, outcome) = if self.import_only {
            report.push(Severity::Info, HISTORY, "not written (--import-only)");
            let outcome = Outcome {
                mode: Mode::Import,
                check: Check::NotApplicable("--import-only".to_owned()),
                features: Vec::new(),
            };
            (self.import_design(model, &step, report)?, outcome)
        } else {
            let mut history_report = Report::default();
            let mapping = history::map(model, &mut history_report);
            let flat = |reason: String| Outcome {
                mode: Mode::Import,
                check: Check::Flattened(reason),
                features: mapping.features.clone(),
            };
            match &mapping.result {
                Err(reason) => {
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
                    (self.import_design(model, &step, report)?, outcome)
                }
                Ok(history) => {
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
                            (self.import_design(model, &step, report)?, flat(reason))
                        }
                    }
                }
            }
        };
        if outcome.mode == Mode::Import {
            self.check_import(model, &bytes, report);
        }
        report.set_outcome(outcome);
        out.write_all(&bytes)
            .map_err(|err| Error::Encode(format!("cannot write the design: {err}")))
    }
}
