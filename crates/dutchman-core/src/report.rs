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
    /// What was affected, in the source's terms (for example `feature "Extrusion3"`).
    pub subject: String,
    /// What happened to it.
    pub detail: String,
}

impl fmt::Display for Loss {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.severity {
            Severity::Info => "note",
            Severity::Degraded => "degraded",
            Severity::Dropped => "dropped",
        };
        write!(f, "{label}: {}: {}", self.subject, self.detail)
    }
}

/// Everything a conversion could not carry across. A conversion that returns a report
/// succeeded; the report says how faithfully.
#[derive(Clone, Debug, Default)]
pub struct Report {
    losses: Vec<Loss>,
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
            detail: detail.into(),
        });
    }

    /// Records cadmpeg's loss notes (from a decode or an export), each under its loss code.
    pub fn extend_from_cadmpeg<'a>(&mut self, notes: impl IntoIterator<Item = &'a LossNote>) {
        for note in notes {
            self.push(
                note.severity.into(),
                note.code.to_string(),
                note.message.clone(),
            );
        }
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
