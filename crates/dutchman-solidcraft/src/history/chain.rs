//! Where a sketch point sits, as an expression in the dimensions that place it.
//!
//! A sketch on the end face of an earlier feature sits where that face is, so when a
//! dimension of the earlier sketch changes, the later sketch moves. The source records the
//! sketch only by its position; this reads the position back as a sum of the dimensions
//! that run from the sketch origin to a point of the earlier sketch, so the later sketch's
//! plane can be offset by that sum.

// Axes are 0 or 1 and point indices come from the sketch itself, so the indexing is in range.
#![allow(clippy::indexing_slicing)]

use serde_json::Value;

/// Closeness (sketch units) for a line to count as running along an axis.
const ALIGNED: f64 = 1e-7;

/// A coordinate as an expression, and whether any dimension parameter is in it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Coordinate {
    pub(crate) expression: String,
    pub(crate) symbolic: bool,
}

fn equal(steps: &mut Vec<Step>, axis: usize, a: usize, b: usize) {
    steps.push(Step {
        axis,
        from: a,
        to: b,
        sign: 1.0,
        term: None,
    });
}

/// One known relation: `C[to] = C[from] + sign * term` along `axis`.
struct Step {
    axis: usize,
    from: usize,
    to: usize,
    sign: f64,
    term: Option<(String, bool)>,
}

fn index(value: &Value, key: &str) -> Option<usize> {
    usize::try_from(value.get(key)?.as_u64()?).ok()
}

fn position(point: &Value) -> Option<[f64; 2]> {
    let pos = point.get("pos")?.as_array()?;
    Some([pos.first()?.as_f64()?, pos.get(1)?.as_f64()?])
}

fn term(constraint: &Value, magnitude: bool) -> Option<(String, bool)> {
    if let Some(name) = constraint.get("param").and_then(Value::as_str) {
        let name = if magnitude {
            format!("abs({name})")
        } else {
            name.to_owned()
        };
        return Some((name, true));
    }
    Some((format!("{} mm", constraint.get("value")?.as_f64()?), false))
}

