//! What the `dutchman` command and the `dutchman-mcp` server share: how a conversion is set
//! up, and the reports of converting, inspecting and listing formats, as the JSON documents
//! `docs/json.md` describes. Both front ends print or send these; neither builds its own.
//!
//! This is the front ends' shared code, not a stable library API.

pub mod anon;
pub mod batch;
pub mod choices;
pub mod dxf;
pub mod formats;
pub mod issue;
pub mod json;
pub mod mcp;
pub mod present;
pub mod progress;
pub mod watch;
pub mod worker;

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use dutchman_core::PartSearch;
use dutchman_core::{Error, Format, Model, Report, Writer};
use dutchman_solidcraft::{SolidCraftVersion, SolidCraftWriter, Verification, DEFAULT_TIMEOUT};
use serde_json::{json, Value};

/// The format written by a conversion; independent of FreeCAD input support.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum OutputFormat {
    /// A SolidCraft design (`.solidcraft`).
    #[default]
    Solidcraft,
    /// A FreeCAD rebuild macro (`.FCMacro`). Experimental.
    Freecad,
    /// A native FreeCAD document (`.FCStd`). Experimental; requires FreeCAD.
    Fcstd,
    /// FreeCAD TechDraw pages as ASCII DXF (convert command).
    Dxf,
}

impl OutputFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Solidcraft => "solidcraft",
            Self::Freecad => "FCMacro",
            Self::Fcstd => "FCStd",
            Self::Dxf => "dxf",
        }
    }
}

/// The selected output adapter.
#[derive(Clone, Debug)]
pub enum OutputWriter {
    SolidCraft(SolidCraftWriter),
    FreeCad(dutchman_freecad::run::FreeCadRunner),
    Dxf,
}

impl Writer for OutputWriter {
    fn extension(&self) -> &'static str {
        match self {
            Self::SolidCraft(writer) => writer.extension(),
            Self::FreeCad(writer) => writer.extension(),
            Self::Dxf => "dxf",
        }
    }

    fn write(
        &self,
        model: &Model,
        out: &mut dyn std::io::Write,
        report: &mut Report,
    ) -> Result<(), Error> {
        match self {
            Self::SolidCraft(writer) => writer.write(model, out, report),
            Self::FreeCad(writer) => writer.write(model, out, report),
            Self::Dxf => Err(Error::Encode("DXF page output uses dutchman convert --to dxf; convert-all and the single-file Writer are not supported yet".into())),
        }
    }
}

/// How a design is converted.
#[derive(Clone, Debug, clap::Args)]
pub struct ConvertOptions {
    /// Output format: SolidCraft (default), a FreeCAD macro/document (experimental),
    /// or partial TechDraw DXF pages (convert command only).
    #[arg(long, value_enum, default_value = "solidcraft")]
    pub to: OutputFormat,
    /// What to keep: fast (the shape only), standard (history, sketches and joints, with an
    /// assembly's history left out when it would not fit in free memory) or full.
    #[arg(long, value_enum, default_value = "standard")]
    pub preset: choices::Preset,
    /// Write the geometry as one non-parametric import, never the feature history.
    #[arg(long, alias = "no-history")]
    pub import_only: bool,
    /// Leave sketches out.
    #[arg(long)]
    pub no_sketches: bool,
    /// Leave assembly joints out.
    #[arg(long)]
    pub no_joints: bool,
    /// Convert even when the estimate says the conversion will not fit in free memory.
    #[arg(long)]
    pub no_memory_check: bool,
    /// The SolidCraft CLI that checks a written history against the original geometry
    /// (default: `$SOLIDCRAFT_CLI`, else `solidcraft-cli` on `PATH`, else the one inside an
    /// installed SolidCraft app).
    #[arg(long, value_name = "PATH", conflicts_with = "no_verify")]
    pub solidcraft: Option<PathBuf>,
    /// Made for the current SolidCraft (any release from the last two weeks); older
    /// versions get simplified features, or pass --solidcraft-version x.y.z (at least
    /// 0.18.2). Defaults to the found CLI's version, else the newest known at build time,
    /// even with --no-verify.
    #[arg(long, value_name = "x.y.z")]
    pub solidcraft_version: Option<SolidCraftVersion>,
    /// Write history without checking it in SolidCraft.
    #[arg(long)]
    pub no_verify: bool,
    /// Check FreeCAD macro output by running FreeCAD headlessly.
    #[arg(long, conflicts_with = "no_verify")]
    pub verify: bool,
    /// Seconds FreeCAD may run before it is killed.
    #[arg(long, value_name = "SECONDS", default_value_t = 300)]
    pub freecad_timeout: u64,
    /// Refuse to replace existing output files (explicit outputs otherwise replace).
    #[arg(long)]
    pub no_clobber: bool,
    /// Seconds each SolidCraft check may run before the history is written unverified.
    #[arg(long, value_name = "SECONDS", default_value_t = DEFAULT_TIMEOUT.as_secs())]
    pub verify_timeout: u64,
    /// The project folder an assembly's part files are searched in when they are not at their
    /// stored paths (default: the nearest folder above the assembly holding an Inventor .ipj,
    /// else two folders up; for convert-all, each folder given).
    #[arg(long, value_name = "DIR")]
    pub project: Option<PathBuf>,
    /// Another folder to search, with its subfolders, for part files the project folder does
    /// not hold; may be given more than once.
    #[arg(long = "search", value_name = "DIR")]
    pub search: Vec<PathBuf>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            to: OutputFormat::default(),
            preset: choices::Preset::default(),
            import_only: false,
            no_sketches: false,
            no_joints: false,
            no_memory_check: false,
            solidcraft: None,
            solidcraft_version: None,
            no_verify: false,
            verify: false,
            freecad_timeout: 300,
            no_clobber: false,
            verify_timeout: DEFAULT_TIMEOUT.as_secs(),
            project: None,
            search: Vec::new(),
        }
    }
}

