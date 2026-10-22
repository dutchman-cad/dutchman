//! `dutchman formats`: the native formats Dutchman knows and what converts from each.

use dutchman_core::Format;
use serde_json::{json, Value};

/// The schema of `dutchman formats --json`.
pub const SCHEMA: &str = "dutchman.formats/1";

/// Whether a format converts today.
#[derive(Clone, Copy)]
enum Status {
    Converts,
    /// In progress; no format is at this stage right now.
    #[allow(dead_code)]
    Coming,
    NotSupported,
}

/// What Dutchman does with each format, in the order listed.
fn rows() -> [(Format, Status, String); 7] {
    let freecad = if Format::FreeCadDocument.codec_id().is_some() {
        (
            Status::Converts,
            "geometry and components; feature history (sketches, pads, simple holes) when every feature maps"
                .to_owned(),
        )
    } else {
        (
            Status::NotSupported,
            format!("in nightly builds for now: {}", dutchman_core::NIGHTLY),
        )
    };
    [
        (
            Format::FusionArchive,
            Status::Converts,
            "geometry; feature history (sketches, constraints, dimensions, extrudes, revolves, lofts, sweeps, spheres, tori, coils) \
             when every feature maps; components when the file says which bodies they own",
        ),
        (
            Format::FusionMultiDocument,
            Status::Converts,
            "as a Fusion archive, with the designs it references merged in by cadmpeg",
        ),
        (
            Format::InventorPart,
            Status::Converts,
            "geometry; feature history (sketches, constraints, extrudes) when every feature maps",
        ),
        (
            Format::InventorAssembly,
            Status::Converts,
            "placed parts and sub-assemblies, read from their files in the project",
        ),
        (
            Format::SolidWorksPart,
            Status::Converts,
            "geometry; feature history waits on cadmpeg binding sketches and extrude profiles",
        ),
        (
            Format::SolidWorksAssembly,
            Status::NotSupported,
            "nothing yet",
        ),
        (Format::FreeCadDocument, freecad.0, freecad.1.as_str()),
    ]
    .map(|(format, status, converts)| (format, status, converts.to_owned()))
}

fn status_word(status: Status) -> &'static str {
    match status {
        Status::Converts => "converts",
        Status::Coming => "coming",
        Status::NotSupported => "not_supported",
    }
}

/// The text listing.
pub fn text() -> String {
    let mut out = String::new();
    for (format, status, converts) in rows() {
        let status = match status {
            Status::Converts => "converts",
            Status::Coming => "coming",
            Status::NotSupported => "not yet",
        };
        out.push_str(&format!("{:<32} {status:<9} {converts}\n", format.name()));
    }
    out
}

/// The JSON listing.
pub fn json() -> Value {
    let formats: Vec<Value> = rows()
        .into_iter()
        .map(|(format, status, converts)| {
            json!({
                "id": format.id(),
                "name": format.name(),
                "extension": format.extension(),
                "status": status_word(status),
                "converts": converts,
            })
        })
        .collect();
    json!({"schema": SCHEMA, "ok": true, "formats": formats})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_format_is_listed_once_and_converts_only_with_a_decoder() {
        let rows = rows();
        assert_eq!(rows.len(), Format::ALL.len());
        for format in Format::ALL {
            let status = rows
                .iter()
                .find(|(f, _, _)| *f == format)
                .map_or(Status::NotSupported, |(_, status, _)| *status);
            assert_eq!(
                matches!(status, Status::Converts),
                format.codec_id().is_some(),
                "{format:?}"
            );
        }
    }
}
