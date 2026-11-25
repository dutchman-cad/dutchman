//! Which way is up in a source design, where that was learned, and what a conversion did
//! about it.
//!
//! A source states its up axis through some origin: today the Fusion exporter's export
//! manifest; later, possibly, the design file itself. [`SourceUpAxis::Unknown`] never stands
//! for a guess: a design whose up axis nothing states keeps its axes.

use serde_json::{json, Value};

/// The up axis a source states.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SourceUpAxis {
    /// +Y is up: the design is stood upright by rotating +90 degrees about X.
    Y,
    /// +Z is up: SolidCraft's own convention.
    Z,
    /// Nothing states it.
    #[default]
    Unknown,
}

impl SourceUpAxis {
    /// The name used in reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::Y => "y",
            Self::Z => "z",
            Self::Unknown => "unknown",
        }
    }
}

/// Where a stated up axis came from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UpAxisOrigin {
    /// The Fusion exporter's `manifest.json`.
    ExportManifest,
    /// The design file itself.
    File,
    /// Nothing stated it.
    #[default]
    None,
}

impl UpAxisOrigin {
    /// The name used in reports.
    pub fn name(self) -> &'static str {
        match self {
            Self::ExportManifest => "export_manifest",
            Self::File => "file",
            Self::None => "none",
        }
    }

    fn phrase(self) -> &'static str {
        match self {
            Self::ExportManifest => "the export",
            Self::File => "the file",
            Self::None => "nothing",
        }
    }
}

/// An up axis a source states (or doesn't), with its origin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpAxisFinding {
    /// The stated axis.
    pub axis: SourceUpAxis,
    /// Where it was read.
    pub origin: UpAxisOrigin,
    /// Why the axis is unknown, when it is.
    pub reason: Option<String>,
}

impl UpAxisFinding {
    /// An axis stated by `origin`.
    pub fn stated(axis: SourceUpAxis, origin: UpAxisOrigin) -> Self {
        Self {
            axis,
            origin,
            reason: None,
        }
    }

    /// No up axis is stated, for `reason`.
    pub fn unknown(origin: UpAxisOrigin, reason: impl Into<String>) -> Self {
        Self {
            axis: SourceUpAxis::Unknown,
            origin,
            reason: Some(reason.into()),
        }
    }
}

/// The up axis the decoded design states itself, when cadmpeg's CADIR carries one. The pinned
/// cadmpeg has no such field yet: when it does, this is the one place that reads it (return
/// `Some(UpAxisFinding::stated(axis, UpAxisOrigin::File))`).
pub(crate) fn from_file(_model: &crate::Model) -> Option<UpAxisFinding> {
    None
}

/// The source's up axis: the file's own statement first, then the export manifest's.
pub(crate) fn resolve(file: Option<UpAxisFinding>, manifest: UpAxisFinding) -> UpAxisFinding {
    match file {
        Some(file) if file.axis != SourceUpAxis::Unknown => file,
        _ => manifest,
    }
}

/// What a person asked for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    /// Stand a design up when its source says it is Y-up; keep the axes otherwise.
    #[default]
    Auto,
    /// Always rotate +90 degrees about X.
    StandUp,
    /// Never rotate.
    KeepAxes,
}

impl Orientation {
    /// The name used in reports and settings.
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::StandUp => "stand_up",
            Self::KeepAxes => "keep_axes",
        }
    }

    /// Reads a name from [`Orientation::name`].
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Auto, Self::StandUp, Self::KeepAxes]
            .into_iter()
            .find(|choice| choice.name() == name)
    }

    /// Whether to rotate, given what the source states.
    pub fn stands_up(self, found: &UpAxisFinding) -> bool {
        match self {
            Self::Auto => found.axis == SourceUpAxis::Y,
            Self::StandUp => true,
            Self::KeepAxes => false,
        }
    }

    /// The sentence that says what was done and why.
    pub fn sentence(self, found: &UpAxisFinding) -> String {
        let stands = self.stands_up(found);
        let source = match found.axis {
            SourceUpAxis::Y => format!("{} says this design is Y-up", found.origin.phrase()),
            SourceUpAxis::Z => format!("{} says this design is Z-up", found.origin.phrase()),
            SourceUpAxis::Unknown => match &found.reason {
                Some(reason) => format!("up axis unknown ({reason})"),
                None => "up axis unknown".to_owned(),
            },
        };
        match (self, stands) {
            (Self::Auto, true) => format!("Stood up: {source}"),
            (Self::Auto, false) => format!("Kept the file's axes: {source}"),
            (Self::StandUp, _) => format!("Stood up as asked (--stand-up); {source}"),
            (Self::KeepAxes, _) => format!("Kept the file's axes as asked (--keep-axes); {source}"),
        }
    }
}

/// What a conversion found and did about the up axis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpAxisReport {
    /// What the source states.
    pub found: UpAxisFinding,
    /// What was asked for, once a writer has acted on it.
    pub applied: Option<(Orientation, bool)>,
    /// Designs of an assembly whose axis differs from the root design's.
    pub disagreement: Option<String>,
}

impl UpAxisReport {
    /// Whether the report has something to say: the source stated or explained its axis, a
    /// design of an assembly disagrees, or a person overrode the automatic choice. A design
    /// from a format that has no up axis to state, converted automatically, says nothing.
    pub fn worth_stating(&self) -> bool {
        self.found.origin != UpAxisOrigin::None
            || self.found.reason.is_some()
            || self.disagreement.is_some()
            || self
                .applied
                .is_some_and(|(mode, _)| mode != Orientation::Auto)
    }

    /// The report's JSON object.
    pub fn json(&self) -> Value {
        let mut out = serde_json::Map::new();
        out.insert("found".to_owned(), json!(self.found.axis.name()));
        out.insert("origin".to_owned(), json!(self.found.origin.name()));
        if let Some(reason) = &self.found.reason {
            out.insert("reason".to_owned(), json!(reason));
        }
        if let Some((mode, stood_up)) = self.applied {
            out.insert("mode".to_owned(), json!(mode.name()));
            out.insert(
                "applied".to_owned(),
                json!(if stood_up { "stood_up" } else { "kept_axes" }),
            );
            out.insert("summary".to_owned(), json!(mode.sentence(&self.found)));
        }
        if let Some(note) = &self.disagreement {
            out.insert("disagreement".to_owned(), json!(note));
        }
        Value::Object(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_stands_up_only_a_stated_y() {
        let y = UpAxisFinding::stated(SourceUpAxis::Y, UpAxisOrigin::ExportManifest);
        let z = UpAxisFinding::stated(SourceUpAxis::Z, UpAxisOrigin::File);
        let unknown = UpAxisFinding::unknown(UpAxisOrigin::None, "no manifest");
        assert!(Orientation::Auto.stands_up(&y));
        assert!(!Orientation::Auto.stands_up(&z));
        assert!(!Orientation::Auto.stands_up(&unknown));
        assert!(Orientation::StandUp.stands_up(&z));
        assert!(!Orientation::KeepAxes.stands_up(&y));
        assert_eq!(
            Orientation::Auto.sentence(&y),
            "Stood up: the export says this design is Y-up"
        );
        assert_eq!(
            Orientation::Auto.sentence(&unknown),
            "Kept the file's axes: up axis unknown (no manifest)"
        );
    }
}
