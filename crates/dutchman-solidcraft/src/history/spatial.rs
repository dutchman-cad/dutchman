//! Spatial (3D) sketches, as SolidCraft sketch features with "3D Sketch" on.
//!
//! SolidCraft stores a 3D sketch as a sketch whose `view.three_d` is set and whose drawn
//! geometry is a list of wires: each has `fit` points, a dense `pts` polyline that carries the
//! geometry, its coordinate `space`, and, for an exact arc, its centre, normal, radius and ends.
//! Lines, exact arcs, full circles (as two half arcs), NURBS curves and points are written at
//! their solved positions. A NURBS curve becomes a dense polyline of the exact curve with fit
//! points along it. Relations to model edges and vertices, constraints, surfaces and
//! source-native geometry are not carried.

use std::collections::BTreeMap;

use dutchman_core::{Report, Severity};
use serde_json::{json, Value};

use super::sketch::{cross, number, unit, v3, V3};
use super::{name, Context};

/// Largest angle (degrees) one segment of a tessellated arc spans.
const ARC_STEP: f64 = 2.0;

/// The most fit points written on a curve.
const MAX_FIT: usize = 64;

/// A mapped 3D sketch, as later features refer to it.
pub(crate) struct Mapped {
    /// The SolidCraft sketch feature's id.
    pub(crate) feature: u64,
    /// The SolidCraft wires each source entity became, in order, by its id.
    pub(crate) wires: BTreeMap<String, Vec<String>>,
}

/// SolidCraft wires as they are built.
#[derive(Default)]
struct Builder {
    wires: Vec<Value>,
    shapes: BTreeMap<String, Vec<String>>,
    lines: u64,
    points: u64,
    /// Entities written as polylines of an exact curve, and construction entities written solid.
    splines: usize,
    construction: usize,
}

impl Builder {
    fn wire(&mut self, entity: &str, fit: &[V3], pts: &[V3], arc: Option<Value>) {
        self.lines += 1;
        let id = format!("w{}", self.lines);
        let mut wire = json!({"id": id, "fit": fit, "pts": pts, "space": "component"});
        if let (Some(arc), Some(fields)) = (arc, wire.as_object_mut()) {
            fields.insert("arc".to_owned(), arc);
        }
        self.wires.push(wire);
        self.shapes.entry(entity.to_owned()).or_default().push(id);
    }

    fn point(&mut self, entity: &str, at: V3) {
        self.points += 1;
        let id = format!("q{}", self.points);
        self.wires
            .push(json!({"id": id, "fit": [at], "pts": [at], "space": "component"}));
        self.shapes.entry(entity.to_owned()).or_default().push(id);
    }
}

