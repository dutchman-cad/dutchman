//! Format-neutral conversion core for Dutchman.
//!
//! A conversion is a cadmpeg decode (native file → [`Model`], cadmpeg's neutral CADIR)
//! followed by a [`Writer`] ([`Model`] → target format). The writer never sees the native
//! file and the decoder never sees the target; the [`Report`] carries everything that could
//! not be carried across. See `docs/ARCHITECTURE.md`.

mod error;
mod format;
mod report;

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

pub use error::Error;
pub use format::Format;

/// The neutral model every conversion passes through: cadmpeg's CADIR (exact B-rep, feature
/// history, product structure; millimetres and radians).
pub type Model = cadmpeg_ir::CadIr;
pub use report::{Loss, Report, Severity};

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

/// Decodes the file at `input` (already checked to be `format`) into the neutral model,
/// recording anything dropped in `report`.
fn read(format: Format, _input: &Path, _report: &mut Report) -> Result<Model, Error> {
    // No decoder is wired yet; v0.1 connects cadmpeg's codec for the first format here.
    Err(Error::NotSupportedYet { format })
}

/// Converts the file at `input` into `output` through `writer`.
///
/// The output file is only created once the input has been read successfully.
pub fn convert(input: &Path, output: &Path, writer: &dyn Writer) -> Result<Report, Error> {
    check_output(output, writer)?;
    let format = Format::from_path(input).ok_or_else(|| Error::UnknownFormat {
        path: input.to_path_buf(),
    })?;
    check_input(input)?;

    let mut report = Report::default();
    let model = read(format, input, &mut report)?;

    let file = File::create(output).map_err(|source| Error::Io {
        path: output.to_path_buf(),
        source,
    })?;
    let mut out = BufWriter::new(file);
    writer.write(&model, &mut out, &mut report)?;
    std::io::Write::flush(&mut out).map_err(|source| Error::Io {
        path: output.to_path_buf(),
        source,
    })?;
    Ok(report)
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
