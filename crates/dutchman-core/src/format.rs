use std::path::{Path, PathBuf};

/// A design format Dutchman recognises, whether or not it can read it yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    InventorPart,
    InventorAssembly,
    FusionArchive,
    FusionMultiDocument,
    SolidWorksPart,
    SolidWorksAssembly,
    FreeCadDocument,
    CatiaPart,
    CreoPart,
    NxPart,
    CreoView,
    Rhino,
    Acis,
    Step,
    Iges,
    /// A shared part suffix whose contents do not identify Creo or NX.
    UnclassifiedPart,
}

impl Format {
    /// Every recognised format.
    pub const ALL: [Format; 16] = [
        Self::FusionArchive,
        Self::FusionMultiDocument,
        Self::InventorPart,
        Self::InventorAssembly,
        Self::SolidWorksPart,
        Self::SolidWorksAssembly,
        Self::FreeCadDocument,
        Self::CatiaPart,
        Self::CreoPart,
        Self::NxPart,
        Self::CreoView,
        Self::Rhino,
        Self::Acis,
        Self::Step,
        Self::Iges,
        Self::UnclassifiedPart,
    ];

    fn metadata(self) -> (&'static str, &'static str, &'static str, &'static str) {
        match self {
            Self::InventorPart => ("inventor_part", "ipt", "Inventor part (.ipt)", "Inventor"),
            Self::InventorAssembly => (
                "inventor_assembly",
                "iam",
                "Inventor assembly (.iam)",
                "Inventor",
            ),
            Self::FusionArchive => ("fusion_archive", "f3d", "Fusion archive (.f3d)", "Fusion"),
            Self::FusionMultiDocument => (
                "fusion_multi_document",
                "f3z",
                "Fusion multi-document archive (.f3z)",
                "Fusion",
            ),
            Self::SolidWorksPart => (
                "solidworks_part",
                "sldprt",
                "SolidWorks part (.sldprt)",
                "SolidWorks",
            ),
            Self::SolidWorksAssembly => (
                "solidworks_assembly",
                "sldasm",
                "SolidWorks assembly (.sldasm)",
                "SolidWorks",
            ),
            Self::FreeCadDocument => (
                "freecad_document",
                "FCStd",
                "FreeCAD document (.FCStd)",
                "FreeCAD",
            ),
            Self::CatiaPart => ("catia_part", "CATPart", "CATIA part (.CATPart)", "CATIA"),
            Self::CreoPart => ("creo_part", "prt", "Creo part (.prt, .prt.N)", "Creo"),
            Self::NxPart => ("nx_part", "prt", "NX part (.prt)", "NX"),
            Self::CreoView => ("creo_view", "pvz", "Creo View (.pvz)", "Creo View"),
            Self::Rhino => ("rhino", "3dm", "Rhino (.3dm)", "Rhino"),
            Self::Acis => ("acis", "sat", "ACIS (.sat, .sab, .smt, .smb)", "ACIS"),
            Self::Step => ("step", "step", "STEP (.step, .stp)", "STEP"),
            Self::Iges => ("iges", "iges", "IGES (.igs, .iges)", "IGES"),
            Self::UnclassifiedPart => (
                "unclassified_part",
                "prt",
                "Creo or NX part (.prt)",
                "Creo or NX",
            ),
        }
    }

