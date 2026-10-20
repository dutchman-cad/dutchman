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

mod container;
mod document;
mod history;
mod step;
#[cfg(test)]
mod testing;
mod verify;

use std::path::PathBuf;
use std::time::Duration;

use dutchman_core::{Error, Model, Report, Severity, Writer};
use serde_json::Value;

pub use verify::{Verification, CLI_ENV, DEFAULT_TIMEOUT, VOLUME_TOLERANCE};

use document::Live;
use verify::Outcome;

/// The SolidCraft design format this adapter writes: the first in which an Import may name a
/// non-STEP source file whose geometry it keeps as restated STEP text. Every newer
/// SolidCraft upgrades it on open.
pub const FORMAT: &str = "solidcraft/23";

/// The report subject of the lines that say what became of the history.
const HISTORY: &str = "history";

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
    ) -> Result<Outcome, Error> {
        let cli: PathBuf = match verify::find_cli(&self.verification) {
            Ok(cli) => cli,
            Err(why) => return Ok(Outcome::Unverified(why)),
        };
        let original = self.history_design(model, history, step, Live::Original)?;
        Ok(verify::compare(&cli, self.timeout, written, &original))
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
        let step = step::restate(model, report)?;
        let bytes = if self.import_only {
            report.push(Severity::Info, HISTORY, "not written (--import-only)");
            self.import_design(model, &step, report)?
        } else {
            let mut history_report = Report::default();
            match history::map(model, &mut history_report) {
                Err(reason) => {
                    report.push(Severity::Info, HISTORY, format!("flattened: {reason}"));
                    self.import_design(model, &step, report)?
                }
                Ok(history) => {
                    let written = self.history_design(model, &history, &step, Live::History)?;
                    let summary =
                        format!("{} of {} features mapped", history.mapped, history.total);
                    match self.check(model, &history, &step, &written)? {
                        Outcome::Matches => {
                            report.append(history_report);
                            report.push(
                                Severity::Info,
                                HISTORY,
                                format!("{summary}; verified against the original in SolidCraft"),
                            );
                            written
                        }
                        Outcome::Unverified(why) => {
                            report.append(history_report);
                            report.push(
                                Severity::Info,
                                HISTORY,
                                format!("{summary}; unverified: {why}"),
                            );
                            written
                        }
                        Outcome::Differs(why) => {
                            report.push(
                                Severity::Info,
                                HISTORY,
                                format!("flattened: replay differs ({why})"),
                            );
                            self.import_design(model, &step, report)?
                        }
                    }
                }
            }
        };
        out.write_all(&bytes)
            .map_err(|err| Error::Encode(format!("cannot write the design: {err}")))
    }
}
