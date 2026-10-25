//! The formats this build knows and what it converts from each.

use dutchman_core::Format;
use serde_json::{json, Value};

pub const SCHEMA: &str = "dutchman.formats/1";

/// What Dutchman can carry from this format.
pub fn converts(format: Format) -> &'static str {
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
            "{}  {}  {status}: {}\n",
            format.name(),
            format.words(),
            converts(format)
        ));
    }
    out.push_str("\nWritten as a SolidCraft design (.solidcraft), or, experimental, as a FreeCAD rebuild macro (.FCMacro, --to freecad)\n");
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
                "status": if format.can_convert() { "converts" } else { "not_supported" },
                "words": format.words(), "converts": converts(format),
            })
        })
        .collect();
    json!({"schema": SCHEMA, "ok": true, "formats": formats})
}
