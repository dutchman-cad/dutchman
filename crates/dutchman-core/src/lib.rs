//! Format-neutral conversion core for Dutchman.
//!
//! A conversion is a cadmpeg decode (native file → [`Model`], cadmpeg's neutral CADIR)
//! followed by a [`Writer`] ([`Model`] → target format). The writer never sees the native
//! file and the decoder never sees the target; the [`Report`] carries everything that could
//! not be carried across. See `docs/ARCHITECTURE.md`.

mod assembly;
mod decode;
mod error;
mod format;
mod hidden;
mod input_edges;
mod locate;
mod names;
pub mod placement;
mod progress;
mod report;

use std::path::Path;

pub use assembly::{
    frame_aligned, local_assembly, Assembly, Joint, JointNote, LeftOut, Motion, Placement,
    MAX_DEPTH, MAX_PARTS, MAX_PLACEMENTS,
};
pub use error::{Error, FILE_DID_NOT_CONVERT, RELEASES};
pub use format::Format;
pub use locate::{PartSearch, MAX_SEARCHED};
pub use progress::{elapsed, Progress, ProgressEvent};

/// The neutral model every conversion passes through.
#[derive(Clone, Debug)]
pub struct Model {
    /// The design as cadmpeg's CADIR: exact B-rep, feature history and product structure, in
    /// millimetres and radians.
    pub ir: cadmpeg_ir::CadIr,
    /// Where each entity came from in the source file, when the decoder recorded it. Writers
    /// that restate geometry through cadmpeg's encoders pass it on.
    pub fidelity: Option<cadmpeg_ir::SourceFidelity>,
    /// The design's name, taken from the input file's name.
    pub name: String,
    /// The input file's name (without its folder), when the model was read from a file.
    pub source_file: Option<String>,
    /// For an assembly: its parts, read from their own files, and where each is placed.
    pub assembly: Option<Assembly>,
    /// The input file's text when it is a STEP file whose geometry is used unchanged: writers
    /// hand it on as it is instead of restating the decoded model as STEP.
    pub source_step: Option<String>,
}
pub use report::{
    Check, Completeness, FeatureOutcome, FeatureStatus, LeftOutGroup, Loss, Mode, Outcome,
    PartHistory, Report, Severity, Stage,
};

/// Whether a source-native CADIR operation identifies a construction binder. Keep native
/// type recognition in the core, so writers need only handle the neutral helper role.
pub fn is_native_binder(kind: &str) -> bool {
    matches!(
        kind,
        "PartDesign::SubShapeBinder" | "PartDesign::ShapeBinder"
    )
}

/// The largest input file Dutchman will open, in bytes. Native CAD files beyond this are
/// rejected up front rather than read into memory.
pub const MAX_INPUT_BYTES: u64 = 1 << 30;

/// Encodes the neutral [`Model`] into a target format. This is the plugin seam: a target
/// (SolidCraft today) implements it and depends on nothing else in Dutchman.
pub trait Writer {
    /// The file extension this writer produces, without the dot.
    fn extension(&self) -> &'static str;

    /// Writes `model` to `out`, recording anything it drops in `report`.
    fn write(
        &self,
        model: &Model,
        out: &mut dyn std::io::Write,
        report: &mut Report,
    ) -> Result<(), Error>;
}

/// Decodes the file at `input` (already identified as `format`) into the neutral model,
/// recording anything dropped in `report`.
pub fn read(format: Format, input: &Path, report: &mut Report) -> Result<Model, Error> {
    read_in(format, input, &PartSearch::default(), report)
}

/// Reads the full drawing graph without requiring a 3D body or filtering hidden bodies.
pub fn read_drawings(input: &Path, report: &mut Report) -> Result<Model, Error> {
    let format = Format::from_path(input).ok_or_else(|| Error::UnknownFormat {
        path: input.to_path_buf(),
    })?;
    if !format.can_convert() {
        return Err(Error::NotSupportedYet { format });
    }
    check_input(input)?;
    let bytes = std::fs::read(input).map_err(|source| Error::Io {
        path: input.to_path_buf(),
        source,
    })?;
    decode::drawings(format, input, &bytes, report)
}

/// [`read`], finding the files an assembly places as `search` says, when they are not at
/// their stored paths.
pub fn read_in(
    format: Format,
    input: &Path,
    search: &PartSearch,
    report: &mut Report,
) -> Result<Model, Error> {
    read_with_progress(format, input, search, report, None)
}

