//! What the `dutchman` command and the `dutchman-mcp` server share: how a conversion is set
//! up, and the reports of converting, inspecting and listing formats, as the JSON documents
//! `docs/json.md` describes. Both front ends print or send these; neither builds its own.
//!
//! This is the front ends' shared code, not a stable library API.

pub mod anon;
pub mod batch;
pub mod choices;
pub mod diagnostics;
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
    /// A SolidCraft design (`.sc`).
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
            Self::Solidcraft => SolidCraftVersion::default().extension(),
            Self::Freecad => "FCMacro",
            Self::Fcstd => "FCStd",
            Self::Dxf => "dxf",
        }
    }
}

/// The selected output adapter.
#[derive(Clone, Debug)]
pub enum OutputWriter {
    /// Retains options so the batch runner can prepare each input's memory budget.
    Configured {
        writer: Box<OutputWriter>,
        options: Box<ConvertOptions>,
    },
    SolidCraft(SolidCraftWriter),
    FreeCad(dutchman_freecad::run::FreeCadRunner),
    Dxf,
}

impl Writer for OutputWriter {
    fn extension(&self) -> &'static str {
        match self {
            Self::Configured { writer, .. } => writer.extension(),
            Self::SolidCraft(writer) => writer.extension(),
            Self::FreeCad(writer) => writer.extension(),
            Self::Dxf => "dxf",
        }
    }

    fn accepts_extension(&self, extension: &str) -> bool {
        match self {
            Self::Configured { writer, .. } => writer.accepts_extension(extension),
            Self::SolidCraft(writer) => writer.accepts_extension(extension),
            Self::FreeCad(writer) => writer.accepts_extension(extension),
            Self::Dxf => extension.eq_ignore_ascii_case(self.extension()),
        }
    }

    fn write(
        &self,
        model: &Model,
        out: &mut dyn std::io::Write,
        report: &mut Report,
    ) -> Result<(), Error> {
        match self {
            Self::Configured { writer, .. } => writer.write(model, out, report),
            Self::SolidCraft(writer) => writer.write(model, out, report),
            Self::FreeCad(writer) => writer.write(model, out, report),
            Self::Dxf => Err(Error::Encode("DXF page output uses dutchman convert --to dxf; convert-all and the single-file Writer are not supported yet".into())),
        }
    }
}

/// How a design is converted.
#[derive(Clone, Debug, clap::Args)]
pub struct ConvertOptions {
    /// Maximum assembly part workers (0: automatic, 1: serial), capped by available memory.
    #[arg(long, value_name = "N", default_value_t = 0)]
    pub threads: usize,
    /// Append an anonymous live diagnostic log, including progress while a conversion runs.
    #[arg(long, value_name = "PATH")]
    pub diagnostics: Option<PathBuf>,
    /// Save a diagnostic log in the platform's default folder, keeping the newest ten.
    #[arg(long, conflicts_with = "diagnostics")]
    pub diagnostics_default: bool,
    /// Private salt shared with an anonymous report when one is requested.
    #[arg(skip)]
    pub diagnostics_salt: Option<anon::Salt>,
    /// Concurrent conversions selected by the front end.
    #[arg(skip = 1_usize)]
    pub diagnostics_threads: usize,
    /// Output format: SolidCraft (default), a FreeCAD macro/document (experimental),
    /// or partial TechDraw DXF pages (convert command only).
    #[arg(long, value_enum, default_value = "solidcraft")]
    pub to: OutputFormat,
    /// What to keep: fast (the shape only), standard (history, sketches and joints, with
    /// history left out when it would not fit in free memory) or full.
    #[arg(long, value_enum, default_value = "standard")]
    pub preset: choices::Preset,
    /// Leave feature history out; keep geometry and the preset's reference sketches.
    #[arg(long, visible_alias = "no-history")]
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
    /// even with --no-verify. Default output uses .sc from 0.25.1, else .solidcraft.
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
            threads: 0,
            diagnostics: None,
            diagnostics_default: false,
            diagnostics_salt: None,
            diagnostics_threads: 1,
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

    /// The CLI version for reports, avoiding discovery for an explicit unchecked target.
    pub(crate) fn reported_solidcraft_version(&self) -> Option<String> {
        let detection = self
            .solidcraft
            .as_ref()
            .map_or(Verification::Auto, |path| Verification::With(path.clone()));
        if self.no_verify && self.solidcraft_version.is_some() {
            dutchman_solidcraft::cached_solidcraft_version(&detection)
        } else {
            dutchman_solidcraft::solidcraft_version(&detection)
        }
    }