impl ConvertOptions {
    /// How these options check written designs in SolidCraft.
    pub fn verification(&self) -> Verification {
        match (self.no_verify, &self.solidcraft) {
            (true, _) => Verification::Off,
            (false, Some(path)) => Verification::With(path.clone()),
            (false, None) => Verification::Auto,
        }
    }

    /// What these options keep: their preset, narrowed by the per-choice switches.
    pub fn choices(&self) -> choices::Choices {
        let mut choices = choices::Choices::of(self.preset);
        choices.history &= !self.import_only;
        choices.sketches &= !self.no_sketches;
        choices.joints &= !self.no_joints;
        choices
    }

    /// Where these options say to look for an assembly's part files.
    pub fn part_search(&self) -> PartSearch {
        PartSearch {
            project: self.project.clone(),
            also: self.search.clone(),
        }
    }

    /// The writer these options describe.
    pub fn writer(&self) -> OutputWriter {
        match self.to {
            OutputFormat::Dxf => OutputWriter::Dxf,
            OutputFormat::Solidcraft => OutputWriter::SolidCraft(
                SolidCraftWriter::new()
                    .import_only(self.import_only)
                    .verification(self.verification())
                    .target_version(self.solidcraft_version)
                    .timeout(Duration::from_secs(self.verify_timeout)),
            ),
            OutputFormat::Freecad | OutputFormat::Fcstd => {
                OutputWriter::FreeCad(dutchman_freecad::run::FreeCadRunner::new(
                    dutchman_freecad::FreeCadWriter::new().import_only(self.import_only),
                    self.to == OutputFormat::Fcstd,
                    self.verify,
                    Duration::from_secs(self.freecad_timeout),
                ))
            }
        }
    }
}

/// Converts `input` into `output`, writing nothing else, and returns the `dutchman.convert/1`
/// report, failed or not. An existing `output` is replaced only when `replace` is set. An
/// assembly's part files are looked for as `search` says (see [`dutchman_core::read_in`]).
pub fn convert(
    input: &Path,
    output: &Path,
    writer: &dyn Writer,
    replace: bool,
    search: &PartSearch,
) -> Value {
    match dutchman_core::convert_traced(input, output, writer, replace, search, &mut |_| {}) {
        Ok(report) => convert_success(input, output, &report),
        Err(err) => json::error(json::CONVERT, json::input(input), &err),
    }
}

