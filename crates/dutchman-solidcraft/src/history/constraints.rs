//! Sketch constraints and dimensions, as SolidCraft sketch constraints.
//!
//! Geometry is already exact (sketches are written at their solved positions), so constraints
//! carry design intent only: a constraint that does not map is dropped and reported, never a
//! reason to flatten. A dimension's value is measured from the solved geometry, so the solver
//! starts satisfied, and it is linked to its SolidCraft parameter where the source names one.

use std::collections::BTreeMap;

use dutchman_core::{Report, Severity};
use serde_json::{json, Value};

use super::sketch::{distance, number, Builder, Curve, Shape, P2};
use super::Context;

/// The constraints of sketch `sketch_id`, mapped onto `builder`'s points and curves. What does
/// not map is reported under `label`.
pub(crate) fn map(
    sketch_id: &str,
    label: &str,
    ctx: &Context,
    builder: &Builder,
    report: &mut Report,
) -> Vec<Value> {
    let mut out = Vec::new();
    let mut dropped: BTreeMap<String, usize> = BTreeMap::new();
    let mut total = 0usize;
    let mapper = Mapper { ctx, builder };
    for row in ctx
        .arena("sketch_constraints")
        .iter()
        .filter(|row| row.get("sketch").and_then(Value::as_str) == Some(sketch_id))
    {
        if row.get("active").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        total += 1;
        let definition = row.get("definition").unwrap_or(&Value::Null);
        match mapper.map(definition) {
            Ok(mapped) => {
                let driven = row.get("driving").and_then(Value::as_bool) == Some(false);
                for mut constraint in mapped {
                    if let Some(fields) = constraint.as_object_mut() {
                        fields.insert("id".to_owned(), json!(format!("k{}", out.len() + 1)));
                        if driven {
                            fields.insert("driven".to_owned(), json!(true));
                        }
                    }
                    out.push(constraint);
                }
            }
            Err(kind) => *dropped.entry(kind).or_default() += 1,
        }
    }
    if !dropped.is_empty() {
        let count: usize = dropped.values().sum();
        let kinds: Vec<String> = dropped
            .iter()
            .map(|(kind, n)| {
                if *n == 1 {
                    kind.clone()
                } else {
                    format!("{kind} ×{n}")
                }
            })
            .collect();
        report.push(
            Severity::Degraded,
            label,
            format!(
                "{count} of {total} constraints not carried ({}); its geometry is exact",
                kinds.join(", ")
            ),
        );
    }
    out
}

/// Maps one constraint definition onto the sketch's points and curves.
struct Mapper<'a> {
    ctx: &'a Context<'a>,
    builder: &'a Builder,
}

