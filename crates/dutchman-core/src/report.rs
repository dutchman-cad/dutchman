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

/// Where in a conversion a loss or a failure arose, so a report of it reaches the code that
/// can fix it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    /// The file itself, or how it was asked for: missing, too large, the wrong kind.
    Input,
    /// cadmpeg reading the file's bytes.
    Decode,
    /// cadmpeg turning what it read into its neutral model.
    Project,
    /// Dutchman mapping the neutral model onto SolidCraft's.
    Map,
    /// Writing the design: Dutchman's writer, and cadmpeg's STEP restatement within it.
    Write,
    /// SolidCraft opening or checking the written design.
    SolidCraft,
}

impl Stage {
    /// Its name in the reports.
    pub fn id(self) -> &'static str {
        match self {
            Stage::Input => "input",
            Stage::Decode => "decode",
            Stage::Project => "project",
            Stage::Map => "map",
            Stage::Write => "write",
            Stage::SolidCraft => "solidcraft",
        }
    }

    /// Every stage, in conversion order.
    pub const ALL: [Stage; 6] = [
        Stage::Input,
        Stage::Decode,
        Stage::Project,
        Stage::Map,
        Stage::Write,
        Stage::SolidCraft,
    ];
}

/// The cadmpeg loss-code words that say the source's bytes could not be read (a record
/// undecoded, unparsed, untyped or badly framed; a stream missing or unreadable), rather than
/// read and not carried into the neutral model (unprojected, unresolved, unbound, kept native,
/// capped).
const DECODE_WORDS: [&str; 11] = [
    "undecoded",
    "not-decoded",
    "unparsed",
    "untyped",
    "framing",
    "unreadable",
    "container-missing",
    "missing-geometry-stream",
    "not-transferred",
    "truncated",
    "malformed",
];

/// The stage of a cadmpeg loss reported while reading a file, from its code.
pub fn read_stage(code: &str) -> Stage {
    if DECODE_WORDS.iter().any(|word| code.contains(word)) {
        Stage::Decode
    } else {
        Stage::Project
    }
}

/// Stable, name-free categories for appearance assignments left out of the output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppearanceLoss {
    /// A redundant face colour (retained assignments do not produce a loss).
    RedundantFace,
    /// A wire body or saved free edge.
    Wire,
    /// Face geometry could not supply an output pick.
    FacePickMissed,
    /// The assignment's target is absent from the decoded model.
    TargetAbsent,
    /// The target belongs to a body with no writable geometry.
    UnwritableBody,
    /// Any other appearance mapping gap.
    Other,
}

impl AppearanceLoss {
    /// Its stable report code, never derived from design contents.
    pub fn code(self) -> &'static str {
        match self {
            Self::RedundantFace => "dutchman/appearance.redundant-face",
            Self::Wire => "dutchman/appearance.wire",
            Self::FacePickMissed => "dutchman/appearance.face-pick-missed",
            Self::TargetAbsent => "dutchman/appearance.target-absent",
            Self::UnwritableBody => "dutchman/appearance.unwritable-body",
            Self::Other => "dutchman/appearance.other",
        }
    }
}

/// A feature the source program already flagged as failed in the file itself, before
/// Dutchman read it. These are kept apart from [`Loss`]es, which are Dutchman's own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceError {
    /// The object's internal name in the source document.
    pub name: String,
    /// The name its program shows for it (the label); the internal name when it has none.
    pub label: String,
    /// The program's own message, when it saved one.
    pub message: Option<String>,
}

/// One thing that did not survive the conversion intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Loss {
    /// How much it matters.
    pub severity: Severity,
    /// What was affected, in the source's terms (for example `feature "Extrusion3"`), or the
    /// loss code when cadmpeg reported it.
    pub subject: String,
    /// A stable cadmpeg or Dutchman loss code, when available.
    pub code: Option<String>,
    /// What happened to it.
    pub detail: String,
    /// Where in the conversion it arose.
    pub stage: Stage,
}

impl Loss {
    /// The project whose code the loss arose in, when it is not Dutchman's own: `cadmpeg` for
    /// what cadmpeg reported, `solidcraft` for what SolidCraft found.
    pub fn upstream(&self) -> Option<&'static str> {
        match self.stage {
            Stage::Decode | Stage::Project => Some("cadmpeg"),
            Stage::Write if self.code.is_some() => Some("cadmpeg"),
            Stage::SolidCraft => Some("solidcraft"),
            _ => None,
        }
    }
}

impl Loss {
    /// What kind of loss this is, in plain words, the same for every loss of its kind: a heading
    /// a front end can group losses under ("Parts of the file kept as-is"), with each loss's code
    /// and detail kept for whoever reports it. It names the part of the design the loss is about,
    /// from its code's area (`f3d/sketch.…` is about sketches) or else its subject's first word,
    /// and what happened to it, from its severity.
    pub fn title(&self) -> String {
        let area = match self.code.as_deref() {
            Some(code) => code
                .split_once('/')
                .map_or(code, |(_, rest)| rest)
                .split('.')
                .next()
                .unwrap_or_default(),
            None => self
                .subject
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
                .next()
                .unwrap_or_default(),
        };
        let noun = area_noun(&area.to_ascii_lowercase(), self.stage);
        let kept = self.code.as_deref().is_some_and(|code| {
            ["retained", "kept", "native", "preserved", "opaque"]
                .iter()
                .any(|word| code.contains(word))
        });
        // Records kept unread are kept as they were, whatever their severity says about it.
        let title = match self.severity {
            Severity::Dropped => format!("{noun} left out"),
            _ if kept => format!("{noun} kept as-is"),
            Severity::Degraded => format!("{noun} carried over with changes"),
            Severity::Info => format!("Notes about {noun}"),
        };
        let mut chars = title.chars();
        chars.next().map_or_else(String::new, |first| {
            first.to_uppercase().chain(chars).collect()
        })
    }
}