/// Converts with ordered progress snapshots and a five-second heartbeat.
pub fn convert_with_progress(
    input: &Path,
    output: &Path,
    writer: &dyn Writer,
    replace: bool,
    search: &PartSearch,
    on_progress: &(dyn Fn(dutchman_core::ProgressEvent) + Send + Sync),
) -> Value {
    let progress = dutchman_core::Progress::new(on_progress);
    match dutchman_core::convert_with_progress(input, output, writer, replace, search, &progress) {
        Ok(report) => convert_success(input, output, &report),
        Err(err) => json::error(json::CONVERT, json::input(input), &err),
    }
}

/// The `dutchman.convert/1` report of a conversion that succeeded.
pub fn convert_success(input: &Path, output: &Path, report: &Report) -> Value {
    let mut out = json::success(json::CONVERT, json::input(input), report);
    let format = if output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("FCMacro"))
    {
        dutchman_freecad::FORMAT
    } else if output
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("FCStd"))
    {
        dutchman_freecad::run::DOCUMENT_FORMAT
    } else {
        dutchman_solidcraft::FORMAT
    };
    out.insert(
        "output".to_owned(),
        json!({"path": output.display().to_string(), "format": format}),
    );
    Value::Object(out)
}

/// What converting a file would carry across, found without writing anything.
pub struct Inspection {
    /// The losses, with the outcome converting would have.
    pub report: Report,
    /// The design's bodies and faces.
    pub bodies: usize,
    pub faces: usize,
    /// For an assembly: how many part files it reads, and how many placements it makes.
    pub assembly: Option<(usize, usize)>,
    /// Whether converting it would succeed: `Err` with why it would fail.
    pub conversion: Result<(), String>,
}

/// Reads `input`, looking for an assembly's part files as `search` says, and works out what
/// converting it would do, without writing anything.
pub fn inspect(input: &Path, search: &PartSearch) -> Result<Inspection, Error> {
    inspect_inner(input, search, None)
}

/// Inspects with read progress and a five-second heartbeat.
pub fn inspect_with_progress(
    input: &Path,
    search: &PartSearch,
    on_progress: &(dyn Fn(dutchman_core::ProgressEvent) + Send + Sync),
) -> Result<Inspection, Error> {
    let progress = dutchman_core::Progress::new(on_progress);
    progress.run(|| inspect_inner(input, search, Some(&progress)))
}

fn inspect_inner(
    input: &Path,
    search: &PartSearch,
    progress: Option<&dutchman_core::Progress<'_>>,
) -> Result<Inspection, Error> {
    if let Some(progress) = progress {
        progress.stage(dutchman_core::Stage::Input);
    }
    let mut report = Report::default();
    let format = Format::from_path(input).ok_or_else(|| Error::UnknownFormat {
        path: input.to_path_buf(),
    })?;
    let model = dutchman_core::read_with_progress(format, input, search, &mut report, progress)?;
    if let Some(progress) = progress {
        progress.stage(dutchman_core::Stage::Map);
    }
    let writer = SolidCraftWriter::new();
    let conversion = writer.convertible(&model);
    let outcome = writer.plan(&model, &mut report);
    report.set_outcome(outcome);
    Ok(Inspection {
        conversion,
        report,
        bodies: model.ir.model.bodies.len(),
        faces: model.ir.model.faces.len(),
        assembly: model
            .assembly
            .as_ref()
            .map(|assembly| (assembly.parts.len(), assembly.placements.len())),
    })
}

/// The `dutchman.inspect/1` report of inspecting `input`, failed or not.
pub fn inspect_report(input: &Path, search: &PartSearch) -> Value {
    match inspect(input, search) {
        Ok(inspection) => inspection.json(input),
        Err(err) => json::error(json::INSPECT, json::input(input), &err),
    }
}

impl Inspection {
    /// The `dutchman.inspect/1` report.
    pub fn json(&self, input: &Path) -> Value {
        let mut out = json::success(json::INSPECT, json::input(input), &self.report);
        out.insert(
            "geometry".to_owned(),
            json!({"bodies": self.bodies, "faces": self.faces}),
        );
        out.insert(
            "conversion".to_owned(),
            match &self.conversion {
                Ok(()) => json!({"possible": true, "reason": null}),
                Err(reason) => json!({"possible": false, "reason": reason}),
            },
        );
        if let Some((parts, placements)) = self.assembly {
            out.insert(
                "assembly".to_owned(),
                json!({"parts": parts, "placements": placements}),
            );
        }
        Value::Object(out)
    }
}
