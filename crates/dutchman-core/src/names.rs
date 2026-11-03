//! The names a conversion meets, recorded in its [`Report`] so an anonymous report can take
//! every one of them out of its messages: the paths and folders it reads from, and the names a
//! design gives its parts, components, bodies, features, sketches, parameters and properties.

use std::path::Path;

use serde::de::{DeserializeSeed, MapAccess, SeqAccess, Visitor};
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
    saw_serialized(&model.ir, report);
}

fn saw_serialized(value: &impl serde::Serialize, report: &mut Report) {
    // The old value tree expanded every coordinate and native payload just to visit
    // names. Walk compact JSON instead, retaining only names in the report.
    if let Ok(text) = serde_json::to_vec(value) {
        let mut reader = serde_json::Deserializer::from_slice(&text);
        if (Names {
            report,
            named: false,
        })
        .deserialize(&mut reader)
        .is_ok()
        {
            return;
        }
    }
    // Caller-built native records can exceed the text reader's nesting limit. Keep
    // the original behavior for those; any names already seen are the same set.
    if let Ok(document) = serde_json::to_value(value) {
        walk(&document, None, report);
    }
}

struct Names<'a> {
    report: &'a mut Report,
    named: bool,
}

impl<'de> DeserializeSeed<'de> for Names<'_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(self, reader: D) -> Result<(), D::Error> {
        reader.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Names<'_> {
    type Value = ();

    fn expecting(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        out.write_str("a serialized design")
    }

    fn visit_str<E: serde::de::Error>(self, text: &str) -> Result<(), E> {
        if self.named {
            saw_stored(text, self.report);
        }
        Ok(())
    }

    fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut rows: A) -> Result<(), A::Error> {
        while rows
            .next_element_seed(Names {
                report: self.report,
                named: self.named,
            })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut fields: A) -> Result<(), A::Error> {
        // Value's object walk is alphabetical. Keep that order at the report's
        // name limit too, while borrowing compact subtrees rather than expanding them.
        let mut ordered = std::collections::BTreeMap::new();
        while let Some((key, value)) =
            fields.next_entry::<String, &'de serde_json::value::RawValue>()?
        {
            ordered.insert(key, value);
        }
        for (key, value) in ordered {
            let mut reader = serde_json::Deserializer::from_str(value.get());
            Names {
                report: self.report,
                named: naming(&key),
            }
            .deserialize(&mut reader)
            .map_err(<A::Error as serde::de::Error>::custom)?;
        }
        Ok(())
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

    #[test]
    fn streaming_names_match_the_value_walk_in_arrays_escaped_keys_and_deep_records() {
        let mut deep = json!("Deep private name");
        for _ in 0..160 {
            deep = json!([deep]);
        }
        for document in [
            json!({
                "native": {"arbitrary": {"name": ["a\\b.f3d", {"id": "not a name", "title": "Bügel"}]}},
                "display_\"name": "Escaped \\\" name",
                "geometry": {"coordinates": [0, -1, 2.5, true, null], "source_name": "part"},
                "description": ["first", ["second"]]
            }),
            json!({"name": deep}),
        ] {
            let mut expected = Report::default();
            walk(&document, None, &mut expected);
            let mut actual = Report::default();
            saw_serialized(&document, &mut actual);
            assert_eq!(actual.names(), expected.names());
        }
    }

    #[test]
    fn streaming_names_keep_the_same_names_at_the_report_limit() {
        #[derive(serde::Serialize)]
        struct SourceOrder {
            z_name: &'static str,
            a_name: &'static str,
            b_name: &'static str,
        }
        let source = SourceOrder {
            z_name: "Zed",
            a_name: "Aye",
            b_name: "Bee",
        };
        let mut expected = Report::default();
        let mut actual = Report::default();
        // Leave two slots below Report's 100,000-name ceiling.
        for i in 0..99_998 {
            let name = format!("existing{i:06}");
            expected.saw(&name);
            actual.saw(&name);
        }
        walk(&serde_json::to_value(&source).unwrap(), None, &mut expected);
        saw_serialized(&source, &mut actual);
        assert_eq!(actual.names(), expected.names());
        assert!(actual.names().contains("Aye"));
        assert!(actual.names().contains("Bee"));
        assert!(!actual.names().contains("Zed"));
    }
}
