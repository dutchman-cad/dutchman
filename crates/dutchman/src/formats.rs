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

/// The listing as a Markdown table, for the README and docs/formats.md.
pub fn markdown() -> String {
    let mut out =
        String::from("| Format | Files | Reads | Level | Converts |\n|---|---|---|---|---|\n");
    for format in Format::ALL {
        let (name, files) = format
            .name()
            .split_once(" (")
            .map(|(name, files)| (name, files.trim_end_matches(')')))
            .unwrap_or((format.name(), ""));
        let name = if format == Format::UnclassifiedPart {
            "Unidentified part (Creo or NX)"
        } else {
            name
        };
        let files = files
            .split(", ")
            .filter(|file| !file.is_empty())
            .map(|file| format!("`{file}`"))
            .collect::<Vec<_>>()
            .join(", ");
        let reads = if format == Format::Iges {
            "Recognised, not converted".to_owned()
        } else {
            capitalised(format.words())
        };
        let level = match format {
            Format::Iges => None,
            _ => format.headline_level(),
        };
        let converts = if format == Format::Iges {
            "Import it in SolidCraft directly (File > Import)".to_owned()
        } else if format.can_convert() || format == Format::FreeCadDocument {
            capitalised(converts(format))
        } else {
            "Not yet".to_owned()
        };
        out.push_str(&format!(
            "| {name} | {files} | {reads} | {} | {converts} |\n",
            level.unwrap_or_else(|| "–".to_owned()),
        ));
    }
    out.push_str("\nFreeCAD TechDraw drawings also convert to DXF (`--to dxf`): page curves, dimension labels, annotations, and templates, with unsupported content reported as Dropped.\n");
    out.push_str("\nOutput is a SolidCraft design (`.sc`), or, experimental, a FreeCAD rebuild macro (`.FCMacro`, `--to freecad`).\n");
    out
}

fn capitalised(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
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