/// The part of a design a loss code's area (or a loss subject's first word) is about, as a plain
/// noun phrase.
fn area_noun(area: &str, stage: Stage) -> &'static str {
    match area {
        "feature" | "features" | "history" => "features",
        "sketch" | "sketches" | "section" | "curve-expression" => "sketches",
        "geometry" | "carrier" | "curve" | "pcurve" | "curve-on-surface" | "exact" | "brep"
        | "tspline" | "cage" | "morph" | "surface" | "shape" => "shapes",
        "topology" | "body" | "bodies" | "body-reference" | "body-recipe" | "edge" | "vertex"
        | "polyedge" | "entity" | "element-map" | "face" => "bodies, faces and edges",
        "assembly" | "occurrence" | "product" | "placement" | "part" | "parts" | "component"
        | "scope" | "graph" => "assembly structure",
        "xref" | "reference" => "links to other files",
        "joint" | "joints" => "joints",
        "parameter" | "parameters" | "config" | "configuration" | "expression" => {
            "parameters and configurations"
        }
        "pmi" | "annotation" | "dimension" | "drawing" | "drawings" | "detail" | "hatch" => {
            "annotations and drawings"
        }
        "appearance" | "material" | "presentation" | "scene" | "layer" | "viewport" | "colour"
        | "color" => "appearances",
        "metadata" | "attributes" | "attribute" | "property-set" | "global" | "object"
        | "asset" | "header" => "properties",
        "tessellation" | "mesh" => "display meshes",
        "work-plane" | "plane" | "datum" => "construction planes and axes",
        "source" | "container" | "structure" | "parse" | "decode" | "directory" | "framing"
        | "native" | "legacy" | "card" | "validation" => "parts of the file",
        "design" => "design details",
        "tool" | "tools" => "tools",
        "writer" | "export" | "target" => "details of the converted design",
        _ => match stage {
            Stage::Input | Stage::Decode => "parts of the file",
            Stage::Project | Stage::Map => "design details",
            Stage::Write | Stage::SolidCraft => "details of the converted design",
        },
    }
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

/// The most names one report records: enough for any real design, bounded for a hostile one.
const MAX_NAMES: usize = 100_000;

/// Everything a conversion could not carry across. A conversion that returns a report
/// succeeded; the report says how faithfully.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// The version of the program that saved the input file, as `major.minor.revision`.
    source_version: Option<String>,
    threads: Option<(usize, usize)>,
    part_positions: Option<(crate::PartPositions, crate::PartPositionCounts)>,
    placement_assumptions: Vec<(String, crate::PlacementAssumption)>,
    history_check_skipped: Option<(u64, u64)>,
    verification_tolerances: Option<serde_json::Value>,
    verification_checks: Vec<serde_json::Value>,
    losses: Vec<Loss>,
    source_errors: Vec<SourceError>,
    source_error_total: u64,
    outcome: Option<Outcome>,
    parts: Vec<PartHistory>,
    completeness: Option<Completeness>,
    joints: std::collections::BTreeMap<String, u64>,
    names: std::collections::BTreeSet<String>,
    /// Linked components the source file places but doesn't contain: how many, and why the
    /// design can't be verified complete.
    missing_links: Option<(u64, String)>,
    up_axis: Option<crate::UpAxisReport>,
}

/// How much of an assembly was placed: every placement its file and its placed sub-assemblies'
/// files record, each sub-assembly counted as often as it is placed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Completeness {
    /// Placements with geometry, including assembly holders of child placements.
    pub placed: u64,
    /// Placements of leaf parts with no bodies. These retain their component and joints.
    pub placed_empty: u64,
    /// Unproven occurrence positions, by part name, including dependent children.
    pub unplaced: std::collections::BTreeMap<String, u64>,
    /// The placements left out, by the file each places and why, most first.
    pub left_out: Vec<LeftOutGroup>,
}

/// Placements of one file left out for one reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeftOutGroup {
    /// The file they place, by name, when they name one.
    pub file: Option<String>,
    /// Why they are left out.
    pub reason: String,
    /// The stage they were lost at.
    pub stage: Stage,
    /// How many there are.
    pub count: u64,
}

impl Completeness {
    /// Every placement recorded, placed or not.
    pub fn placements(&self) -> u64 {
        self.left_out.iter().fold(
            self.placed
                .saturating_add(self.placed_empty)
                .saturating_add(self.unplaced_count()),
            |total, group| total.saturating_add(group.count),
        )
    }

    /// How many positions could not be read.
    pub fn unplaced_count(&self) -> u64 {
        self.unplaced
            .values()
            .fold(0u64, |n, count| n.saturating_add(*count))
    }

    /// Plain explanation of unreadable positions, when present.
    pub fn unplaced_message(&self) -> Option<String> {
        match self.unplaced_count() {
            0 => None,
            1 => Some(
                "1 part couldn't be placed (its position in the archive couldn't be read).".into(),
            ),
            n => Some(format!(
                "{n} parts couldn't be placed (their position in the archive couldn't be read)."
            )),
        }
    }

    /// How many placements were left out.
    pub fn left_out_count(&self) -> u64 {
        self.left_out
            .iter()
            .fold(0u64, |total, group| total.saturating_add(group.count))
    }

    /// Whether every placement has geometry or holds child placements.
    pub fn is_complete(&self) -> bool {
        self.placed_empty == 0 && self.left_out.is_empty() && self.unplaced.is_empty()
    }
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
    /// An editable prefix and sketches, hidden beside the visible original geometry.
    PartialHistory,
    /// Editable feature history, with the original geometry kept beside it.
    History,
    /// Imported geometry without body-building history, possibly with reference sketches.
    Import,
}

/// What became of the history's check, with the reason where there is one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// Only part of the history is retained; the reason states its replay check.
    Partial(String),
    /// The replayed history matched the original geometry.
    Verified,
    /// A rebuilt result differs from its source; original geometry is retained.
    Differs(String),
    /// The history was written without being checked.
    Unverified(String),
    /// No body-building prefix was retained: it did not map or could not replay.
    Flattened(String),
    /// There was no history to write, or it was not asked for.
    NotApplicable(String),
}

