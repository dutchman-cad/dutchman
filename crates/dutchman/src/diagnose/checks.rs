//! Conservative advisory checks on stored assembly placements and face boundaries.
use dutchman_core::Model;
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Check transforms, repeated instances of one definition, and missing torus trims.
/// Different definitions with similar boxes are deliberately not treated as duplicates.
pub(super) fn placements(model: &Model, joints: bool) -> Value {
    let mut findings = Vec::new();
    let mut next = 1;
    walk(model, None, joints, &mut next, &mut findings);
    json!({"status": "checked", "advisory": true,
        "coincident_transform_tolerance": 1e-9,
        "bounds": "unavailable: no independent assembly envelope in the decoded model",
        "findings": findings})
}

fn walk(
    model: &Model,
    number: Option<usize>,
    joints: bool,
    next: &mut usize,
    findings: &mut Vec<Value>,
) {
    // Assembly geometry is checked through its extracted parts, including own_part.
    // The container's IR can also retain those faces; checking it would duplicate
    // findings and assign the root's faces to an unrelated part number.
    if model.assembly.is_none() {
        for mut finding in dutchman_solidcraft::compare::topology(model) {
            if let Some(fields) = finding.as_object_mut() {
                fields.insert("part".into(), json!(number.unwrap_or(1)));
            }
            findings.push(finding);
        }
    }
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
        walk(part, Some(part_number), joints, next, findings);
    }
    let mut seen = BTreeMap::new();
    for (index, placement) in assembly.placements.iter().enumerate() {
        let part = numbers.get(placement.part).copied();
        let transform = if joints {
            &placement.transform
        } else {
            assembly
                .stored_placements
                .get(index)
                .unwrap_or(&placement.transform)
        };
        if transform.iter().flatten().any(|v| !v.is_finite()) {
            findings
                .push(json!({"part": part, "placement": index+1, "code": "placement_nonfinite"}));
            continue;
        }
        // Formatting avoids integer overflow for very large but finite coordinates.
        let key: Vec<String> = transform
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
        let report = placements(&model, true);
        assert_eq!(report["findings"].as_array().unwrap().len(), 2);
        assert!(!report.to_string().contains("private"));

        // Omitting joints uses the stored pose, including for sanity checks.
        model.assembly.as_mut().unwrap().stored_placements = [0.0, 1.0, 2.0, 3.0]
            .into_iter()
            .map(|x| {
                let mut transform = dutchman_core::placement::IDENTITY;
                transform[0][3] = x;
                transform
            })
            .collect();
        assert_eq!(placements(&model, false)["findings"], json!([]));
    }

    #[test]
    fn missing_torus_trims_are_numbered_once_in_the_owning_part() {
        let mut part = dutchman_core::read_in(
            dutchman_core::Format::FusionArchive,
            &std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d"),
            &dutchman_core::PartSearch::default(),
            &mut dutchman_core::Report::default(),
        )
        .unwrap();
        let face = part.ir.model.faces.first_mut().unwrap();
        let surface_id = face.surface.clone();
        face.loops =
            serde_json::from_value(json!({"classification": "unspecified", "loops": []})).unwrap();
        part.ir
            .model
            .surfaces
            .iter_mut()
            .find(|s| s.id == surface_id)
            .unwrap()
            .geometry = serde_json::from_value(json!({"kind": "torus",
                "center": {"x": 0, "y": 0, "z": 0}, "axis": {"x": 0, "y": 0, "z": 1},
                "ref_direction": {"x": 1, "y": 0, "z": 0}, "major_radius": 5, "minor_radius": 1}))
        .unwrap();
        let mut container = part.clone();
        container.assembly = Some(dutchman_core::Assembly {
            own_part: Some(0),
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
        assert_eq!(
            placements(&container, true)["findings"],
            json!([
                {"part": 1, "body": 1, "face": 1, "code": "geometry_untrimmed_torus"}
            ])
        );
    }
}
