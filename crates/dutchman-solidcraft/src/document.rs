//! The design document: the shape SolidCraft gives a file it imports as STEP.
//!
//! The import's body names and assembly tree are left for SolidCraft to read from the STEP
//! itself, so SolidCraft's STEP reader stays the only interpreter of that text.

use dutchman_core::{Model, Report, Severity};
use serde_json::{json, Value};

use crate::FORMAT;

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
        "features": [{
            "id": FEATURE,
            "name": feature_name(model),
            "type": "import",
            "component": COMPONENT,
            "suppressed": false,
            "file": source_file(model),
            "step": "",
            "step_blob": step_blob,
        }],
    })
}

/// Records what the design does not carry over from the model.
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
fn component_name(design: &str) -> String {
    let name: String = design.trim().chars().take(MAX_COMPONENT_NAME).collect();
    if name.is_empty() {
        "Import".to_owned()
    } else {
        name
    }
}

/// The timeline name SolidCraft gives the import: the STEP product's name, which cadmpeg takes
/// from the first body, unless that is generic; else the design's name.
fn feature_name(model: &Model) -> String {
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