impl Mapper<'_> {
    /// The SolidCraft constraints for one CADIR constraint definition (none when it is already
    /// true by construction), or the kind that did not map.
    fn map(&self, definition: &Value) -> Result<Vec<Value>, String> {
        let kind = definition
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let field = |key: &str| definition.get(key).unwrap_or(&Value::Null);
        let unmapped = || match kind {
            "native" => format!(
                "native {}",
                definition
                    .get("native_kind")
                    .and_then(Value::as_str)
                    .unwrap_or("constraint")
            ),
            other => other.replace('_', " "),
        };
        let mapped = match kind {
            "coincident" => {
                let points = field("entities")
                    .as_array()
                    .map(|ids| {
                        ids.iter()
                            .map(|id| self.point(&json!({"kind": "entity", "entity": id})))
                            .collect::<Option<Vec<_>>>()
                    })
                    .unwrap_or_default();
                points.map(|points| coincident(&points))
            }
            "coincident_loci" => field("loci")
                .as_array()
                .and_then(|loci| {
                    loci.iter()
                        .map(|l| self.point(l))
                        .collect::<Option<Vec<_>>>()
                })
                .map(|points| coincident(&points)),
            "point_on_object" => {
                let p = self.point(field("point"));
                match (p, self.shape(field("entity"))) {
                    (Some(p), Some(Shape::Curve(c))) => Some(vec![
                        json!({"type": "point_on_curve", "p": p, "c": c.index}),
                    ]),
                    (Some(p), Some(Shape::Point(q))) => Some(coincident(&[p, *q])),
                    _ => None,
                }
            }
            "midpoint" => self
                .point(field("point"))
                .zip(self.curve(field("entity"), &["line"]))
                .map(|(p, l)| vec![json!({"type": "midpoint", "p": p, "l": l.index})]),
            "concentric" => self.pair("concentric", definition, &["circle", "arc"]),
            "collinear" => self.pair("collinear", definition, &["line"]),
            "parallel" => self.pair("parallel", definition, &["line"]),
            "perpendicular" => self.pair("perpendicular", definition, &["line"]),
            "tangent" => self.pair("tangent", definition, &["line", "circle", "arc"]),
            "equal" => self.equal(definition),
            "horizontal" | "vertical" => self
                .curve(field("entity"), &["line"])
                .map(|l| vec![json!({"type": kind, "l": l.index})]),
            "fixed" => match self.shape(field("entity")) {
                Some(Shape::Point(p)) => Some(vec![json!({"type": "fix", "p": p})]),
                Some(Shape::Curve(c)) if c.kind == "line" => c.start.zip(c.end).map(|(a, b)| {
                    vec![
                        json!({"type": "fix", "p": a}),
                        json!({"type": "fix", "p": b}),
                    ]
                }),
                _ => None,
            },
            "symmetric" => {
                let p = self.point(field("first"));
                let q = self.point(field("second"));
                let l = self.curve(field("axis"), &["line"]);
                match (p, q, l) {
                    (Some(p), Some(q), Some(l)) => Some(vec![
                        json!({"type": "symmetric", "p": p, "q": q, "l": l.index}),
                    ]),
                    _ => None,
                }
            }
            "distance_loci" | "horizontal_distance" | "vertical_distance" => {
                self.loci_distance(kind, definition)
            }
            "distance" => self.entity_distance(definition),
            "radius" | "diameter" => self.radius(kind, definition),
            _ => None,
        };
        mapped.ok_or_else(unmapped)
    }

    /// What the source entity `id` became.
    fn shape(&self, id: &Value) -> Option<&Shape> {
        self.builder.shapes.get(id.as_str()?)
    }

    /// The curve the source entity `id` became, if it is one of `kinds`.
    fn curve(&self, id: &Value, kinds: &[&str]) -> Option<&Curve> {
        match self.shape(id)? {
            Shape::Curve(curve) if kinds.contains(&curve.kind) => Some(curve),
            _ => None,
        }
    }

    /// The point a CADIR locus names: a point entity, or a curve's start, end or centre.
    fn point(&self, locus: &Value) -> Option<usize> {
        let shape = self.shape(locus.get("entity")?)?;
        match (locus.get("kind")?.as_str()?, shape) {
            ("entity", Shape::Point(p)) => Some(*p),
            ("start", Shape::Curve(c)) => c.start,
            ("end", Shape::Curve(c)) => c.end,
            ("center", Shape::Curve(c)) => c.center,
            _ => None,
        }
    }

    fn position(&self, point: usize) -> Option<P2> {
        self.builder.positions.get(point).copied()
    }

    /// A two-curve relation of SolidCraft type `kind` between `first` and `second`.
    fn pair(&self, kind: &str, definition: &Value, kinds: &[&str]) -> Option<Vec<Value>> {
        let a = self.curve(definition.get("first")?, kinds)?;
        let b = self.curve(definition.get("second")?, kinds)?;
        Some(vec![json!({"type": kind, "a": a.index, "b": b.index})])
    }

    fn equal(&self, definition: &Value) -> Option<Vec<Value>> {
        let a = self.curve(definition.get("first")?, &["line", "circle", "arc"])?;
        let b = self.curve(definition.get("second")?, &["line", "circle", "arc"])?;
        let round = |kind: &str| kind == "circle" || kind == "arc";
        (a.kind == b.kind || (round(a.kind) && round(b.kind)))
            .then(|| vec![json!({"type": "equal", "a": a.index, "b": b.index})])
    }

    /// The SolidCraft parameter a dimension's `parameter` drives, as a `param` field.
    fn param(&self, definition: &Value) -> Option<String> {
        self.ctx
            .params
            .get(definition.get("parameter")?.as_str()?)
            .cloned()
    }

    fn dimension(&self, mut constraint: Value, definition: &Value) -> Vec<Value> {
        if let (Some(param), Some(fields)) = (self.param(definition), constraint.as_object_mut()) {
            fields.insert("param".to_owned(), json!(param));
        }
        vec![constraint]
    }

    fn loci_distance(&self, kind: &str, definition: &Value) -> Option<Vec<Value>> {
        let first = definition.get("first")?;
        let second = definition.get("second")?;
        let (p, q) = (self.point(first), self.point(second));
        let constraint = match (kind, p, q) {
            ("distance_loci", Some(p), Some(q)) => {
                let value = distance(self.position(p)?, self.position(q)?);
                json!({"type": "distance", "p": p, "q": q, "value": value})
            }
            ("distance_loci", Some(p), None) | ("distance_loci", None, Some(p)) => {
                let line = if q.is_none() { second } else { first };
                let l = self.curve(line.get("entity")?, &["line"])?;
                let value = point_line(
                    self.position(p)?,
                    self.position(l.start?)?,
                    self.position(l.end?)?,
                )?;
                json!({"type": "point_line_distance", "p": p, "l": l.index, "value": value})
            }
            ("horizontal_distance", Some(p), Some(q)) => {
                let value = (self.position(q)?[0] - self.position(p)?[0]).abs();
                json!({"type": "distance_x", "p": p, "q": q, "value": value})
            }
            ("vertical_distance", Some(p), Some(q)) => {
                let value = (self.position(q)?[1] - self.position(p)?[1]).abs();
                json!({"type": "distance_y", "p": p, "q": q, "value": value})
            }
            _ => return None,
        };
        Some(self.dimension(constraint, definition))
    }

    fn entity_distance(&self, definition: &Value) -> Option<Vec<Value>> {
        let entities = definition.get("entities")?.as_array()?;
        let constraint = match entities.as_slice() {
            [line] => {
                let l = self.curve(line, &["line"])?;
                let value = distance(self.position(l.start?)?, self.position(l.end?)?);
                json!({"type": "length", "l": l.index, "value": value})
            }
            [first, second] => {
                let p = self.point(&json!({"kind": "entity", "entity": first}))?;
                let q = self.point(&json!({"kind": "entity", "entity": second}))?;
                let value = distance(self.position(p)?, self.position(q)?);
                json!({"type": "distance", "p": p, "q": q, "value": value})
            }
            _ => return None,
        };
        Some(self.dimension(constraint, definition))
    }

    fn radius(&self, kind: &str, definition: &Value) -> Option<Vec<Value>> {
        let c = self.curve(definition.get("entity")?, &["circle", "arc"])?;
        let radius = match c.kind {
            "circle" => number(self.builder_curve(c.index)?.get("r"))?,
            _ => distance(self.position(c.center?)?, self.position(c.start?)?),
        };
        let value = if kind == "diameter" {
            radius * 2.0
        } else {
            radius
        };
        Some(self.dimension(
            json!({"type": kind, "c": c.index, "value": value}),
            definition,
        ))
    }

    fn builder_curve(&self, index: usize) -> Option<&Value> {
        self.builder.curve_json(index)
    }
}

