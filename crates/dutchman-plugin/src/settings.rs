//! The person's own defaults for the three dialog options, kept in SolidCraft's per-user settings
//! (`user-storage@0.1`). The importer stores the options of each accepted import; the dialog
//! hook offers them back as the form's defaults for the next file. Nothing else is stored: the
//! parts folder is a path the person picks each time, and a stored path would grant nothing.
use crate::{Options, UpAxis};
use serde_json::{json, Value};

/// The one settings key. Its value is a JSON object of the three options.
pub const KEY: &str = "options";

/// The options as stored: only the three the dialog keeps, never the parts folder.
pub fn encode(options: &Options) -> String {
    json!({
        "editable_history": options.editable_history,
        "up_axis": match options.up_axis {
            UpAxis::Auto => "auto",
            UpAxis::AsIs => "as_is",
            UpAxis::StandUp => "stand_up",
        },
        "include_hidden_bodies": options.include_hidden_bodies,
    })
    .to_string()
}

/// The dialog's field updates for stored settings: a `default` for each option the settings hold
/// with a valid value. Absent, damaged or unknown settings change nothing, so the form keeps
/// the manifest's defaults.
pub fn field_defaults(stored: Option<&str>) -> String {
    let saved: Value = stored
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or(Value::Null);
    let mut fields = serde_json::Map::new();
    for name in ["editable_history", "include_hidden_bodies"] {
        if let Some(value) = saved.get(name).filter(|value| value.is_boolean()) {
            fields.insert(name.into(), json!({ "default": value }));
        }
    }
    if let Some(value) = saved
        .get("up_axis")
        .filter(|value| matches!(value.as_str(), Some("auto" | "as_is" | "stand_up")))
    {
        fields.insert("up_axis".into(), json!({ "default": value }));
    }
    json!({ "fields": fields }).to_string()
}
