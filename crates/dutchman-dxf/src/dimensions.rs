//! Dimension labels use neutral measurements and replacement text only.

use cadmpeg_ir::semantic_annotations::{DimensionKind, SemanticAnnotation};

pub(crate) fn label(annotation: &SemanticAnnotation) -> Option<String> {
    if let Some(text) = &annotation.text_override {
        return Some(text.clone());
    }
    let value = annotation.value?.get();
    let (prefix, suffix) = match annotation.dimension_kind.as_ref()? {
        DimensionKind::Distance {}
        | DimensionKind::HorizontalDistance {}
        | DimensionKind::VerticalDistance {} => ("", ""),
        DimensionKind::Radius {} => ("R", ""),
        DimensionKind::Diameter {} => ("⌀", ""),
        DimensionKind::Angle {} | DimensionKind::ThreePointAngle {} => ("", "°"),
        DimensionKind::Other { .. } => return None,
    };
    let mut text = format!("{prefix}{value}{suffix}");
    if annotation.upper_tolerance.is_some() || annotation.lower_tolerance.is_some() {
        let upper = annotation.upper_tolerance.map_or(0.0, |v| v.get());
        let lower = annotation.lower_tolerance.map_or(0.0, |v| v.get());
        text.push_str(&format!(" ({upper:+}/{lower:+})"));
    }
    Some(text)
}

/// Generate a conventional presentation from reference points, never a source layout.
pub(crate) fn lines(
    annotation: &SemanticAnnotation,
    entities: &mut Vec<(String, crate::entities::Entity)>,
) -> bool {
    use crate::entities::Entity;
    let [a, b] = annotation.reference_anchors.as_slice() else {
        return false;
    };
    let Some([x, y, _]) = annotation.position.map(|p| p.get()) else {
        return false;
    };
    let a = a.get();
    let b = b.get();
    let [ax, ay] = a;
    let [bx, by] = b;
    let (start, end, extensions) = match annotation.dimension_kind {
        Some(DimensionKind::HorizontalDistance {}) => ([ax, y], [bx, y], true),
        Some(DimensionKind::VerticalDistance {}) => ([x, ay], [x, by], true),
        Some(DimensionKind::Distance {}) => {
            let dx = bx - ax;
            let dy = by - ay;
            let length = dx.hypot(dy);
            if length <= 1e-12 {
                return false;
            }
            let offset = ((x - ax) * -dy + (y - ay) * dx) / length;
            let [sx, sy] = [-dy * offset / length, dx * offset / length];
            ([ax + sx, ay + sy], [bx + sx, by + sy], true)
        }
        Some(DimensionKind::Radius {} | DimensionKind::Diameter {}) => (a, b, false),
        _ => return false,
    };
    let [sx, sy] = start;
    let [ex, ey] = end;
    let dx = ex - sx;
    let dy = ey - sy;
    let length = dx.hypot(dy);
    if length <= 1e-12 {
        return false;
    }
    let mut line = |start, end| entities.push(("DIMENSIONS".into(), Entity::Line { start, end }));
    if extensions {
        line(a, start);
        line(b, end);
    }
    line(start, end);
    let size = (length / 4.0).min(2.5);
    for (tip, direction) in [(start, 1.0), (end, -1.0)] {
        if direction > 0.0 && matches!(annotation.dimension_kind, Some(DimensionKind::Radius {})) {
            continue;
        }
        let [tx, ty] = tip;
        for side in [-1.0, 1.0] {
            line(
                tip,
                [
                    tx + size * (direction * dx - side * dy * 0.4) / length,
                    ty + size * (direction * dy + side * dx * 0.4) / length,
                ],
            );
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::scalar::FiniteReal;
    use cadmpeg_ir::semantic_annotations::{SemanticAnnotationId, SemanticAnnotationKind};

    #[test]
    fn labels_require_measurements_and_keep_replacements_and_tolerances() {
        let mut annotation = SemanticAnnotation::new(
            SemanticAnnotationId::mint("synthetic:test:annotation#dimension").unwrap(),
            "dimension".into(),
            SemanticAnnotationKind::Dimension,
            "Dimension".into(),
            0,
            "native".into(),
        );
        annotation.dimension_kind = Some(DimensionKind::Diameter {});
        assert_eq!(label(&annotation), None);
        annotation.value = FiniteReal::new(30.0);
        annotation.upper_tolerance = FiniteReal::new(0.2);
        annotation.lower_tolerance = FiniteReal::new(-0.1);
        assert_eq!(label(&annotation).as_deref(), Some("⌀30 (+0.2/-0.1)"));
        annotation.text_override = Some("TYP".into());
        assert_eq!(label(&annotation).as_deref(), Some("TYP"));
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;
    use crate::entities::Entity;
    use cadmpeg_ir::semantic_annotations::{SemanticAnnotationId, SemanticAnnotationKind};
    use cadmpeg_ir::units::FiniteVector;

    #[test]
    fn anchors_generate_page_space_extensions_and_arrows_without_scaling_value() {
        let mut annotation = SemanticAnnotation::new(
            SemanticAnnotationId::mint("synthetic:test:annotation#width").unwrap(),
            "width".into(),
            SemanticAnnotationKind::Dimension,
            "Dimension".into(),
            0,
            "native".into(),
        );
        annotation.dimension_kind = Some(DimensionKind::HorizontalDistance {});
        annotation.reference_anchors = vec![
            FiniteVector::new([35.0, 180.0]).unwrap(),
            FiniteVector::new([95.0, 180.0]).unwrap(),
        ];
        annotation.position = Some(FiniteVector::new([65.0, 160.0, 0.0]).unwrap());
        let mut entities = Vec::new();
        assert!(lines(&annotation, &mut entities));
        assert_eq!(entities.len(), 7);
        assert!(matches!(
            entities[0].1,
            Entity::Line {
                start: [35.0, 180.0],
                end: [35.0, 160.0]
            }
        ));
        assert!(matches!(
            entities[2].1,
            Entity::Line {
                start: [35.0, 160.0],
                end: [95.0, 160.0]
            }
        ));
        annotation.dimension_kind = Some(DimensionKind::Angle {});
        assert!(!lines(&annotation, &mut entities));
        assert_eq!(entities.len(), 7);
        annotation.reference_anchors.clear();
        assert!(!lines(&annotation, &mut entities));
    }
}
