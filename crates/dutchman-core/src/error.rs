use std::path::PathBuf;

use crate::Format;

/// Why a conversion stopped. Anything that merely lost detail is a [`crate::Loss`] instead.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Reading or writing a file failed.
    #[error("{}: {source}", path.display())]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The input is not a regular file.
    #[error("{}: not a file", path.display())]
    NotAFile { path: PathBuf },
    /// The input is larger than Dutchman will open.
    #[error("{}: the file is {bytes} bytes, over Dutchman's {limit}-byte limit", path.display())]
    TooLarge {
        path: PathBuf,
        bytes: u64,
        limit: u64,
    },
    /// The input's extension names no format Dutchman knows.
    #[error(
        "{}: Dutchman does not convert this kind of file. It converts {}.",
        path.display(),
        Format::supported_names()
    )]
    UnknownFormat { path: PathBuf },
    /// The format is known but no reader exists for it yet.
    #[error(
        "{} files are not supported yet. Export a STEP file for SolidCraft to open, or follow \
         the {} work in cadmpeg (https://github.com/cadmpeg/cadmpeg).",
        format.name(),
        format.family()
    )]
    NotSupportedYet { format: Format },
    /// The output path does not end in the writer's extension.
    #[error("{}: output must be a .{expected} file", path.display())]
    WrongOutputExtension {
        path: PathBuf,
        expected: &'static str,
    },
    /// The input is in a known format but its contents could not be decoded.
    #[error("cannot read this design: {0}")]
    Decode(String),
    /// The model could not be encoded into the target format.
    #[error("cannot convert this design: {0}")]
    Encode(String),
}
