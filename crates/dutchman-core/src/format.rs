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
    OnshapeBundle,
    Iges,
    /// A circuit board file, read by the optional electronics module.
    ModuleBoard,
    /// A shared part suffix whose contents do not identify Creo or NX.
    UnclassifiedPart,
    /// An Inventor drawing (.idw).
    InventorDrawing,
    /// A SolidWorks drawing (.slddrw).
    SolidWorksDrawing,
    /// A Fusion drawing (.f2d).
    FusionDrawing,
    /// A DWG drawing: AutoCAD, Inventor, or an Onshape drawing export.
    DwgDrawing,
    /// A DXF drawing, such as an Onshape drawing export.
    DxfDrawing,
    /// A Solid Edge drawing (.dft).
    SolidEdgeDrawing,
}

impl Format {
    /// Every recognised format.
    pub const ALL: [Format; 24] = [
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
        Self::OnshapeBundle,
        Self::Iges,
        Self::ModuleBoard,
        Self::UnclassifiedPart,
        Self::InventorDrawing,
        Self::SolidWorksDrawing,
        Self::FusionDrawing,
        Self::DwgDrawing,
        Self::DxfDrawing,
        Self::SolidEdgeDrawing,
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
            Self::OnshapeBundle => (
                "onshape_bundle",
                "onshape-bundle",
                "Onshape bundle (.onshape-bundle)",
                "Onshape",
            ),
            Self::Iges => ("iges", "iges", "IGES (.igs, .iges)", "IGES"),
            Self::ModuleBoard => (
                "module_board",
                "PcbDoc",
                "Circuit board (.PcbDoc)",
                "Circuit board",
            ),
            Self::UnclassifiedPart => (
                "unclassified_part",
                "prt",
                "Creo or NX part (.prt)",
                "Creo or NX",
            ),
            Self::InventorDrawing => (
                "inventor_drawing",
                "idw",
                "Inventor drawing (.idw)",
                "Inventor",
            ),
            Self::SolidWorksDrawing => (
                "solidworks_drawing",
                "slddrw",
                "SolidWorks drawing (.slddrw)",
                "SolidWorks",
            ),
            Self::FusionDrawing => ("fusion_drawing", "f2d", "Fusion drawing (.f2d)", "Fusion"),
            Self::DwgDrawing => (
                "dwg_drawing",
                "dwg",
                "DWG drawing (.dwg)",
                "AutoCAD or Inventor",
            ),
            Self::DxfDrawing => ("dxf_drawing", "dxf", "DXF drawing (.dxf)", "AutoCAD"),
            Self::SolidEdgeDrawing => (
                "solid_edge_drawing",
                "dft",
                "Solid Edge drawing (.dft)",
                "Solid Edge",
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
            Self::OnshapeBundle => &["onshape-bundle"],
            Self::Iges => &["igs", "iges"],
            // The fallback: with the module installed, `dutchman-electronics info` lists them all.
            Self::ModuleBoard => crate::module::FALLBACK_EXTENSIONS,
            Self::InventorDrawing => &["idw"],
            Self::SolidWorksDrawing => &["slddrw"],
            Self::FusionDrawing => &["f2d"],
            Self::DwgDrawing => &["dwg"],
            Self::DxfDrawing => &["dxf"],
            Self::SolidEdgeDrawing => &["dft"],
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
            Self::SolidWorksPart | Self::SolidWorksAssembly => Some("sldprt"),
            Self::UnclassifiedPart | Self::ModuleBoard => None,
            Self::InventorDrawing
            | Self::SolidWorksDrawing
            | Self::FusionDrawing
            | Self::DwgDrawing
            | Self::DxfDrawing
            | Self::SolidEdgeDrawing => None,
            Self::FreeCadDocument => Some("fcstd"),
            Self::CatiaPart => Some("catia"),
            Self::CreoPart => Some("creo"),
            Self::NxPart => Some("nx"),
            Self::CreoView => Some("pvz"),
            Self::Rhino => Some("rhino"),
            Self::Acis => Some("sat"),
            Self::Step => Some("step"),
            Self::OnshapeBundle => Some("onshape-bundle"),
            Self::Iges => Some("iges"),
        }
    }

    /// Codec available in this build.
    pub fn codec_id(self) -> Option<&'static str> {
        if self == Self::OnshapeBundle && !cfg!(feature = "onshape") {
            return None;
        }
        if self == Self::FreeCadDocument && !cfg!(feature = "freecad") {
            return None;
        }
        self.registry_id()
    }

    /// Whether the geometry is read by an optional module, a separate program.
    pub fn needs_module(self) -> bool {
        self == Self::ModuleBoard
    }

    /// cadmpeg's published headline, in its format-support ladder wording.
    pub fn headline_level(self) -> Option<String> {
        cadmpeg_registry::headline_level(self.registry_id()?)
            .ok()
            .flatten()
            .map(|level| format!("L{level}"))
    }

    /// Whether this is a drawing format: converting it writes a report, and a SolidCraft
    /// drawing when its sheets can be read.
    pub fn is_drawing(self) -> bool {
        matches!(
            self,
            Self::InventorDrawing
                | Self::SolidWorksDrawing
                | Self::FusionDrawing
                | Self::DwgDrawing
                | Self::DxfDrawing
                | Self::SolidEdgeDrawing
        )
    }

    /// Whether Dutchman can read usable geometry for this format.
    pub fn reads_geometry(self) -> bool {
        !matches!(self, Self::UnclassifiedPart | Self::Iges) && !self.is_drawing()
    }

    /// Plain words for the reader's current capability.
    pub fn words(self) -> &'static str {
        if self == Self::ModuleBoard {
            "reads geometry with the electronics module"
        } else if self == Self::Iges {
            "open it in SolidCraft directly"
        } else if self.is_drawing() {
            "recognized, but reading its sheets isn't supported yet. The report lists what was found."
        } else if self.reads_geometry() {
            "reads geometry"
        } else if self.codec_id().is_some() {
            "opens, no geometry yet"
        } else {
            "recognized, not read yet"
        }
    }

    /// Whether this build can attempt geometry conversion.
    pub fn can_convert(self) -> bool {
        if self.needs_module() {
            return crate::module::installed();
        }
        // A drawing is always accepted: it gets a report, and a drawing when it can be read.
        if self.is_drawing() {
            return true;
        }
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
        Self::ALL
            .into_iter()
            .find(|f| {
                f.extensions()
                    .iter()
                    .any(|suffix| ext.eq_ignore_ascii_case(suffix))
            })
            .or_else(|| crate::module::reads_extension(ext).then_some(Self::ModuleBoard))
    }
}

#[cfg(test)]
mod drawing_words_tests {
    use super::Format;

    #[test]
    fn every_drawing_format_says_one_thing_about_what_it_reads() {
        for format in Format::ALL.into_iter().filter(|f| f.is_drawing()) {
            let words = format.words();
            assert_eq!(
                words,
                "recognized, but reading its sheets isn't supported yet. The report lists what was found."
            );
            let message = crate::drawing::not_decoded_message(format);
            assert!(message.starts_with(&format!("{}: no drawing was written", format.name())));
            assert!(!message.contains("accepted") && !message.contains("cannot be read"));
            assert!(format.status_for(std::path::Path::new("x")).contains(words));
        }
    }
}
