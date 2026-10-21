//! What the `dutchman` command and the `dutchman-mcp` server share: how a conversion is set
//! up, and the reports of converting, inspecting and listing formats, as the JSON documents
//! `docs/json.md` describes. Both front ends print or send these; neither builds its own.
//!
//! This is the front ends' shared code, not a stable library API.

pub mod batch;
pub mod formats;
pub mod json;
pub mod mcp;

use std::path::{Path, PathBuf};
use std::time::Duration;

use dutchman_core::{Error, Format, Report};
use dutchman_solidcraft::{SolidCraftWriter, Verification, DEFAULT_TIMEOUT};
use serde_json::{json, Value};

/// How a design is converted.
#[derive(Clone, Debug, clap::Args)]
pub struct ConvertOptions {
    /// Write the geometry as one non-parametric import, never the feature history.
    #[arg(long)]
    pub import_only: bool,
    /// The SolidCraft CLI that checks a written history against the original geometry
    /// (default: `$SOLIDCRAFT_CLI`, else `solidcraft-cli` on `PATH`).
    #[arg(long, value_name = "PATH", conflicts_with = "no_verify")]
    pub solidcraft: Option<PathBuf>,
    /// Write history without checking it in SolidCraft.
    #[arg(long)]
    pub no_verify: bool,
    /// Seconds each SolidCraft check may run before the history is written unverified.
    #[arg(long, value_name = "SECONDS", default_value_t = DEFAULT_TIMEOUT.as_secs())]
    pub verify_timeout: u64,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        Self {
            import_only: false,
            solidcraft: None,
            no_verify: false,
            verify_timeout: DEFAULT_TIMEOUT.as_secs(),
        }
    }
}

impl ConvertOptions {
    /// The writer these options describe.
    pub fn writer(&self) -> SolidCraftWriter {
        let verification = match (self.no_verify, &self.solidcraft) {
            (true, _) => Verification::Off,
            (false, Some(path)) => Verification::With(path.clone()),
            (false, None) => Verification::Auto,
        };
        SolidCraftWriter::new()
            .import_only(self.import_only)
            .verification(verification)
            .timeout(Duration::from_secs(self.verify_timeout))
    }
}

/// Converts `input` into `output`, writing nothing else, and returns the `dutchman.convert/1`
/// report, failed or not.
pub fn convert(input: &Path, output: &Path, writer: &SolidCraftWriter) -> Value {
    match dutchman_core::convert(input, output, writer) {
        Ok(report) => convert_success(input, output, &report),
        Err(err) => json::failure(json::CONVERT, json::input(input), &err.to_string()),
    }
}

/// The `dutchman.convert/1` report of a conversion that succeeded.
pub fn convert_success(input: &Path, output: &Path, report: &Report) -> Value {
    let mut out = json::success(json::CONVERT, json::input(input), report);
    out.insert(
        "output".to_owned(),
        json!({"path": output.display().to_string(), "format": dutchman_solidcraft::FORMAT}),
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

/// Reads `input` and works out what converting it would do, without writing anything.
pub fn inspect(input: &Path) -> Result<Inspection, Error> {
    let mut report = Report::default();
    let format = Format::from_path(input).ok_or_else(|| Error::UnknownFormat {
        path: input.to_path_buf(),
    })?;
    let model = dutchman_core::read(format, input, &mut report)?;
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
pub fn inspect_report(input: &Path) -> Value {
    match inspect(input) {
        Ok(inspection) => inspection.json(input),
        Err(err) => json::failure(json::INSPECT, json::input(input), &err.to_string()),
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