/// Maps a `spatial_sketch` feature whose operation is `definition`.
pub(crate) fn map(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    report: &mut Report,
) -> Result<Value, String> {
    let sketch_id = definition
        .get("sketch")
        .and_then(Value::as_str)
        .ok_or("the 3D sketch it binds is not resolved")?;
    let sketch = ctx
        .row("spatial_sketches", sketch_id)
        .ok_or("the 3D sketch it binds is missing")?;
    let id = ctx.id();
    let name = name(feature)
        .or_else(|| sketch.get("name").and_then(Value::as_str))
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Sketch{id}"));
    let subject = format!("3D sketch \"{name}\"");

    let mut builder = Builder::default();
    let mut dropped: BTreeMap<String, usize> = BTreeMap::new();
    for entity in ctx
        .arena("spatial_sketch_entities")
        .iter()
        .filter(|entity| entity.get("sketch").and_then(Value::as_str) == Some(sketch_id))
    {
        let Some(entity_id) = entity.get("id").and_then(Value::as_str) else {
            continue;
        };
        let geometry = entity.get("geometry").unwrap_or(&Value::Null);
        let before = builder.wires.len();
        match entity_geometry(entity_id, geometry, &mut builder) {
            Ok(()) => {}
            Err(why) => {
                *dropped.entry(why).or_default() += 1;
            }
        }
        if builder.wires.len() > before
            && entity.get("construction").and_then(Value::as_bool) == Some(true)
        {
            builder.construction += 1;
        }
    }
    for (why, count) in &dropped {
        report.push(
            Severity::Degraded,
            subject.clone(),
            format!(
                "{count} {} left out: {why}",
                if *count == 1 { "curve" } else { "curves" }
            ),
        );
    }
    if builder.splines > 0 {
        report.push(
            Severity::Info,
            subject.clone(),
            format!(
                "{} written as {}; moving a fit point redraws a curve as a SolidCraft spline",
                if builder.splines == 1 {
                    "1 spline".to_owned()
                } else {
                    format!("{} splines", builder.splines)
                },
                if builder.splines == 1 {
                    "a dense curve through the source curve"
                } else {
                    "dense curves through the source curves"
                },
            ),
        );
    }
    if builder.construction > 0 {
        report.push(
            Severity::Info,
            subject.clone(),
            format!(
                "{} construction curve{} drawn as ordinary 3D geometry",
                builder.construction,
                if builder.construction == 1 { "" } else { "s" }
            ),
        );
    }
    let constraints = ctx
        .arena("spatial_sketch_constraints")
        .iter()
        .filter(|row| row.get("sketch").and_then(Value::as_str) == Some(sketch_id))
        .count();
    if constraints > 0 {
        report.push(
            Severity::Info,
            subject,
            format!(
                "{constraints} constraint{} not carried: the geometry is written at its solved \
                 positions, and links to model edges and vertices are not kept",
                if constraints == 1 { "" } else { "s" }
            ),
        );
    }

    ctx.spatial.insert(
        sketch_id.to_owned(),
        Mapped {
            feature: id,
            wires: builder.shapes,
        },
    );
    Ok(json!({
        "id": id,
        "name": name,
        "type": "sketch",
        "suppressed": false,
        "plane": {"type": "origin", "name": "XY"},
        "sketch": {
            "points": [{"id": "origin", "pos": [0.0, 0.0], "fixed": true}],
            "curves": [],
            "constraints": [],
            "counters": {"q": builder.points, "w": builder.lines},
            "view": {"three_d": true},
            "wires": builder.wires,
        },
    }))
}

/// The wires one entity's `geometry` becomes, or why none.
fn entity_geometry(entity: &str, geometry: &Value, builder: &mut Builder) -> Result<(), String> {
    let kind = geometry
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("native");
    match kind {
        "point" => {
            let at = v3(geometry.get("position")).ok_or("a point without a position")?;
            builder.point(entity, at);
        }
        "line" => {
            let a = v3(geometry.get("start")).ok_or("a line without a start")?;
            let b = v3(geometry.get("end")).ok_or("a line without an end")?;
            builder.wire(entity, &[a, b], &[a, b], None);
        }
        "circle" => {
            let circle = Circle::read(geometry)?;
            let start = circle.angle0;
            // Two half arcs: SolidCraft's arc is a start, a through point and an end.
            for half in 0..2 {
                let from = start + f64::from(half) * std::f64::consts::PI;
                circle.arc(entity, from, from + std::f64::consts::PI, builder);
            }
        }
        "arc" => {
            let circle = Circle::read(geometry)?;
            let a = number(geometry.get("start_angle")).ok_or("an arc without a start angle")?;
            let mut b = number(geometry.get("end_angle")).ok_or("an arc without an end angle")?;
            while b <= a {
                b += std::f64::consts::TAU;
            }
            if b - a >= std::f64::consts::TAU - 1e-9 {
                for half in 0..2 {
                    let from = a + f64::from(half) * std::f64::consts::PI;
                    circle.arc(entity, from, from + std::f64::consts::PI, builder);
                }
            } else {
                circle.arc(entity, a, b, builder);
            }
        }
        "nurbs" => {
            let (fit, pts) = nurbs(geometry.get("curve"))?;
            builder.splines += usize::from(fit.len() > 2 || pts.len() > 2);
            builder.wire(entity, &fit, &pts, None);
        }
        "nurbs_surface" => return Err("a surface drawn in the 3D sketch is not a curve".to_owned()),
        "native" => {
            let native = geometry
                .get("native_kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            return Err(format!("source geometry \"{native}\" has no neutral form"));
        }
        other => {
            return Err(format!(
                "{} geometry is not mapped",
                other.replace('_', " ")
            ))
        }
    }
    Ok(())
}

