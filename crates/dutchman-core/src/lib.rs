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
mod locate;
pub mod placement;
mod report;

use std::path::Path;

pub use assembly::{
    local_assembly, Assembly, Joint, Motion, Placement, MAX_DEPTH, MAX_PARTS, MAX_PLACEMENTS,
};
pub use error::Error;
pub use format::Format;
pub use locate::{PartSearch, MAX_SEARCHED};

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
}
pub use report::{
    Check, FeatureOutcome, FeatureStatus, Loss, Mode, Outcome, Report, Severity, Stage,
};

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

/// [`read`], finding the files an assembly places as `search` says, when they are not at
/// their stored paths.
pub fn read_in(
    format: Format,
    input: &Path,
    search: &PartSearch,
    report: &mut Report,
) -> Result<Model, Error> {
    let finder = locate::Finder::new(search);
    let mut model = assembly::read(
        format,
        input,
        report,
        &decode_file,
        &finder,
        &mut Vec::new(),
    )?;
    if model.assembly.is_none() {
        match assembly::local(&model) {
            assembly::Components::None => {}
            assembly::Components::Assembly(components, left_out) => {
                for reason in left_out {
                    report.push(Severity::Dropped, "joints", reason);
                }
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
    Ok(model)
}

/// Decodes one file, without reading the parts an assembly places.
fn decode_file(format: Format, input: &Path, report: &mut Report) -> Result<Model, Error> {
    if format.codec_id().is_none() {
        return Err(Error::NotSupportedYet { format });
    }
    check_input(input)?;
    let bytes = std::fs::read(input).map_err(|source| Error::Io {
        path: input.to_path_buf(),
        source,
    })?;
    decode::decode(format, input, &bytes, report)
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
    on_stage(Stage::Input);
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
    on_stage(Stage::Decode);
    let model = read_in(format, input, search, &mut report)?;

    // Encode in memory so a failed conversion never leaves a partial file behind.
    let mut bytes = Vec::new();
    on_stage(Stage::Map);
    writer.write(&model, &mut bytes, &mut report)?;
    on_stage(Stage::Write);
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