    /// Stable report identifier.
    pub fn id(self) -> &'static str {
        self.metadata().0
    }
    /// Preferred extension, without the dot.
    pub fn extension(self) -> &'static str {
        self.metadata().1
    }
    /// Accepted suffixes, including alternate spellings.
    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Self::InventorPart => &["ipt"],
            Self::InventorAssembly => &["iam"],
            Self::FusionArchive => &["f3d"],
            Self::FusionMultiDocument => &["f3z"],
            Self::SolidWorksPart => &["sldprt"],
            Self::SolidWorksAssembly => &["sldasm"],
            Self::FreeCadDocument => &["FCStd"],
            Self::CatiaPart => &["CATPart"],
            Self::CreoPart => &["prt", "prt.N"],
            Self::NxPart | Self::UnclassifiedPart => &["prt"],
            Self::CreoView => &["pvz"],
            Self::Rhino => &["3dm"],
            Self::Acis => &["sat", "sab", "smt", "smb"],
            Self::Step => &["step", "stp"],
            Self::Iges => &["igs", "iges"],
        }
    }
    /// Human-readable format name.
    pub fn name(self) -> &'static str {
        self.metadata().2
    }
    /// Program or format family, for messages.
    pub fn family(self) -> &'static str {
        self.metadata().3
    }

    fn registry_id(self) -> Option<&'static str> {
        match self {
            Self::FusionArchive | Self::FusionMultiDocument => Some("f3d"),
            Self::InventorPart | Self::InventorAssembly => Some("inventor"),
            Self::SolidWorksPart => Some("sldprt"),
            Self::SolidWorksAssembly | Self::UnclassifiedPart => None,
            Self::FreeCadDocument => Some("fcstd"),
            Self::CatiaPart => Some("catia"),
            Self::CreoPart => Some("creo"),
            Self::NxPart => Some("nx"),
            Self::CreoView => Some("pvz"),
            Self::Rhino => Some("rhino"),
            Self::Acis => Some("sat"),
            Self::Step => Some("step"),
            Self::Iges => Some("iges"),
        }
    }

    /// Codec available in this build.
    pub fn codec_id(self) -> Option<&'static str> {
        if self == Self::FreeCadDocument && !cfg!(feature = "freecad") {
            return None;
        }
        self.registry_id()
    }

    /// Whether the reader can supply geometry for this format.
    pub fn reads_geometry(self) -> bool {
        !matches!(
            self,
            Self::CreoPart | Self::NxPart | Self::SolidWorksAssembly | Self::UnclassifiedPart
        )
    }

    /// Plain words for the reader's current capability.
    pub fn words(self) -> &'static str {
        if self.reads_geometry() {
            "reads geometry"
        } else if self.codec_id().is_some() {
            "opens, no geometry yet"
        } else {
            "recognised, not read yet"
        }
    }

    /// Whether this build can attempt geometry conversion.
    pub fn can_convert(self) -> bool {
        self.codec_id().is_some() && self.reads_geometry()
    }

    /// The status shown before conversion results.
    pub fn status(self) -> String {
        self.status_with_extension(self.extension())
    }

    /// The same reader status with the suffix used by this input.
    pub fn status_for(self, path: &Path) -> String {
        self.status_with_extension(
            &Self::source_extension(path).unwrap_or_else(|| self.extension().to_owned()),
        )
    }

    fn status_with_extension(self, extension: &str) -> String {
        format!("{} .{}: {}", self.family(), extension, self.words())
    }

    /// The input suffix, keeping the part suffix on numbered saves.
    pub fn source_extension(path: &Path) -> Option<String> {
        let ext = path.extension()?.to_str()?;
        if Self::part_revision(path).is_some() {
            return Some(format!("prt.{ext}"));
        }
        Some(ext.to_owned())
    }

    /// A numbered part's path without its one revision suffix, and its revision digits.
    pub fn part_revision(path: &Path) -> Option<(PathBuf, &str)> {
        let ext = path.extension()?.to_str()?;
        let part = path.with_extension("");
        if ext.is_empty()
            || !ext.bytes().all(|b| b.is_ascii_digit())
            || !part.extension()?.eq_ignore_ascii_case("prt")
        {
            return None;
        }
        Some((part, ext))
    }

    /// Whether the geometry lives in separate part files.
    pub fn is_assembly(self) -> bool {
        matches!(self, Self::InventorAssembly | Self::SolidWorksAssembly)
    }

    /// Format named by a registry codec.
    pub fn from_codec_id(id: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|format| format.registry_id() == Some(id))
    }

    /// Names of formats this build converts.
    pub fn supported_names() -> String {
        let names: Vec<_> = Self::ALL
            .into_iter()
            .filter(|f| f.can_convert())
            .map(Self::name)
            .collect();
        let list = match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => names.concat(),
        };
        format!("{list} files")
    }

    /// Identifies extensions, using cadmpeg content detection for the shared `.prt` suffix.
    pub fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?;
        if ext.eq_ignore_ascii_case("prt") || Self::part_revision(path).is_some() {
            return Some(crate::decode::detect_part(path).unwrap_or(Self::UnclassifiedPart));
        }
        Self::ALL.into_iter().find(|f| {
            f.extensions()
                .iter()
                .any(|suffix| ext.eq_ignore_ascii_case(suffix))
        })
    }
}
