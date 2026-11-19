//! Neutral spatial relations supported by SolidCraft 0.44's wire solver.

use std::collections::BTreeMap;

use dutchman_core::{Report, Severity};
use serde_json::{json, Value};

use super::super::sketch::{cross, dot, number, unit, v3, COINCIDENT, PARALLEL, V3};
use super::{Builder, Context};

/// Read a SolidCraft wire coordinate (CADIR coordinates use named fields instead).
fn position(value: &Value) -> Option<V3> {
    let values = value.as_array()?;
    Some([
        number(values.first())?,
        number(values.get(1))?,
        number(values.get(2))?,
    ])
}

fn separation(a: V3, b: V3) -> f64 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// SolidCraft 0.44 joins drawn wire ends within this distance (wire_solve::MEET, mm).
const JOIN: f64 = 1e-9;
/// SolidCraft 0.44's hard ceiling is twice its 200,000-point ceiling.
pub(super) const MAX_CONSTRAINTS: usize = 400_000;

fn parallel(a: V3, b: V3) -> bool {
    let product = cross(a, b);
    dot(product, product).sqrt() <= PARALLEL
}

struct Mapper<'a, 'model> {
    builder: &'a Builder,
    ctx: &'a Context<'model>,
}

impl Mapper<'_, '_> {
    fn wire(&self, entity: &Value) -> Option<&Value> {
        let [id] = self.builder.shapes.get(entity.as_str()?)?.as_slice() else {
            return None;
        };
        self.builder.wire_by_id(id)
    }

    fn line(&self, entity: &Value) -> Option<&Value> {
        self.wire(entity).filter(|w| {
            w.get("arc").is_none()
                && w.get("fit")
                    .and_then(Value::as_array)
                    .is_some_and(|p| p.len() == 2)
        })
    }

    fn point(&self, entity: &Value) -> Option<Value> {
        let wire = self.wire(entity)?;
        (wire.get("fit")?.as_array()?.len() == 1)
            .then(|| json!({"wire": wire.get("id"), "index": 0}))
    }

    fn at(&self, point: &Value) -> Option<V3> {
        let wire = self.builder.wire_by_id(point.get("wire")?.as_str()?)?;
        position(
            wire.get("fit")?
                .as_array()?
                .get(usize::try_from(point.get("index")?.as_u64()?).ok()?)?,
        )
    }

    fn distance(&self, a: Value, b: Value, parameter: &Value) -> Option<Value> {
        let value = separation(self.at(&a)?, self.at(&b)?);
        // SolidCraft's published wire dimension range, in mm.
        if !(1e-6..=1e6).contains(&value) {
            return None;
        }
        let parameter = parameter.as_str()?;
        if self
            .ctx
            .row("parameters", parameter)
            .and_then(|p| number(p.pointer("/value/value")))
            .is_some_and(|v| (v - value).abs() > COINCIDENT)
        {
            return None;
        }
        let param = self.ctx.params.get(parameter)?;
        Some(json!({"type": "distance", "a": a, "b": b, "value": value, "param": param}))
    }

    fn length(&self, entity: &Value, parameter: &Value) -> Option<Value> {
        let wire = self.line(entity)?;
        self.distance(
            json!({"wire": wire.get("id"), "index": 0}),
            json!({"wire": wire.get("id"), "index": 1}),
            parameter,
        )
    }