/// The coordinate expressions of every point of a sketch (`None` where the dimensions do not
/// reach the point).
pub(crate) fn coordinates(
    points: &[Value],
    curves: &[Value],
    constraints: &[Value],
) -> Vec<[Option<Coordinate>; 2]> {
    let at: Vec<Option<[f64; 2]>> = points.iter().map(position).collect();
    let line = |curve: usize| -> Option<(usize, usize)> {
        let curve = curves.get(curve)?;
        (curve.get("type")?.as_str()? == "line")
            .then(|| Some((index(curve, "a")?, index(curve, "b")?)))
            .flatten()
    };
    // The axis a line runs along, from where it is drawn.
    let along = |curve: usize| -> Option<usize> {
        let (a, b) = line(curve)?;
        let (a, b) = (at.get(a).copied()??, at.get(b).copied()??);
        let (dx, dy) = ((b[0] - a[0]).abs(), (b[1] - a[1]).abs());
        if dy <= ALIGNED * dx.max(1.0) {
            Some(0)
        } else if dx <= ALIGNED * dy.max(1.0) {
            Some(1)
        } else {
            None
        }
    };
    let mut steps: Vec<Step> = Vec::new();
    for constraint in constraints {
        if constraint.get("driven").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let kind = constraint.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "coincident" => {
                if let (Some(p), Some(q)) = (index(constraint, "p"), index(constraint, "q")) {
                    equal(&mut steps, 0, p, q);
                    equal(&mut steps, 1, p, q);
                }
            }
            "horizontal" | "vertical" => {
                if let Some((a, b)) = index(constraint, "l").and_then(line) {
                    equal(&mut steps, usize::from(kind == "horizontal"), a, b);
                }
            }
            "parallel" | "perpendicular" => {
                for key in ["a", "b"] {
                    if let Some(curve) = index(constraint, key) {
                        if let (Some(axis), Some((a, b))) = (along(curve), line(curve)) {
                            // A line along x keeps one y; along y, one x.
                            equal(&mut steps, 1 - axis, a, b);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    for constraint in constraints {
        if constraint.get("driven").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let kind = constraint.get("type").and_then(Value::as_str).unwrap_or("");
        let (Some(p), pos) = (index(constraint, "p"), |i: usize| {
            at.get(i).copied().flatten()
        }) else {
            continue;
        };
        match kind {
            "distance_x" | "distance_y" => {
                let (Some(q), axis) = (index(constraint, "q"), usize::from(kind == "distance_y"))
                else {
                    continue;
                };
                let (Some(from), Some(to)) = (pos(p), pos(q)) else {
                    continue;
                };
                steps.push(Step {
                    axis,
                    from: p,
                    to: q,
                    sign: if to[axis] >= from[axis] { 1.0 } else { -1.0 },
                    term: term(constraint, true),
                });
            }
            "point_line_distance" => {
                let Some(curve) = index(constraint, "l") else {
                    continue;
                };
                let (Some(axis), Some((a, b))) = (along(curve), line(curve)) else {
                    continue;
                };
                // A line along x sits at one y: the point is a distance from it in y.
                let axis = 1 - axis;
                let (Some(from), Some(to)) = (pos(a), pos(p)) else {
                    continue;
                };
                equal(&mut steps, axis, a, b);
                steps.push(Step {
                    axis,
                    from: a,
                    to: p,
                    sign: if to[axis] >= from[axis] { 1.0 } else { -1.0 },
                    term: term(constraint, false),
                });
            }
            _ => {}
        }
    }
    let mut known: Vec<[Option<Coordinate>; 2]> = vec![[None, None]; points.len()];
    if let Some(origin) = known.first_mut() {
        *origin = [
            Some(Coordinate {
                expression: "0 mm".to_owned(),
                symbolic: false,
            }),
            Some(Coordinate {
                expression: "0 mm".to_owned(),
                symbolic: false,
            }),
        ];
    }
    for (index, point) in points.iter().enumerate() {
        if point.get("fixed").and_then(Value::as_bool) == Some(true) && index > 0 {
            if let Some(p) = at.get(index).copied().flatten() {
                known[index] = p.map(|v| {
                    Some(Coordinate {
                        expression: format!("{v} mm"),
                        symbolic: false,
                    })
                });
            }
        }
    }
    // Relations run both ways; settle until nothing new is reached.
    for _ in 0..points.len().saturating_add(1) {
        let mut changed = false;
        for step in &steps {
            for (from, to, sign) in [
                (step.from, step.to, step.sign),
                (step.to, step.from, -step.sign),
            ] {
                let Some(base) = known.get(from).and_then(|c| c[step.axis].clone()) else {
                    continue;
                };
                if known.get(to).is_none_or(|c| c[step.axis].is_some()) {
                    continue;
                }
                let value = match &step.term {
                    None => base,
                    Some((name, symbolic)) => Coordinate {
                        expression: if base.expression == "0 mm" {
                            if sign > 0.0 {
                                name.clone()
                            } else {
                                format!("-{name}")
                            }
                        } else {
                            format!(
                                "{} {} {name}",
                                base.expression,
                                if sign > 0.0 { "+" } else { "-" }
                            )
                        },
                        symbolic: base.symbolic || *symbolic,
                    },
                };
                known[to][step.axis] = Some(value);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    known
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_chain_of_dimensions_from_the_origin_is_a_sum() {
        // Origin to p1 coincident; p1 above a line p2-p3 by L, which is above p4-p5 by H.
        let points = vec![
            json!({"id": "origin", "pos": [0.0, 0.0], "fixed": true}),
            json!({"id": "p1", "pos": [0.0, 0.0]}),
            json!({"id": "p2", "pos": [1.0, -8.0]}),
            json!({"id": "p3", "pos": [2.0, -8.0]}),
            json!({"id": "p4", "pos": [1.0, -9.65]}),
            json!({"id": "p5", "pos": [2.0, -9.65]}),
        ];
        let curves = vec![
            json!({"type": "line", "a": 2, "b": 3}),
            json!({"type": "line", "a": 4, "b": 5}),
        ];
        let constraints = vec![
            json!({"type": "coincident", "p": 1, "q": 0}),
            json!({"type": "parallel", "a": 0, "b": 1}),
            json!({"type": "point_line_distance", "p": 1, "l": 0, "param": "L", "value": 8.0}),
            json!({"type": "point_line_distance", "p": 2, "l": 1, "param": "H", "value": 1.65}),
        ];
        let known = coordinates(&points, &curves, &constraints);
        let bottom = known[5][1].clone().unwrap();
        assert_eq!(bottom.expression, "-L - H");
        assert!(bottom.symbolic);
        assert_eq!(known[5][0], None);
    }
}