/// Coincidences joining every point to the first, leaving out points that are already one.
fn coincident(points: &[usize]) -> Vec<Value> {
    let Some((first, rest)) = points.split_first() else {
        return Vec::new();
    };
    rest.iter()
        .filter(|p| *p != first)
        .map(|q| json!({"type": "coincident", "p": first, "q": q}))
        .collect()
}

/// The distance from `p` to the line through `a` and `b`.
fn point_line(p: P2, a: P2, b: P2) -> Option<f64> {
    let length = distance(a, b);
    (length > 0.0)
        .then(|| ((b[0] - a[0]) * (a[1] - p[1]) - (a[0] - p[0]) * (b[1] - a[1])).abs() / length)
}

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const SKETCH: &str = "f3d:model:sketch#1";

    fn entity(key: &str, geometry: Value) -> Value {
        json!({"id": key, "sketch": SKETCH, "geometry": geometry})
    }

    fn line(key: &str, a: P2, b: P2) -> Value {
        entity(
            key,
            json!({"kind": "line",
            "start": {"u": a[0], "v": a[1]}, "end": {"u": b[0], "v": b[1]}}),
        )
    }

    /// A 40 × 30 rectangle (`b`, `r`, `t`, `l`), a circle `c` of radius 5 at (20, 15), a
    /// diagonal construction line `d` and a point `m` at the middle of the bottom edge.
    fn entities() -> Vec<Value> {
        vec![
            line("b", [0.0, 0.0], [40.0, 0.0]),
            line("r", [40.0, 0.0], [40.0, 30.0]),
            line("t", [40.0, 30.0], [0.0, 30.0]),
            line("l", [0.0, 30.0], [0.0, 0.0]),
            entity(
                "c",
                json!({"kind": "circle", "center": {"u": 20.0, "v": 15.0}, "radius": 5.0}),
            ),
            line("d", [0.0, 0.0], [40.0, 30.0]),
            entity(
                "m",
                json!({"kind": "point", "position": {"u": 20.0, "v": 0.0}}),
            ),
        ]
    }

    fn run(constraints: Vec<Value>) -> (Vec<Value>, Report) {
        let rows: Vec<Value> = constraints
            .into_iter()
            .enumerate()
            .map(|(i, definition)| {
                json!({"id": format!("f3d:model:sketch-constraint#{i}"), "sketch": SKETCH,
                       "definition": definition})
            })
            .collect();
        let model = json!({"sketch_constraints": rows});
        let mut ctx = Context::new(&model);
        ctx.params
            .insert("f3d:model:parameter#1".to_owned(), "width".to_owned());
        let mut builder = Builder::default();
        let mut report = Report::default();
        for e in entities() {
            builder.entity(&e, &mut report).unwrap();
        }
        let out = map(SKETCH, "sketch \"Base\"", &ctx, &builder, &mut report);
        (out, report)
    }

    fn locus(kind: &str, entity: &str) -> Value {
        json!({"kind": kind, "entity": entity})
    }

    #[test]
    fn geometric_relations_map_onto_curves_and_points() {
        let (out, report) = run(vec![
            json!({"kind": "horizontal", "entity": "b"}),
            json!({"kind": "vertical", "entity": "r"}),
            json!({"kind": "parallel", "first": "b", "second": "t"}),
            json!({"kind": "perpendicular", "first": "b", "second": "l"}),
            json!({"kind": "midpoint", "point": locus("entity", "m"), "entity": "b"}),
            json!({"kind": "point_on_object", "point": locus("center", "c"), "entity": "d"}),
            json!({"kind": "fixed", "entity": "m"}),
            json!({"kind": "coincident_loci", "loci": [locus("end", "b"), locus("start", "r")]}),
        ]);
        assert!(report.losses().is_empty(), "{:?}", report.losses());
        let types: Vec<&str> = out.iter().map(|c| c["type"].as_str().unwrap()).collect();
        assert_eq!(
            types,
            [
                "horizontal",
                "vertical",
                "parallel",
                "perpendicular",
                "midpoint",
                "point_on_curve",
                "fix"
            ]
        );
        // The shared corner is one point already: its coincidence adds nothing.
        assert_eq!(out[0], json!({"id": "k1", "type": "horizontal", "l": 0}));
        assert_eq!(out[5]["c"], 5);
    }

    #[test]
    fn dimensions_measure_the_geometry_and_link_their_parameter() {
        let (out, _) = run(vec![
            json!({"kind": "distance", "entities": ["b"], "parameter": "f3d:model:parameter#1"}),
            json!({"kind": "horizontal_distance", "first": locus("start", "b"),
                   "second": locus("center", "c"), "parameter": "f3d:model:parameter#9"}),
            json!({"kind": "distance_loci", "first": locus("center", "c"),
                   "second": locus("entity", "b"), "parameter": "f3d:model:parameter#9"}),
            json!({"kind": "diameter", "entity": "c", "parameter": "f3d:model:parameter#9"}),
        ]);
        assert_eq!(
            out[0],
            json!({"id": "k1", "type": "length", "l": 0, "value": 40.0, "param": "width"})
        );
        assert_eq!(out[1]["type"], "distance_x");
        assert_eq!(out[1]["value"], 20.0);
        assert!(
            out[1].get("param").is_none(),
            "an unmapped parameter is not linked"
        );
        assert_eq!(out[2]["type"], "point_line_distance");
        assert_eq!(out[2]["value"], 15.0);
        assert_eq!(
            out[3],
            json!({"id": "k4", "type": "diameter", "c": 4, "value": 10.0})
        );
    }

    #[test]
    fn unmapped_constraints_are_dropped_and_counted() {
        let (out, report) = run(vec![
            json!({"kind": "native", "native_kind": "Linear Dimension-1", "entities": []}),
            json!({"kind": "native", "native_kind": "Linear Dimension-1", "entities": []}),
            json!({"kind": "angle", "first": "b", "second": "d", "parameter": "x"}),
            json!({"kind": "horizontal", "entity": "c"}),
            json!({"kind": "vertical", "entity": "l"}),
        ]);
        assert_eq!(out.len(), 1);
        let loss = report.losses().first().unwrap();
        assert_eq!(loss.severity, Severity::Degraded);
        assert_eq!(loss.subject, "sketch \"Base\"");
        assert_eq!(
            loss.detail,
            "4 of 5 constraints not carried (angle, horizontal, native Linear Dimension-1 ×2); its geometry is exact"
        );
    }

    #[test]
    fn reference_dimensions_stay_reference() {
        let rows = vec![json!({"kind": "radius", "entity": "c", "parameter": "p"})];
        let (out, _) = run(rows);
        assert!(out[0].get("driven").is_none());
    }
}
