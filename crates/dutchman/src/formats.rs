//! The formats this build knows and what it converts from each.

use dutchman_core::Format;
use serde_json::{json, Value};

pub const SCHEMA: &str = "dutchman.formats/1";

/// What Dutchman can carry from this format.
pub fn converts(format: Format) -> &'static str {
    if format == Format::Iges {
        return "SolidCraft imports IGES itself (File > Import)";
    }
    if format == Format::FreeCadDocument && format.codec_id().is_none() {
        return "left out of this build (the `freecad` feature); release builds convert them: https://github.com/dutchman-cad/dutchman/releases";
    }
    if !format.can_convert() {
        return "nothing yet; reading geometry is not supported yet";
    }
    match format {
        Format::FusionArchive => {
            "geometry; feature history when every feature maps; components and mapped joints"
        }
        Format::FusionMultiDocument => {
            "as a Fusion archive, with referenced designs merged in by cadmpeg"
        }
        Format::InventorPart => "geometry; feature history when every feature maps",
        Format::InventorAssembly => {
            "placed parts and sub-assemblies, read from their files in the project"
        }
        Format::FreeCadDocument => {
            "geometry and components; feature history and joints when they map"
        }
        Format::SolidWorksPart => {
            "geometry; feature history waits on cadmpeg binding sketches and extrude profiles"
        }
        _ => "geometry through STEP; solids and surfaces SolidCraft can import",
    }
}

/// The text listing.
pub fn text() -> String {
    let mut out = String::new();
    for format in Format::ALL {
        let status = if format.can_convert() {
            "converts"
        } else {
            "not yet"
        };
        out.push_str(&format!(
            "{}  {}{}  {status}: {}\n",
            format.name(),
            format.words(),
            format
                .headline_level()
                .map(|level| format!("  {level}"))
                .unwrap_or_default(),
            converts(format)
        ));
    }
    out.push_str("\nFreeCAD TechDraw → DXF  drawings: page curves, dimension labels, annotations, and templates; unsupported content reported as Dropped\n");
    out.push_str("\nWritten as a SolidCraft design (.sc), or, experimental, as a FreeCAD rebuild macro (.FCMacro, --to freecad)\n");
    out
}

/// A compact Markdown table for documentation.
pub fn markdown() -> String {
    let mut out = String::from(
        "| Format | Files | Reads geometry | Level | Converts |\n|---|---|---|---|---|\n",
    );
    for format in Format::ALL {
        let name = format
            .name()
            .split_once(" (")
            .map_or(format.name(), |(name, _)| name);
        let files = format
            .extensions()
            .iter()
            .map(|extension| format!("`.{extension}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let converts = if format == Format::Iges {
            "Import it in SolidCraft directly"
        } else if format == Format::FreeCadDocument && format.codec_id().is_none() {
            "Enable the `freecad` feature"
        } else if !format.can_convert() {
            "Not supported yet"
        } else {
            match format {
                Format::FusionArchive | Format::InventorPart => {
                    "Geometry; feature history when every feature maps"
                }
                Format::FusionMultiDocument => "As Fusion; referenced designs merged",
                Format::InventorAssembly => "Placed parts and sub-assemblies",
                Format::FreeCadDocument => "Geometry; history and joints when they map",
                _ => "Geometry through STEP",
            }
        };
        out.push_str(&format!(
            "| {name} | {files} | {} | {} | {converts} |\n",
            if format.can_convert() { "Yes" } else { "No" },
            format.headline_level().unwrap_or_else(|| "—".to_owned()),
        ));
    }
    out
}

/// The JSON listing.
pub fn json() -> Value {
    let formats: Vec<_> = Format::ALL
        .into_iter()
        .map(|format| {
            json!({
                "id": format.id(), "name": format.name(), "extension": format.extension(),
                "extensions": format.extensions(),
                "level": format.headline_level(),
                "status": if format.can_convert() { "converts" } else { "not_supported" },
                "words": format.words(), "converts": converts(format),
            })
        })
        .collect();
    json!({"schema": SCHEMA, "ok": true, "formats": formats, "drawings": [{"input": Format::FreeCadDocument.id(), "name": "FreeCAD TechDraw", "output": "dxf", "converts": "page curves, dimension labels, annotations, and templates; unsupported content reported as Dropped"}]})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_lists_each_format_and_iges_guidance() {
        let table = markdown();
        assert!(table.starts_with("| Format | Files | Reads geometry | Level | Converts |\n"));
        assert_eq!(table.lines().count(), Format::ALL.len() + 2);
        for format in Format::ALL {
            let name = format.name().split_once(" (").unwrap().0;
            assert_eq!(
                table
                    .lines()
                    .filter(|line| line.starts_with(&format!("| {name} |")))
                    .count(),
                1
            );
        }
        let iges = table
            .lines()
            .find(|line| line.starts_with("| IGES |"))
            .unwrap();
        assert!(iges.ends_with("| Import it in SolidCraft directly |"));
        let freecad = table
            .lines()
            .find(|line| line.starts_with("| FreeCAD document |"))
            .unwrap();
        if cfg!(feature = "freecad") {
            assert!(freecad.contains("| Yes |"));
        } else {
            assert!(freecad.contains("| No |"));
            assert!(freecad.contains("Enable the `freecad` feature"));
        }
    }
}