/// What became of one assembly part's history.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartHistory {
    /// The part's name.
    pub name: String,
    /// What became of its history: as for a whole design, with `NotApplicable` when it has
    /// no source features.
    pub check: Check,
    /// Its source features that mapped or need no feature, and all it has, suppressed ones
    /// not counted.
    pub mapped: usize,
    pub total: usize,
    /// Of those, the ones that became SolidCraft features, and the ones that need one: all but
    /// those that need none, such as a component insert or a joint.
    pub rebuilt: usize,
    pub needing: usize,
    /// Kept features that re-apply to a base body as the design ends, not counted as rebuilt.
    pub reapplied: usize,
}

/// The features a source row produced in the saved design.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FeatureOutput {
    /// Final feature ids, in output timeline order. Empty when no feature was written.
    pub ids: Vec<u64>,
    /// How the link was established; never inferred from a feature's name.
    pub method: OutputMatch,
}

/// Evidence for a source-to-output link.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputMatch {
    /// Recorded while the writer emitted the feature, including any id rebasing.
    Emitted,
    /// No independently produced feature survives in the saved design.
    #[default]
    NoOutput,
    /// This report describes a plan; no design was saved.
    NotWritten,
}

/// What became of one source feature.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureOutcome {
    /// Link to the features retained in the saved design.
    pub output: FeatureOutput,
    /// Its name in the source, when it has one.
    pub name: Option<String>,
    /// Its kind in cadmpeg's model (`extrude`, `sketch`, …).
    pub kind: String,
    /// Whether it mapped.
    pub status: FeatureStatus,
    /// Why it did not map, or was left out.
    pub reason: Option<String>,
    /// Its ordinal in the design's timeline, as the source model numbers it.
    pub ordinal: Option<u64>,
    /// A short description of its form (`extrude, to a plane`), without names or sizes.
    pub variant: Option<String>,
    /// It mapped, but its profile region was not checked against the source.
    pub profile_unverified: bool,
    /// It mapped, but re-applies to a base body that already holds its result (the body as
    /// the design ends), so editing it may not change the shape: kept, not rebuilt.
    pub reapplied: bool,
    /// A base feature written with its body as the design ends, which later features re-apply
    /// to.
    pub final_body: bool,
    /// It mapped with a selection the source does not record, which Dutchman inferred: never
    /// an exact reading of the source.
    pub inferred: Option<Inferred>,
}

/// Where a partial history hands over to the imported rest of the design: the converted
/// features come first, then one imported body holds the source's final shape.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestImported {
    /// The part the history belongs to, when the rows of several parts are given together.
    pub part: Option<String>,
    /// How many features, from the first, are editable.
    pub converted: usize,
    /// The first feature that is not converted.
    pub from: String,
    /// Why it is not converted.
    pub reason: Option<String>,
}

impl RestImported {
    /// The first feature not converted in a design's rows, given as each row's status, name
    /// and reason in timeline order; `None` when every feature converted. Rows of an
    /// assembly are named "part: feature"; the first part to stop is the one reported.
    pub fn find<'a>(
        rows: impl IntoIterator<Item = (FeatureStatus, Option<&'a str>, Option<&'a str>)>,
    ) -> Option<Self> {
        let rows: Vec<_> = rows.into_iter().collect();
        let split = |name: &'a str| name.split_once(": ");
        let of_parts = !rows.is_empty()
            && rows
                .iter()
                .all(|(_, name, _)| name.is_some_and(|name| split(name).is_some()));
        let blocked = rows
            .iter()
            .position(|(status, _, _)| *status == FeatureStatus::Blocked)?;
        let (_, name, reason) = rows.get(blocked)?;
        let name = name.unwrap_or("the next feature");
        let (part, from) = match split(name) {
            Some((part, from)) if of_parts => (Some(part), from),
            _ => (None, name),
        };
        let converted = rows
            .iter()
            .take(blocked)
            .filter(|(status, row, _)| {
                *status == FeatureStatus::Mapped
                    && (!of_parts || row.and_then(split).map(|(of, _)| of) == part)
            })
            .count();
        Some(Self {
            part: part.map(str::to_owned),
            converted,
            from: from.to_owned(),
            reason: reason
                .map(|reason| reason.trim().trim_end_matches('.').to_owned())
                .filter(|reason| !reason.is_empty()),
        })
    }

    /// The same, from a report's feature rows.
    pub fn of(features: &[FeatureOutcome]) -> Option<Self> {
        Self::find(
            features
                .iter()
                .map(|f| (f.status, f.name.as_deref(), f.reason.as_deref())),
        )
    }

    /// The sentence every report gives: "Features 1-7 are editable; from Hole 1 on, the
    /// part is one imported body.", then why the first feature did not convert.
    pub fn sentence(&self) -> String {
        let editable = match self.converted {
            0 => "no feature is editable".to_owned(),
            1 => "feature 1 is editable".to_owned(),
            n => format!("features 1\u{2013}{n} are editable"),
        };
        let mut text = format!(
            "{editable}; from {} on, the part is one imported body.",
            self.from
        );
        match &self.part {
            Some(part) => text.insert_str(0, &format!("{part}: ")),
            None => {
                if let Some(first) = text.get_mut(..1) {
                    first.make_ascii_uppercase();
                }
            }
        }
        if let Some(reason) = &self.reason {
            text.push_str(&format!(" {}: {reason}.", self.from));
        }
        text
    }
}

/// How a selection the source does not record was inferred: candidates rebuilt in the target
/// and measured against the source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inferred {
    /// What was inferred.
    pub selection: InferredSelection,
    /// The candidates measured.
    pub candidates: usize,
    /// Of those, the ones that matched the source: the one kept.
    pub matches: usize,
    /// What the kept candidate was measured against.
    pub against: InferredAgainst,
    /// The largest distance between the kept candidate's surface and the source's, in
    /// nanometres, when the two solids were compared directly.
    pub deviation_nm: Option<u64>,
}