    fn tangent(&self, first: &Value, second: &Value) -> Option<Vec<Value>> {
        if let Some((a, b)) = self.line(first).zip(self.line(second)) {
            let af = a.get("fit")?.as_array()?;
            let bf = b.get("fit")?.as_array()?;
            let direction = |fit: &[Value]| -> Option<V3> {
                let (a, b) = (position(fit.first()?)?, position(fit.get(1)?)?);
                unit(std::array::from_fn(|i| {
                    b.get(i).copied().unwrap_or(0.0) - a.get(i).copied().unwrap_or(0.0)
                }))
            };
            if !parallel(direction(af)?, direction(bf)?) {
                return None;
            }
            for (i, p) in af.iter().enumerate() {
                for (j, q) in bf.iter().enumerate() {
                    if separation(position(p)?, position(q)?) <= JOIN {
                        return Some(vec![
                            json!({"type":"collinear","a":a.get("id"),"b":b.get("id")}),
                            json!({"type":"coincident","a":{"wire":a.get("id"),"index":i},"b":{"wire":b.get("id"),"index":j}}),
                        ]);
                    }
                }
            }
            return None;
        }
        let (line, arc) = self
            .line(first)
            .zip(self.wire(second))
            .or_else(|| self.line(second).zip(self.wire(first)))?;
        let fits = arc.get("fit")?.as_array()?;
        let normal = position(arc.get("arc")?.get("normal")?)?;
        let center = position(arc.get("arc")?.get("center")?)?;
        let ends = line.get("fit")?.as_array()?;
        let a = position(ends.first()?)?;
        let b = position(ends.get(1)?)?;
        let along = unit(std::array::from_fn(|i| {
            b.get(i).copied().unwrap_or(0.0) - a.get(i).copied().unwrap_or(0.0)
        }))?;
        for (line_end, end) in ends.iter().enumerate() {
            let p = position(end)?;
            for arc_end in [0, 2] {
                let q = position(fits.get(arc_end)?)?;
                let radius = std::array::from_fn(|i| {
                    q.get(i).copied().unwrap_or(0.0) - center.get(i).copied().unwrap_or(0.0)
                });
                let tangent = unit(cross(normal, radius))?;
                // Only an already satisfied endpoint tangency can be carried without moving geometry.
                let line_sign = if line_end == 0 { 1.0 } else { -1.0 };
                let arc_sign = if arc_end == 0 { -1.0 } else { 1.0 };
                if separation(p, q) <= JOIN
                    && parallel(along, tangent)
                    && dot(along, tangent) * line_sign * arc_sign > 0.0
                {
                    return Some(vec![
                        json!({"type": "tangent", "line": line.get("id"), "arc": arc.get("id"), "line_end": line_end, "arc_end": arc_end}),
                    ]);
                }
            }
        }
        None
    }

    #[cfg(test)]
    fn map(&self, definition: &Value) -> Option<Vec<Value>> {
        self.limited(definition, MAX_CONSTRAINTS)
    }

    fn limited(&self, definition: &Value, remaining: usize) -> Option<Vec<Value>> {
        let field = |key: &str| definition.get(key).unwrap_or(&Value::Null);
        match field("kind").as_str()? {
            "coincident" => {
                let (a, b) = (self.point(field("first"))?, self.point(field("second"))?);
                (separation(self.at(&a)?, self.at(&b)?) <= COINCIDENT)
                    .then(|| vec![json!({"type":"coincident","a":a,"b":b})])
            }
            "point_distance" => Some(vec![self.distance(
                self.point(field("first"))?,
                self.point(field("second"))?,
                field("parameter"),
            )?]),
            "line_length" => Some(vec![self.length(field("entity"), field("parameter"))?]),
            "repeated_line_length" => field("entities")
                .as_array()?
                .iter()
                .take(remaining)
                .map(|e| self.length(e, field("parameter")))
                .collect(),
            "parallel_to_direction" => {
                let line = self.line(field("entity"))?;
                let direction = unit(v3(Some(field("direction")))?)?;
                let fit = line.get("fit")?.as_array()?;
                let (a, b) = (position(fit.first()?)?, position(fit.get(1)?)?);
                let along = unit(std::array::from_fn(|i| {
                    b.get(i).copied().unwrap_or(0.0) - a.get(i).copied().unwrap_or(0.0)
                }))?;
                if !parallel(along, direction) {
                    return None;
                }
                let axis = [
                    ([1.0, 0.0, 0.0], "x"),
                    ([0.0, 1.0, 0.0], "y"),
                    ([0.0, 0.0, 1.0], "z"),
                ]
                .into_iter()
                .find(|(axis, _)| parallel(direction, *axis))?
                .1;
                Some(vec![
                    json!({"type": "along_axis", "line": line.get("id"), "axis": axis}),
                ])
            }
            "tangent" => self.tangent(field("first"), field("second")),
            _ => None,
        }
    }
}

