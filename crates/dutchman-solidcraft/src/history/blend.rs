//! Blends picked exclusively from their own historical input-edge geometry, on the body
//! SolidCraft rebuilds from the history before the blend, never on the final body.
//!
//! Mapping keeps each selected input edge ([`Pending`]); once SolidCraft has replayed the
//! mapped history ([`crate::verify::replay_edges`]), [`prove`] finds it among the edges of the
//! bodies the blend starts from. SolidCraft reports each rebuilt edge's ends, the point
//! halfway along it and its length, but no carrier or tangent, so the rule is:
//!
//! - The input edge must be a straight line (its midpoint on the segment between its ends and
//!   its tangent along that segment) or a circular arc (its tangent square to the radius of
//!   the circle through its ends and midpoint, in that circle's plane). Other curves and
//!   closed edges are refused.
//! - Its midpoint must lie on exactly one rebuilt edge across every body: within the segment
//!   of a straight one (a length equal to its chord proves it straight), or at the midpoint of
//!   a curved one. Anything else, including two rebuilt edges through it, is refused.
//! - That edge must have the input edge's ends, in either order, and its length: the chord of
//!   a line, `radius × angle` of an arc. A straight rebuilt edge then lies along the input
//!   tangent; a curved one is not checked for a tangent SolidCraft does not report.

use dutchman_core::{Report, Severity};
use serde_json::{json, Map, Value};

use super::sketch::{cross, dot, number, unit, v3, V3};
use super::{describe, name, Context};
use crate::verify::{ReplayedBodies, ReplayedEdge};

const LIMIT: usize = 256;
/// How far apart, in millimetres, two positions may be and still be the same.
const TOLERANCE: f64 = 1e-6;
/// How far from square or parallel two unit directions may be.
const ANGLE: f64 = 1e-6;
const INPUT: &str =
    "its input edge midpoint cannot be proven to select exactly one rebuilt edge yet";

type Segment = (V3, V3);

/// A blend whose input edges wait for SolidCraft's replay of the history before it.
#[derive(Clone, Debug)]
pub(crate) struct Pending {
    /// The blend's index in the mapped features.
    pub(crate) feature: usize,
    /// Its source feature's index in the mapping's outcomes.
    pub(crate) outcome: usize,
    /// Its source feature, as the report names it.
    pub(crate) subject: String,
    edges: Vec<Input>,
    /// A 45-degree chamfer, written as an equal-distance one.
    angle_chamfer: bool,
}

/// One selected input edge, from its feature input topology.
#[derive(Clone, Debug)]
struct Input {
    /// The point at the middle of its carrier's parameter interval.
    point: V3,
    ends: Segment,
    tangent: V3,
    /// Its length: the chord of a line, or the arc's.
    length: f64,
    straight: bool,
}