/// Reads with coarse progress, including each separate assembly part file.
pub fn read_with_progress(
    format: Format,
    input: &Path,
    search: &PartSearch,
    report: &mut Report,
    progress: Option<&Progress<'_>>,
) -> Result<Model, Error> {
    if !format.is_assembly() && format != Format::FusionMultiDocument {
        if let Some(progress) = progress {
            progress.designs_total(1);
        }
    }
    let read = std::sync::Mutex::new(std::collections::BTreeSet::new());
    let finder = locate::Finder::new(search);
    let mut model = assembly::read(
        format,
        input,
        report,
        &|format, path, report| {
            let model = decode_file(format, path, report, progress)?;
            // The codec owns Fusion archive members; do not count the whole archive as one.
            if format != Format::FusionMultiDocument {
                if let Some(progress) = progress {
                    let first = read.lock().is_ok_and(|mut read| {
                        read.insert(path.canonicalize().unwrap_or_else(|_| path.to_path_buf()))
                    });
                    if first {
                        progress.design_read();
                    }
                    if format.is_assembly() {
                        progress.stage(Stage::Input);
                    }
                }
            }
            Ok(model)
        },
        &finder,
        &mut Vec::new(),
    )?;
    for path in finder.paths() {
        names::saw_stored(&path, report);
    }
    if let Some(searched) = finder.searched_note() {
        report.push_at(Stage::Input, Severity::Info, "parts search", searched);
    }
    if model.assembly.is_none() {
        match assembly::local(&model) {
            assembly::Components::None => {}
            // The writer reports the assembly's joint notes: what it leaves out depends on
            // what it can build.
            // A STEP file's own product structure around one part placed once is no assembly to
            // build: the file goes to SolidCraft as it is.
            assembly::Components::Assembly(components)
                if model.source_step.is_some()
                    && components.parts.len() == 1
                    && components.placements.len() == 1
                    && components.joints.is_empty()
                    && components.left_out.is_empty() => {}
            assembly::Components::Assembly(components) => {
                model.assembly = Some(components);
            }
            assembly::Components::Unplaceable(reason) => report.push(
                Severity::Degraded,
                "components",
                format!(
                    "the design places components, but {reason}; it converts as stored, so \
                     placements beyond the stored geometry are not shown"
                ),
            ),
        }
    }
    if let Some(assembly) = &model.assembly {
        report.set_completeness(assembly.completeness());
    }
    Ok(model)
}

/// The part of `model` holding only `bodies` (CADIR body ids) and what they are made of, named
/// `name`; why it cannot stand alone, else.
pub fn with_bodies(model: &Model, bodies: &[&str], name: String) -> Result<Model, String> {
    let document = serde_json::to_value(&model.ir).map_err(|err| err.to_string())?;
    assembly::geometry_only(model, &document, bodies, name)
}

/// Decodes one file, without reading the parts an assembly places.
fn decode_file(
    format: Format,
    input: &Path,
    report: &mut Report,
    progress: Option<&Progress<'_>>,
) -> Result<Model, Error> {
    if !format.can_convert() {
        return Err(Error::NotSupportedYet { format });
    }
    if let Some(progress) = progress {
        progress.stage(Stage::Input);
    }
    names::saw_path(input, report);
    check_input(input)?;
    let bytes = std::fs::read(input).map_err(|source| Error::Io {
        path: input.to_path_buf(),
        source,
    })?;
    if let Some(progress) = progress {
        progress.stage(Stage::Decode);
    }
    let model = decode::decode_with_progress(format, input, &bytes, report, progress)?;
    names::saw_model(&model, report);
    if format != Format::FreeCadDocument {
        return Ok(model);
    }
    match hidden::shown(&model) {
        Ok(None) => Ok(model),
        Ok(Some((shown, hidden))) => {
            if hidden > 0 {
                let body = if hidden == 1 { "body" } else { "bodies" };
                report.push(
                    Severity::Info,
                    "bodies",
                    format!("{hidden} hidden {body} not converted: FreeCAD hides them or a container above them"),
                );
            }
            Ok(shown)
        }
        Err(why) => {
            report.push(
                Severity::Degraded,
                "bodies",
                format!("hidden bodies converted too, as {why}"),
            );
            Ok(model)
        }
    }
}

