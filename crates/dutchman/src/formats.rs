//! `dutchman formats`: the native formats Dutchman knows and what converts from each.

use dutchman_core::Format;
use serde_json::{json, Value};

/// The schema of `dutchman formats --json`.
pub const SCHEMA: &str = "dutchman.formats/1";

/// Whether a format converts today.
#[derive(Clone, Copy)]
enum Status {
    Converts,
    Coming,
    NotSupported,
}

/// What Dutchman does with each format, in the order listed.
fn rows() -> [(Format, Status, &'static str); 5] {
    [
        (
            Format::FusionArchive,
            Status::Converts,
            "geometry; feature history (sketches, constraints, dimensions, extrudes, revolves) \
             when every feature maps; components when the file says which bodies they own",
        ),
        (
            Format::InventorPart,
            Status::Converts,
            "geometry; feature history waits on cadmpeg resolving extrude profiles",
        ),
        (
            Format::InventorAssembly,
            Status::Converts,
            "placed parts and sub-assemblies, read from the part files next to it",
        ),
        (Format::SolidWorksPart, Status::Coming, "geometry"),
        (
            Format::SolidWorksAssembly,
            Status::NotSupported,
            "nothing yet",
        ),
    ]
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
            let (_, status, _) = rows
                .iter()
                .find(|(f, _, _)| *f == format)
                .copied()
                .unwrap_or((format, Status::NotSupported, ""));
            assert_eq!(
                matches!(status, Status::Converts),
                format.codec_id().is_some(),
                "{format:?}"
            );
        }
    }
}