pub(super) fn map(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    report: &mut Report,
) -> Result<Value, String> {
    let kind = definition
        .get("definition")
        .and_then(Value::as_str)
        .ok_or("its blend kind is not resolved")?;
    let groups = definition
        .get("groups")
        .and_then(Value::as_array)
        .filter(|groups| groups.len() == 1)
        .ok_or("blends with several independently dimensioned groups are not supported yet")?;
    let group = groups.first().ok_or("its edge group is missing")?;
    let mut out = Map::new();
    out.insert("type".to_owned(), json!(kind));
    out.insert("suppressed".to_owned(), json!(false));
    // A tangency weight of exactly 1 blends with a circular cross-section: the fillet's faces
    // are circular cylinders and tori, as an unweighted fillet's are.
    let weighted = match group.get("tangency_weight").filter(|v| !v.is_null()) {
        None => false,
        Some(weight) if kind == "fillet" && number(Some(weight)) == Some(1.0) => true,
        Some(_) if kind == "fillet" => {
            return Err("fillets with a tangency weight are not supported yet".to_owned())
        }
        Some(_) => false,
    };
    if kind == "fillet" {
        let law = group.get("radius").ok_or("its radius is not given")?;
        match law.get("kind").and_then(Value::as_str) {
            Some("constant") => {
                out.insert("radius".to_owned(), json!(length(law.get("radius"))?));
                out.insert("style".to_owned(), json!({"type": "constant"}));
            }
            Some("variable") => {
                return Err(
                    "variable fillets need a proven chain direction, which is not supported yet"
                        .to_owned(),
                );
            }
            _ => {
                return Err("fillets other than constant-radius are not supported yet".to_owned());
            }
        }
    } else {
        let spec = group
            .get("spec")
            .ok_or("its chamfer dimensions are not given")?;
        let distance = match spec.get("kind").and_then(Value::as_str) {
            Some("distance") => spec.get("distance"),
            Some("two_distances") if number(spec.get("first")) == number(spec.get("second")) => {
                spec.get("first")
            }
            Some("distance_angle")
                if number(spec.get("angle"))
                    .is_some_and(|a| (a - std::f64::consts::FRAC_PI_4).abs() <= 1e-12) =>
            {
                spec.get("distance")
            }
            Some("two_distances" | "distance_angle") => {
                return Err("asymmetric chamfers need an input reference face ordering, which is not supported yet".to_owned());
            }
            _ => return Err("its chamfer dimensions are not resolved".to_owned()),
        };
        out.insert("distance".to_owned(), json!(length(distance)?));
    }
    let selection = group.get("edges").ok_or("its input edges are not given")?;
    match selection.get("kind").and_then(Value::as_str) {
        Some("historical") => {}
        Some("native" | "unresolved") => {
            return Err("its edges are a selection cadmpeg does not resolve to edges yet".to_owned())
        }
        Some("historical_partial") => {
            return Err("some of its edges are not resolved in its input topology".to_owned())
        }
        _ => return Err("blends require edges from their own feature input topology; edges of the final body are not supported yet".to_owned()),
    }
    let state_id = selection
        .pointer("/value/state")
        .and_then(Value::as_str)
        .ok_or("its input topology is not resolved")?;
    let states = ctx.arena("feature_input_topologies");
    if states.len() > 10_000 {
        return Err("its input topology exceeds the blend resolution limit".to_owned());
    }
    let mut states = states
        .iter()
        .filter(|state| state.get("id").and_then(Value::as_str) == Some(state_id));
    let state = states.next().ok_or("its input topology is missing")?;
    if states.next().is_some() || state.get("input_of") != feature.get("id") {
        return Err("its input topology does not uniquely belong to this feature".to_owned());
    }
    let ids = selection
        .pointer("/value/edges")
        .and_then(Value::as_array)
        .filter(|ids| !ids.is_empty() && ids.len() <= LIMIT)
        .ok_or("its input edge selection is empty or exceeds the blend limit")?;
    let members = state
        .get("edges")
        .and_then(Value::as_array)
        .ok_or("its input edges are missing")?;
    let geometry = state
        .get("edge_geometry")
        .and_then(Value::as_array)
        .ok_or("its input edge geometry is missing")?;
    if members.len() > 10_000 || geometry.len() > 10_000 {
        return Err("its input topology exceeds the blend resolution limit".to_owned());
    }
    let angle_chamfer =
        group.pointer("/spec/kind").and_then(Value::as_str) == Some("distance_angle");
    let mut inputs = Vec::new();
    let mut points = Vec::new();
    for edge in ids {
        if !edge.is_string() || members.iter().filter(|member| *member == edge).count() != 1 {
            return Err(
                "its selected edge is not a unique member of its input topology".to_owned(),
            );
        }
        let mut samples = geometry
            .iter()
            .filter(|sample| sample.get("edge") == Some(edge));
        let sample = samples
            .next()
            .ok_or("its selected input edge has no geometry")?;
        if samples.next().is_some() {
            return Err("its selected input edge has repeated geometry".to_owned());
        }
        let point = v3(sample.get("interior")).ok_or("its input edge midpoint is missing")?;
        let ends = (
            v3(sample.get("start")).ok_or("its input edge start is missing")?,
            v3(sample.get("end")).ok_or("its input edge end is missing")?,
        );
        let tangent = v3(sample.get("tangent"))
            .and_then(unit)
            .ok_or("its input edge tangent is missing")?;
        if near(ends.0, ends.1) {
            return Err("blends on closed input edges are not supported yet".to_owned());
        }
        if near(point, ends.0) || near(point, ends.1) {
            return Err(INPUT.to_owned());
        }
        let input = if on_segment(point, ends) {
            if !parallel(sub(ends.1, ends.0), tangent) {
                return Err(INPUT.to_owned());
            }
            Input {
                point,
                ends,
                tangent,
                length: distance(ends.0, ends.1),
                straight: true,
            }
        } else {
            let length = arc_length(point, ends, tangent).ok_or(
                "blends on curved input edges other than circular arcs are not supported yet",
            )?;
            Input {
                point,
                ends,
                tangent,
                length,
                straight: false,
            }
        };
        if angle_chamfer && !input.straight {
            return Err("distance-and-angle chamfers on non-perpendicular input faces are not supported yet".to_owned());
        }
        inputs.push(input);
        points.push(point);
    }
    out.insert("edges".to_owned(), json!(points));
    let id = ctx.id();
    out.insert("id".to_owned(), json!(id));
    out.insert(
        "name".to_owned(),
        json!(name(feature)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Blend{id}"))),
    );
    ctx.blends.push(Pending {
        feature: 0,
        outcome: 0,
        subject: String::new(),
        edges: inputs,
        angle_chamfer,
    });
    if weighted {
        report.push(
            Severity::Info,
            describe(feature),
            "tangency weight 1.0 treated as a circular fillet",
        );
    }
    Ok(Value::Object(out))
}

/// The body the blend's input edges lie on, among `bodies`: what SolidCraft rebuilt from the
/// history before it. Each input edge must select exactly one rebuilt edge (see the module
/// note), and all of them one body.
pub(crate) fn prove(pending: &Pending, bodies: &ReplayedBodies) -> Result<String, String> {
    let mut chosen: Vec<(usize, usize)> = Vec::new();
    for input in &pending.edges {
        let mut on = Vec::new();
        for (b, (_, edges)) in bodies.iter().enumerate() {
            for (e, edge) in edges.iter().enumerate() {
                let hit = if straight(edge) {
                    on_segment(input.point, (edge.start, edge.end))
                } else {
                    near(input.point, edge.mid)
                };
                if hit {
                    on.push((b, e));
                }
            }
        }
        let [(b, e)] = on.as_slice() else {
            return Err(INPUT.to_owned());
        };
        let edges = bodies
            .get(*b)
            .map(|(_, edges)| edges.as_slice())
            .ok_or(INPUT)?;
        let edge = edges.get(*e).ok_or(INPUT)?;
        if !same(input.ends, (edge.start, edge.end))
            || !same_length(edge.length, input.length)
            || straight(edge) != input.straight
            || (input.straight && !parallel(sub(edge.end, edge.start), input.tangent))
        {
            return Err(INPUT.to_owned());
        }
        // A 45-degree chamfer has equal setbacks only at a right-angle corner.
        if pending.angle_chamfer && !right_angle(edge, edges) {
            return Err("distance-and-angle chamfers on non-perpendicular input faces are not supported yet".to_owned());
        }
        if chosen.contains(&(*b, *e)) {
            return Err("its input edge selection repeats a rebuilt edge".to_owned());
        }
        chosen.push((*b, *e));
    }
    let body = chosen.first().map(|(b, _)| *b).ok_or(INPUT)?;
    if chosen.iter().any(|(b, _)| *b != body) {
        return Err(
            "its input edges lie on more than one rebuilt body; blends across bodies are not supported yet"
                .to_owned(),
        );
    }
    bodies
        .get(body)
        .map(|(name, _)| name.clone())
        .ok_or_else(|| INPUT.to_owned())
}

/// Whether the faces either side of a straight `edge` meet square, judged at both its ends by
/// the two other edges there, which must be straight: the corner of a prism.
fn right_angle(edge: &ReplayedEdge, edges: &[ReplayedEdge]) -> bool {
    let Some(along) = unit(sub(edge.end, edge.start)) else {
        return false;
    };
    [edge.start, edge.end].into_iter().all(|vertex| {
        let mut directions = Vec::new();
        for other in edges {
            if same((other.start, other.end), (edge.start, edge.end)) {
                continue;
            }
            let far = if near(vertex, other.start) {
                other.end
            } else if near(vertex, other.end) {
                other.start
            } else {
                continue;
            };
            if !straight(other) {
                return false;
            }
            let d = sub(far, vertex);
            let across = sub(d, scale(along, dot(d, along)));
            match unit(across) {
                Some(across) => directions.push(across),
                None => return false,
            }
        }
        match directions.as_slice() {
            [a, b] => dot(*a, *b).abs() <= ANGLE,
            _ => false,
        }
    })
}

/// The length of the circular arc from `ends.0` through `point` to `ends.1`, when `tangent`
/// is the tangent at `point` of the circle through the three; `None` for any other curve.
fn arc_length(point: V3, ends: Segment, tangent: V3) -> Option<f64> {
    let a = sub(ends.0, point);
    let b = sub(ends.1, point);
    let normal = cross(a, b);
    let n2 = dot(normal, normal);
    if !n2.is_finite() || n2 <= TOLERANCE * TOLERANCE * dot(a, a) * dot(b, b) {
        return None;
    }
    let towards = sub(scale(b, dot(a, a)), scale(a, dot(b, b)));
    let offset = scale(cross(towards, normal), 1.0 / (2.0 * n2));
    let center = add(point, offset);
    let radius = distance(point, center);
    let out = unit(sub(point, center))?;
    if dot(out, tangent).abs() > ANGLE || dot(unit(normal)?, tangent).abs() > ANGLE {
        return None;
    }
    let angle = |p: V3, q: V3| {
        let (u, v) = (sub(p, center), sub(q, center));
        let c = cross(u, v);
        dot(c, c).sqrt().atan2(dot(u, v))
    };
    let length = radius * (angle(ends.0, point) + angle(point, ends.1));
    (length.is_finite() && length > 0.0).then_some(length)
}

/// Whether SolidCraft's edge is straight: as long as its chord.
fn straight(edge: &ReplayedEdge) -> bool {
    let chord = distance(edge.start, edge.end);
    chord > TOLERANCE && same_length(edge.length, chord)
}

fn same_length(a: f64, b: f64) -> bool {
    (a - b).abs() <= TOLERANCE * a.abs().max(b.abs()).max(1.0)
}

fn parallel(direction: V3, tangent: V3) -> bool {
    unit(direction).is_some_and(|direction| (dot(direction, tangent).abs() - 1.0).abs() <= ANGLE)
}

fn length(value: Option<&Value>) -> Result<String, String> {
    number(value)
        .filter(|v| *v > 0.0)
        .map(|v| format!("{v} mm"))
        .ok_or_else(|| "its blend dimension is not a positive finite length".to_owned())
}

fn near(a: V3, b: V3) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() <= TOLERANCE)
}

fn on_segment(point: V3, (a, b): Segment) -> bool {
    let direction = sub(b, a);
    let length2 = dot(direction, direction);
    if !length2.is_finite() || length2 <= 0.0 {
        return false;
    }
    let t = dot(sub(point, a), direction) / length2;
    if !(0.0..=1.0).contains(&t) {
        return false;
    }
    let closest = [0, 1, 2]
        .map(|i| a.get(i).copied().unwrap_or(0.0) + t * direction.get(i).copied().unwrap_or(0.0));
    near(point, closest)
}

fn same(a: Segment, b: Segment) -> bool {
    (near(a.0, b.0) && near(a.1, b.1)) || (near(a.0, b.1) && near(a.1, b.0))
}

fn sub(a: V3, b: V3) -> V3 {
    [0, 1, 2].map(|i| a.get(i).copied().unwrap_or(0.0) - b.get(i).copied().unwrap_or(0.0))
}

fn add(a: V3, b: V3) -> V3 {
    [0, 1, 2].map(|i| a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0))
}

fn scale(a: V3, k: f64) -> V3 {
    a.map(|v| v * k)
}

fn distance(a: V3, b: V3) -> f64 {
    let d = sub(a, b);
    dot(d, d).sqrt()
}
