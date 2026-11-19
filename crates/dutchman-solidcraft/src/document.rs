//! The design document, in one of two shapes:
//!
//! - an **import design**: what SolidCraft makes of a file it imports as STEP;
//! - a **history design**: the replayed history, then the original geometry as an import,
//!   suppressed, at the end of the timeline, unless the history already is that geometry.
//!
//! SolidCraft reads an import's assembly tree from the STEP itself. Imports receiving
//! explicit looks also name their output bodies so each look has a stable target.

use std::collections::HashSet;

use dutchman_core::{Model, Report, Severity};
use serde_json::{json, Value};

use crate::history::History;
use crate::{SolidCraftVersion, FORMAT};

/// The timeline name of the original geometry in a history design.
pub(crate) const ORIGINAL: &str = "Original geometry (imported)";

/// SolidCraft's longest component name.
const MAX_COMPONENT_NAME: usize = 128;

/// The ids SolidCraft gives the parts of a fresh import: its component, the component's
/// occurrence, and the Import feature.
const COMPONENT: u64 = 1;
const OCCURRENCE: u64 = 2;
pub(crate) const FEATURE: u64 = 3;

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

/// Applies explicit import body names and their definition-level looks.
pub(crate) fn import_looks(doc: &mut Value, looks: &History) {
    if let Some(names) = looks.import_body_names() {
        if let Some(fields) = doc
            .get_mut("features")
            .and_then(Value::as_array_mut)
            .and_then(|features| features.first_mut())
            .and_then(Value::as_object_mut)
        {
            fields.insert("body_names".to_owned(), names.clone());
        }
    }
    if let (false, Some(fields)) = (
        looks.looks.is_empty() && looks.face_looks.is_empty(),
        doc.as_object_mut(),
    ) {
        fields.insert(
            "appearances".to_owned(),
            json!({"bodies": looks.looks, "faces": looks.face_looks}),
        );
    }
}

/// Names and colours the visible final shape beside an editable history copy.
pub(crate) fn original_looks(doc: &mut Value, looks: &History) {
    if let Some(names) = looks.import_body_names() {
        if let Some(fields) = doc
            .get_mut("features")
            .and_then(Value::as_array_mut)
            .and_then(|features| {
                features
                    .iter_mut()
                    .find(|f| f.get("name").and_then(Value::as_str) == Some(ORIGINAL))
            })
            .and_then(Value::as_object_mut)
        {
            fields.insert("body_names".to_owned(), names.clone());
        }
    }
    if looks.looks.is_empty() && looks.face_looks.is_empty() {
        return;
    }
    let Some(fields) = doc.as_object_mut() else {
        return;
    };
    let appearances = fields.entry("appearances").or_insert_with(|| json!({}));
    let Some(appearances) = appearances.as_object_mut() else {
        return;
    };
    if let Some(bodies) = appearances
        .entry("bodies")
        .or_insert_with(|| json!({}))
        .as_object_mut()
    {
        bodies.extend(
            looks
                .looks
                .iter()
                .map(|(name, look)| (name.clone(), look.clone())),
        );
    }
    if let Some(faces) = appearances
        .entry("faces")
        .or_insert_with(|| json!([]))
        .as_array_mut()
    {
        faces.extend(looks.face_looks.iter().cloned());
    }
}

/// Appends mapped sketches to the import's component, hidden but editable. Parameters stay
/// available to their dimensions; none of the body-building history is retained.
pub(crate) fn keep_sketches(doc: &mut Value, references: &History) {
    if references.features.is_empty() {
        return;
    }
    let mut references = references.clone();
    references.relocate(FEATURE, COMPONENT, "", &mut Default::default());
    let hidden: Vec<Value> = references
        .features
        .iter()
        .filter_map(|feature| feature.get("id").cloned())
        .collect();
    if let Some(features) = doc.get_mut("features").and_then(Value::as_array_mut) {
        features.extend(references.features);
    }
    if let Some(fields) = doc.as_object_mut() {
        fields.insert("params".to_owned(), json!(references.params));
        fields.insert("next_id".to_owned(), json!(references.next_id));
        fields.insert("visibility".to_owned(), json!({"hidden_sketches": hidden}));
    }
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
    /// As `History`, for a history that already is the original geometry, which is left out.
    Whole,
    /// Editable history in a hidden component; the final original remains visible.
    Partial,
    /// As `Partial`, for a whole history that could not be checked against the original.
    Unverified,
    /// The original geometry, with the history suppressed: what the replay is checked against.
    Original,
}

