use std::fmt;

use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::report::Severity as CadmpegSeverity;

/// How much a [`Loss`] matters to the person reading the converted design.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// Detail that has no effect on the geometry (a colour, a note).
    Info,
    /// Something carried across in a degraded form (history flattened to a body).
    Degraded,
    /// Something dropped entirely (a body or feature that is missing from the output).
    Dropped,
}

impl From<CadmpegSeverity> for Severity {
    /// How a cadmpeg finding reads to someone looking at the converted design.
    fn from(severity: CadmpegSeverity) -> Self {
        match severity {
            CadmpegSeverity::Info => Severity::Info,
            CadmpegSeverity::Warning => Severity::Degraded,
            CadmpegSeverity::Error | CadmpegSeverity::Blocking => Severity::Dropped,
        }
    }
}

/// One thing that did not survive the conversion intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loss {
    /// How much it matters.
    pub severity: Severity,
    /// What was affected, in the source's terms (for example `feature "Extrusion3"`), or the
    /// loss code when cadmpeg reported it.
    pub subject: String,
    /// cadmpeg's stable loss code (`step/attribute.source-record-not-written`), when cadmpeg
    /// reported the loss.
    pub code: Option<String>,
    /// What happened to it.
    pub detail: String,
}

impl fmt::Display for Loss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.severity {
            Severity::Info => "Info",
            Severity::Degraded => "Degraded",
            Severity::Dropped => "Dropped",
        };
        write!(f, "{label}: {}: {}", self.subject, self.detail)
    }
}

/// Everything a conversion could not carry across. A conversion that returns a report
/// succeeded; the report says how faithfully.
#[derive(Clone, Debug, Default)]
pub struct Report {
    losses: Vec<Loss>,
    outcome: Option<Outcome>,
}

/// What a writer made of the design: which form it wrote and what became of the history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// The form written.
    pub mode: Mode,
    /// Whether a written history was checked against the original geometry.
    pub check: Check,
    /// What became of each source feature, in timeline order.
    pub features: Vec<FeatureOutcome>,
}

/// The form a design was written in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Editable feature history, with the original geometry kept beside it.
    History,
    /// The geometry alone, without history.
    Import,
}

/// What became of the history's check, with the reason where there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// The replayed history matched the original geometry.
    Verified,
    /// The history was written without being checked.
    Unverified(String),
    /// No history was written: it did not map, or its replay did not match.
    Flattened(String),
    /// There was no history to write, or it was not asked for.
    NotApplicable(String),
}

/// What became of one source feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureOutcome {
    /// Its name in the source, when it has one.
    pub name: Option<String>,
    /// Its kind in cadmpeg's model (`extrude`, `sketch`, …).
    pub kind: String,
    /// Whether it mapped.
    pub status: FeatureStatus,
    /// Why it did not map, or was left out.
    pub reason: Option<String>,
}

/// Whether a source feature mapped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureStatus {
    /// It became a feature of the target.
    Mapped,
    /// The target needs no feature for it (a principal datum plane).
    NotNeeded,
    /// It was suppressed in the source and left out.
    LeftOut,
    /// It does not map, so the history cannot be written.
    Blocked,
}

impl Report {
    /// Records a loss.
    pub fn push(
        &mut self,
        severity: Severity,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.losses.push(Loss {
            severity,
            subject: subject.into(),
            code: None,
            detail: detail.into(),
        });
    }

    /// Records cadmpeg's loss notes (from a decode or an export), each under its loss code.
    pub fn extend_from_cadmpeg<'a>(&mut self, notes: impl IntoIterator<Item = &'a LossNote>) {
        for note in notes {
            let code = note.code.to_string();
            self.losses.push(Loss {
                severity: note.severity.into(),
                subject: code.clone(),
                code: Some(code),
                detail: note.message.clone(),
            });
        }
    }

    /// Records every loss of `other`, after this report's own.
    pub fn append(&mut self, other: Report) {
        self.losses.extend(other.losses);
    }

    /// Records every loss of `other` under `prefix` (`part.ipt: …`), after this report's own.
    pub fn append_prefixed(&mut self, other: Report, prefix: &str) {
        self.losses
            .extend(other.losses.into_iter().map(|loss| Loss {
                subject: format!("{prefix}: {}", loss.subject),
                ..loss
            }));
    }

    /// Records what the writer made of the design.
    pub fn set_outcome(&mut self, outcome: Outcome) {
        self.outcome = Some(outcome);
    }

    /// What the writer made of the design, when it said.
    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    /// The recorded losses, in the order they occurred.
    pub fn losses(&self) -> &[Loss] {
        &self.losses
    }

    /// Whether the conversion was lossless.
    pub fn is_lossless(&self) -> bool {
        self.losses.is_empty()
    }
}