/// The selection a feature's source leaves out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferredSelection {
    /// The sketch profile region an extrude, revolve or loft sweeps.
    ProfileRegion,
    /// The edges a fillet or chamfer rounds.
    Edges,
}

/// What an inferred candidate was measured against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InferredAgainst {
    /// The source's final solid: volume, area, box and, for one body, the distance between
    /// the surfaces.
    OriginalShape,
    /// The bodies the source stores right after the feature: where each one is.
    StoredBodies,
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
    /// It stops body-building history, or follows the first blocking feature.
    Blocked,
}

impl Report {
    /// Features the source file already had flagged as failed in FreeCAD, before converting.
    pub fn source_errors(&self) -> &[SourceError] {
        &self.source_errors
    }

    /// How many features the source file had flagged as failed, which can exceed the rows kept.
    pub fn source_error_total(&self) -> u64 {
        self.source_error_total.max(self.source_errors.len() as u64)
    }

    /// Records features the source file already had flagged as failed in FreeCAD. `total`
    /// counts them all; `errors` holds the ones kept.
    pub fn record_source_errors(&mut self, errors: Vec<SourceError>, total: u64) {
        self.source_error_total += total.max(errors.len() as u64);
        self.source_errors.extend(errors);
    }

    /// The plain sentence for a file that already had errors in FreeCAD, or `None`.
    pub fn source_errors_message(&self) -> Option<String> {
        const SHOWN: usize = 10;
        if self.source_errors.is_empty() {
            return None;
        }
        let mut names: Vec<&str> = Vec::new();
        for error in &self.source_errors {
            if !names.contains(&error.label.as_str()) {
                names.push(&error.label);
            }
        }
        let total = names.len().max(self.source_error_total() as usize);
        let mut list = names
            .iter()
            .take(SHOWN)
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        if total > SHOWN {
            list.push_str(&format!(" and {} more", total - SHOWN));
        }
        Some(format!(
            "This file already had errors in FreeCAD before converting: {list} (fix them in \
             FreeCAD for a full conversion)."
        ))
    }

    /// Memory needed and available, in bytes, when a writer skipped the history check.
    pub fn history_check_skipped(&self) -> Option<(u64, u64)> {
        self.history_check_skipped
    }

    /// Record a history check skipped for memory; the converted design is still usable.
    pub fn skip_history_check(&mut self, needed: u64, available: u64) {
        self.history_check_skipped = Some((needed, available));
    }

    /// The tolerances the writer's history check used, as it states them, when it ran one.
    pub fn verification_tolerances(&self) -> Option<&serde_json::Value> {
        self.verification_tolerances.as_ref()
    }

    /// Records the tolerances the writer's history check used.
    pub fn set_verification_tolerances(&mut self, tolerances: serde_json::Value) {
        self.verification_tolerances = Some(tolerances);
    }

    /// The version of the program that saved the input file, as `major.minor.revision`, when
    /// the file states it as numbers (an Inventor file does). Never read from a name or message.
    pub fn source_version(&self) -> Option<&str> {
        self.source_version.as_deref()
    }

    /// Records the saving program's version once; the first one recorded is kept.
    pub fn record_source_version(&mut self, version: String) {
        self.source_version.get_or_insert(version);
    }

    /// What the source states about its up axis and what the writer did about it.
    pub fn up_axis(&self) -> Option<&crate::UpAxisReport> {
        self.up_axis.as_ref()
    }

    /// Records what the source states about its up axis, with a note when the designs of an
    /// assembly disagree. The first one recorded is kept.
    pub fn record_up_axis(&mut self, found: crate::UpAxisFinding, disagreement: Option<String>) {
        if self.up_axis.is_none() {
            self.up_axis = Some(crate::UpAxisReport {
                found,
                applied: None,
                disagreement,
            });
        }
    }

    /// Records the orientation a writer applied. A source no reader recorded anything for is
    /// recorded as unknown.
    pub fn record_orientation(&mut self, mode: crate::Orientation, stands_up: bool) {
        let entry = self.up_axis.get_or_insert_with(|| crate::UpAxisReport {
            found: crate::UpAxisFinding::default(),
            applied: None,
            disagreement: None,
        });
        entry.applied = Some((mode, stands_up));
    }

    /// Requested and bounded maximum Dutchman part workers, excluding child-process threads.
    pub fn threads(&self) -> Option<(usize, usize)> {
        self.threads
    }

    /// Records the bounded number of part workers used by this conversion.
    pub fn set_threads(&mut self, requested: usize, used: usize) {
        let used = self
            .threads
            .filter(|(previous, _)| *previous == requested)
            .map_or(used, |(_, previous)| previous.max(used));
        self.threads = Some((requested, used));
    }

    /// Records a loss in Dutchman's own mapping.
    pub fn push(
        &mut self,
        severity: Severity,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.push_at(Stage::Map, severity, subject, detail);
    }

    /// Records a dropped appearance with a fixed cause and a human-readable explanation.
    pub fn push_appearance(&mut self, cause: AppearanceLoss, detail: impl Into<String>) {
        self.losses.push(Loss {
            severity: Severity::Dropped,
            subject: "appearance".to_owned(),
            code: Some(cause.code().to_owned()),
            detail: detail.into(),
            stage: Stage::Map,
        });
    }

    /// Records a loss in Dutchman's own mapping under a stable `code`.
    pub fn push_coded(
        &mut self,
        severity: Severity,
        subject: impl Into<String>,
        code: &str,
        detail: impl Into<String>,
    ) {
        self.losses.push(Loss {
            severity,
            subject: subject.into(),
            code: Some(code.to_owned()),
            detail: detail.into(),
            stage: Stage::Map,
        });
    }

    /// Records a loss that arose at `stage`.
    pub fn push_at(
        &mut self,
        stage: Stage,
        severity: Severity,
        subject: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.losses.push(Loss {
            severity,
            subject: subject.into(),
            code: None,
            detail: detail.into(),
            stage,
        });
    }