    /// The default output suffix, respecting the writer's target release.
    pub fn output_extension(&self) -> &'static str {
        if self.to == OutputFormat::Solidcraft {
            self.output_writer().extension()
        } else {
            self.to.extension()
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
            threads: self.threads,
            project: self.project.clone(),
            also: self.search.clone(),
        }
    }

    /// The writer these options describe, retaining options for per-input batch preparation.
    pub fn writer(&self) -> OutputWriter {
        OutputWriter::Configured {
            writer: Box::new(self.output_writer()),
            options: Box::new(self.clone()),
        }
    }

    fn output_writer(&self) -> OutputWriter {
        match self.to {
            OutputFormat::Dxf => OutputWriter::Dxf,
            OutputFormat::Solidcraft => OutputWriter::SolidCraft(
                SolidCraftWriter::new()
                    .threads(self.threads)
                    .import_only(!self.choices().history)
                    .sketches(self.choices().sketches)
                    .joints(self.choices().joints)
                    .verification(self.verification())
                    .target_version(self.solidcraft_version)
                    .timeout(Duration::from_secs(self.verify_timeout)),
            ),
            OutputFormat::Freecad | OutputFormat::Fcstd => {
                OutputWriter::FreeCad(dutchman_freecad::run::FreeCadRunner::new(
                    dutchman_freecad::FreeCadWriter::new().import_only(!self.choices().history),
                    self.to == OutputFormat::Fcstd,
                    self.verify,
                    Duration::from_secs(self.freecad_timeout),
                ))
            }
        }
    }
}

/// A writer with the history decision fixed before decoding.
#[derive(Clone, Debug)]
pub struct PreparedWriter {
    writer: OutputWriter,
    history_note: Option<String>,
    solidcraft: Option<Value>,
}

impl Writer for PreparedWriter {
    fn extension(&self) -> &'static str {
        self.writer.extension()
    }

    fn accepts_extension(&self, extension: &str) -> bool {
        self.full.accepts_extension(extension)
    }

    fn write(
        &self,
        model: &Model,
        out: &mut dyn std::io::Write,
        report: &mut Report,
    ) -> Result<(), Error> {
        if let Some(note) = &self.history_note {
            report.push(dutchman_core::Severity::Info, "history", note.clone());
        }
        self.writer.write(model, out, report)
    }
}

impl ConvertOptions {
    /// Prepare the history choice from the pre-read estimate, without decoding.
    pub fn writer_for_input(&self, input: &Path) -> PreparedWriter {
        self.prepare_writer(input, choices::available_memory())
    }

    fn prepare_writer(&self, input: &Path, available: Option<u64>) -> PreparedWriter {
        let mut options = self.clone();
        let mut history_note = None;
        if !self.no_memory_check && self.choices().history_within_budget && self.choices().history {
            if let (Some(estimate), Some(available)) = (
                choices::estimate_with_threads(input, &self.choices(), self.threads),
                available,
            ) {
                if estimate.peak_bytes > available {
                    options.import_only = true;
                    history_note = Some(format!(
                        "feature history left out: this conversion needs about {:.1} GB with history and {:.1} GB are free; use the full preset to keep it",
                        choices::gb(estimate.peak_bytes), choices::gb(available)
                    ));
                }
            }
        }
        let solidcraft = json::solidcraft_info(self);
        if let Some(target) = solidcraft
            .as_ref()
            .and_then(|value| value.get("target"))
            .and_then(Value::as_str)
        {
            options.solidcraft_version = target.parse().ok();
        }
        PreparedWriter {
            writer: options.output_writer(),
            history_note,
            solidcraft,
        }
    }

    fn preflight(&self, input: &Path, available: Option<u64>) -> Result<PreparedWriter, Value> {
        if let Some(message) = choices::memory_check_with(input, self, available).refusal() {
            return Err(json::memory_failure(json::CONVERT, input, self, &message));
        }
        Ok(self.prepare_writer(input, available))
    }
}

/// Convert with the shared memory policy, prepared writer, and estimate/choice report fields.
pub fn convert_options_with_progress(
    input: &Path,
    output: &Path,
    options: &ConvertOptions,
    replace: bool,
    search: &PartSearch,
    on_progress: &(dyn Fn(dutchman_core::ProgressEvent) + Send + Sync),
) -> Value {
    convert_options_with_observer(input, output, options, replace, search, on_progress, None)
}

pub(crate) fn convert_options_with_observer(
    input: &Path,
    output: &Path,
    options: &ConvertOptions,
    replace: bool,
    search: &PartSearch,
    on_progress: &(dyn Fn(dutchman_core::ProgressEvent) + Send + Sync),
    observer: Option<std::sync::Arc<dyn dutchman_core::MemberObserver>>,
) -> Value {
    if options.diagnostics.as_ref().is_some_and(|path| {
        path == output
            || std::fs::canonicalize(path)
                .ok()
                .zip(std::fs::canonicalize(output).ok())
                .is_some_and(|(a, b)| a == b)
    }) {
        return json::failure(
            json::CONVERT,
            json::input(input),
            "the diagnostic log would be replaced by the output; choose another --diagnostics path",
        );
    }
    diagnostics::run(input, options, on_progress, |callback, log| {
        let available = if options.no_memory_check || options.preset == choices::Preset::Fast {
            None
        } else {
            choices::available_memory()
        };
        let prepared = options.preflight(input, available);
        let progress = dutchman_core::Progress::new(callback)
            .with_members(log.map(diagnostics::Log::observer).or(observer));
        convert_prepared(input, output, options, replace, search, &progress, prepared)
    })
}