/// A circle in model space: its centre, unit normal and in-plane axes, and radius.
struct Circle {
    center: V3,
    normal: V3,
    x: V3,
    y: V3,
    radius: f64,
    /// The parameter zero of a full circle (its reference direction).
    angle0: f64,
}

impl Circle {
    fn read(geometry: &Value) -> Result<Self, String> {
        let center = v3(geometry.get("center")).ok_or("a circle without a centre")?;
        let normal = v3(geometry.get("normal"))
            .and_then(unit)
            .ok_or("a circle without a normal")?;
        let x = v3(geometry.get("reference_direction"))
            .and_then(unit)
            .ok_or("a circle without a reference direction")?;
        let y = unit(cross(normal, x)).ok_or("a circle whose reference lies along its normal")?;
        let radius = number(geometry.get("radius"))
            .filter(|radius| *radius > 0.0)
            .ok_or("a circle without a radius")?;
        Ok(Circle {
            center,
            normal,
            x,
            y,
            radius,
            angle0: 0.0,
        })
    }

    fn at(&self, angle: f64) -> V3 {
        let (sin, cos) = angle.sin_cos();
        [0, 1, 2].map(|i| {
            self.center.get(i).copied().unwrap_or(0.0)
                + self.radius
                    * (cos * self.x.get(i).copied().unwrap_or(0.0)
                        + sin * self.y.get(i).copied().unwrap_or(0.0))
        })
    }

    /// An exact arc from parameter `from` to `to` (counterclockwise about the normal).
    fn arc(&self, entity: &str, from: f64, to: f64, builder: &mut Builder) {
        let segments = ((to - from).to_degrees() / ARC_STEP)
            .ceil()
            .clamp(8.0, 720.0) as usize;
        let pts: Vec<V3> = (0..=segments)
            .map(|i| self.at(from + (to - from) * i as f64 / segments as f64))
            .collect();
        let (start, end) = (self.at(from), self.at(to));
        let arc = json!({
            "center": self.center,
            "normal": self.normal,
            "radius": self.radius,
            "start": start,
            "end": end,
        });
        builder.wire(
            entity,
            &[start, self.at((from + to) / 2.0), end],
            &pts,
            Some(arc),
        );
    }
}

