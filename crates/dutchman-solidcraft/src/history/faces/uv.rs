//! Interior appearance picks in a surface's parameter space. Sampling is bounded;
//! unresolved carriers, disconnected trim charts and heavy surfaces give no pick.

use cadmpeg_ir::eval::{self, admission::EvaluationAdmission as Admission};
use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::Point3;
use serde_json::Value;

use super::{inside, v3, Context, V3};

const SAMPLES: usize = 16;
const MAX_EDGES: usize = 128;

pub(super) fn point(ctx: &Context, face: &Value, geometry: &Value) -> Option<V3> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pick(ctx, face, geometry, 0.0).or_else(|| {
            if geometry.get("kind").and_then(Value::as_str) == Some("torus") {
                // Move the minor-angle seam away from an inner torus band.
                pick(ctx, face, geometry, std::f64::consts::PI)
            } else {
                None
            }
        })
    }))
    .ok()
    .flatten()
}

fn xyz(geometry: &SurfaceGeometry, uv: [f64; 2]) -> Option<V3> {
    let p = eval::decode::surface_point(Admission::Standard, geometry, uv[0], uv[1])
        .ok()?
        .get();
    Some([p.x, p.y, p.z])
}

fn parameters(geometry: &SurfaceGeometry, p: V3) -> Option<([f64; 2], f64)> {
    let p = Point3 {
        x: p[0],
        y: p[1],
        z: p[2],
    };
    let solved = geometry.solved()?;
    let uv = match solved {
        SolvedSurfaceGeometry::Nurbs(surface) => {
            if surface.u_count().checked_mul(surface.v_count())? > 4096
                || surface.u_degree() > 16
                || surface.v_degree() > 16
            {
                return None;
            }
            eval::nurbs_surface_parameter_near_point(Admission::Standard, surface, p, None)
                .ok()??
        }
        _ => eval::analytic_surface_parameters_solved(solved, p)?,
    }
    .get();
    let uv = [uv.u, uv.v];
    let on = xyz(geometry, uv)?;
    let residual = (on[0] - p.x).hypot(on[1] - p.y).hypot(on[2] - p.z);
    // Native trim caches can approximate their support. The final pick must
    // clear the boundary by at least eight times this measured deviation.
    (residual < 0.002).then_some((uv, residual))
}

fn interval(value: Option<&Value>) -> Option<[f64; 2]> {
    let values = value?.as_array()?;
    Some([values.first()?.as_f64()?, values.get(1)?.as_f64()?])
}

fn sample_parameter([a, b]: [f64; 2], n: usize) -> f64 {
    match n {
        0 => a,
        SAMPLES => b,
        _ => (a + (b - a) * n as f64 / SAMPLES as f64).clamp(a.min(b), a.max(b)),
    }
}

fn edge_points(
    ctx: &Context,
    coedge: &Value,
    surface: &SurfaceGeometry,
) -> Option<(Vec<[f64; 2]>, f64)> {
    let edge = ctx.row("edges", coedge.get("edge")?.as_str()?)?;
    // Parameter-space carriers avoid inverse evaluation on free-form surfaces.
    if let Some(uses) = coedge.get("pcurves").and_then(Value::as_array) {
        for usage in uses.iter().take(2) {
            let Some((geometry, range)) = (|| {
                let row = ctx.row("pcurves", usage.get("pcurve")?.as_str()?)?;
                let geometry: PcurveGeometry =
                    serde_json::from_value(row.get("geometry")?.clone()).ok()?;
                let range = interval(usage.get("parameter_range"))?;
                Some((geometry, range))
            })() else {
                continue;
            };
            let points: Option<Vec<_>> = (0..=SAMPLES)
                .map(|n| {
                    let t = sample_parameter(range, n);
                    let p = eval::decode::pcurve_uv(Admission::Standard, &geometry, t)
                        .ok()?
                        .get();
                    Some([p.u, p.v])
                })
                .collect();
            if let Some(mut points) = points {
                let endpoint = |key: &str| {
                    let vertex = ctx.row("vertices", edge.get(key)?.as_str()?)?;
                    v3(ctx
                        .row("points", vertex.get("point")?.as_str()?)?
                        .get("position"))
                };
                let reversed = coedge.get("sense").and_then(Value::as_str) == Some("reversed");
                let start = endpoint(if reversed { "end" } else { "start" })?;
                let end = endpoint(if reversed { "start" } else { "end" })?;
                let distance = |a: V3, b: V3| (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2]);
                let first = xyz(surface, *points.first()?)?;
                let last = xyz(surface, *points.last()?)?;
                if distance(last, start) < distance(first, start) {
                    points.reverse();
                }
                if distance(xyz(surface, *points.first()?)?, start) < 1e-4
                    && distance(xyz(surface, *points.last()?)?, end) < 1e-4
                {
                    return Some((points, 0.0));
                }
            }
        }
    }
    let carrier = edge.get("carrier")?;
    let curve = ctx.row("curves", carrier.get("curve")?.as_str()?)?;
    let geometry: CurveGeometry = serde_json::from_value(curve.get("geometry")?.clone()).ok()?;
    let range = interval(carrier.get("param_range"))?;
    let samples: Vec<_> = (0..=SAMPLES)
        .map(|n| {
            let t = sample_parameter(range, n);
            let p = eval::decode::curve_point(Admission::Standard, &geometry, t)
                .ok()?
                .get();
            parameters(surface, [p.x, p.y, p.z])
        })
        .collect::<Option<_>>()?;
    let deviation = samples.iter().map(|(_, error)| *error).fold(0.0, f64::max);
    let mut points: Vec<_> = samples.into_iter().map(|(uv, _)| uv).collect();
    if coedge.get("sense").and_then(Value::as_str) == Some("reversed") {
        points.reverse();
    }
    Some((points, deviation))
}

