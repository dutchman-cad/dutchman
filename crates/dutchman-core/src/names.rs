//! The names a conversion meets, recorded in its [`Report`] so an anonymous report can take
//! every one of them out of its messages: the paths and folders it reads from, and the names a
//! design gives its parts, components, bodies, features, sketches, parameters and properties.

use std::path::Path;

use serde_json::Value;

use crate::{Model, Report};

/// Keys whose string values name something in a design, or hold text its author wrote.
const NAMING: [&str; 15] = [
    "name",
    "label",
    "title",
    "author",
    "company",
    "description",
    "comment",
    "path",
    "file",
    "stored",
    "project",
    "designer",
    "engineer",
    "vendor",
    "number",
];

/// Records `path`, and each of its folders and its file name, as names the conversion met.
pub(crate) fn saw_path(path: &Path, report: &mut Report) {
    report.saw(&path.display().to_string());
    if let Ok(full) = std::fs::canonicalize(path) {
        report.saw(&full.display().to_string());
    }
    saw_stored(&path.display().to_string(), report);
}

/// Records a path as a file stores it, whichever separators the saving machine used: whole,
/// and each of its parts.
pub(crate) fn saw_stored(stored: &str, report: &mut Report) {
    report.saw(stored);
    for part in stored.split(['/', '\\']) {
        report.saw(part);
        if let Some((stem, _)) = part.rsplit_once('.') {
            report.saw(stem);
        }
    }
}

/// Records every name `model` gives anything, and its own name and file's.
pub(crate) fn saw_model(model: &Model, report: &mut Report) {
    report.saw(&model.name);
    if let Some(file) = &model.source_file {
        saw_stored(file, report);
    }
    if let Ok(document) = serde_json::to_value(&model.ir) {
        walk(&document, None, report);
    }
}

fn walk(value: &Value, key: Option<&str>, report: &mut Report) {
    match value {
        Value::String(text) => {
            if key.is_some_and(naming) {
                saw_stored(text, report);
            }
        }
        Value::Array(values) => values.iter().for_each(|v| walk(v, key, report)),
        Value::Object(fields) => fields
            .iter()
            .for_each(|(k, v)| walk(v, Some(k.as_str()), report)),
        _ => {}
    }
}

/// Whether a key's value names something: `name`, `display_name`, `part_number`, ….
fn naming(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    NAMING.iter().any(|word| key.contains(word))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn names_and_paths_anywhere_in_a_design_are_recorded() {
        let mut report = Report::default();
        let design = json!({"model": {
            "features": [{"id": "x#1", "name": "Bügel Schraube 3"}],
            "product_definitions": [{"label": "Client Housing", "kind": "part"}],
            "properties": [{"key": "PartNumber", "part_number": "ACME-0042"}],
            "occurrences": [{"stored": "C:\\Work\\Secret Project\\pin.ipt"}],
            "edges": [{"id": "e#1", "kind": "line"}],
        }});
        walk(&design, None, &mut report);
        let names = report.names();
        for name in [
            "Bügel Schraube 3",
            "Client Housing",
            "ACME-0042",
            "C:\\Work\\Secret Project\\pin.ipt",
            "Secret Project",
            "pin.ipt",
            "pin",
        ] {
            assert!(names.contains(name), "{name}");
        }
        assert!(!names.contains("line"), "kinds are not names");
        assert!(!names.contains("e#1"), "ids are not names");
    }

    #[test]
    fn a_path_is_recorded_whole_and_by_its_folders() {
        let mut report = Report::default();
        saw_path(
            Path::new("/srv/Someone's Projects/Rover Mk2/arm.ipt"),
            &mut report,
        );
        for name in ["Someone's Projects", "Rover Mk2", "arm.ipt", "arm", "srv"] {
            assert!(report.names().contains(name), "{name}");
        }
    }
}
