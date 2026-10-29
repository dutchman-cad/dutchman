//! Exact neutral curves placed in millimetre page space.

use crate::entities::Entity;
use cadmpeg_ir::drawings::{Drawing, DrawingCurveGeometry};

pub(crate) fn curve(
    geometry: &DrawingCurveGeometry,
    drawing: &Drawing,
    [ox, oy]: [f64; 2],
) -> Option<Entity> {
    let [x, y] = drawing.position?.get();
    let scale = drawing.scale.map_or(1.0, |s| s.get());
    let angle = drawing
        .rotation_degrees
        .map_or(0.0, |r| r.get())
        .to_radians();
    Some(placed_curve(geometry, [x + ox, y + oy], scale, angle))
}

pub(crate) fn placed_curve(
    geometry: &DrawingCurveGeometry,
    [x, y]: [f64; 2],
    scale: f64,
    angle: f64,
) -> Entity {
    let place = |[u, v]: [f64; 2]| {
        [
            x + scale * (u * angle.cos() - v * angle.sin()),
            y + scale * (u * angle.sin() + v * angle.cos()),
        ]
    };
    match geometry {
        DrawingCurveGeometry::Line { start, end } => Entity::Line {
            start: place(start.get()),
            end: place(end.get()),
        },
        DrawingCurveGeometry::Circle { center, radius } => Entity::Circle {
            center: place(center.get()),
            radius: radius.get() * scale,
        },
        DrawingCurveGeometry::Arc {
            center,
            radius,
            start,
            middle,
            end,
        } => {
            let [cx, cy] = center.get();
            let theta =
                |[px, py]: [f64; 2]| (py - cy).atan2(px - cx).to_degrees().rem_euclid(360.0);
            let a = theta(start.get());
            let b = theta(end.get());
            let m = theta(middle.get());
            let (a, b) = if (m - a).rem_euclid(360.0) < (b - a).rem_euclid(360.0) {
                (a, b)
            } else {
                (b, a)
            };
            Entity::Arc {
                center: place(center.get()),
                radius: radius.get() * scale,
                start_degrees: a + angle.to_degrees(),
                end_degrees: b + angle.to_degrees(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadmpeg_ir::{scalar::PositiveReal, units::FiniteVector};

    #[test]
    fn transforms_exact_curves_and_preserves_clockwise_arc_sweep() {
        let line = DrawingCurveGeometry::Line {
            start: FiniteVector::new([1.0, 2.0]).unwrap(),
            end: FiniteVector::new([3.0, 4.0]).unwrap(),
        };
        let Entity::Line { start, end } =
            placed_curve(&line, [100.0, 50.0], 2.0, std::f64::consts::FRAC_PI_2)
        else {
            panic!("line expected")
        };
        assert!((start[0] - 96.0).abs() < 1e-10);
        assert!((start[1] - 52.0).abs() < 1e-10);
        assert!((end[0] - 92.0).abs() < 1e-10);
        assert!((end[1] - 56.0).abs() < 1e-10);
        let arc = DrawingCurveGeometry::Arc {
            center: FiniteVector::new([0.0, 0.0]).unwrap(),
            radius: PositiveReal::new(1.0).unwrap(),
            start: FiniteVector::new([1.0, 0.0]).unwrap(),
            middle: FiniteVector::new([0.0, -1.0]).unwrap(),
            end: FiniteVector::new([-1.0, 0.0]).unwrap(),
        };
        let Entity::Arc {
            radius,
            start_degrees,
            end_degrees,
            ..
        } = placed_curve(&arc, [0.0, 0.0], 2.0, 0.0)
        else {
            panic!("arc expected")
        };
        assert_eq!(radius, 2.0);
        assert_eq!(start_degrees, 180.0);
        assert_eq!(end_degrees, 0.0);
    }
}