/// A vertical ray starts below every trim. Periodic copies of each segment
/// count once, so a cylindrical ring need not close in an unrolled chart.
fn periodic_winding(rings: &[Vec<[f64; 2]>], p: [f64; 2], period: f64) -> i32 {
    let mut winding = 0;
    for ring in rings {
        for (a, b) in ring
            .iter()
            .zip(ring.iter().cycle().skip(1))
            .take(ring.len())
        {
            let mut b = *b;
            b[0] += ((a[0] - b[0]) / period).round() * period;
            let u = p[0] + (((a[0] + b[0]) / 2.0 - p[0]) / period).round() * period;
            if (a[0] > u) != (b[0] > u) && p[1] > a[1] + (b[1] - a[1]) * (u - a[0]) / (b[0] - a[0])
            {
                winding += if b[0] > a[0] { 1 } else { -1 };
            }
        }
    }
    winding
}

fn periodic_inside(rings: &[Vec<[f64; 2]>], p: [f64; 2], period: f64) -> bool {
    periodic_winding(rings, p, period).unsigned_abs() % 2 == 1
}

/// A minor great-circle triangle has a unique interior on its oriented side.
/// This avoids the longitude singularity at a spherical corner's pole.
fn spherical_triangle(ctx: &Context, face: &Value, geometry: &Value) -> Option<V3> {
    use super::super::sketch::{cross, dot, unit};
    let center = v3(geometry.get("center"))?;
    let radius = geometry.get("radius")?.as_f64().filter(|r| *r > 0.0)?;
    let loops = face.pointer("/loops/loops")?.as_array()?;
    if loops.len() != 1 {
        return None;
    }
    let coedges = ctx
        .row("loops", loops.first()?.as_str()?)?
        .pointer("/boundary/coedges")?
        .as_array()?;
    if coedges.len() != 3 {
        return None;
    }
    let mut sides = Vec::new();
    let distance = |a: V3, b: V3| (a[0] - b[0]).hypot(a[1] - b[1]).hypot(a[2] - b[2]);
    for id in coedges {
        let coedge = ctx.row("coedges", id.as_str()?)?;
        let edge = ctx.row("edges", coedge.get("edge")?.as_str()?)?;
        let carrier = edge.get("carrier")?;
        let curve = ctx
            .row("curves", carrier.get("curve")?.as_str()?)?
            .get("geometry")?;
        if curve.get("kind")?.as_str()? != "circle"
            || distance(v3(curve.get("center"))?, center) > radius * 1e-6
            || (curve.get("radius")?.as_f64()? - radius).abs() > radius * 1e-6
        {
            return None;
        }
        let range = interval(carrier.get("param_range"))?;
        if (range[1] - range[0]).abs() > std::f64::consts::PI {
            return None;
        }
        let geometry: CurveGeometry = serde_json::from_value(curve.clone()).ok()?;
        let endpoint = |key: &str| {
            let vertex = ctx.row("vertices", edge.get(key)?.as_str()?)?;
            v3(ctx
                .row("points", vertex.get("point")?.as_str()?)?
                .get("position"))
        };
        let mut ends = [endpoint("start")?, endpoint("end")?];
        for (t, p) in range.into_iter().zip(ends) {
            let evaluated = eval::decode::curve_point(Admission::Standard, &geometry, t)
                .ok()?
                .get();
            if distance([evaluated.x, evaluated.y, evaluated.z], p) > radius * 1e-5 {
                return None;
            }
        }
        if coedge.get("sense").and_then(Value::as_str) == Some("reversed") {
            ends.reverse();
        }
        let mut directions = Vec::new();
        for p in ends {
            let d = [p[0] - center[0], p[1] - center[1], p[2] - center[2]];
            if (dot(d, d).sqrt() - radius).abs() > radius * 1e-5 {
                return None;
            }
            directions.push(unit(d)?);
        }
        sides.push((*directions.first()?, *directions.get(1)?));
    }
    if sides
        .iter()
        .zip(sides.iter().cycle().skip(1))
        .take(3)
        .any(|((_, end), (start, _))| distance(*end, *start) > 1e-5)
    {
        return None;
    }
    let mut sum = [0.0; 3];
    for (start, _) in &sides {
        sum = [sum[0] + start[0], sum[1] + start[1], sum[2] + start[2]];
    }
    let p = unit(sum)?;
    let sign = match face.get("sense").and_then(Value::as_str) {
        Some("forward") => 1.0,
        Some("reversed") => -1.0,
        _ => return None,
    };
    if sides
        .iter()
        .any(|(a, b)| dot(cross(*a, *b), p) * sign <= 1e-6)
    {
        return None;
    }
    Some([
        center[0] + radius * p[0],
        center[1] + radius * p[1],
        center[2] + radius * p[2],
    ])
}