/// Converts the file at `input` into `output` through `writer`, replacing an existing output.
///
/// The output file is only written once the whole conversion has succeeded.
pub fn convert(input: &Path, output: &Path, writer: &dyn Writer) -> Result<Report, Error> {
    convert_to(input, output, writer, true)
}

/// [`convert`], replacing an existing `output` only when `replace` is set; otherwise an
/// existing output is refused as [`Error::OutputExists`], before anything is read, and again
/// when writing if one appeared meanwhile.
pub fn convert_to(
    input: &Path,
    output: &Path,
    writer: &dyn Writer,
    replace: bool,
) -> Result<Report, Error> {
    convert_traced(
        input,
        output,
        writer,
        replace,
        &PartSearch::default(),
        &mut |_| {},
    )
}

/// [`convert_to`], finding an assembly's files as `search` says (see [`read_in`]), and telling
/// `on_stage` as each stage begins: decoding (with projection, which cadmpeg does as it
/// reads), then mapping and writing, then writing the file.
pub fn convert_traced(
    input: &Path,
    output: &Path,
    writer: &dyn Writer,
    replace: bool,
    search: &PartSearch,
    on_stage: &mut dyn FnMut(Stage),
) -> Result<Report, Error> {
    convert_impl(input, output, writer, replace, search, on_stage, None)
}

/// Converts with stage, document, and elapsed progress and a five-second heartbeat.
pub fn convert_with_progress(
    input: &Path,
    output: &Path,
    writer: &dyn Writer,
    replace: bool,
    search: &PartSearch,
    progress: &Progress<'_>,
) -> Result<Report, Error> {
    progress.run(|| {
        convert_impl(
            input,
            output,
            writer,
            replace,
            search,
            &mut |_| {},
            Some(progress),
        )
    })
}

#[allow(clippy::too_many_arguments)]
fn convert_impl(
    input: &Path,
    output: &Path,
    writer: &dyn Writer,
    replace: bool,
    search: &PartSearch,
    on_stage: &mut dyn FnMut(Stage),
    progress: Option<&Progress<'_>>,
) -> Result<Report, Error> {
    let mut stage = |stage| {
        on_stage(stage);
        if let Some(progress) = progress {
            progress.stage(stage);
        }
    };
    stage(Stage::Input);
    check_output(output, writer)?;
    if !replace && output.exists() {
        return Err(Error::OutputExists {
            path: output.to_path_buf(),
        });
    }
    let format = Format::from_path(input).ok_or_else(|| Error::UnknownFormat {
        path: input.to_path_buf(),
    })?;

    let mut report = Report::default();
    if progress.is_none() {
        stage(Stage::Decode);
    }
    let model = read_with_progress(format, input, search, &mut report, progress)?;

    // Encode in memory so a failed conversion never leaves a partial file behind.
    let mut bytes = Vec::new();
    stage(Stage::Map);
    writer.write(&model, &mut bytes, &mut report)?;
    stage(Stage::Write);
    write_output(output, &bytes, replace)?;
    Ok(report)
}

/// Writes `bytes` to `output`; when `replace` is not set, only if no file is there.
fn write_output(output: &Path, bytes: &[u8], replace: bool) -> Result<(), Error> {
    use std::io::Write as _;
    let io = |source| Error::Io {
        path: output.to_path_buf(),
        source,
    };
    if replace {
        return std::fs::write(output, bytes).map_err(io);
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(output)
        .map_err(|source| match source.kind() {
            std::io::ErrorKind::AlreadyExists => Error::OutputExists {
                path: output.to_path_buf(),
            },
            _ => io(source),
        })?;
    file.write_all(bytes).map_err(io)
}

fn check_output(output: &Path, writer: &dyn Writer) -> Result<(), Error> {
    let expected = writer.extension();
    let matches = output
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case(expected));
    if matches {
        Ok(())
    } else {
        Err(Error::WrongOutputExtension {
            path: output.to_path_buf(),
            expected,
        })
    }
}

fn check_input(input: &Path) -> Result<(), Error> {
    let meta = std::fs::metadata(input).map_err(|source| Error::Io {
        path: input.to_path_buf(),
        source,
    })?;
    if !meta.is_file() {
        return Err(Error::NotAFile {
            path: input.to_path_buf(),
        });
    }
    if meta.len() > MAX_INPUT_BYTES {
        return Err(Error::TooLarge {
            path: input.to_path_buf(),
            bytes: meta.len(),
            limit: MAX_INPUT_BYTES,
        });
    }
    Ok(())
}
