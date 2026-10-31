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
    /// The format is known but conversion is not supported yet, or not in this build.
    #[error("{}", not_supported(*format))]
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
    /// Structured, anonymous facts from cadmpeg's model checks.
    #[error("cannot read this design: {message}")]
    Checks { message: String, facts: CheckFacts },
    /// The conversion exceeds the selected memory budget.
    #[error("{0}")]
    MemoryBudget(String),
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
            Error::MemoryBudget(_)
            | Error::Io { .. }
            | Error::NotAFile { .. }
            | Error::TooLarge { .. }
            | Error::UnknownFormat { .. }
            | Error::OutputExists { .. }
            | Error::WrongOutputExtension { .. }
            | Error::Assembly(_) => Stage::Input,
            Error::NotSupportedYet { .. } | Error::Decode(_) => Stage::Decode,
            Error::NoGeometry(_) | Error::Checks { .. } => Stage::Project,
            Error::Encode(_) | Error::Restate(_) => Stage::Write,
        }
    }

    /// The project whose code it stopped in, when it is not Dutchman's own.
    pub fn upstream(&self) -> Option<&'static str> {
        match self {
            // These formats are refused by Dutchman before calling a reader.
            Error::NotSupportedYet {
                format: Format::FreeCadDocument | Format::Iges,
            } => None,
            Error::NotSupportedYet { .. }
            | Error::Decode(_)
            | Error::Checks { .. }
            | Error::NoGeometry(_)
            | Error::Restate(_) => Some("cadmpeg"),
            _ => None,
        }
    }
}

/// Where Dutchman's release builds are.
pub const RELEASES: &str = "https://github.com/dutchman-cad/dutchman/releases";

/// The issue form for one file that did not convert.
pub const FILE_DID_NOT_CONVERT: &str =
    "https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml";

/// What to do with a file of a format this build does not convert.
fn not_supported(format: Format) -> String {
    match format {
        Format::Iges => {
            "Dutchman doesn't convert IGES yet: open it in SolidCraft directly (File > Import)."
                .to_owned()
        }
        // Built without the `freecad` feature.
        Format::FreeCadDocument => {
            format!(
                "this build leaves FreeCAD files out (built without the `freecad` feature); \
                 Dutchman's release builds convert them: {RELEASES}"
            )
        }
        _ if !format.reads_geometry() => format!("{}; reading geometry is not supported yet. Export a STEP file and convert that instead.", format.status()),
        _ => format!(
            "{} files are not supported yet. Export a STEP file for SolidCraft to open; \
             Dutchman's issues (https://github.com/dutchman-cad/dutchman/issues) say when {} \
             files will convert.",
            format.name(),
            format.family()
        ),
    }
}

/// Counts only: no source identities, messages, or geometry are retained here.
#[derive(Clone, Debug, Default)]
pub struct CheckFacts {
    pub findings: std::collections::BTreeMap<String, u64>,
    pub curve_kinds: std::collections::BTreeMap<String, u64>,
    pub(crate) retry: bool,
}

impl Error {
    /// A fixed category safe to include in anonymous diagnostics.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Io { .. } => "io",
            Self::NotAFile { .. } => "not_a_file",
            Self::TooLarge { .. } => "too_large",
            Self::UnknownFormat { .. } => "unknown_format",
            Self::NotSupportedYet { .. } => "not_supported_yet",
            Self::OutputExists { .. } => "output_exists",
            Self::WrongOutputExtension { .. } => "wrong_output_extension",
            Self::Decode(_) => "decode",
            Self::Checks { .. } => "cadmpeg_checks",
            Self::MemoryBudget(_) => "memory_budget",
            Self::Encode(_) => "encode",
            Self::Assembly(_) => "assembly",
            Self::NoGeometry(_) => "no_geometry",
            Self::Restate(_) => "restate",
        }
    }
}

impl CheckFacts {
    /// Accept only the pinned validator's fixed codes across process boundaries.
    pub fn recognizes_code(code: &str) -> bool {
        serde_json::from_value::<cadmpeg_ir::report::check::Check>(serde_json::json!(code)).is_ok()
    }
}
