//! Every number Dutchman's checks use to decide that a rebuilt history matches its source.
//!
//! Each lives here once; the checks in `verify.rs` and `history/` read it from here, and
//! [`report`] states them in the conversion report, so what is written for a user is what
//! the checks use.

use serde_json::{json, Value};

/// The relative difference in body volume at which a replay no longer matches the original,
/// when SolidCraft gives no area and box to bound its meshes' error by.
pub(crate) const VOLUME_RELATIVE: f64 = 1e-4;

/// SolidCraft measures a body from a mesh whose chords keep within this fraction of the body's
/// box diagonal from its faces.
pub(crate) const MESH_CHORD_RELATIVE: f64 = 5e-5;

/// The closest, in millimetres, SolidCraft's measuring mesh keeps to a body's faces.
pub(crate) const MESH_CHORD_FLOOR_MM: f64 = 1e-4;

/// The least a body's centre or box corner may move, in millimetres, between the replay and
/// the original.
pub(crate) const POSITION_MIN_MM: f64 = 0.01;

/// The share of the larger box diagonal a body's centre or box corner may move: four mesh
/// chords.
pub(crate) const POSITION_DIAGONAL_RELATIVE: f64 = 4.0 * MESH_CHORD_RELATIVE;

/// The relative difference in body area at which a replay no longer matches the original.
pub(crate) const AREA_RELATIVE: f64 = 1e-3;

/// Named parameter replies rounded 10/3 mm to 3.33333333333 (1e-12 relative)
/// and 1 rad to 57.2957795131 degrees (3.1e-13 relative). Five times the
/// larger observed error permits serialization roundoff, scaled by max(1, |source|).
pub(crate) const PARAMETER_RELATIVE: f64 = 5e-12;

/// A body the source stores with its own exact box: the least its centre may differ from a
/// replayed body's, in millimetres, and the share of the replayed box diagonal.
pub(crate) const STORED_EXACT: (f64, f64) = (0.5, 0.01);

/// A body the source stores with a box over sampled points: the same two numbers.
pub(crate) const STORED_SAMPLED: (f64, f64) = (1.0, 0.02);

/// A body volume, in cubic millimetres, below which SolidCraft gives the body none.
pub(crate) const NO_VOLUME_MM3: f64 = 1e-6;

/// Fillet and chamfer input-edge proof: how far apart two positions may be, in millimetres.
pub(crate) const EDGE_POSITION_MM: f64 = 1e-6;

/// Fillet and chamfer input-edge proof: how far from square or parallel two unit directions
/// may be.
pub(crate) const EDGE_DIRECTION: f64 = 1e-6;

/// Fillet and chamfer input-edge proof: how far, in millimetres, the nearest rebuilt edge must
/// be for a refusal to blame the rebuilt body rather than the position tolerance.
pub(crate) const EDGE_BLAME_MM: f64 = 0.01;

/// Profile face proof: how far apart two positions may be, in millimetres.
pub(crate) const FACE_POSITION_MM: f64 = 1e-5;

/// Profile face proof: how far from square or parallel two unit directions may be.
pub(crate) const FACE_DIRECTION: f64 = 1e-5;

/// Profile face proof: the relative difference in area of a face with straight edges only.
pub(crate) const FACE_AREA_STRAIGHT: f64 = 1e-4;

/// Profile face proof: the relative difference in area of a face with a curved edge, which
/// SolidCraft measures from a mesh a little under the exact area.
pub(crate) const FACE_AREA_CURVED: f64 = 0.02;

/// Profile face proof: how far, in millimetres, a rebuilt face's plane may be from the
/// sketch plane.
pub(crate) const FACE_PLANE_MM: f64 = 1e-4;

/// Profile face proof: the share of a straight edge's length (at least 1 mm) its rebuilt
/// pieces may leave uncovered or overlap.
pub(crate) const FACE_LINE_COVER: f64 = 1e-4;

/// Profile face proof: the share of a curved edge's radius (at least 1 mm) its rebuilt pieces
/// may lie off the circle.
pub(crate) const FACE_ARC_RADIUS: f64 = 1e-3;

/// Profile face proof: how far apart, in radians, the turn a curved edge's rebuilt pieces
/// span may be from its own.
pub(crate) const FACE_TURN_RAD: f64 = 2e-3;

/// Extrude end-cap proof: the least, in square millimetres, and the share of the source's cap
/// area that the swept profile's area may differ by.
pub(crate) const CAP_AREA: (f64, f64) = (1.0, 0.03);

/// Extrude end-cap proof: how far, in millimetres, a face's plane may be from the end plane.
pub(crate) const CAP_PLANE_MM: f64 = 1e-4;

