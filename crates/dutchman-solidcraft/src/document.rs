//! The design document, in one of two shapes:
//!
//! - an **import design**: what SolidCraft makes of a file it imports as STEP;
//! - a **history design**: the replayed history, then the original geometry as an import,
//!   suppressed, at the end of the timeline.
//!
//! An import's body names and assembly tree are left for SolidCraft to read from the STEP
//! itself, so SolidCraft's STEP reader stays the only interpreter of that text.

use dutchman_core::{Model, Report, Severity};
use serde_json::{json, Value};

use crate::history::History;
use crate::FORMAT;

/// The timeline name of the original geometry in a history design.
pub(crate) const ORIGINAL: &str = "Original geometry (imported)";

/// SolidCraft's longest component name.
const MAX_COMPONENT_NAME: usize = 128;

/// The ids SolidCraft gives the parts of a fresh import: its component, the component's
/// occurrence, and the Import feature.
const COMPONENT: u64 = 1;
const OCCURRENCE: u64 = 2;
const FEATURE: u64 = 3;

/// The identity placement, column-major.
const IDENTITY: [[f64; 4]; 4] = [
    [1.0, 0.0, 0.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// A design holding the model as one Import feature in its own component, the STEP text kept
/// in the container at `step_blob`.
pub(crate) fn import_design(model: &Model, step_blob: &str) -> Value {
    let component = component_name(&model.name);
    json!({
        "format": FORMAT,
        "name": model.name,
        "units": "mm",
        "params": [],
        "next_id": FEATURE + 1,
        "components": [{ "id": COMPONENT, "name": component, "parent": 0 }],
        "occurrences": [{
            "id": OCCURRENCE,
            "name": format!("{component}:1"),
            "component": COMPONENT,
            "parent": 0,
            "grounded": false,
            "transform": IDENTITY,
        }],
        "features": [import_feature(model, FEATURE, &feature_name(model), Some(COMPONENT), false, step_blob)],
    })
}

/// A row-major 3 × 4 rigid transform as SolidCraft's column-major 4 × 4.
pub(crate) fn column_major(rows: &[[f64; 4]; 3]) -> [[f64; 4]; 4] {
    let mut out = IDENTITY;
    for (column, out_column) in out.iter_mut().enumerate() {
        for (row, values) in rows.iter().enumerate() {
            if let (Some(cell), Some(value)) = (out_column.get_mut(row), values.get(column)) {
                *cell = *value;
            }
        }
    }
    out
}

/// Which side of a history design is live.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Live {
    /// The replayed history, as the design is written.
    History,
    /// The original geometry, with the history suppressed: what the replay is checked against.
    Original,
}

/// A design holding the replayed `history`, then the original geometry (STEP at
/// `step_blob`) as an Import at the end of the timeline, with only the `live` side
/// unsuppressed.
pub(crate) fn history_design(
    model: &Model,
    history: &History,
    step_blob: &str,
    live: Live,
) -> Value {
    let mut features = history.features.clone();
    if live == Live::Original {
        for feature in &mut features {
            if let Some(fields) = feature.as_object_mut() {
                fields.insert("suppressed".to_owned(), Value::Bool(true));
            }
        }
    }
    let original = history.next_id;
    features.push(import_feature(
        model,
        original,
        ORIGINAL,
        None,
        live == Live::History,
        step_blob,
    ));
    let mut doc = json!({
        "format": FORMAT,
        "name": model.name,
        "units": "mm",
        "params": history.params,
        "next_id": original.saturating_add(1),
        "features": features,
    });
    if let (Some(first), Some(fields)) = (history.sheet_rules.first(), doc.as_object_mut()) {
        fields.insert(
            "sheet".to_owned(),
            json!({"active": first.get("name"), "rules": history.sheet_rules}),
        );
    }
    let has_looks = !history.looks.is_empty() || !history.face_looks.is_empty();
    if let (true, Some(fields)) = (has_looks, doc.as_object_mut()) {
        let mut appearances = json!({"bodies": history.looks});
        if let (false, Some(look_fields)) =
            (history.face_looks.is_empty(), appearances.as_object_mut())
        {
            look_fields.insert("faces".to_owned(), json!(history.face_looks));
        }
        fields.insert("appearances".to_owned(), appearances);
    }
    doc
}

/// An Import feature holding the model's STEP restatement, in `component` (the root when
/// `None`).
pub(crate) fn import_feature(
    model: &Model,
    id: u64,
    name: &str,
    component: Option<u64>,
    suppressed: bool,
    step_blob: &str,
) -> Value {
    let mut feature = json!({
        "id": id,
        "name": name,
        "type": "import",
        "suppressed": suppressed,
        "file": source_file(model),
        "step": "",
        "step_blob": step_blob,
    });
    if let (Some(component), Some(fields)) = (component, feature.as_object_mut()) {
        fields.insert("component".to_owned(), json!(component));
    }
    feature
}

/// Records what an import design does not carry over from the model.
pub(crate) fn report_unmapped(model: &Model, report: &mut Report) {
    let m = &model.ir.model;
    let flattened = [
        (m.features.len(), "feature history"),
        (m.sketches.len(), "sketches"),
        (m.parameters.len(), "parameters"),
    ];
    for (count, what) in flattened {
        if count > 0 {
            report.push(
                Severity::Degraded,
                what,
                format!("{count} carried as a non-parametric body, not as editable history"),
            );
        }
    }
}

/// The component's name: the design's, as SolidCraft names a component after the file it
/// imports.
pub(crate) fn component_name(design: &str) -> String {
    let name: String = design.trim().chars().take(MAX_COMPONENT_NAME).collect();
    if name.is_empty() {
        "Import".to_owned()
    } else {
        name
    }
}

/// The timeline name SolidCraft gives the import: the STEP product's name, which cadmpeg takes
/// from the first body, unless that is generic; else the design's name.
pub(crate) fn feature_name(model: &Model) -> String {
    model
        .ir
        .model
        .bodies
        .first()
        .and_then(|body| body.name.as_deref())
        .filter(|name| !generic_name(name))
        .map_or_else(|| model.name.clone(), str::to_owned)
}

/// A name SolidCraft treats as no name at all.
fn generic_name(name: &str) -> bool {
    name.trim().is_empty()
        || name.starts_with('(')
        || name.eq_ignore_ascii_case("import")
        || name.eq_ignore_ascii_case("unnamed")
}

/// The file the import came from, as the Import feature records it.
fn source_file(model: &Model) -> String {
    model
        .source_file
        .clone()
        .unwrap_or_else(|| format!("{}.step", model.name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn component_names_are_trimmed_and_capped() {
        assert_eq!(component_name("  part  "), "part");
        assert_eq!(component_name(""), "Import");
        assert_eq!(component_name(&"x".repeat(300)).len(), MAX_COMPONENT_NAME);
    }

    #[test]
    fn generic_names_match_solidcraft() {
        for name in ["", "  ", "(unnamed)", "Import", "UNNAMED"] {
            assert!(generic_name(name), "{name:?}");
        }
        assert!(!generic_name("Bracket"));
    }
}

#[cfg(test)]
mod history_tests {
    use super::*;

    fn history() -> History {
        History {
            params: vec![json!({"name": "d1", "expr": "10 mm", "unit": "mm", "model": true})],
            features: vec![
                json!({"id": 1, "name": "Sketch1", "type": "sketch", "suppressed": false}),
                json!({"id": 2, "name": "Extrude1", "type": "extrude", "suppressed": false}),
            ],
            looks: Default::default(),
            face_looks: Vec::new(),
            sheet_rules: Vec::new(),
            next_id: 3,
            mapped: 2,
            total: 2,
        }
    }

    #[test]
    fn the_original_follows_the_history_suppressed() {
        let model = crate::testing::cube();
        let doc = history_design(&model, &history(), "blobs/x.step", Live::History);
        let features = doc["features"].as_array().unwrap();
        assert_eq!(features.len(), 3);
        let original = features.last().unwrap();
        assert_eq!(original["id"], 3);
        assert_eq!(original["name"], ORIGINAL);
        assert_eq!(original["type"], "import");
        assert_eq!(original["suppressed"], true);
        assert_eq!(original["step_blob"], "blobs/x.step");
        assert!(original.get("component").is_none(), "at the root");
        assert!(features.iter().take(2).all(|f| f["suppressed"] == false));
        assert_eq!(doc["next_id"], 4);
        assert_eq!(doc["params"], json!(history().params));
        assert!(doc.get("components").is_none());
    }

    #[test]
    fn the_check_design_has_only_the_original_live() {
        let model = crate::testing::cube();
        let doc = history_design(&model, &history(), "blobs/x.step", Live::Original);
        let features = doc["features"].as_array().unwrap();
        assert!(features.iter().take(2).all(|f| f["suppressed"] == true));
        assert_eq!(features.last().unwrap()["suppressed"], false);
    }
}
