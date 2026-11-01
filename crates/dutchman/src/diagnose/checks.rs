//! Conservative advisory checks on stored assembly placements.
use dutchman_core::Model;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Check finite transforms and repeated instances of the same definition at the same place.
/// Different definitions with similar boxes are deliberately not treated as duplicates.
pub(super) fn placements(model: &Model) -> Value {
    let mut findings = Vec::new();
    let mut next = 1;
    walk(model, None, &mut next, &mut findings);
    json!({"status": "checked", "advisory": true,
        "coincident_transform_tolerance": 1e-9,
        "bounds": "unavailable: no independent assembly envelope in the decoded model",
        "findings": findings})
}

fn walk(model: &Model, number: Option<usize>, next: &mut usize, findings: &mut Vec<Value>) {
    for (index, body) in model.ir.model.bodies.iter().enumerate() {
        if body
            .transform
            .is_some_and(|t| t.affine_rows().iter().flatten().any(|v| !v.is_finite()))
        {
            findings.push(json!({"part": number.unwrap_or(1), "body": index+1, "code": "placement_nonfinite"}));
        }
    }
    let Some(assembly) = &model.assembly else {
        return;
    };
    let mut numbers = Vec::new();
    for part in &assembly.parts {
        let part_number = *next;
        *next = next.saturating_add(1);
        numbers.push(part_number);
        walk(part, Some(part_number), next, findings);
    }
    let mut seen = BTreeMap::new();
    for (index, placement) in assembly.placements.iter().enumerate() {
        let part = numbers.get(placement.part).copied();
        if placement.transform.iter().flatten().any(|v| !v.is_finite()) {
            findings
                .push(json!({"part": part, "placement": index+1, "code": "placement_nonfinite"}));
            continue;
        }
        // Formatting avoids integer overflow for very large but finite coordinates.
        let key: Vec<String> = placement
            .transform
            .iter()
            .flatten()
            .map(|v| format!("{v:.9}"))
            .collect();
        if let Some(first) = seen.insert((placement.part, key), index + 1) {
            findings.push(
                json!({"part": part, "placement": index+1, "other_placement": first,
                "code": "placement_coincident"}),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_coincident_instances_of_the_same_definition_are_flagged() {
        let mut model = dutchman_core::read_in(
            dutchman_core::Format::FusionArchive,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d"),
            &dutchman_core::PartSearch::default(),
            &mut dutchman_core::Report::default(),
        )
        .unwrap();
        let part = model.clone();
        let placement = dutchman_core::Placement {
            name: Some("private".into()),
            part: 0,
            transform: dutchman_core::placement::IDENTITY,
            hidden: false,
        };
        let mut shifted = placement.clone();
        shifted.transform[0][3] = 100.0;
        let mut nonfinite = placement.clone();
        nonfinite.transform[0][3] = f64::NAN;
        model.assembly = Some(dutchman_core::Assembly {
            own_part: None,
            parts: vec![part],
            definitions: vec![],
            identities: vec![],
            placements: vec![placement.clone(), placement, shifted, nonfinite],
            joints: vec![],
            left_out: vec![],
            held: vec![],
            joint_notes: vec![],
            stored_placements: vec![],
        });
        let report = placements(&model);
        assert_eq!(report["findings"].as_array().unwrap().len(), 2);
        assert!(!report.to_string().contains("private"));
    }
}
