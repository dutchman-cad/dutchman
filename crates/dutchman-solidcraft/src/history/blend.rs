//! Blends picked exclusively from their own historical input-edge geometry.
//! A single convex polygon extrusion gives a complete, provable pre-feature edge set.
//! Other preceding geometry is refused rather than selecting from the final body.

use serde_json::{json, Map, Value};

use super::sketch::{dot, number, unit, v3, V3};
use super::{name, Context};

const LIMIT: usize = 256;
const TOLERANCE: f64 = 1e-7;
const INPUT: &str =
    "its input edge midpoint cannot be proven to select exactly one rebuilt edge yet";

type Segment = (V3, V3);

pub(super) fn map(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    preceding: &[Value],
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
    if kind == "fillet" {
        if group.get("tangency_weight").is_some_and(|v| !v.is_null()) {
            return Err("fillets with a tangency weight are not supported yet".to_owned());
        }
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
    if selection.get("kind").and_then(Value::as_str) != Some("historical") {
        return Err("blends require edges from their own feature input topology; final-body and unresolved selections are not supported yet".to_owned());
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
    let (body, rebuilt) = input_edges(ctx, preceding).ok_or(INPUT)?;
    let angle_chamfer =
        group.pointer("/spec/kind").and_then(Value::as_str) == Some("distance_angle");
    let mut selected = Vec::new();
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
        let segment = (
            v3(sample.get("start")).ok_or("its input edge start is missing")?,
            v3(sample.get("end")).ok_or("its input edge end is missing")?,
        );
        let tangent = v3(sample.get("tangent"))
            .and_then(unit)
            .ok_or("its input edge tangent is missing")?;
        let mut matches = rebuilt
            .iter()
            .enumerate()
            .filter(|(_, candidate)| on_segment(point, **candidate));
        let (index, candidate) = matches.next().ok_or(INPUT)?;
        if matches.next().is_some()
            || !same(segment, *candidate)
            || near(point, segment.0)
            || near(point, segment.1)
            || !unit(sub(candidate.1, candidate.0))
                .is_some_and(|direction| (dot(direction, tangent).abs() - 1.0).abs() <= 1e-9)
        {
            return Err(INPUT.to_owned());
        }
        // A 45-degree chamfer has equal setbacks only at a right-angle corner.
        if angle_chamfer && !right_angle(*candidate, &rebuilt) {
            return Err("distance-and-angle chamfers on non-perpendicular input faces are not supported yet".to_owned());
        }
        if selected.contains(&index) {
            return Err("its input edge selection repeats a rebuilt edge".to_owned());
        }
        selected.push(index);
        points.push(point);
    }
    out.insert("body".to_owned(), json!(body));
    out.insert("edges".to_owned(), json!(points));
    let id = ctx.id();
    out.insert("id".to_owned(), json!(id));
    out.insert(
        "name".to_owned(),
        json!(name(feature)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Blend{id}"))),
    );
    Ok(Value::Object(out))
}

/// At a convex prism vertex the other two edges point along the support faces.
fn right_angle(edge: Segment, rebuilt: &[Segment]) -> bool {
    let directions: Vec<_> = rebuilt
        .iter()
        .filter(|candidate| !same(edge, **candidate))
        .filter_map(|(a, b)| {
            if near(edge.0, *a) {
                unit(sub(*b, *a))
            } else if near(edge.0, *b) {
                unit(sub(*a, *b))
            } else {
                None
            }
        })
        .collect();
    match directions.as_slice() {
        [a, b] => dot(*a, *b).abs() <= 1e-9,
        _ => false,
    }
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

/// Reconstructs an input for which every edge's location is known without a kernel.
/// Refuse prior modifications, multiple bodies, curved profiles and split profile selections.
fn input_edges(ctx: &Context, preceding: &[Value]) -> Option<(String, Vec<Segment>)> {
    let mut solids = preceding
        .iter()
        .filter(|feature| feature.get("type").and_then(Value::as_str) != Some("sketch"));
    let extrude = solids.next()?;
    if solids.next().is_some()
        || extrude.get("type")?.as_str()? != "extrude"
        || extrude.get("operation")?.as_str()? != "new_body"
        || ctx.named_bodies.len() != 1
    {
        return None;
    }
    let body = ctx.named_bodies.first()?.1.clone();
    let sketch = ctx
        .sketches
        .values()
        .find(|sketch| Some(sketch.feature) == extrude.get("sketch").and_then(Value::as_u64))?;
    let extent = extrude.get("extent")?;
    if extent
        .as_object()?
        .keys()
        .any(|key| !matches!(key.as_str(), "direction" | "distance"))
    {
        return None;
    }
    let sign = match extent.get("direction")?.as_str()? {
        "positive" => 1.0,
        "negative" => -1.0,
        _ => return None,
    };
    let distance = extent
        .get("distance")?
        .as_str()?
        .strip_suffix(" mm")?
        .parse::<f64>()
        .ok()?;
    if !distance.is_finite() || distance <= 0.0 {
        return None;
    }
    if sketch.shapes.len() > LIMIT {
        return None;
    }
    let profiles = extrude.get("profiles")?;
    let curves: Vec<&super::sketch::Curve> = sketch
        .shapes
        .values()
        .map(|shape| match shape {
            super::sketch::Shape::Curve(curve) if curve.kind == "line" => Some(curve),
            _ => None,
        })
        .collect::<Option<_>>()?;
    if !(3..=LIMIT).contains(&curves.len()) {
        return None;
    }
    match profiles.get("type")?.as_str()? {
        "all" => {}
        "curves" => {
            let loops = profiles.get("loops")?.as_array()?;
            let ids = loops.first()?.as_array()?;
            if loops.len() != 1
                || ids.len() != curves.len()
                || curves
                    .iter()
                    .any(|curve| !ids.iter().any(|id| id.as_str() == Some(curve.id.as_str())))
            {
                return None;
            }
        }
        _ => return None,
    }
    let mut boundary = Vec::new();
    for curve in &curves {
        let a = sketch.world(sketch.points.get(curve.start?)?.1);
        let b = sketch.world(sketch.points.get(curve.end?)?.1);
        if near(a, b) {
            return None;
        }
        boundary.push((a, b));
    }
    // Every corner must join exactly two non-collinear sides. This excludes dangling
    // curves, overlapping sides and corners SolidCraft merges into one longer edge.
    for (a, b) in &boundary {
        let neighbours: Vec<_> = boundary
            .iter()
            .filter(|segment| near(*a, segment.0) || near(*a, segment.1))
            .collect();
        if neighbours.len() != 2 {
            return None;
        }
        let other = neighbours
            .into_iter()
            .find(|segment| !same(**segment, (*a, *b)))?;
        let c = if near(*a, other.0) { other.1 } else { other.0 };
        let ab = sub(*b, *a);
        let ac = sub(c, *a);
        let cross = super::sketch::cross(ab, ac);
        if dot(cross, cross) <= TOLERANCE * TOLERANCE * dot(ab, ab) * dot(ac, ac) {
            return None;
        }
    }
    let shift = |p: V3| {
        [0, 1, 2].map(|i| {
            p.get(i).copied().unwrap_or(0.0)
                + sign * distance * sketch.normal.get(i).copied().unwrap_or(0.0)
        })
    };
    // Order one connected ring and require convexity. This also excludes crossings,
    // nested loops and multiple profiles, whose evaluated topology needs a kernel.
    let first = boundary.first()?.0;
    let mut ring = vec![first];
    let mut at = first;
    let mut previous = None;
    for _ in 0..boundary.len() {
        let next = boundary.iter().find_map(|(a, b)| {
            let next = if near(at, *a) {
                *b
            } else if near(at, *b) {
                *a
            } else {
                return None;
            };
            (!previous.is_some_and(|previous| near(previous, next))).then_some(next)
        })?;
        previous = Some(at);
        at = next;
        if near(at, first) {
            break;
        }
        if ring.iter().any(|point| near(*point, at)) {
            return None;
        }
        ring.push(at);
    }
    if !near(at, first) || ring.len() != boundary.len() {
        return None;
    }
    let mut orientation = None;
    for i in 0..ring.len() {
        let a = *ring.get(i)?;
        let b = *ring.get((i + 1) % ring.len())?;
        for c in ring.iter().filter(|c| !near(**c, a) && !near(**c, b)) {
            let side = dot(super::sketch::cross(sub(b, a), sub(*c, a)), sketch.normal);
            if !side.is_finite()
                || side.abs() <= TOLERANCE
                || orientation.is_some_and(|sign| side * sign <= 0.0)
            {
                return None;
            }
            orientation = Some(side.signum());
        }
    }
    let mut edges = Vec::new();
    for (a, b) in boundary {
        edges.extend([(a, b), (shift(a), shift(b))]);
    }
    for point in ring {
        edges.push((point, shift(point)));
    }
    if edges
        .iter()
        .any(|(a, b)| a.iter().chain(b).any(|v| !v.is_finite()))
    {
        return None;
    }
    Some((body, edges))
}

fn sub(a: V3, b: V3) -> V3 {
    [0, 1, 2].map(|i| a.get(i).copied().unwrap_or(0.0) - b.get(i).copied().unwrap_or(0.0))
}
