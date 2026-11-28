//! The person's own defaults for the three dialog options, kept in SolidCraft's per-user settings
//! (`user-storage@0.1`). The importer stores the options of each accepted import, once as the
//! latest choice and once under the file's format (Fusion, Inventor, SolidWorks or FreeCAD); the
//! dialog hook offers back the choice made for the selected file's format, or the latest choice
//! for a format with none, as the form's defaults. Nothing else is stored: the
//! parts folder is a path the person picks each time, and a stored path would grant nothing.
use crate::{Options, UpAxis};
use serde_json::{json, Value};

/// The one settings key. Its value is a JSON object of the three options.
pub const KEY: &str = "options";

/// The format a file extension belongs to, as the settings key for that format.
pub fn format_key(extension: &str) -> Option<&'static str> {
    match extension.to_ascii_lowercase().as_str() {
        "f3d" | "f3z" => Some("fusion"),
        "ipt" | "iam" => Some("inventor"),
        "sldprt" | "sldasm" => Some("solidworks"),
        "fcstd" => Some("freecad"),
        _ => None,
    }
}

/// The three options as stored: never the parts folder.
fn options_value(options: &Options) -> Value {
    json!({
        "editable_history": options.editable_history,
        "up_axis": match options.up_axis {
            UpAxis::Auto => "auto",
            UpAxis::AsIs => "as_is",
            UpAxis::StandUp => "stand_up",
        },
        "include_hidden_bodies": options.include_hidden_bodies,
    })
}

/// The settings to store after an accepted import of a file with `extension`: the latest
/// options, and the options of that file's format beside the other formats' earlier ones in
/// `previous`. Damaged earlier settings are dropped.
pub fn encode(options: &Options, extension: &str, previous: Option<&str>) -> String {
    let mut stored = options_value(options);
    let mut formats = previous
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .and_then(|saved| saved.get("formats").and_then(Value::as_object).cloned())
        .unwrap_or_default();
    if let Some(key) = format_key(extension) {
        formats.insert(key.into(), options_value(options));
    }
    if let Some(object) = stored.as_object_mut() {
        object.insert("formats".into(), Value::Object(formats));
    }
    stored.to_string()
}

/// The dialog's field updates for stored settings and the selected file's extension: a
/// `default` for each option with a valid value, taken from the file's format when it has
/// saved choices and from the latest choice otherwise. Absent, damaged or unknown settings
/// change nothing, so the form keeps the manifest's defaults.
pub fn field_defaults(stored: Option<&str>, extension: &str) -> String {
    let saved: Value = stored
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or(Value::Null);
    let own = format_key(extension).and_then(|key| saved.get("formats")?.get(key));
    let mut fields = serde_json::Map::new();
    // Each option falls back separately, so one damaged value in a format's choices does not
    // hide the others.
    let pick = |name: &str, valid: &dyn Fn(&Value) -> bool| {
        [own, Some(&saved)]
            .into_iter()
            .flatten()
            .filter_map(|source| source.get(name))
            .find(|value| valid(value))
            .cloned()
    };
    for name in ["editable_history", "include_hidden_bodies"] {
        if let Some(value) = pick(name, &|value| value.is_boolean()) {
            fields.insert(name.into(), json!({ "default": value }));
        }
    }
    if let Some(value) = pick("up_axis", &|value| {
        matches!(value.as_str(), Some("auto" | "as_is" | "stand_up"))
    }) {
        fields.insert("up_axis".into(), json!({ "default": value }));
    }
    json!({ "fields": fields }).to_string()
}
