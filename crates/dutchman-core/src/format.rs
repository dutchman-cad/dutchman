use std::path::Path;

/// A native design format Dutchman recognises, whether or not it can read it yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Format {
    /// Autodesk Inventor part (`.ipt`).
    InventorPart,
    /// Autodesk Inventor assembly (`.iam`).
    InventorAssembly,
    /// Autodesk Fusion archive (`.f3d`).
    FusionArchive,
    /// SolidWorks part (`.sldprt`).
    SolidWorksPart,
    /// SolidWorks assembly (`.sldasm`).
    SolidWorksAssembly,
}

impl Format {
    /// Every recognised format.
    pub const ALL: [Format; 5] = [
        Format::InventorPart,
        Format::InventorAssembly,
        Format::FusionArchive,
        Format::SolidWorksPart,
        Format::SolidWorksAssembly,
    ];

    /// A stable identifier for scripts (`fusion_archive`, `inventor_part`, …).
    pub fn id(self) -> &'static str {
        match self {
            Format::InventorPart => "inventor_part",
            Format::InventorAssembly => "inventor_assembly",
            Format::FusionArchive => "fusion_archive",
            Format::SolidWorksPart => "solidworks_part",
            Format::SolidWorksAssembly => "solidworks_assembly",
        }
    }

    /// The format's file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            Format::InventorPart => "ipt",
            Format::InventorAssembly => "iam",
            Format::FusionArchive => "f3d",
            Format::SolidWorksPart => "sldprt",
            Format::SolidWorksAssembly => "sldasm",
        }
    }

    /// A human-readable name, for messages.
    pub fn name(self) -> &'static str {
        match self {
            Format::InventorPart => "Inventor part (.ipt)",
            Format::InventorAssembly => "Inventor assembly (.iam)",
            Format::FusionArchive => "Fusion archive (.f3d)",
            Format::SolidWorksPart => "SolidWorks part (.sldprt)",
            Format::SolidWorksAssembly => "SolidWorks assembly (.sldasm)",
        }
    }

    /// The product family, as cadmpeg's work on it is named.
    pub fn family(self) -> &'static str {
        match self {
            Format::InventorPart | Format::InventorAssembly => "Inventor",
            Format::FusionArchive => "Fusion",
            Format::SolidWorksPart | Format::SolidWorksAssembly => "SolidWorks",
        }
    }

    /// The names of the formats Dutchman converts, for messages:
    /// `Inventor part (.ipt) and Fusion archive (.f3d) files`.
    pub fn supported_names() -> String {
        let names: Vec<&str> = Format::ALL
            .into_iter()
            .filter(|format| format.codec_id().is_some())
            .map(Format::name)
            .collect();
        let list = match names.split_last() {
            Some((last, rest)) if !rest.is_empty() => format!("{} and {last}", rest.join(", ")),
            _ => names.concat(),
        };
        format!("{list} files")
    }

    /// The cadmpeg codec that decodes this format, when Dutchman is built with it.
    pub fn codec_id(self) -> Option<&'static str> {
        match self {
            Format::FusionArchive => Some("f3d"),
            Format::InventorPart | Format::InventorAssembly => Some("inventor"),
            Format::SolidWorksPart => Some("sldprt"),
            Format::SolidWorksAssembly => None,
        }
    }

    /// Whether the format is an assembly, whose geometry lives in the part files it places.
    pub fn is_assembly(self) -> bool {
        matches!(self, Format::InventorAssembly | Format::SolidWorksAssembly)
    }

    /// The format a cadmpeg codec decodes, when it decodes exactly one Dutchman names.
    pub fn from_codec_id(id: &str) -> Option<Format> {
        match id {
            "f3d" => Some(Format::FusionArchive),
            "sldprt" => Some(Format::SolidWorksPart),
            _ => None,
        }
    }

    /// Identifies a format from a path's extension, ignoring case.
    pub fn from_path(path: &Path) -> Option<Format> {
        let ext = path.extension()?.to_str()?;
        Format::ALL
            .into_iter()
            .find(|format| ext.eq_ignore_ascii_case(format.extension()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_by_extension_ignoring_case() {
        assert_eq!(
            Format::from_path(Path::new("a/b.IPT")),
            Some(Format::InventorPart)
        );
        assert_eq!(
            Format::from_path(Path::new("x.sldasm")),
            Some(Format::SolidWorksAssembly)
        );
        assert_eq!(Format::from_path(Path::new("x.step")), None);
        assert_eq!(Format::from_path(Path::new("noext")), None);
    }
}