/// Carry decoded neutral constraints; report relations with no target form.
pub(super) fn map(
    sketch: &str,
    label: &str,
    builder: &Builder,
    ctx: &Context,
    report: &mut Report,
) -> Vec<Value> {
    map_limited(sketch, label, builder, ctx, report, MAX_CONSTRAINTS)
}

fn map_limited(
    sketch: &str,
    label: &str,
    builder: &Builder,
    ctx: &Context,
    report: &mut Report,
    limit: usize,
) -> Vec<Value> {
    let mapper = Mapper { builder, ctx };
    let mut out = Vec::new();
    let mut capped = false;
    let mut dropped: BTreeMap<String, usize> = BTreeMap::new();
    for row in ctx
        .arena("spatial_sketch_constraints")
        .iter()
        .filter(|r| r.get("sketch").and_then(Value::as_str) == Some(sketch))
    {
        let remaining = limit.saturating_sub(out.len());
        if remaining == 0 {
            capped = true;
            continue;
        }
        let definition = row.get("definition").unwrap_or(&Value::Null);
        if let Some(mapped) = mapper.limited(definition, remaining) {
            capped |= mapped.len() > remaining
                || (definition.get("kind").and_then(Value::as_str) == Some("repeated_line_length")
                    && definition
                        .get("entities")
                        .and_then(Value::as_array)
                        .is_some_and(|entities| entities.len() > remaining));
            out.extend(mapped.into_iter().take(remaining));
        } else {
            let kind = definition
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .replace('_', " ");
            *dropped.entry(kind).or_default() += 1;
        }
    }
    if capped {
        report.push(Severity::Info, label, format!("Additional 3D sketch constraints not carried: SolidCraft accepts at most {limit} constraints in one sketch; its solved geometry is retained"));
    }
    for (index, c) in out.iter_mut().enumerate() {
        if let Some(fields) = c.as_object_mut() {
            fields.insert("id".to_owned(), json!(format!("k{}", index + 1)));
        }
    }
    for (kind, count) in dropped {
        let noun = if count == 1 {
            "constraint"
        } else {
            "constraints"
        };
        report.push(Severity::Info, label, format!("{count} 3D sketch {noun} not carried ({kind}): mapping this relation to SolidCraft is not supported yet; its solved geometry is retained"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn builder() -> Builder {
        let mut b = Builder::default();
        b.point("p", [0.0, 0.0, 0.0]);
        b.point("q", [3.0, 4.0, 0.0]);
        b.wire(
            "l",
            &[[0.0, 0.0, 0.0], [3.0, 4.0, 0.0]],
            &[[0.0, 0.0, 0.0], [3.0, 4.0, 0.0]],
            None,
        );
        b
    }

    #[test]
    fn large_sketch_resolves_dimensions_through_the_wire_index() {
        let source = json!({});
        let mut ctx = Context::testing(&source);
        ctx.params.insert("p".to_owned(), "length".to_owned());
        let mut b = Builder::default();
        b.point("point", [0.0, 0.0, 0.0]);
        for i in 0..100_000 {
            let x = f64::from(i) * 10.0;
            b.wire(
                &format!("line{i}"),
                &[[x, 0.0, 0.0], [x, 3.0, 4.0]],
                &[],
                None,
            );
        }
        // Check the stored lookup work, including mixed point/line IDs, without a timer.
        // Each ID resolves to one vector slot rather than walking the preceding wires.
        assert_eq!(b.wire_indices.len(), 100_001);
        for (index, wire) in b.wires.iter().enumerate() {
            assert_eq!(
                b.wire_indices.get(wire["id"].as_str().unwrap()),
                Some(&index)
            );
        }
        let mapper = Mapper {
            builder: &b,
            ctx: &ctx,
        };
        for i in (0..100_000).rev() {
            let dimension = mapper
                .length(&json!(format!("line{i}")), &json!("p"))
                .unwrap();
            assert_eq!(dimension["value"], 5.0);
            assert_eq!(dimension["a"]["wire"], format!("w{}", i + 1));
            // Fail on the first unindexed relation, before a scan regression can run long.
            assert_eq!(b.indexed_lookups.get(), 3 * (100_000 - i) as usize);
        }
        // Every dimension uses exactly three indexed lookups: line and two endpoints.
        assert_eq!(b.indexed_lookups.get(), 300_000);
        assert!(b.wire_by_id("missing").is_none());
        assert_eq!(mapper.at(&json!({"wire":"q1","index":0})), Some([0.0; 3]));
    }

    #[test]
    fn neutral_dimensions_measure_in_three_dimensions_and_keep_the_parameter() {
        let wire = json!({});
        let mut ctx = Context::testing(&wire);
        ctx.params
            .insert("parameter".to_owned(), "length".to_owned());
        let b = builder();
        let m = Mapper {
            builder: &b,
            ctx: &ctx,
        };
        for definition in [
            json!({"kind": "point_distance", "first": "p", "second": "q", "parameter": "parameter"}),
            json!({"kind": "line_length", "entity": "l", "parameter": "parameter"}),
            json!({"kind": "repeated_line_length", "entities": ["l"], "parameter": "parameter"}),
        ] {
            let out = m.map(&definition).unwrap();
            assert_eq!(out[0]["value"], 5.0);
            assert_eq!(out[0]["param"], "length");
        }
        assert!(m
            .map(&json!({"kind": "line_length", "entity": "l", "parameter": "missing"}))
            .is_none());
        assert!(m
            .map(&json!({"kind": "line_length", "entity": "p", "parameter": "parameter"}))
            .is_none());
    }

    #[test]
    fn repeated_dimensions_respect_the_constraint_budget() {
        let wire = json!({"spatial_sketch_constraints":[
            {"sketch":"s","definition":{"kind":"repeated_line_length","entities":["l","m","n"],"parameter":"parameter"}},
            {"sketch":"s","definition":{"kind":"point_distance","first":"p","second":"q","parameter":"parameter"}}
        ]});
        let mut ctx = Context::testing(&wire);
        ctx.params
            .insert("parameter".to_owned(), "length".to_owned());
        let mut b = builder();
        b.wire("m", &[[10.0, 0.0, 0.0], [13.0, 4.0, 0.0]], &[], None);
        b.wire("n", &[[20.0, 0.0, 0.0], [23.0, 4.0, 0.0]], &[], None);
        let mut report = Report::default();
        let out = map_limited("s", "sketch", &b, &ctx, &mut report, 2);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0]["a"]["wire"], "w1");
        assert_eq!(out[1]["a"]["wire"], "w2");
        assert!(out
            .iter()
            .all(|c| c["value"] == 5.0 && c["param"] == "length"));
        assert!(report
            .losses()
            .iter()
            .any(|l| l.detail.contains("at most 2 constraints")));
    }

    #[test]
    fn endpoint_tangency_keeps_continuation_and_refuses_foldback() {
        let wire = json!({});
        let ctx = Context::testing(&wire);
        for folded in [false, true] {
            let mut b = Builder::default();
            let fit = if folded {
                [[0.0, 0.0, 0.0], [0.0, 0.0, 5.0]]
            } else {
                [[0.0, 0.0, -5.0], [0.0, 0.0, 0.0]]
            };
            b.wire("line", &fit, &fit, None);
            super::super::entity_geometry("arc",&json!({"kind":"arc","center":{"x":2.0,"y":0.0,"z":0.0},"normal":{"x":0.0,"y":1.0,"z":0.0},"reference_direction":{"x":-1.0,"y":0.0,"z":0.0},"radius":2.0,"start_angle":0.0,"end_angle":std::f64::consts::FRAC_PI_2}),&mut b).unwrap();
            let m = Mapper {
                builder: &b,
                ctx: &ctx,
            };
            assert_eq!(
                m.map(&json!({"kind":"tangent","first":"line","second":"arc"}))
                    .is_some(),
                !folded
            );
        }
    }

    #[test]
    fn tangent_lines_keep_their_shared_end_and_common_carrier() {
        let wire = json!({});
        let ctx = Context::testing(&wire);
        let mut b = Builder::default();
        b.wire("a", &[[0.0, 0.0, 0.0], [0.0, 0.0, 10.0]], &[], None);
        b.wire("b", &[[0.0, 0.0, 10.0], [0.0, 0.0, 30.0]], &[], None);
        let m = Mapper {
            builder: &b,
            ctx: &ctx,
        };
        let mapped = m
            .map(&json!({"kind":"tangent","first":"a","second":"b"}))
            .unwrap();
        assert_eq!(mapped[0], json!({"type":"collinear","a":"w1","b":"w2"}));
        assert_eq!(
            mapped[1],
            json!({"type":"coincident","a":{"wire":"w1","index":1},"b":{"wire":"w2","index":0}})
        );
    }

    #[test]
    fn axes_keep_both_signs_but_do_not_guess_a_nonprincipal_direction() {
        let wire = json!({});
        let ctx = Context::testing(&wire);
        let b = builder();
        let m = Mapper {
            builder: &b,
            ctx: &ctx,
        };
        for (axis, direction) in [
            ("x", json!({"x": -1.0,"y":0.0,"z":0.0})),
            ("y", json!({"x":0.0,"y":1.0,"z":0.0})),
            ("z", json!({"x":0.0,"y":0.0,"z":1.0})),
        ] {
            let mut axis_builder = Builder::default();
            let endpoint = v3(Some(&direction)).unwrap();
            axis_builder.wire("l", &[[0.0, 0.0, 0.0], endpoint], &[], None);
            let axis_mapper = Mapper {
                builder: &axis_builder,
                ctx: &ctx,
            };
            assert_eq!(
                axis_mapper
                    .map(
                        &json!({"kind":"parallel_to_direction","entity":"l","direction":direction})
                    )
                    .unwrap()[0]["axis"],
                axis
            );
        }
        assert!(m.map(&json!({"kind":"parallel_to_direction","entity":"l","direction":{"x":1.0,"y":1.0,"z":0.0}})).is_none());
    }

    #[test]
    fn spatial_decoded_point_coincidence_is_carried() {
        let source = json!({"spatial_sketch_constraints":[
            {"sketch":"s","definition":{"kind":"coincident","first":"p","second":"q"}}
        ]});
        let ctx = Context::testing(&source);
        let mut b = Builder::default();
        b.point("p", [0.0, 0.0, 0.0]);
        b.point("q", [0.0, 0.0, 0.0]);
        let out = map("s", "sketch", &b, &ctx, &mut Report::default());
        assert_eq!(
            out,
            vec![json!({"id":"k1","type":"coincident",
            "a":{"wire":"q1","index":0},"b":{"wire":"q2","index":0}})]
        );
    }

    #[test]
    fn spatial_meeting_endpoints_do_not_invent_relations() {
        let source = json!({});
        let ctx = Context::testing(&source);
        let mut b = builder();
        b.wire("next", &[[3.0, 4.0, 0.0], [6.0, 7.0, 8.0]], &[], None);
        let mut report = Report::default();
        assert!(map("s", "sketch", &b, &ctx, &mut report).is_empty());
        assert!(report.losses().is_empty());
    }

    #[test]
    fn unsupported_neutral_relations_are_reported_without_guessing_native_operands() {
        let wire = json!({"spatial_sketch_constraints":[{"sketch":"s","definition":{"kind":"native","native_kind":"Fixed","operands":[]}}]});
        let ctx = Context::testing(&wire);
        let mut report = Report::default();
        assert!(map("s", "sketch", &Builder::default(), &ctx, &mut report).is_empty());
        assert_eq!(report.losses().len(), 1);
        assert!(report.losses()[0].detail.contains("not supported yet"));
    }
}