fn pick(ctx: &Context, face: &Value, geometry: &Value, phase: f64) -> Option<V3> {
    let surface: SurfaceGeometry = serde_json::from_value(geometry.clone()).ok()?;
    if matches!(surface.solved(), Some(SolvedSurfaceGeometry::Sphere(_))) {
        if let Some(p) = spherical_triangle(ctx, face, geometry) {
            return Some(p);
        }
    }
    if let SolvedSurfaceGeometry::Nurbs(surface) = surface.solved()? {
        if surface.u_count().checked_mul(surface.v_count())? > 4096
            || surface.u_degree() > 16
            || surface.v_degree() > 16
        {
            return None;
        }
    }
    let periodic = match surface.solved()? {
        SolvedSurfaceGeometry::Cylinder(_)
        | SolvedSurfaceGeometry::Cone(_)
        | SolvedSurfaceGeometry::Sphere(_) => [true, false],
        SolvedSurfaceGeometry::Torus(_) => [true, true],
        _ => [false, false],
    };
    let loop_ids: Vec<&str> = if face
        .pointer("/loops/classification")
        .and_then(Value::as_str)
        == Some("classified")
    {
        std::iter::once(face.pointer("/loops/outer")?.as_str())
            .chain(
                face.pointer("/loops/inner")?
                    .as_array()?
                    .iter()
                    .map(Value::as_str),
            )
            .collect::<Option<_>>()?
    } else {
        face.pointer("/loops/loops")?
            .as_array()?
            .iter()
            .map(Value::as_str)
            .collect::<Option<_>>()?
    };
    if loop_ids.is_empty() || loop_ids.len() > 16 {
        return None;
    }
    let mut loops = Vec::new();
    let mut joins = Vec::new();
    let mut maximum_deviation = 0.0_f64;
    let mut edge_count = 0usize;
    for &id in &loop_ids {
        let row = ctx.row("loops", id)?;
        let coedges = row.pointer("/boundary/coedges")?.as_array()?;
        edge_count = edge_count.checked_add(coedges.len())?;
        if edge_count > MAX_EDGES || coedges.is_empty() {
            return None;
        }
        let mut polygon: Vec<[f64; 2]> = Vec::new();
        for id in coedges {
            let coedge = ctx.row("coedges", id.as_str()?)?;
            let (mut points, deviation) = edge_points(ctx, coedge, &surface)?;
            if phase != 0.0 {
                for point in &mut points {
                    point[1] = (point[1] + phase + std::f64::consts::PI)
                        .rem_euclid(std::f64::consts::TAU)
                        - std::f64::consts::PI;
                }
            }
            maximum_deviation = maximum_deviation.max(deviation);
            for (n, p) in points.iter_mut().enumerate() {
                if let Some(previous) = polygon.last() {
                    for (axis, periodic) in periodic.iter().enumerate() {
                        if *periodic {
                            let (a, b) = (p.get_mut(axis)?, previous.get(axis)?);
                            *a +=
                                ((b - *a) / std::f64::consts::TAU).round() * std::f64::consts::TAU;
                        }
                    }
                }
                if n == 0 {
                    if let Some(previous) = polygon.last() {
                        joins.push((*previous, *p));
                    }
                }
                polygon.push(*p);
            }
        }
        if polygon.len() < 3 {
            return None;
        }
        joins.push((*polygon.first()?, *polygon.last()?));
        loops.push(polygon);
    }
    // A candidate must be inside an odd number of trim rings, and comfortably
    // away from their sampled boundaries. This also respects holes and concavity.
    let min = [0, 1].map(|axis| {
        loops
            .iter()
            .flatten()
            .filter_map(|p| p.get(axis))
            .copied()
            .fold(f64::INFINITY, f64::min)
    });
    let max = [0, 1].map(|axis| {
        loops
            .iter()
            .flatten()
            .filter_map(|p| p.get(axis))
            .copied()
            .fold(f64::NEG_INFINITY, f64::max)
    });
    let torus_band = matches!(surface.solved(), Some(SolvedSurfaceGeometry::Torus(_)))
        && loops.iter().any(|ring| {
            ring.first()
                .zip(ring.last())
                .is_some_and(|(a, b)| (a[0] - b[0]).abs() > std::f64::consts::PI)
        });
    let periodic_ray = torus_band
        || matches!(
            surface.solved(),
            Some(SolvedSurfaceGeometry::Cylinder(_) | SolvedSurfaceGeometry::Cone(_))
        );
    let mut scale = [max[0] - min[0], max[1] - min[1]];
    // A local toroidal band must also prove which side of its winding trims
    // belongs to this face. Wider or seam-crossing V charts remain unresolved.
    if torus_band && scale[1] >= std::f64::consts::PI {
        return None;
    }
    if periodic_ray {
        scale[0] = scale[0].min(std::f64::consts::TAU);
    }
    if scale.iter().any(|s| !s.is_finite() || *s <= 1e-12) {
        return None;
    }
    if joins.iter().any(|(a, b)| {
        let mut u = a[0] - b[0];
        if periodic_ray {
            u -= (u / std::f64::consts::TAU).round() * std::f64::consts::TAU;
        }
        (u / scale[0]).hypot((a[1] - b[1]) / scale[1]) > 1e-4
    }) {
        return None;
    }
    let normalized: Vec<Vec<_>> = loops
        .iter()
        .map(|ring| {
            ring.iter()
                .map(|p| [(p[0] - min[0]) / scale[0], (p[1] - min[1]) / scale[1]])
                .collect()
        })
        .collect();
    let mut best = None;
    let mut clearance = 0.0;
    let period = std::f64::consts::TAU / scale[0];
    for u in 1..32 {
        for v in 1..32 {
            let p = [f64::from(u) / 32.0, f64::from(v) / 32.0];
            let covered = if periodic_ray {
                periodic_inside(&normalized, p, period)
                    && (!torus_band || {
                        let winding = periodic_winding(&normalized, p, period);
                        match face.get("sense").and_then(Value::as_str) {
                            Some("forward") => winding > 0,
                            Some("reversed") => winding < 0,
                            _ => false,
                        }
                    })
            } else {
                normalized.iter().filter(|ring| inside(ring, p)).count() % 2 == 1
            };
            if !covered {
                continue;
            }
            let mut distance = f64::INFINITY;
            for ring in &normalized {
                for (a, b) in ring
                    .iter()
                    .zip(ring.iter().cycle().skip(1))
                    .take(ring.len())
                {
                    let (mut a, mut b) = (*a, *b);
                    if periodic_ray {
                        b[0] += ((a[0] - b[0]) / period).round() * period;
                        let offset = ((p[0] - (a[0] + b[0]) / 2.0) / period).round() * period;
                        a[0] += offset;
                        b[0] += offset;
                    }
                    let d = [b[0] - a[0], b[1] - a[1]];
                    let length = d[0] * d[0] + d[1] * d[1];
                    let t = if length > 0.0 {
                        (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / length).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    distance = distance.min((p[0] - a[0] - t * d[0]).hypot(p[1] - a[1] - t * d[1]));
                }
            }
            if distance > clearance {
                clearance = distance;
                best = Some(p);
            }
        }
    }
    let p = best.filter(|_| clearance > 0.01)?;
    let point = xyz(
        &surface,
        [min[0] + p[0] * scale[0], min[1] + p[1] * scale[1] - phase],
    )?;
    let mut physical_clearance = f64::INFINITY;
    for ring in &loops {
        let boundary: Vec<_> = ring
            .iter()
            .map(|uv| xyz(&surface, [uv[0], uv[1] - phase]))
            .collect::<Option<_>>()?;
        for (a, b) in boundary
            .iter()
            .zip(boundary.iter().cycle().skip(1))
            .take(boundary.len())
        {
            let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let length = d.iter().map(|v| v * v).sum::<f64>();
            let t = if length > 0.0 {
                ((point[0] - a[0]) * d[0] + (point[1] - a[1]) * d[1] + (point[2] - a[2]) * d[2])
                    / length
            } else {
                0.0
            }
            .clamp(0.0, 1.0);
            physical_clearance = physical_clearance.min(
                (point[0] - a[0] - t * d[0])
                    .hypot(point[1] - a[1] - t * d[1])
                    .hypot(point[2] - a[2] - t * d[2]),
            );
        }
    }
    (physical_clearance > (maximum_deviation * 8.0).max(1e-7)).then_some(point)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;
    use serde_json::json;

    fn patch(pcurves: bool, hole: bool) -> (Value, Value) {
        let surface = json!({"kind":"nurbs", "u_degree":1, "v_degree":1,
            "u_knots":[0,0,1,1], "v_knots":[0,0,1,1], "u_periodic":false, "v_periodic":false,
            "poles":{"form":"polynomial","rows":[
                [{"x":0,"y":0,"z":0},{"x":0,"y":1,"z":0}],
                [{"x":1,"y":0,"z":0},{"x":1,"y":1,"z":1}]]}});
        let mut model = json!({"surfaces":[{"id":"s","geometry":surface}],
            "points":[],"vertices":[],"edges":[],"curves":[],"coedges":[],"loops":[],"pcurves":[]});
        let mut rings = vec![vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]];
        if hole {
            rings.push(vec![[0.25, 0.25], [0.75, 0.25], [0.75, 0.75], [0.25, 0.75]]);
        }
        let point = |uv: [f64; 2]| json!({"x":uv[0],"y":uv[1],"z":uv[0]*uv[1]});
        for (ring, vertices) in rings.iter().enumerate() {
            for (n, a) in vertices.iter().enumerate() {
                let b = vertices[(n + 1) % vertices.len()];
                let direction: [f64; 3] = [b[0] - a[0], b[1] - a[1], b[0] * b[1] - a[0] * a[1]];
                let length = direction[0].hypot(direction[1]).hypot(direction[2]);
                let id = format!("{ring}-{n}");
                let next = format!("{ring}-{}", (n + 1) % vertices.len());
                model["points"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"id":format!("p{id}"),"position":point(*a)}));
                model["vertices"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"id":format!("v{id}"),"point":format!("p{id}")}));
                model["edges"].as_array_mut().unwrap().push(
                    json!({"id":format!("e{id}"),"start":format!("v{id}"),"end":format!("v{next}"),
                    "carrier":{"curve":format!("g{id}"),"param_range":[0,length]}}),
                );
                model["curves"].as_array_mut().unwrap().push(json!({"id":format!("g{id}"),"geometry":{"kind":"line",
                    "origin":point(*a),"direction":{"x":direction[0]/length,"y":direction[1]/length,"z":direction[2]/length}}}));
                model["coedges"].as_array_mut().unwrap().push(json!({"id":format!("c{id}"),"edge":format!("e{id}"),"sense":"forward",
                    "pcurves":if pcurves {json!([{"pcurve":format!("q{id}"),"parameter_range":[0,1]}])} else {json!([])}}));
                model["pcurves"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"id":format!("q{id}"),"geometry":{"kind":"line",
                    "origin":{"u":a[0],"v":a[1]},"direction":{"u":b[0]-a[0],"v":b[1]-a[1]}}}));
            }
            model["loops"].as_array_mut().unwrap().push(json!({"id":format!("l{ring}"),"boundary":{"coedges":(0..4).map(|n|format!("c{ring}-{n}")).collect::<Vec<_>>()}}));
        }
        let face = json!({"surface":"s","loops":{"loops":(0..rings.len()).map(|n|format!("l{n}")).collect::<Vec<_>>()}});
        (model, face)
    }

    #[test]
    fn nurbs_appearance_points_use_trim_curves_and_stay_out_of_holes() {
        for hole in [false, true] {
            let (model, face) = patch(true, hole);
            let ctx = Context::new(&model);
            let p = point(&ctx, &face, &model["surfaces"][0]["geometry"]).unwrap();
            assert!((p[2] - p[0] * p[1]).abs() < 1e-9);
            assert!(p[0] > 0.0 && p[0] < 1.0 && p[1] > 0.0 && p[1] < 1.0);
            if hole {
                assert!(!(p[0] > 0.25 && p[0] < 0.75 && p[1] > 0.25 && p[1] < 0.75));
            }
        }
    }

    #[test]
    fn nurbs_appearance_points_can_recover_parameters_from_spatial_edges() {
        let (model, face) = patch(false, false);
        let ctx = Context::new(&model);
        let p = point(&ctx, &face, &model["surfaces"][0]["geometry"]).unwrap();
        assert!((p[2] - p[0] * p[1]).abs() < 1e-5);
    }

    #[test]
    fn nurbs_trim_sampling_keeps_exact_knot_endpoints() {
        let (mut model, face) = patch(false, false);
        let (a, b) = (-0.02_f64, -0.000002_f64);
        assert!(a + (b - a) > b);
        model["curves"][0]["geometry"] = json!({"kind":"nurbs", "degree":1,
            "knots":[a,a,b,b], "periodic":false,
            "poles":{"form":"polynomial","points":[{"x":0,"y":0,"z":0},{"x":1,"y":0,"z":0}]}});
        model["edges"][0]["carrier"]["param_range"] = json!([a, b]);
        let ctx = Context::new(&model);
        let p = point(&ctx, &face, &model["surfaces"][0]["geometry"]).unwrap();
        assert!((p[2] - p[0] * p[1]).abs() < 1e-5);
        assert!(p[0] > 0.25 && p[0] < 0.75 && p[1] > 0.25 && p[1] < 0.75);
    }

    #[test]
    fn classified_trim_rings_keep_the_hole_clear() {
        let (model, mut face) = patch(true, true);
        face["loops"] = json!({"classification":"classified","outer":"l0","inner":["l1"]});
        let ctx = Context::new(&model);
        let p = point(&ctx, &face, &model["surfaces"][0]["geometry"]).unwrap();
        assert!(!(p[0] > 0.25 && p[0] < 0.75 && p[1] > 0.25 && p[1] < 0.75));
    }

    #[test]
    fn disconnected_trim_charts_do_not_assign_an_appearance() {
        let (mut model, face) = patch(true, false);
        model["pcurves"][1]["geometry"]["origin"]["u"] = json!(3.0);
        model["curves"][1]["geometry"]["origin"]["x"] = json!(3.0);
        let ctx = Context::new(&model);
        assert!(point(&ctx, &face, &model["surfaces"][0]["geometry"]).is_none());
    }

    #[test]
    fn appearance_picks_exclude_a_planar_hole_before_using_outer_vertices() {
        let (mut model, face) = patch(true, true);
        model["surfaces"][0]["geometry"] = json!({"kind":"plane",
            "origin":{"x":0,"y":0,"z":0},"normal":{"x":0,"y":0,"z":1},
            "u_axis":{"x":1,"y":0,"z":0}});
        for point in model["points"].as_array_mut().unwrap() {
            point["position"]["z"] = json!(0);
        }
        let ctx = Context::new(&model);
        let in_hole = |p: V3| p[0] > 0.25 && p[0] < 0.75 && p[1] > 0.25 && p[1] < 0.75;
        assert!(in_hole(super::super::point_on(&ctx, &face).unwrap()));
        let p = super::super::appearance_point(&ctx, &face).unwrap();
        assert!(!in_hole(p));
        assert!(p[0] > 0.0 && p[0] < 1.0 && p[1] > 0.0 && p[1] < 1.0);
        assert_eq!(p[2], 0.0);
    }

    #[test]
    fn appearance_picks_follow_a_long_cylinder_arc_instead_of_its_complement() {
        let (mut model, face) = patch(true, false);
        let end = 3.0 * std::f64::consts::FRAC_PI_2;
        model["surfaces"][0]["geometry"] = json!({"kind":"cylinder",
            "origin":{"x":0,"y":0,"z":0},"axis":{"x":0,"y":0,"z":1},
            "ref_direction":{"x":1,"y":0,"z":0},"radius":1});
        for point in model["points"].as_array_mut().unwrap() {
            let u = point["position"]["x"].as_f64().unwrap() * end;
            let v = point["position"]["y"].clone();
            point["position"] = json!({"x":u.cos(),"y":u.sin(),"z":v});
        }
        for pcurve in model["pcurves"].as_array_mut().unwrap() {
            for field in ["origin", "direction"] {
                let u = pcurve["geometry"][field]["u"].as_f64().unwrap();
                pcurve["geometry"][field]["u"] = json!(u * end);
            }
        }
        let angle = |p: V3| p[1].atan2(p[0]).rem_euclid(std::f64::consts::TAU);
        let ctx = Context::new(&model);
        assert!(angle(super::super::point_on(&ctx, &face).unwrap()) > end);
        let p = super::super::appearance_point(&ctx, &face).unwrap();
        assert!(angle(p) > 0.0 && angle(p) < end);
        assert!(p[2] > 0.0 && p[2] < 1.0);
    }

    #[test]
    fn approximate_trim_edges_still_pick_the_exact_support_away_from_the_boundary() {
        let (mut model, face) = patch(false, false);
        model["curves"][0]["geometry"]["origin"]["z"] = json!(0.00005);
        let ctx = Context::new(&model);
        let p = point(&ctx, &face, &model["surfaces"][0]["geometry"]).unwrap();
        assert!((p[2] - p[0] * p[1]).abs() < 1e-8);
        assert!(p[0] > 0.25 && p[0] < 0.75 && p[1] > 0.25 && p[1] < 0.75);
        model["curves"][0]["geometry"]["origin"]["z"] = json!(0.01);
        let ctx = Context::new(&model);
        assert!(point(&ctx, &face, &model["surfaces"][0]["geometry"]).is_none());
    }

    #[test]
    fn periodic_cylinder_appearance_charts_keep_seam_crossing_holes_clear() {
        let tau = std::f64::consts::TAU;
        let lower = (0..=16).map(|n| [f64::from(n) * tau / 16.0, 0.0]).collect();
        let upper = (0..=16)
            .rev()
            .map(|n| [f64::from(n) * tau / 16.0, 10.0])
            .collect();
        let hole = vec![[-0.2, 4.0], [-0.2, 6.0], [0.2, 6.0], [0.2, 4.0]];
        let rings = vec![lower, upper, hole];
        assert!(!periodic_inside(&rings, [0.1, 5.0], tau));
        assert!(!periodic_inside(&rings, [tau + 0.1, 5.0], tau));
        assert!(!periodic_inside(&rings, [-0.1, 5.0], tau));
        assert!(periodic_inside(&rings, [std::f64::consts::PI, 5.0], tau));
        assert!(periodic_inside(&rings, [tau - 0.3, 5.0], tau));
        assert!(!periodic_inside(&rings, [std::f64::consts::PI, -1.0], tau));
        assert!(!periodic_inside(&rings, [std::f64::consts::PI, 11.0], tau));
    }

    #[test]
    fn winding_conical_and_toroidal_trims_pick_the_faces_own_band() {
        for (torus, seam) in [(false, false), (true, false), (true, true)] {
            for reversed in [false, true] {
                let surface = if torus {
                    json!({"kind":"torus","center":{"x":0,"y":0,"z":0},"axis":{"x":0,"y":0,"z":1},
                        "ref_direction":{"x":1,"y":0,"z":0},"major_radius":3,"minor_radius":1})
                } else {
                    json!({"kind":"cone","origin":{"x":0,"y":0,"z":0},"axis":{"x":0,"y":0,"z":1},
                        "ref_direction":{"x":1,"y":0,"z":0},"radius":1,"ratio":1,"half_angle":0.5_f64.atan()})
                };
                let geometry: SurfaceGeometry = serde_json::from_value(surface.clone()).unwrap();
                let bounds = if seam {
                    [std::f64::consts::PI, -std::f64::consts::FRAC_PI_2]
                } else if torus {
                    [0.2, 0.8]
                } else {
                    [0.0, 2.0]
                };
                let mut model = json!({"points":[],"vertices":[],"edges":[],"coedges":[],"pcurves":[],"loops":[]});
                for (n, v) in bounds.into_iter().enumerate() {
                    let p = xyz(&geometry, [0.0, v]).unwrap();
                    let direction = if (n == 0) != reversed { 1.0 } else { -1.0 };
                    model["points"].as_array_mut().unwrap().push(
                        json!({"id":format!("p{n}"),"position":{"x":p[0],"y":p[1],"z":p[2]}}),
                    );
                    model["vertices"]
                        .as_array_mut()
                        .unwrap()
                        .push(json!({"id":format!("v{n}"),"point":format!("p{n}")}));
                    model["edges"].as_array_mut().unwrap().push(
                        json!({"id":format!("e{n}"),"start":format!("v{n}"),"end":format!("v{n}")}),
                    );
                    model["coedges"].as_array_mut().unwrap().push(
                        json!({"id":format!("c{n}"),"edge":format!("e{n}"),"sense":"forward",
                        "pcurves":[{"pcurve":format!("q{n}"),"parameter_range":[0,std::f64::consts::TAU]}]}),
                    );
                    model["pcurves"].as_array_mut().unwrap().push(json!({"id":format!("q{n}"),"geometry":{"kind":"line","origin":{"u":0,"v":v},"direction":{"u":direction,"v":0}}}));
                    model["loops"].as_array_mut().unwrap().push(
                        json!({"id":format!("l{n}"),"boundary":{"coedges":[format!("c{n}")]}}),
                    );
                }
                let mut face = json!({"sense":if reversed {"reversed"} else {"forward"},"loops":{"loops":["l0","l1"]}});
                let ctx = Context::new(&model);
                let p = point(&ctx, &face, &surface).unwrap();
                if torus {
                    let v = p[2].atan2(p[0].hypot(p[1]) - 3.0);
                    if seam {
                        let v = v.rem_euclid(std::f64::consts::TAU);
                        assert!(
                            v > std::f64::consts::PI && v < 3.0 * std::f64::consts::FRAC_PI_2,
                            "{p:?}"
                        );
                        assert!(pick(&ctx, &face, &surface, 0.0).is_none());
                    } else {
                        assert!(v > 0.2 && v < 0.8, "{p:?}");
                    }
                    face["sense"] = json!(if reversed { "forward" } else { "reversed" });
                    assert!(point(&ctx, &face, &surface).is_none());
                } else {
                    assert!(p[2] > 0.0 && p[2] < 2.0);
                    assert!((p[0].hypot(p[1]) - 1.0 - 0.5 * p[2]).abs() < 1e-9);
                }
            }
        }
    }

    #[test]
    fn a_spherical_corner_keeps_its_appearance_across_the_pole() {
        let vertices = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
        let surface = json!({"kind":"sphere","center":{"x":0,"y":0,"z":0},"axis":{"x":0,"y":0,"z":1},
            "ref_direction":{"x":1,"y":0,"z":0},"radius":1});
        let mut model = json!({"points":[],"vertices":[],"curves":[],"edges":[],"coedges":[],
            "loops":[{"id":"l","boundary":{"coedges":["c0","c1","c2"]}}]});
        for (n, p) in vertices.into_iter().enumerate() {
            let axis = vertices[(n + 2) % 3];
            model["points"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id":format!("p{n}"),"position":{"x":p[0],"y":p[1],"z":p[2]}}));
            model["vertices"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id":format!("v{n}"),"point":format!("p{n}")}));
            model["curves"].as_array_mut().unwrap().push(json!({"id":format!("g{n}"),"geometry":{"kind":"circle","center":{"x":0,"y":0,"z":0},
                "axis":{"x":axis[0],"y":axis[1],"z":axis[2]},"ref_direction":{"x":p[0],"y":p[1],"z":p[2]},"radius":1}}));
            model["edges"].as_array_mut().unwrap().push(
                json!({"id":format!("e{n}"),"start":format!("v{n}"),"end":format!("v{}",(n+1)%3),
                "carrier":{"curve":format!("g{n}"),"param_range":[0,std::f64::consts::FRAC_PI_2]}}),
            );
            model["coedges"]
                .as_array_mut()
                .unwrap()
                .push(json!({"id":format!("c{n}"),"edge":format!("e{n}"),"sense":"forward"}));
        }
        let mut face = json!({"sense":"forward","loops":{"loops":["l"]}});
        let ctx = Context::new(&model);
        let p = point(&ctx, &face, &surface).unwrap();
        assert!(p.iter().all(|p| (*p - 1.0 / 3.0_f64.sqrt()).abs() < 1e-9));
        face["sense"] = json!("reversed");
        assert!(spherical_triangle(&ctx, &face, &surface).is_none());
        model["curves"][0]["geometry"]["radius"] = json!(2);
        let ctx = Context::new(&model);
        face["sense"] = json!("forward");
        assert!(spherical_triangle(&ctx, &face, &surface).is_none());
    }
}
