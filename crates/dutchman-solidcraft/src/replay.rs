//! The command script that rebuilds a written history step by step in `solidcraft-cli run`.

use std::io::Read;

use serde_json::{json, Value};

/// The largest `document.json` read from a design.
const DOCUMENT_LIMIT: u64 = 256 << 20;

/// The commands that roll the timeline to just after feature `id` and inspect the bodies there.
pub(crate) fn roll_commands(id: u64) -> [Value; 2] {
    [
        json!({"command": "timeline.roll_to", "params": {"feature": id}}),
        json!({"command": "document.inspect", "params": {}}),
    ]
}

/// The script Dutchman gives `solidcraft-cli run` to rebuild the history of the design in
/// `design` (a `.sc` archive): for every feature, roll the timeline to just after it and
/// inspect the bodies. Run it with `solidcraft-cli run <script> --in <design>`.
pub fn replay_script(design: &[u8]) -> Result<Vec<Value>, String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(design))
        .map_err(|err| format!("not a SolidCraft design archive: {err}"))?;
    let file = archive
        .by_name("document.json")
        .map_err(|err| format!("the design has no document.json: {err}"))?;
    let mut text = String::new();
    file.take(DOCUMENT_LIMIT)
        .read_to_string(&mut text)
        .map_err(|err| format!("cannot read document.json: {err}"))?;
    let document: Value =
        serde_json::from_str(&text).map_err(|err| format!("document.json: {err}"))?;
    let features = document
        .get("features")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    Ok(features
        .iter()
        .filter_map(|feature| feature.get("id").and_then(Value::as_u64))
        .flat_map(roll_commands)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn archive(document: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("document.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(document.as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn rolls_to_every_feature() {
        let design = archive(r#"{"features":[{"id":3},{"id":7},{"name":"no id"}]}"#);
        let script = replay_script(&design).unwrap();
        assert_eq!(script.len(), 4);
        assert_eq!(script[2]["params"]["feature"], 7);
        assert_eq!(script[3]["command"], "document.inspect");
    }

    #[test]
    fn refuses_what_is_not_a_design() {
        assert!(replay_script(b"nope").is_err());
    }
}