/// The document of a design holding `params` and no geometry or features.
pub(crate) fn parameters_design(model: &Model, params: &[Value]) -> Value {
    json!({
        "format": FORMAT,
        "name": model.name,
        "units": "mm",
        "params": params,
        "next_id": 1,
        "features": [],
    })
}

/// A design holding the replayed `history`, then the original geometry (STEP at
/// `step_blob`) as an Import at the end of the timeline, with only the `live` side
/// unsuppressed.
pub(crate) fn history_design(
    model: &Model,
    history: &History,
    step_blob: &str,
    live: Live,
    target: SolidCraftVersion,
) -> Value {
    let mut history = history.clone();
    let component = history.next_id;
    let occurrence = component.saturating_add(1);
    let hidden = matches!(live, Live::Partial | Live::Unverified);
    if hidden {
        history.relocate(0, component, "", &mut Default::default());
        history.next_id = occurrence.saturating_add(1);
    }
    let mut features = history.features.clone();
    if live == Live::Original {
        for feature in &mut features {
            if let Some(fields) = feature.as_object_mut() {
                fields.insert("suppressed".to_owned(), Value::Bool(true));
            }
        }
    }
    let original = history.next_id;
    if live != Live::Whole {
        features.push(import_feature(
            model,
            original,
            ORIGINAL,
            None,
            live == Live::History,
            step_blob,
        ));
    }
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
    if let (true, Some(fields)) = (hidden, doc.as_object_mut()) {
        let part = if live == Live::Unverified {
            "Editable history (unverified)"
        } else {
            "Editable history (partial)"
        };
        fields.insert(
            "components".to_owned(),
            json!([editable_history_component(component, part, 0, target)]),
        );
        fields.insert("occurrences".to_owned(), json!([{"id": occurrence, "name": format!("{part}:1"), "component": component, "parent": 0, "grounded": true, "transform": IDENTITY}]));
        let sketches: Vec<Value> = history
            .features
            .iter()
            .filter(|f| f.get("type").and_then(Value::as_str) == Some("sketch"))
            .filter_map(|f| f.get("id").cloned())
            .collect();
        let mut visibility = serde_json::Map::new();
        visibility.insert("hidden_occurrences".to_owned(), json!([[occurrence]]));
        visibility.insert("hidden_sketches".to_owned(), json!(sketches));
        if !history.hidden_bodies.is_empty() {
            visibility.insert("hidden_bodies".to_owned(), json!(history.hidden_bodies));
        }
        fields.insert("visibility".to_owned(), Value::Object(visibility));
    }
    // A body the source hides stays hidden when the history rebuilds it.
    let rebuilt = matches!(live, Live::History | Live::Whole);
    if let (true, false, Some(fields)) = (
        rebuilt,
        history.hidden_bodies.is_empty(),
        doc.as_object_mut(),
    ) {
        fields.insert(
            "visibility".to_owned(),
            json!({"hidden_bodies": history.hidden_bodies}),
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

/// A hidden editable-history copy. Hiding its History bar entries leaves rebuilding and
/// Browser access intact; older targets keep their existing component fields.
pub(crate) fn editable_history_component(
    id: u64,
    name: &str,
    parent: u64,
    target: SolidCraftVersion,
) -> Value {
    let mut component = json!({"id": id, "name": name, "parent": parent});
    if target >= SolidCraftVersion::V0_30_0 {
        if let Some(fields) = component.as_object_mut() {
            fields.insert("history_hidden".to_owned(), Value::Bool(true));
        }
    }
    component
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

/// Attributes have no per-occurrence or evaluation-state destination in the writer.
pub(crate) fn report_scoped_attributes(model: &Model, report: &mut Report) {
    let mut seen = report
        .losses()
        .iter()
        .filter(|loss| loss.subject == "attributes")
        .map(|loss| loss.detail.clone())
        .collect();
    report_scoped_attributes_with_seen(model, report, &mut seen);
}

fn report_scoped_attributes_with_seen(
    model: &Model,
    report: &mut Report,
    seen: &mut HashSet<String>,
) {
    for attribute in &model.ir.model.attributes {
        let reason = if attribute.target.state.is_some() {
            "attributes for an evaluation state are not supported yet"
        } else if attribute.target.path.is_some() {
            "attributes for one placed instance are not supported yet"
        } else {
            continue;
        };
        let detail = format!("Attribute {} is left out: {reason}", attribute.id);
        if seen.insert(detail.clone()) {
            report.push(Severity::Dropped, "attributes", detail);
        }
    }
    if let Some(assembly) = &model.assembly {
        for part in &assembly.parts {
            report_scoped_attributes_with_seen(part, report, seen);
        }
    }
}

/// Records what an import design does not carry over from the model.
pub(crate) fn report_unmapped(model: &Model, references: Option<&History>, report: &mut Report) {
    let m = &model.ir.model;
    let sketches = if references.is_none() {
        m.sketches.len()
    } else {
        0
    };
    let parameters = if references.is_some_and(|history| !history.features.is_empty()) {
        0
    } else {
        m.parameters.len()
    };
    let flattened = [
        (m.features.len(), "feature history"),
        (sketches, "sketches"),
        (parameters, "parameters"),
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
        .geometry
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
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]

    use super::*;

    fn attribute_model(scope: Option<(&str, Value)>) -> Model {
        let mut target = json!({"entity": {"kind": "document"}});
        if let Some((key, value)) = scope {
            target[key] = value;
        }
        let mut ir = cadmpeg_ir::CadIr::empty();
        ir.model.attributes.push(
            serde_json::from_value(json!({
                "id": "synthetic:test:attribute#number", "target": target,
                "name": "Part number", "values": [{"kind": "string", "value": "A"}]
            }))
            .unwrap(),
        );
        Model {
            ir,
            fidelity: None,
            name: "Synthetic".into(),
            source_file: None,
            assembly: None,
            source_step: None,
        }
    }

    fn attribute_report(scope: Option<(&str, Value)>) -> Report {
        let model = attribute_model(scope);
        let mut report = Report::default();
        report_scoped_attributes(&model, &mut report);
        report_scoped_attributes(&model, &mut report);
        report
    }

    #[test]
    fn scoped_attributes_preserve_order_and_deduplicate_across_parts_and_passes() {
        let mut model = attribute_model(Some((
            "state",
            json!({"of": {"kind": "tool", "value": "synthetic:test:feature#earlier"}}),
        )));
        let attribute = model.ir.model.attributes[0].clone();
        for index in (0..1024).rev() {
            let mut distinct = serde_json::to_value(&attribute).unwrap();
            distinct["id"] = json!(format!("synthetic:test:attribute#{index}"));
            model
                .ir
                .model
                .attributes
                .push(serde_json::from_value(distinct).unwrap());
        }
        let part = model.clone();
        model.assembly = Some(dutchman_core::Assembly {
            own_part: None,
            parts: vec![part],
            definitions: vec![],
            identities: vec![],
            placements: vec![],
            joints: vec![],
            left_out: vec![],
            held: vec![],
            joint_notes: vec![],
            stored_placements: vec![],
        });
        let reason = "attributes for an evaluation state are not supported yet";
        let first = format!("Attribute {} is left out: {reason}", attribute.id);
        let mut report = Report::default();
        report.push(Severity::Degraded, "attributes", first.clone());
        let second = format!("Attribute synthetic:test:attribute#1023 is left out: {reason}");
        report.push(Severity::Dropped, "other", second.clone());
        report_scoped_attributes(&model, &mut report);
        let expected: Vec<_> = std::iter::once(first)
            .chain(std::iter::once(second))
            .chain((0..1024).rev().map(|index| {
                format!("Attribute synthetic:test:attribute#{index} is left out: {reason}")
            }))
            .collect();
        report_scoped_attributes(&model, &mut report);
        assert_eq!(
            report
                .losses()
                .iter()
                .map(|loss| &loss.detail)
                .collect::<Vec<_>>(),
            expected.iter().collect::<Vec<_>>()
        );
        assert_eq!(report.losses()[0].severity, Severity::Degraded);
    }

    #[test]
    fn definition_attributes_keep_the_existing_report_behavior() {
        assert!(attribute_report(None).losses().is_empty());
    }

    #[test]
    fn occurrence_attributes_are_left_out_with_one_loss() {
        let report = attribute_report(Some((
            "path",
            json!([
                {"occurrence": {"kind": "occurrence", "value": "synthetic:test:occurrence#one"}}
            ]),
        )));
        assert_eq!(report.losses().len(), 1);
        assert_eq!(report.losses()[0].severity, Severity::Dropped);
        assert!(report.losses()[0]
            .detail
            .ends_with("attributes for one placed instance are not supported yet"));
    }

    #[test]
    fn state_attributes_are_left_out_with_one_loss() {
        let report = attribute_report(Some((
            "state",
            json!({
                "of": {"kind": "tool", "value": "synthetic:test:feature#earlier"}
            }),
        )));
        assert_eq!(report.losses().len(), 1);
        assert!(report.losses()[0]
            .detail
            .ends_with("attributes for an evaluation state are not supported yet"));
    }

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
            default_bodies: Default::default(),
            stored_bodies: Default::default(),
            params: vec![json!({"name": "d1", "expr": "10 mm", "unit": "mm", "model": true})],
            features: vec![
                json!({"id": 1, "name": "Sketch1", "type": "sketch", "suppressed": false}),
                json!({"id": 2, "name": "Extrude1", "type": "extrude", "suppressed": false}),
            ],
            looks: Default::default(),
            face_looks: Vec::new(),
            sheet_rules: Vec::new(),
            next_id: 3,
            pattern_replay_required: false,
            mapped: 2,
            not_needed: 0,
            reapplied: 0,
            total: 2,
            blends: Vec::new(),
            hidden: Vec::new(),
            hidden_bodies: Vec::new(),
            unconfirmed: Default::default(),
            measured: Default::default(),
            stop: None,
            sources: Default::default(),
        }
    }

    #[test]
    fn bodies_the_source_hides_stay_hidden_when_the_history_rebuilds_them() {
        let model = crate::testing::cube();
        let mut hiding = history();
        hiding.hidden_bodies = vec!["Body 2".to_owned()];
        let version = SolidCraftVersion::V0_30_0;
        for live in [Live::History, Live::Whole, Live::Partial, Live::Unverified] {
            let doc = history_design(&model, &hiding, "blobs/x.step", live, version);
            assert_eq!(
                doc["visibility"]["hidden_bodies"],
                json!(["Body 2"]),
                "{live:?}"
            );
        }
        // Suppressed history builds nothing, so there is nothing to hide.
        let doc = history_design(&model, &hiding, "blobs/x.step", Live::Original, version);
        assert!(doc.get("visibility").is_none());
        // Nothing hidden writes no visibility for a live history.
        let doc = history_design(&model, &history(), "blobs/x.step", Live::History, version);
        assert!(doc.get("visibility").is_none());
    }

    #[test]
    fn only_hidden_copies_hide_history_on_new_targets() {
        let model = crate::testing::cube();
        for target in [
            "0.29.0", "0.30.0", "0.31.0", "0.40.0", "0.41.0", "0.42.0", "0.43.0", "0.44.0",
        ] {
            let version = target.parse().unwrap();
            for live in [
                Live::Partial,
                Live::Unverified,
                Live::History,
                Live::Whole,
                Live::Original,
            ] {
                let doc = history_design(&model, &history(), "blobs/x.step", live, version);
                assert_eq!(doc["format"], FORMAT);
                // The newest targets keep format 23 and the 0.30 representation.
                if matches!(target, "0.40.0" | "0.41.0" | "0.42.0" | "0.43.0" | "0.44.0") {
                    assert_eq!(
                        doc,
                        history_design(
                            &model,
                            &history(),
                            "blobs/x.step",
                            live,
                            SolidCraftVersion::V0_30_0,
                        )
                    );
                }
                if matches!(live, Live::Partial | Live::Unverified) {
                    let component = &doc["components"][0];
                    assert_eq!(
                        component.get("history_hidden"),
                        (version >= SolidCraftVersion::V0_30_0).then_some(&Value::Bool(true))
                    );
                    assert_eq!(doc["visibility"]["hidden_occurrences"], json!([[4]]));
                    assert!(doc["features"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .all(|f| f["suppressed"] == false));
                } else {
                    assert!(doc.get("components").is_none());
                }
            }
        }
    }

    #[test]
    fn the_original_follows_the_history_suppressed() {
        let model = crate::testing::cube();
        let doc = history_design(
            &model,
            &history(),
            "blobs/x.step",
            Live::History,
            SolidCraftVersion::default(),
        );
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
    fn a_whole_history_leaves_the_original_out() {
        let model = crate::testing::cube();
        let doc = history_design(
            &model,
            &history(),
            "",
            Live::Whole,
            SolidCraftVersion::default(),
        );
        let features = doc["features"].as_array().unwrap();
        assert_eq!(features.len(), 2);
        assert!(features.iter().all(|f| f["name"] != ORIGINAL));
    }

    #[test]
    fn the_check_design_has_only_the_original_live() {
        let model = crate::testing::cube();
        let doc = history_design(
            &model,
            &history(),
            "blobs/x.step",
            Live::Original,
            SolidCraftVersion::default(),
        );
        let features = doc["features"].as_array().unwrap();
        assert!(features.iter().take(2).all(|f| f["suppressed"] == true));
        assert_eq!(features.last().unwrap()["suppressed"], false);
    }
}
