//! Holds for the freedom a source sketch leaves.
//!
//! A source sketch is often not fully dimensioned: a chamfer end that slides along its edge, a
//! corner nothing pins. The source application leaves such a point where it was drawn when a
//! dimension is edited, because it only moves what the constraints force. SolidCraft's solver
//! settles the same freedom wherever the edit lands it, so one edit of a parameter can fling a
//! loose point far from the profile. When a sketch has a parameter-driven dimension, each
//! remaining freedom (of a sketch only a few short of determined) is held at its drawn position by a dimension from the sketch origin that
//! has no parameter, so an edit moves what the dimensions move and nothing else.

// The matrices are built here with one row per equation and one column per unknown, so every
// index below is in range by construction.
#![allow(clippy::indexing_slicing)]

use serde_json::{json, Value};

/// Step of the numerical derivative, in sketch units.
const STEP: f64 = 1e-6;
/// A pivot smaller than this is no pivot: that direction is free.
const PIVOT: f64 = 1e-7;
/// A sketch left with more freedom than this was drawn loose on purpose, not left a dimension
/// short, and keeps it.
const MAX_HELD: usize = 4;
/// Sketches above this many unknowns are left alone (the elimination is cubic).
const MAX_UNKNOWNS: usize = 240;

/// One curve, as the residuals read it.
#[derive(Clone, Copy)]
enum Shape {
    Line(usize, usize),
    /// Centre point, radius unknown.
    Circle(usize, usize),
    /// Centre, start, end points.
    Arc(usize, usize, usize),
}

struct Sketch {
    shapes: Vec<Option<Shape>>,
    /// The unknowns: two per point, then one per circle radius.
    start: Vec<f64>,
    fixed: Vec<bool>,
}

fn index(value: &Value, key: &str) -> Option<usize> {
    usize::try_from(value.get(key)?.as_u64()?).ok()
}

impl Sketch {
    fn read(points: &[Value], curves: &[Value]) -> Option<Sketch> {
        let mut start = Vec::new();
        let mut fixed = Vec::new();
        for point in points {
            let pos = point.get("pos")?.as_array()?;
            start.push(pos.first()?.as_f64()?);
            start.push(pos.get(1)?.as_f64()?);
            let held = point.get("fixed").and_then(Value::as_bool) == Some(true);
            fixed.extend([held, held]);
        }
        let mut shapes = Vec::new();
        for curve in curves {
            let held = curve.get("fixed").and_then(Value::as_bool) == Some(true);
            let shape = match curve.get("type")?.as_str()? {
                "line" => Shape::Line(index(curve, "a")?, index(curve, "b")?),
                "circle" => {
                    let radius = curve.get("r")?.as_f64()?;
                    start.push(radius);
                    fixed.push(held);
                    Shape::Circle(index(curve, "c")?, start.len() - 1)
                }
                "arc" => Shape::Arc(index(curve, "c")?, index(curve, "a")?, index(curve, "b")?),
                _ => return None,
            };
            if held {
                match shape {
                    Shape::Line(a, b) => {
                        for p in [a, b] {
                            fixed[2 * p] = true;
                            fixed[2 * p + 1] = true;
                        }
                    }
                    Shape::Circle(c, _) => {
                        fixed[2 * c] = true;
                        fixed[2 * c + 1] = true;
                    }
                    Shape::Arc(c, a, b) => {
                        for p in [c, a, b] {
                            fixed[2 * p] = true;
                            fixed[2 * p + 1] = true;
                        }
                    }
                }
            }
            shapes.push(Some(shape));
        }
        if start.len() > MAX_UNKNOWNS {
            return None;
        }
        Some(Sketch {
            shapes,
            start,
            fixed,
        })
    }

    fn shape(&self, curve: usize) -> Option<Shape> {
        *self.shapes.get(curve)?
    }
}