/// Fit points and a dense polyline of a NURBS curve given as CADIR JSON.
fn nurbs(curve: Option<&Value>) -> Result<(Vec<V3>, Vec<V3>), String> {
    let curve = curve.ok_or("a spline without its NURBS data")?;
    let degree = curve
        .get("degree")
        .and_then(Value::as_u64)
        .and_then(|degree| usize::try_from(degree).ok())
        .filter(|degree| *degree >= 1)
        .ok_or("a spline whose degree is not valid")?;
    let poles = curve.get("poles").ok_or("a spline without poles")?;
    let rows = poles
        .get("points")
        .and_then(Value::as_array)
        .ok_or("a spline without poles")?;
    let rational = poles.get("form").and_then(Value::as_str) == Some("rational");
    let mut points: Vec<V3> = Vec::with_capacity(rows.len());
    let mut weights: Vec<f64> = Vec::with_capacity(rows.len());
    for row in rows {
        points.push(
            v3(if rational {
                row.get("point")
            } else {
                Some(row)
            })
            .ok_or("a spline with a malformed pole")?,
        );
        weights.push(if rational {
            number(row.get("weight"))
                .filter(|w| *w > 0.0)
                .ok_or("a spline with a non-positive weight")?
        } else {
            1.0
        });
    }
    let knots: Vec<f64> = curve
        .get("knots")
        .and_then(Value::as_array)
        .ok_or("a spline without knots")?
        .iter()
        .map(Value::as_f64)
        .collect::<Option<_>>()
        .ok_or("a spline with malformed knots")?;
    let count = points.len();
    if count <= degree || knots.len() != count + degree + 1 {
        return Err("a spline whose knots do not fit its poles".to_owned());
    }
    if knots.windows(2).any(|pair| pair.first() > pair.get(1)) {
        return Err("a spline whose knots decrease".to_owned());
    }
    let (Some(&lo), Some(&hi)) = (knots.get(degree), knots.get(count)) else {
        return Err("a spline whose knots do not fit its poles".to_owned());
    };
    if !lo.is_finite() || !hi.is_finite() || hi <= lo {
        return Err("a spline with an empty parameter range".to_owned());
    }
    let eval = |t: f64| spline_point(degree, &knots, &points, &weights, t);

    // The distinct knot values inside the domain bound the polynomial pieces.
    let mut breaks: Vec<f64> = vec![lo];
    for knot in &knots {
        if *knot > lo
            && *knot < hi
            && breaks
                .last()
                .is_none_or(|last| knot - last > 1e-12 * (hi - lo))
        {
            breaks.push(*knot);
        }
    }
    breaks.push(hi);
    let polyline = degree == 1 && !rational;
    let per_piece = if polyline {
        1
    } else {
        (512 / breaks.len().saturating_sub(1).max(1)).clamp(4, 48)
    };
    let mut pts: Vec<V3> = Vec::new();
    for pair in breaks.windows(2) {
        let (Some(&a), Some(&b)) = (pair.first(), pair.get(1)) else {
            continue;
        };
        for step in 0..per_piece {
            pts.push(
                eval(a + (b - a) * step as f64 / per_piece as f64)
                    .ok_or("a spline that cannot be evaluated")?,
            );
        }
    }
    pts.push(eval(hi).ok_or("a spline that cannot be evaluated")?);
    pts.dedup_by(|a, b| a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-9));
    if pts.len() < 2 {
        return Err("a spline of no length".to_owned());
    }
    let fit = if polyline {
        pts.clone()
    } else {
        let n = count.clamp(3, MAX_FIT);
        (0..n)
            .map(|i| eval(lo + (hi - lo) * i as f64 / (n - 1) as f64))
            .collect::<Option<Vec<_>>>()
            .ok_or("a spline that cannot be evaluated")?
    };
    Ok((fit, pts))
}

/// The point of a NURBS curve at parameter `t` of its domain.
fn spline_point(
    degree: usize,
    knots: &[f64],
    points: &[V3],
    weights: &[f64],
    t: f64,
) -> Option<V3> {
    let count = points.len();
    // The span [knots[i], knots[i + 1]) holding t, the last one for the end of the domain.
    let mut span = degree;
    while span + 1 < count && *knots.get(span + 1)? <= t {
        span += 1;
    }
    let mut basis = vec![0.0_f64; degree + 1];
    let mut left = vec![0.0_f64; degree + 1];
    let mut right = vec![0.0_f64; degree + 1];
    *basis.first_mut()? = 1.0;
    for j in 1..=degree {
        *left.get_mut(j)? = t - knots.get(span + 1 - j)?;
        *right.get_mut(j)? = knots.get(span + j)? - t;
        let mut saved = 0.0;
        for r in 0..j {
            let denominator = right.get(r + 1)? + left.get(j - r)?;
            let term = if denominator == 0.0 {
                0.0
            } else {
                basis.get(r)? / denominator
            };
            *basis.get_mut(r)? = saved + right.get(r + 1)? * term;
            saved = left.get(j - r)? * term;
        }
        *basis.get_mut(j)? = saved;
    }
    let mut sum = [0.0_f64; 3];
    let mut total = 0.0;
    for (j, b) in basis.iter().enumerate() {
        let index = span - degree + j;
        let weighted = b * weights.get(index)?;
        let pole = points.get(index)?;
        for (s, p) in sum.iter_mut().zip(pole) {
            *s += weighted * p;
        }
        total += weighted;
    }
    (total > 0.0 && total.is_finite()).then(|| sum.map(|s| s / total))
}