/// The tolerances a history check uses, as report fields. Every number is a millimetre
/// distance, a unitless share (a fraction of the size it applies to) or a radian angle, as
/// its name says.
pub fn report() -> Value {
    json!({
        "version": 1,
        "replay": {
            "bodies": "the replayed and original design hold the same number of bodies",
            "volume_relative": VOLUME_RELATIVE,
            "area_relative": AREA_RELATIVE,
            "position_min_mm": POSITION_MIN_MM,
            "position_diagonal_relative": POSITION_DIAGONAL_RELATIVE,
            "mesh_chord_relative": MESH_CHORD_RELATIVE,
            "mesh_chord_floor_mm": MESH_CHORD_FLOOR_MM,
            "face_count": "not compared: SolidCraft merges faces that share a surface",
            "center": "the center of each body's box, not its center of mass",
        },
        "source_comparison": {
            "parameter_relative": PARAMETER_RELATIVE,
            "parameter_scale_floor": 1.0,
            "centroid_min_mm": POSITION_MIN_MM,
            "centroid_diagonal_relative": POSITION_DIAGONAL_RELATIVE,
            "counts": "raw kernel counts are informational; available merged analytic face and edge counts must match; free-form or mesh totals are not compared; analytic surface-kind counts must match when available",
            "manifold": "not checked: no explicit evaluator result",
        },
        "stored_bodies": {
            "exact_box_min_mm": STORED_EXACT.0,
            "exact_box_diagonal_relative": STORED_EXACT.1,
            "sampled_box_min_mm": STORED_SAMPLED.0,
            "sampled_box_diagonal_relative": STORED_SAMPLED.1,
        },
        "import": {"no_volume_mm3": NO_VOLUME_MM3},
        "edge_proof": {
            "position_mm": EDGE_POSITION_MM,
            "direction": EDGE_DIRECTION,
            "blame_distance_mm": EDGE_BLAME_MM,
        },
        "face_proof": {
            "position_mm": FACE_POSITION_MM,
            "direction": FACE_DIRECTION,
            "area_relative_straight": FACE_AREA_STRAIGHT,
            "area_relative_curved": FACE_AREA_CURVED,
            "plane_mm": FACE_PLANE_MM,
            "line_cover_relative": FACE_LINE_COVER,
            "arc_radius_relative": FACE_ARC_RADIUS,
            "turn_rad": FACE_TURN_RAD,
        },
        "cap_proof": {
            "area_min_mm2": CAP_AREA.0,
            "area_relative": CAP_AREA.1,
            "plane_mm": CAP_PLANE_MM,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dutchman_core::{Check, Mode, Outcome, Report};

    /// The numbers the checks used before they were gathered here: moving them changed nothing.
    #[test]
    fn the_numbers_are_the_ones_the_checks_always_used() {
        assert_eq!(VOLUME_RELATIVE, 1e-4);
        assert_eq!(crate::VOLUME_TOLERANCE, 1e-4);
        assert_eq!(MESH_CHORD_RELATIVE, 5e-5);
        assert_eq!(MESH_CHORD_FLOOR_MM, 1e-4);
        assert_eq!(POSITION_MIN_MM, 0.01);
        assert_eq!(POSITION_DIAGONAL_RELATIVE, 4.0 * 5e-5);
        assert_eq!(AREA_RELATIVE, 1e-3);
        assert_eq!(STORED_EXACT, (0.5, 0.01));
        assert_eq!(STORED_SAMPLED, (1.0, 0.02));
        assert_eq!(NO_VOLUME_MM3, 1e-6);
        assert_eq!(
            (EDGE_POSITION_MM, EDGE_DIRECTION, EDGE_BLAME_MM),
            (1e-6, 1e-6, 0.01)
        );
        assert_eq!((FACE_POSITION_MM, FACE_DIRECTION), (1e-5, 1e-5));
        assert_eq!((FACE_AREA_STRAIGHT, FACE_AREA_CURVED), (1e-4, 0.02));
        assert_eq!((FACE_PLANE_MM, FACE_LINE_COVER), (1e-4, 1e-4));
        assert_eq!((FACE_ARC_RADIUS, FACE_TURN_RAD), (1e-3, 2e-3));
        assert_eq!(CAP_AREA, (1.0, 0.03));
        assert_eq!(CAP_PLANE_MM, 1e-4);
    }

    #[test]
    fn the_report_spells_the_box_center_field_in_american() {
        let report = report();
        assert!(report["replay"]["center"].is_string());
        assert!(report["replay"].get("centre").is_none());
    }

    #[test]
    fn the_report_states_the_numbers_the_checks_use() {
        let report = report();
        assert_eq!(report["version"], 1);
        assert_eq!(report["replay"]["volume_relative"], crate::VOLUME_TOLERANCE);
        assert_eq!(report["replay"]["area_relative"], 1e-3);
        assert_eq!(
            report["source_comparison"]["parameter_relative"],
            PARAMETER_RELATIVE
        );
        assert_eq!(
            report["source_comparison"]["centroid_min_mm"],
            POSITION_MIN_MM
        );
        assert_eq!(report["replay"]["position_min_mm"], 0.01);
        assert_eq!(report["replay"]["position_diagonal_relative"], 2e-4);
        assert_eq!(report["stored_bodies"]["exact_box_min_mm"], 0.5);
        assert_eq!(
            report["stored_bodies"]["sampled_box_diagonal_relative"],
            0.02
        );
        assert_eq!(report["edge_proof"]["position_mm"], 1e-6);
        assert_eq!(report["face_proof"]["area_relative_curved"], 0.02);
        assert_eq!(report["cap_proof"]["area_relative"], 0.03);
    }

    fn outcome(mode: Mode, check: Check) -> Outcome {
        Outcome {
            mode,
            check,
            features: Vec::new(),
        }
    }

    #[test]
    fn tolerances_are_stated_only_when_a_replay_was_compared() {
        let stated = |mode, check| {
            let mut report = Report::default();
            crate::state_tolerances(&mut report, &outcome(mode, check));
            report.verification_tolerances().is_some()
        };
        assert!(stated(Mode::History, Check::Verified));
        assert!(stated(Mode::PartialHistory, Check::Partial("x".to_owned())));
        assert!(!stated(
            Mode::Import,
            Check::Flattened("replay differs (a body sits 6.000 mm away)".to_owned())
        ));
        assert!(stated(
            Mode::History,
            Check::Flattened("replay differs (a body sits 6.000 mm away)".to_owned())
        ));
        assert!(!stated(
            Mode::History,
            Check::Unverified("no CLI".to_owned())
        ));
        assert!(!stated(
            Mode::History,
            Check::Flattened("not mapped".to_owned())
        ));
        assert!(!stated(Mode::Import, Check::NotApplicable("x".to_owned())));
    }
}