    /// Records cadmpeg's loss notes from reading a file, each under its loss code: at the
    /// decode stage when the code says the source could not be read, else at projection.
    pub fn extend_from_cadmpeg<'a>(&mut self, notes: impl IntoIterator<Item = &'a LossNote>) {
        for note in notes {
            let code = note.code.to_string();
            self.push_cadmpeg(read_stage(&code), note, code);
        }
    }

    /// Records cadmpeg's loss notes from restating the model as STEP, at the write stage.
    pub fn extend_from_cadmpeg_export<'a>(
        &mut self,
        notes: impl IntoIterator<Item = &'a LossNote>,
    ) {
        for note in notes {
            self.push_cadmpeg(Stage::Write, note, note.code.to_string());
        }
    }

    fn push_cadmpeg(&mut self, stage: Stage, note: &LossNote, code: String) {
        self.losses.push(Loss {
            severity: note.severity.into(),
            subject: code.clone(),
            code: Some(code.clone()),
            detail: if code == "f3d/material.not-transferred" {
                "Materials and appearances were not carried over.".to_owned()
            } else {
                note.message.clone()
            },
            stage,
        });
    }

    // A child report can have a different local limit. Keep the conversion's request
    // and the largest part-worker count observed in any child or earlier phase.
    fn merge_threads(&mut self, other: Option<(usize, usize)>) {
        if let Some((requested, used)) = other {
            let (requested, used) = self
                .threads
                .map_or((requested, used), |(parent, previous)| {
                    (parent, previous.max(used))
                });
            self.set_threads(requested, used);
        }
    }

    fn merge_history_check(&mut self, other: Option<(u64, u64)>) {
        if let Some((needed, available)) = other {
            if self
                .history_check_skipped
                .is_none_or(|(previous, _)| needed > previous)
            {
                self.skip_history_check(needed, available);
            }
        }
    }

    /// Accumulate source-occurrence placement rules for this conversion.
    pub fn record_part_positions(
        &mut self,
        mode: crate::PartPositions,
        counts: crate::PartPositionCounts,
    ) {
        match &mut self.part_positions {
            Some((_, total)) => total.add(counts),
            None => self.part_positions = Some((mode, counts)),
        }
    }
    /// Forgets how a part was placed and the notes about it, for a part read on its own whose
    /// place something else states: a parts package says where each of its parts goes.
    pub(crate) fn forget_part_positions(&mut self) {
        self.part_positions = None;
        self.placement_assumptions.clear();
        self.losses.retain(|loss| loss.subject != "part positions");
    }
    /// Records a part placed by an assumption, by the names the design gives it.
    pub(crate) fn assume_placement(&mut self, part: String, why: crate::PlacementAssumption) {
        self.placement_assumptions.push((part, why));
    }
    /// The parts placed by an assumption, most doubtful reason first, then by name.
    pub fn placement_assumptions(&self) -> &[(String, crate::PlacementAssumption)] {
        &self.placement_assumptions
    }
    /// Selected policy and disjoint placement counts, with fallback reasons.
    pub fn part_positions(&self) -> Option<(crate::PartPositions, crate::PartPositionCounts)> {
        self.part_positions
    }
    /// One user-facing placement summary after the whole source has been read.
    pub(crate) fn note_part_positions(&mut self, mode: crate::PartPositions) {
        let (mode, counts) = *self
            .part_positions
            .get_or_insert((mode, Default::default()));
        self.push(Severity::Info, "part positions", format!(
            "{} parts placed as last shown in the original program ({} directly bound, {} holders derived from displayed children), {} at current poses, {} as first placed ({mode}); first-placement reasons: {} without a current pose, {} ambiguous, {} unsupported, {} joint checks, {} bounds checks, {} inserted-design checks; {} holder conflicts",
            counts.displayed, counts.displayed.saturating_sub(counts.derived_holders), counts.derived_holders, counts.current, counts.first_placed, counts.no_current_pose, counts.ambiguous, counts.unsupported, counts.joints, counts.bounds, counts.instances, counts.holder_conflicts
        ));
        self.placement_assumptions
            .sort_by(|a, b| (a.1, &a.0).cmp(&(b.1, &b.0)));
        self.note_placement_assumptions();
    }

    /// Names the parts placed by an assumption. A part the decoder put at the origin is a
    /// warning; a stored pose is usually right, so it stays a note.
    fn note_placement_assumptions(&mut self) {
        const SHOWN: usize = 20;
        const ISSUE: &str = "https://github.com/dutchman-cad/dutchman/issues/512";
        let names = |parts: &[&str]| {
            let shown = parts
                .iter()
                .take(SHOWN)
                .copied()
                .collect::<Vec<_>>()
                .join("; ");
            match parts.len().saturating_sub(SHOWN) {
                0 => shown,
                more => format!("{shown} and {more} more (all in the JSON report)"),
            }
        };
        let of = |wanted: &dyn Fn(crate::PlacementAssumption) -> bool| {
            self.placement_assumptions
                .iter()
                .filter(|(_, why)| wanted(*why))
                .map(|(part, _)| part.clone())
                .collect::<Vec<_>>()
        };
        let identity = of(&|why| why == crate::PlacementAssumption::Identity);
        let stored = of(&|why| why != crate::PlacementAssumption::Identity);
        if !identity.is_empty() {
            let parts: Vec<&str> = identity.iter().map(String::as_str).collect();
            self.push(Severity::Degraded, "part positions", format!(
                "{} parts sit at their design's origin because their placement could not be read, so they may not be where the source has them: {}. See {ISSUE}",
                parts.len(), names(&parts)
            ));
        }
        if !stored.is_empty() {
            let parts: Vec<&str> = stored.iter().map(String::as_str).collect();
            self.push(Severity::Info, "part positions", format!(
                "{} parts rest on a stored pose that no joint, saved view or usable later pose confirms. Most are right; check these if one looks misplaced: {}. See {ISSUE}",
                parts.len(), names(&parts)
            ));
        }
    }

    /// Source comparisons that ran, including explicit reasons for unavailable checks.
    pub fn verification_checks(&self) -> &[serde_json::Value] {
        &self.verification_checks
    }

    /// Records a bounded comparison report for one part.
    pub fn record_verification_checks(&mut self, checks: serde_json::Value) {
        if self.verification_checks.len() < MAX_NAMES {
            self.verification_checks.push(checks);
        }
    }

    /// Records every loss of `other`, after this report's own.
    pub fn append(&mut self, other: Report) {
        if let Some(up) = other.up_axis.clone() {
            self.record_up_axis(up.found, up.disagreement);
        }
        for checks in other.verification_checks {
            self.record_verification_checks(checks);
        }
        if let Some(version) = other.source_version {
            self.record_source_version(version);
        }
        self.merge_threads(other.threads);
        if let Some((mode, counts)) = other.part_positions {
            self.record_part_positions(mode, counts);
        }
        self.placement_assumptions
            .extend(other.placement_assumptions);
        self.merge_history_check(other.history_check_skipped);
        if self.verification_tolerances.is_none() {
            self.verification_tolerances = other.verification_tolerances;
        }
        self.source_error_total += other.source_error_total;
        self.source_errors.extend(other.source_errors);
        self.losses.extend(other.losses);
        self.names.extend(other.names);
    }

    /// Records every loss of `other` under `prefix` (`part.ipt: …`), after this report's own.
    pub fn append_prefixed(&mut self, other: Report, prefix: &str) {
        if let Some(up) = other.up_axis.clone() {
            self.record_up_axis(up.found, up.disagreement);
        }
        for checks in other.verification_checks {
            self.record_verification_checks(checks);
        }
        if let Some(version) = other.source_version {
            self.record_source_version(version);
        }
        self.merge_threads(other.threads);
        if let Some((mode, counts)) = other.part_positions {
            self.record_part_positions(mode, counts);
        }
        self.placement_assumptions
            .extend(other.placement_assumptions);
        self.merge_history_check(other.history_check_skipped);
        if self.verification_tolerances.is_none() {
            self.verification_tolerances = other.verification_tolerances;
        }
        self.source_error_total += other.source_error_total;
        self.source_errors
            .extend(other.source_errors.into_iter().map(|error| SourceError {
                label: format!("{prefix}: {}", error.label),
                ..error
            }));
        self.losses
            .extend(other.losses.into_iter().map(|loss| Loss {
                subject: format!("{prefix}: {}", loss.subject),
                ..loss
            }));
        self.names.extend(other.names);
    }

    /// Records a name the conversion met: a file's, folder's or path's, or one the design
    /// gives a part, component, body, feature, sketch or parameter. They are never reported;
    /// they are what an anonymous report must take out of messages.
    pub fn saw(&mut self, name: &str) {
        let name = name.trim();
        if !name.is_empty() && self.names.len() < MAX_NAMES {
            self.names.insert(name.to_owned());
        }
    }

    /// Every name the conversion met (see [`Report::saw`]).
    pub fn names(&self) -> &std::collections::BTreeSet<String> {
        &self.names
    }

    /// Records what the writer made of the design.
    pub fn set_outcome(&mut self, mut outcome: Outcome) {
        // A design missing linked components is never verified complete, whatever its replay says.
        if let (Some((_, why)), Check::Verified) = (&self.missing_links, &outcome.check) {
            outcome.check = Check::Unverified(why.clone());
        }
        if let Some(message) = self
            .completeness
            .as_ref()
            .and_then(Completeness::unplaced_message)
        {
            // The missing designs are the larger cause: say so first.
            outcome.check = Check::Partial(match &self.missing_links {
                Some((_, why)) => format!("{why} {message}"),
                None => message,
            });
        }
        self.outcome = Some(outcome);
    }

    /// The source places linked components whose designs aren't in the file. The written
    /// design leaves them out, says so, and is not marked verified.
    pub fn record_missing_links(&mut self, names: &[String]) {
        self.record_missing(names, false);
    }

    /// A Fusion `.f3d` places components inserted from other Fusion designs that the file
    /// doesn't contain (cadmpeg states their prototype as unresolved). One Dropped loss names
    /// each missing design and one summary says what to do; the design is left out and never
    /// marked verified.
    pub fn record_missing_inserted_designs(&mut self, names: &[String]) {
        self.record_missing(names, true);
    }

    fn record_missing(&mut self, names: &[String], inserted: bool) {
        let count = names.len() as u64;
        if count == 0 {
            return;
        }
        let plural = count != 1;
        let why = if inserted {
            for name in names {
                let what = if name.is_empty() { "(unnamed)" } else { name };
                self.push(
                    Severity::Dropped,
                    format!("inserted design {what}"),
                    format!(
                        "the component {what} is inserted from another Fusion design that this \
                         .f3d doesn't contain, so it is left out"
                    ),
                );
            }
            format!(
                "{count} inserted component{} {} from other Fusion designs that this .f3d \
                 doesn't contain; convert the whole export folder (or its zip) so the \
                 inserted designs come along, or export the assembly as a Fusion Archive \
                 (.f3z) from Fusion's File > Export.",
                if plural { "s" } else { "" },
                if plural { "come" } else { "comes" },
            )
        } else {
            let named = if names.iter().all(|name| name.is_empty()) {
                String::new()
            } else {
                format!(" ({})", names.join(", "))
            };
            format!(
                "{count} linked component{} {} in this file{named}. In Fusion, export the \
                 assembly as .f3z, or put the linked designs' .f3d files next to it",
                if plural { "s" } else { "" },
                if plural { "aren't" } else { "isn't" },
            )
        };
        self.push(Severity::Dropped, "linked components", why.clone());
        self.missing_links = Some((count, why));
        if let Some(completeness) = self.completeness.take() {
            self.set_completeness(completeness);
        }
        if let Some(outcome) = self.outcome.take() {
            self.set_outcome(outcome);
        }
    }

    /// The sentence saying linked or inserted components are missing from the file, when some are.
    pub fn missing_links_message(&self) -> Option<&str> {
        self.missing_links.as_ref().map(|(_, why)| why.as_str())
    }

    /// What the writer made of the design, when it said.
    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    /// Update the writer's recorded outcome after a final check, without copying its features.
    pub fn outcome_mut(&mut self) -> Option<&mut Outcome> {
        self.outcome.as_mut()
    }

    /// Records what became of one assembly part's history.
    pub fn push_part(&mut self, part: PartHistory) {
        self.parts.push(part);
    }

    /// What became of each assembly part's history, in the order they were written.
    pub fn parts(&self) -> &[PartHistory] {
        &self.parts
    }

    /// Records how much of the assembly was placed.
    pub fn set_completeness(&mut self, mut completeness: Completeness) {
        if let Some((count, why)) = &self.missing_links {
            if !completeness
                .left_out
                .iter()
                .any(|group| &group.reason == why)
            {
                completeness.left_out.push(LeftOutGroup {
                    file: None,
                    reason: why.clone(),
                    stage: Stage::Decode,
                    count: *count,
                });
            }
        }
        self.completeness = Some(completeness);
        if let Some(outcome) = self.outcome.take() {
            self.set_outcome(outcome);
        }
    }

    /// How much of the assembly was placed, for an assembly.
    pub fn completeness(&self) -> Option<&Completeness> {
        self.completeness.as_ref()
    }

    /// Records the joints written, by kind (`revolute`, `slider`, `rigid`).
    pub fn set_joints(&mut self, joints: std::collections::BTreeMap<String, u64>) {
        self.joints = joints;
    }

    /// The joints written, by kind; empty when none were.
    pub fn joints(&self) -> &std::collections::BTreeMap<String, u64> {
        &self.joints
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

#[cfg(test)]
mod tests {

    #[test]
    fn missing_inserted_designs_are_named_one_each_with_one_summary() {
        let mut report = Report::default();
        report.set_completeness(Completeness {
            placed: 1,
            placed_empty: 0,
            unplaced: [("Local".to_owned(), 1)].into(),
            left_out: Vec::new(),
        });
        report.record_missing_inserted_designs(&["Washer".to_owned(), String::new()]);
        report.set_outcome(Outcome {
            mode: Mode::History,
            check: Check::Verified,
            features: Vec::new(),
        });
        let dropped: Vec<_> = report
            .losses()
            .iter()
            .filter(|loss| loss.severity == Severity::Dropped)
            .map(|loss| loss.subject.as_str())
            .collect();
        assert_eq!(
            dropped,
            [
                "inserted design Washer",
                "inserted design (unnamed)",
                "linked components"
            ]
        );
        let summary = "2 inserted components come from other Fusion designs that this .f3d \
                       doesn't contain; convert the whole export folder (or its zip) so the \
                       inserted designs come along, or export the assembly as a Fusion Archive \
                       (.f3z) from Fusion's File > Export.";
        let left_out = &report.completeness().unwrap().left_out;
        assert_eq!(
            (left_out[0].reason.as_str(), left_out[0].count),
            (summary, 2)
        );
        let Some(Check::Partial(why)) = report.outcome().map(|o| &o.check) else {
            panic!("expected partial");
        };
        assert!(why.starts_with(summary), "{why}");
        let mut one = Report::default();
        one.record_missing_inserted_designs(&["Washer".to_owned()]);
        assert!(one.losses().last().unwrap().detail.starts_with(
            "1 inserted component comes from other Fusion designs that this .f3d doesn't contain; \
             convert the whole export folder (or its zip) so the inserted designs come along, \
             or export the assembly as a Fusion Archive (.f3z) from Fusion's File > Export."
        ));
    }

    #[test]
    fn missing_linked_components_are_left_out_and_never_verified() {
        let mut report = Report::default();
        report.set_completeness(Completeness {
            placed: 2,
            placed_empty: 0,
            unplaced: Default::default(),
            left_out: Vec::new(),
        });
        report.record_missing_links(&["Pin".to_owned()]);
        report.set_outcome(Outcome {
            mode: Mode::History,
            check: Check::Verified,
            features: Vec::new(),
        });
        let Some(Check::Unverified(why)) = report.outcome().map(|outcome| &outcome.check) else {
            panic!("expected unverified");
        };
        assert!(
            why.contains("1 linked component isn't in this file (Pin)"),
            "{why}"
        );
        assert!(why.contains("export the assembly as .f3z"), "{why}");
        let left_out = &report.completeness().unwrap().left_out;
        assert_eq!(left_out.len(), 1);
        assert_eq!(left_out[0].count, 1);
        // A conversion with no missing links keeps its verified result.
        let mut clean = Report::default();
        clean.record_missing_links(&[]);
        clean.set_outcome(Outcome {
            mode: Mode::History,
            check: Check::Verified,
            features: Vec::new(),
        });
        assert_eq!(
            clean.outcome().map(|outcome| &outcome.check),
            Some(&Check::Verified)
        );
    }
    use super::*;

    #[test]
    fn verification_evidence_survives_both_report_merges() {
        let evidence = serde_json::json!({"part":"Cube", "geometry":{"body_count":{"status":"pass","source":1,"written":1}}});
        let mut a = Report::default();
        a.record_verification_checks(evidence.clone());
        let mut b = Report::default();
        b.append(a.clone());
        b.append_prefixed(a, "part");
        assert_eq!(b.verification_checks(), &[evidence.clone(), evidence]);
    }

    #[test]
    fn cadmpeg_losses_are_staged_by_their_code() {
        for code in [
            "f3d/geometry.not-transferred",
            "f3d/geometry.brep-blob-undecoded",
            "f3d/history.record-framing-failed",
            "f3d/parameter.record-unparsed",
            "f3d/source.archive-entry-unreadable",
            "f3d/container.missing-geometry-stream",
        ] {
            assert_eq!(read_stage(code), Stage::Decode, "{code}");
        }
        for code in [
            "f3d/feature.definition-incomplete",
            "f3d/feature.scope-payload-uninterpreted",
            "f3d/sketch.curve-unprojected",
            "f3d/feature.profile-selection-native",
            "f3d/assembly.components-external",
            "f3d/parameter.expression-unbound",
        ] {
            assert_eq!(read_stage(code), Stage::Project, "{code}");
        }
    }

    #[test]
    fn a_loss_has_a_plain_title_shared_by_its_kind() {
        let mut report = Report::default();
        let coded = |severity, code: &str| Loss {
            severity,
            subject: code.to_owned(),
            code: Some(code.to_owned()),
            detail: "detail".to_owned(),
            stage: Stage::Project,
        };
        report.push(Severity::Degraded, "feature \"Chamfer1\"", "not mapped yet");
        report.push(Severity::Dropped, "body \"Body2\"", "no geometry");
        report.push_appearance(AppearanceLoss::Wire, "a wire body");
        let titles: Vec<String> = [
            coded(Severity::Info, "f3d/source.unindexed-records-retained"),
            coded(Severity::Degraded, "f3d/source.unindexed-records-retained"),
            coded(Severity::Info, "f3d/source.manifest-metadata-unresolved"),
            coded(
                Severity::Degraded,
                "f3d/sketch.name-composed-from-feature-ordinal",
            ),
            coded(Severity::Dropped, "sldprt/unheard-of.thing"),
        ]
        .iter()
        .chain(report.losses())
        .map(Loss::title)
        .collect();
        assert_eq!(
            titles,
            [
                "Parts of the file kept as-is",
                "Parts of the file kept as-is",
                "Notes about parts of the file",
                "Sketches carried over with changes",
                "Design details left out",
                "Features carried over with changes",
                "Bodies, faces and edges left out",
                "Appearances left out",
            ]
        );
    }

    #[test]
    fn a_loss_names_the_project_it_arose_in() {
        let mut report = Report::default();
        report.push(Severity::Degraded, "history", "a fillet is not mapped yet");
        report.push_at(Stage::SolidCraft, Severity::Info, "geometry", "not checked");
        report.push_at(Stage::Project, Severity::Info, "validation", "a finding");
        let upstream: Vec<_> = report.losses().iter().map(Loss::upstream).collect();
        assert_eq!(upstream, [None, Some("solidcraft"), Some("cadmpeg")]);
    }
}

#[cfg(test)]
mod worker_report_tests {
    use super::Report;

    #[test]
    fn fusion_material_notes_use_plain_language_and_keep_the_code() {
        use cadmpeg_ir::report::loss::{
            LossKind, LossNamespace, LossNote, LossTaxonomy, NamespacedLossKind,
        };
        let kind = LossKind::Namespaced(NamespacedLossKind::new(
            LossNamespace::new("f3d").unwrap(),
            "material.not-transferred",
            LossTaxonomy::MaterialNotTransferred,
        ));
        let note = LossNote::new(
            kind,
            "Materials/appearances (.protein assets, ACT/design assignments) were not transferred.",
        );
        let mut report = Report::default();
        report.extend_from_cadmpeg([&note]);
        assert_eq!(
            report.losses()[0].detail,
            "Materials and appearances were not carried over."
        );
        assert_eq!(
            report.losses()[0].code.as_deref(),
            Some("f3d/material.not-transferred")
        );
    }

    #[test]
    fn child_reports_keep_peak_workers_and_the_conversion_request() {
        let mut report = Report::default();
        report.set_threads(0, 4);
        let mut child = Report::default();
        child.set_threads(1, 1);
        report.append(child);
        assert_eq!(report.threads(), Some((0, 4)));
        let mut child = Report::default();
        child.set_threads(0, 6);
        report.append_prefixed(child, "part.f3d");
        report.set_threads(0, 1);
        assert_eq!(report.threads(), Some((0, 6)));
        let mut outer = Report::default();
        outer.append_prefixed(report, "assembly.f3z");
        assert_eq!(outer.threads(), Some((0, 6)));
    }
}

#[cfg(test)]
mod rest_imported_tests {
    use super::*;

    fn rows<'a>(
        rows: &'a [(FeatureStatus, &'a str, Option<&'a str>)],
    ) -> impl Iterator<Item = (FeatureStatus, Option<&'a str>, Option<&'a str>)> + 'a {
        rows.iter()
            .map(|(status, name, reason)| (*status, Some(*name), *reason))
    }

    #[test]
    fn a_part_says_where_its_editable_features_end() {
        use FeatureStatus::{Blocked, Mapped, NotNeeded};
        let table = [
            (Mapped, "Sketch 1", None),
            (NotNeeded, "Plane 1", None),
            (Mapped, "Extrude 1", None),
            (Blocked, "Hole 1", Some("hole is not mapped yet.")),
            (Blocked, "Hole 2", Some("after the first blocking feature")),
        ];
        let rest = RestImported::find(rows(&table)).unwrap();
        assert_eq!((rest.converted, rest.from.as_str()), (2, "Hole 1"));
        assert_eq!(
            rest.sentence(),
            "Features 1\u{2013}2 are editable; from Hole 1 on, the part is one imported body. \
             Hole 1: hole is not mapped yet."
        );
        let one = [(Mapped, "Sketch 1", None), (Blocked, "Loft 1", None)];
        assert_eq!(
            RestImported::find(rows(&one)).unwrap().sentence(),
            "Feature 1 is editable; from Loft 1 on, the part is one imported body."
        );
        let all = [(Mapped, "Sketch 1", None)];
        assert_eq!(RestImported::find(rows(&all)), None);
    }

    #[test]
    fn an_assembly_names_the_first_part_to_stop() {
        use FeatureStatus::{Blocked, Mapped};
        let table = [
            (Mapped, "a.ipt: Sketch 1", None),
            (Mapped, "a.ipt: Extrude 1", None),
            (Mapped, "b.ipt: Sketch 1", None),
            (Blocked, "b.ipt: Loft 1", Some("lofts are not mapped")),
        ];
        let rest = RestImported::find(rows(&table)).unwrap();
        assert_eq!(rest.part.as_deref(), Some("b.ipt"));
        assert_eq!(
            rest.sentence(),
            "b.ipt: feature 1 is editable; from Loft 1 on, the part is one imported body. \
             Loft 1: lofts are not mapped."
        );
    }
}
