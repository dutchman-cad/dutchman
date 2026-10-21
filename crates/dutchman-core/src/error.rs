use std::path::PathBuf;

use crate::{Format, Stage};

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
        "{} files are not supported yet. Export a STEP file for SolidCraft to open; Dutchman's \
         issues (https://github.com/dutchman-cad/dutchman/issues) say when {} files will \
         convert.",
        format.name(),
        format.family()
    )]
    NotSupportedYet { format: Format },
    /// The output file exists and replacing it was not allowed.
    #[error("{}: already exists; it is not replaced when overwriting is turned off", path.display())]
    OutputExists { path: PathBuf },
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
    /// An assembly's files do not make a whole assembly: its parts are missing, or it places
    /// itself.
    #[error("cannot read this design: {0}")]
    Assembly(String),
    /// The model was read but holds no geometry the target can take.
    #[error("cannot convert this design: {0}")]
    NoGeometry(String),
    /// cadmpeg could not restate the model as STEP for the target.
    #[error("cannot convert this design: {0}")]
    Restate(String),
}

impl Error {
    /// Where in the conversion it stopped.
    pub fn stage(&self) -> Stage {
        match self {
            Error::Io { .. }
            | Error::NotAFile { .. }
            | Error::TooLarge { .. }
            | Error::UnknownFormat { .. }
            | Error::OutputExists { .. }
            | Error::WrongOutputExtension { .. }
            | Error::Assembly(_) => Stage::Input,
            Error::NotSupportedYet { .. } | Error::Decode(_) => Stage::Decode,
            Error::NoGeometry(_) => Stage::Project,
            Error::Encode(_) | Error::Restate(_) => Stage::Write,
        }
    }

    /// The project whose code it stopped in, when it is not Dutchman's own.
    pub fn upstream(&self) -> Option<&'static str> {
        match self {
            Error::NotSupportedYet { .. }
            | Error::Decode(_)
            | Error::NoGeometry(_)
            | Error::Restate(_) => Some("cadmpeg"),
            _ => None,
        }
    }
}