fn point(v: &[f64], p: usize) -> [f64; 2] {
    [v[2 * p], v[2 * p + 1]]
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

fn norm(a: [f64; 2]) -> f64 {
    a[0].hypot(a[1])
}

/// The radius a circle or arc has at `v`.
fn radius(shape: Shape, v: &[f64]) -> Option<f64> {
    match shape {
        Shape::Circle(_, r) => v.get(r).copied(),
        Shape::Arc(c, a, _) => Some(norm(sub(point(v, a), point(v, c)))),
        Shape::Line(..) => None,
    }
}

fn centre(shape: Shape) -> Option<usize> {
    match shape {
        Shape::Circle(c, _) | Shape::Arc(c, _, _) => Some(c),
        Shape::Line(..) => None,
    }
}

/// The residuals of one constraint at `v`; `None` for a kind this analysis does not model.
fn residuals(sketch: &Sketch, constraint: &Value, v: &[f64]) -> Option<Vec<f64>> {
    let kind = constraint.get("type")?.as_str()?;
    let p = || index(constraint, "p");
    let q = || index(constraint, "q");
    let a = || sketch.shape(index(constraint, "a")?);
    let b = || sketch.shape(index(constraint, "b")?);
    let line = |s: Shape| match s {
        Shape::Line(x, y) => Some(sub(point(v, y), point(v, x))),
        _ => None,
    };
    Some(match kind {
        "coincident" => {
            let d = sub(point(v, p()?), point(v, q()?));
            vec![d[0], d[1]]
        }
        "fix" => {
            let at = point(v, p()?);
            let was = point(&sketch.start, p()?);
            vec![at[0] - was[0], at[1] - was[1]]
        }
        "horizontal" | "vertical" => {
            let Shape::Line(x, y) = sketch.shape(index(constraint, "l")?)? else {
                return None;
            };
            let d = sub(point(v, y), point(v, x));
            vec![if kind == "horizontal" { d[1] } else { d[0] }]
        }
        "parallel" => vec![cross(line(a()?)?, line(b()?)?)],
        "perpendicular" => vec![dot(line(a()?)?, line(b()?)?)],
        "collinear" => {
            let (Shape::Line(x, _), Shape::Line(..)) = (a()?, b()?) else {
                return None;
            };
            let d = line(a()?)?;
            let Shape::Line(u, w) = b()? else {
                return None;
            };
            vec![
                cross(d, sub(point(v, u), point(v, x))),
                cross(d, sub(point(v, w), point(v, x))),
            ]
        }
        "distance_x" => vec![point(v, q()?)[0] - point(v, p()?)[0]],
        "distance_y" => vec![point(v, q()?)[1] - point(v, p()?)[1]],
        "distance" => vec![norm(sub(point(v, q()?), point(v, p()?)))],
        "length" => vec![norm(line(sketch.shape(index(constraint, "l")?)?)?)],
        "point_line_distance" => {
            let Shape::Line(x, y) = sketch.shape(index(constraint, "l")?)? else {
                return None;
            };
            let d = sub(point(v, y), point(v, x));
            vec![cross(d, sub(point(v, p()?), point(v, x))) / norm(d).max(1e-12)]
        }
        "point_on_curve" => {
            let shape = sketch.shape(index(constraint, "c")?)?;
            let at = point(v, p()?);
            match shape {
                Shape::Line(x, y) => {
                    let d = sub(point(v, y), point(v, x));
                    vec![cross(d, sub(at, point(v, x))) / norm(d).max(1e-12)]
                }
                _ => vec![norm(sub(at, point(v, centre(shape)?))) - radius(shape, v)?],
            }
        }
        "concentric" => {
            let d = sub(point(v, centre(a()?)?), point(v, centre(b()?)?));
            vec![d[0], d[1]]
        }
        "midpoint" => {
            let Shape::Line(x, y) = sketch.shape(index(constraint, "l")?)? else {
                return None;
            };
            let m = point(v, p()?);
            let (s, e) = (point(v, x), point(v, y));
            vec![m[0] - (s[0] + e[0]) / 2.0, m[1] - (s[1] + e[1]) / 2.0]
        }
        "diameter" | "radius" => {
            vec![radius(sketch.shape(index(constraint, "c")?)?, v)?]
        }
        "equal" => match (a()?, b()?) {
            (Shape::Line(..), Shape::Line(..)) => {
                vec![norm(line(a()?)?) - norm(line(b()?)?)]
            }
            (x, y) => vec![radius(x, v)? - radius(y, v)?],
        },
        "angle" | "line_angle" => {
            if kind == "line_angle" {
                let d = line(sketch.shape(index(constraint, "l")?)?)?;
                vec![d[1].atan2(d[0])]
            } else {
                let (x, y) = (line(a()?)?, line(b()?)?);
                vec![cross(x, y).atan2(dot(x, y))]
            }
        }
        "tangent" => {
            let (x, y) = (a()?, b()?);
            match (x, y) {
                (Shape::Line(s, e), round) | (round, Shape::Line(s, e)) => {
                    let d = sub(point(v, e), point(v, s));
                    let c = point(v, centre(round)?);
                    let mut out = vec![
                        cross(d, sub(c, point(v, s))).abs() / norm(d).max(1e-12)
                            - radius(round, v)?,
                    ];
                    // Where the line and the arc share an end, that end is the touching
                    // point: the line is square to the radius there.
                    if let Shape::Arc(_, a, b) = round {
                        for end in [a, b] {
                            if end == s || end == e {
                                out.push(dot(d, sub(point(v, end), c)));
                            }
                        }
                    }
                    out
                }
                _ => {
                    let gap = norm(sub(point(v, centre(x)?), point(v, centre(y)?)));
                    let (rx, ry) = (radius(x, v)?, radius(y, v)?);
                    let (rx0, ry0) = (radius(x, &sketch.start)?, radius(y, &sketch.start)?);
                    let gap0 = norm(sub(
                        point(&sketch.start, centre(x)?),
                        point(&sketch.start, centre(y)?),
                    ));
                    let outer = (gap0 - (rx0 + ry0)).abs() <= (gap0 - (rx0 - ry0).abs()).abs();
                    vec![if outer {
                        gap - (rx + ry)
                    } else {
                        gap - (rx - ry).abs()
                    }]
                }
            }
        }
        _ => return None,
    })
}

/// The Jacobian rows of every equation, by central differences.
fn jacobian(sketch: &Sketch, constraints: &[&Value]) -> Option<Vec<Vec<f64>>> {
    let n = sketch.start.len();
    let mut rows: Vec<Vec<f64>> = Vec::new();
    let mut v = sketch.start.clone();
    for constraint in constraints {
        residuals(sketch, constraint, &v)?;
        let mut columns: Vec<Vec<f64>> = Vec::with_capacity(n);
        for k in 0..n {
            let at = v[k];
            v[k] = at + STEP;
            let up = residuals(sketch, constraint, &v)?;
            v[k] = at - STEP;
            let down = residuals(sketch, constraint, &v)?;
            v[k] = at;
            columns.push(
                up.iter()
                    .zip(&down)
                    .map(|(u, d)| (u - d) / (2.0 * STEP))
                    .collect(),
            );
        }
        for equation in 0..columns.first().map_or(0, Vec::len) {
            rows.push(columns.iter().map(|c| c[equation]).collect());
        }
    }
    // An arc's end points lie on its circle about the centre.
    for shape in sketch.shapes.iter().flatten() {
        if let Shape::Arc(c, a, b) = *shape {
            let residual = |v: &[f64]| {
                norm(sub(point(v, a), point(v, c))) - norm(sub(point(v, b), point(v, c)))
            };
            let mut v = sketch.start.clone();
            let mut row = vec![0.0; n];
            for (k, cell) in row.iter_mut().enumerate() {
                let at = v[k];
                v[k] = at + STEP;
                let up = residual(&v);
                v[k] = at - STEP;
                let down = residual(&v);
                v[k] = at;
                *cell = (up - down) / (2.0 * STEP);
            }
            rows.push(row);
        }
    }
    // Fixed unknowns do not move.
    for (k, held) in sketch.fixed.iter().enumerate() {
        if *held {
            let mut row = vec![0.0; n];
            row[k] = 1.0;
            rows.push(row);
        }
    }
    Some(rows)
}

/// Row-reduces `rows` in place; the columns without a pivot are the free directions.
fn free_columns(rows: &mut [Vec<f64>], n: usize) -> Vec<usize> {
    let mut pivots = Vec::new();
    let mut row = 0;
    for column in 0..n {
        let Some(best) = (row..rows.len())
            .max_by(|&x, &y| rows[x][column].abs().total_cmp(&rows[y][column].abs()))
        else {
            break;
        };
        if rows[best][column].abs() <= PIVOT {
            continue;
        }
        rows.swap(row, best);
        let pivot = rows[row][column];
        for cell in &mut rows[row] {
            *cell /= pivot;
        }
        let reduced = rows[row].clone();
        for (other, target) in rows.iter_mut().enumerate() {
            if other != row {
                let factor = target[column];
                if factor != 0.0 {
                    for (cell, from) in target.iter_mut().zip(&reduced) {
                        *cell -= factor * from;
                    }
                }
            }
        }
        pivots.push(column);
        row += 1;
    }
    (0..n).filter(|c| !pivots.contains(c)).collect()
}

/// The dimensions that hold what the sketch's constraints leave free, as SolidCraft
/// constraints appended after `constraints`. Empty when the sketch has no parameter-driven
/// dimension, is fully determined, or uses a curve or constraint this analysis does not model.
pub(crate) fn holds(points: &[Value], curves: &[Value], constraints: &[Value]) -> Vec<Value> {
    if !constraints.iter().any(|c| c.get("param").is_some()) {
        return Vec::new();
    }
    let Some(sketch) = Sketch::read(points, curves) else {
        return Vec::new();
    };
    let active: Vec<&Value> = constraints
        .iter()
        .filter(|c| c.get("driven").and_then(Value::as_bool) != Some(true))
        .collect();
    let n = sketch.start.len();
    let Some(mut rows) = jacobian(&sketch, &active) else {
        return Vec::new();
    };
    let mut probe = rows.clone();
    if free_columns(&mut probe, n).len() > MAX_HELD {
        return Vec::new();
    }
    let mut out = Vec::new();
    let origin =
        |p: usize| sketch.start[2 * p].abs() <= 1e-9 && sketch.start[2 * p + 1].abs() <= 1e-9;
    // Each pass holds one free point or coordinate, then reduces again with that hold in.
    for _ in 0..n {
        let mut work = rows.clone();
        let free: Vec<usize> = free_columns(&mut work, n)
            .into_iter()
            .filter(|&c| c < 2 * points.len())
            .collect();
        // A loose point sitting on the sketch origin is tied to it; any other loose point
        // has a coordinate held at its drawn distance (a zero distance is no dimension).
        let tied = (1..points.len()).filter(|&p| origin(p)).find(|&p| {
            let mut trial = rows.clone();
            for axis in 0..2 {
                let mut row = vec![0.0; n];
                row[2 * p + axis] = 1.0;
                trial.push(row);
            }
            free_columns(&mut trial, n).len() < free_columns(&mut work.clone(), n).len()
        });
        if let Some(point) = tied {
            out.push(json!({"type": "coincident", "p": point, "q": 0}));
            for axis in 0..2 {
                let mut row = vec![0.0; n];
                row[2 * point + axis] = 1.0;
                rows.push(row);
            }
            continue;
        }
        let Some(&column) = free.iter().find(|&&c| sketch.start[c].abs() > 1e-9) else {
            break;
        };
        let (point, axis) = (column / 2, column % 2);
        let value = sketch.start[column];
        let kind = ["distance_x", "distance_y"][axis];
        let (p, q) = if value > 0.0 { (0, point) } else { (point, 0) };
        out.push(json!({"type": kind, "p": p, "q": q, "value": value.abs()}));
        let mut row = vec![0.0; n];
        row[column] = 1.0;
        rows.push(row);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(id: &str, x: f64, y: f64) -> Value {
        json!({"id": id, "pos": [x, y], "fixed": id == "origin"})
    }

    #[test]
    fn a_point_sliding_along_a_dimensioned_edge_is_held() {
        // A step profile: the notch corner (3) only lies on the vertical through point 2,
        // so its height is free while the width is dimensioned.
        let points = vec![
            point("origin", 0.0, 0.0),
            point("p1", 0.0, 0.0),
            point("p2", 4.0, 0.0),
            point("p3", 4.0, -1.0),
        ];
        let curves = vec![
            json!({"type": "line", "a": 1, "b": 2}),
            json!({"type": "line", "a": 2, "b": 3}),
        ];
        let constraints = vec![
            json!({"type": "coincident", "p": 1, "q": 0}),
            json!({"type": "horizontal", "l": 0}),
            json!({"type": "vertical", "l": 1}),
            json!({"type": "distance_x", "p": 1, "q": 2, "value": 4.0, "param": "W"}),
        ];
        let held = holds(&points, &curves, &constraints);
        assert_eq!(held.len(), 1, "{held:?}");
        assert_eq!(held[0]["type"], "distance_y");
        assert_eq!(held[0]["q"], 0);
        assert_eq!(held[0]["p"], 3);
        assert_eq!(held[0]["value"], 1.0);
        assert!(held[0].get("param").is_none());
    }

    #[test]
    fn a_determined_sketch_gains_no_hold() {
        let points = vec![
            point("origin", 0.0, 0.0),
            point("p1", 0.0, 0.0),
            point("p2", 4.0, 0.0),
        ];
        let curves = vec![json!({"type": "line", "a": 1, "b": 2})];
        let constraints = vec![
            json!({"type": "coincident", "p": 1, "q": 0}),
            json!({"type": "horizontal", "l": 0}),
            json!({"type": "distance_x", "p": 1, "q": 2, "value": 4.0, "param": "W"}),
        ];
        assert!(holds(&points, &curves, &constraints).is_empty());
    }

    #[test]
    fn a_sketch_without_a_parameter_is_left_alone() {
        let points = vec![
            point("origin", 0.0, 0.0),
            point("p1", 1.0, 1.0),
            point("p2", 4.0, 0.0),
        ];
        let curves = vec![json!({"type": "line", "a": 1, "b": 2})];
        assert!(holds(&points, &curves, &[]).is_empty());
    }

    #[test]
    fn an_unmodelled_constraint_leaves_the_sketch_alone() {
        let points = vec![
            point("origin", 0.0, 0.0),
            point("p1", 1.0, 1.0),
            point("p2", 4.0, 0.0),
        ];
        let curves = vec![json!({"type": "line", "a": 1, "b": 2})];
        let constraints = vec![
            json!({"type": "smooth", "a": 0, "b": 0}),
            json!({"type": "distance_x", "p": 1, "q": 2, "value": 3.0, "param": "W"}),
        ];
        assert!(holds(&points, &curves, &constraints).is_empty());
    }
}