fn convert_prepared(
    input: &Path,
    output: &Path,
    options: &ConvertOptions,
    replace: bool,
    search: &PartSearch,
    progress: &dutchman_core::Progress<'_>,
    prepared: Result<PreparedWriter, Value>,
) -> Value {
    let writer = match prepared {
        Ok(writer) => writer,
        Err(report) => return report,
    };
    let mut report = match dutchman_core::convert_with_progress(
        input, output, &writer, replace, search, progress,
    ) {
        Ok(report) => convert_success(input, output, &report),
        Err(err) => json::error(json::CONVERT, json::input(input), &err),
    };
    if let (Some(solidcraft), Some(fields)) = (writer.solidcraft, report.as_object_mut()) {
        fields.insert("solidcraft".to_owned(), solidcraft);
    }
    json::conversion_choices(&mut report, input, options);
    report
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

#[cfg(test)]
mod option_tests {
    use super::*;

    #[test]
    fn configured_batch_writers_apply_the_memory_guard_before_creating_output() {
        let Some(available) = choices::available_memory() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("synthetic.step");
        std::fs::File::create(&input)
            .unwrap()
            .set_len(available.saturating_add(2_000_000_000))
            .unwrap();
        let folder = dir.path().join("never-created");
        let job = batch::Job {
            input,
            output: folder.join("synthetic.solidcraft"),
            replace: true,
            search: PartSearch::default(),
        };
        let options = ConvertOptions {
            preset: choices::Preset::Full,
            ..Default::default()
        };
        let result = std::sync::Mutex::new(None);
        batch::run_each(
            &[job],
            &options.writer(),
            1,
            &std::sync::atomic::AtomicBool::new(false),
            |event| {
                if let batch::Event::Finished(_, report) = event {
                    *result.lock().unwrap() = Some(report);
                }
            },
        );
        let report = result.into_inner().unwrap().unwrap();
        assert_eq!(report["ok"], false);
        assert_eq!(report["choices"]["preset"], "full");
        assert!(report["error"]["message"]
            .as_str()
            .unwrap()
            .starts_with("this conversion needs about "));
        assert!(!folder.exists());
    }

    #[test]
    fn standard_leaves_history_out_for_single_parts_before_decode() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures/synthetic_cube.ipt");
        let options = ConvertOptions {
            no_verify: true,
            ..Default::default()
        };
        let mut reduced = options.choices();
        reduced.history = false;
        let available = choices::estimate(&fixture, &reduced).unwrap().peak_bytes;
        let writer = options.preflight(&fixture, Some(available)).unwrap();
        assert!(writer.history_note.is_some());
        let fusion = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d");
        let fusion_available = choices::estimate(&fusion, &reduced).unwrap().peak_bytes;
        assert!(options
            .preflight(&fusion, Some(fusion_available))
            .unwrap()
            .history_note
            .is_some());
        let part =
            dutchman_core::read(Format::InventorPart, &fixture, &mut Report::default()).unwrap();
        assert!(part.assembly.is_none());
        let mut report = Report::default();
        writer.write(&part, &mut Vec::new(), &mut report).unwrap();
        let reasons: Vec<_> = report
            .losses()
            .iter()
            .filter(|loss| {
                loss.subject == "history" && loss.detail.starts_with("feature history left out:")
            })
            .collect();
        assert_eq!(reasons.len(), 1);
        assert_eq!(reasons[0].severity, dutchman_core::Severity::Info);
        let mut json = convert_success(&fixture, Path::new("test.solidcraft"), &report);
        json::conversion_choices(&mut json, &fixture, &options);
        assert_eq!(json["choices"]["history"], false);
    }

    #[test]
    fn memory_refusal_never_decodes_or_progresses_past_input() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("synthetic.ipt");
        // Deliberately not a CAD file: reaching decode would give a different error.
        std::fs::write(&input, b"synthetic").unwrap();
        for preset in [choices::Preset::Standard, choices::Preset::Full] {
            let options = ConvertOptions {
                preset,
                ..Default::default()
            };
            let events = std::sync::Mutex::new(Vec::new());
            let callback = |event| events.lock().unwrap().push(event);
            let progress = dutchman_core::Progress::new(&callback);
            let output = dir.path().join("never-written.solidcraft");
            let report = convert_prepared(
                &input,
                &output,
                &options,
                false,
                &PartSearch::default(),
                &progress,
                options.preflight(&input, Some(0)),
            );
            assert_eq!(report["ok"], false);
            assert_eq!(report["error"]["stage"], "input");
            assert!(report["error"]["message"]
                .as_str()
                .unwrap()
                .starts_with("this conversion needs about "));
            assert!(events.lock().unwrap().is_empty());
            assert!(!output.exists());
        }
    }
}
