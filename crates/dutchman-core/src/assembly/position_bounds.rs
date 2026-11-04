//! Conservative geometry bounds for placement admission. The stored envelope uses actual
//! topology vertices (an inner bound); candidate envelopes include complete curve/surface
//! supports (outer bounds). Missing or unsupported bounds never admit a current pose.

use super::*;
use cadmpeg_ir::geometry::{SolvedCurveGeometry as Curve, SolvedSurfaceGeometry as Surface};
use cadmpeg_ir::math::Point3;

#[derive(Clone, Copy, Debug)]
pub(super) struct Bounds {
    low: [f64; 3],
    high: [f64; 3],
}

impl Bounds {
    pub(super) fn point(point: [f64; 3]) -> Self {
        Self {
            low: point,
            high: point,
        }
    }
    pub(super) fn include(&mut self, other: Self) {
        for ((low, high), (a, b)) in self
            .low
            .iter_mut()
            .zip(&mut self.high)
            .zip(other.low.into_iter().zip(other.high))
        {
            *low = low.min(a);
            *high = high.max(b);
        }
    }
    pub(super) fn placed(self, at: &Placement3x4) -> Self {
        let mut result = None;
        for mask in 0..8 {
            let point = std::array::from_fn::<_, 3, _>(|i| {
                if mask & (1 << i) == 0 {
                    self.low.get(i).copied().unwrap_or(0.0)
                } else {
                    self.high.get(i).copied().unwrap_or(0.0)
                }
            });
            let moved = at.map(|row| {
                row.iter()
                    .take(3)
                    .zip(point)
                    .map(|(a, b)| a * b)
                    .sum::<f64>()
                    + row.get(3).copied().unwrap_or(0.0)
            });
            add(&mut result, Self::point(moved));
        }
        result.unwrap_or(self)
    }
    pub(super) fn grown(mut self) -> Self {
        for (low, high) in self.low.iter_mut().zip(&mut self.high) {
            let pad = (*high - *low) * 0.05;
            *low -= pad;
            *high += pad;
        }
        self
    }
    pub(super) fn contains(self, other: Self) -> bool {
        self.low
            .into_iter()
            .zip(self.high)
            .zip(other.low.into_iter().zip(other.high))
            .all(|((lo, hi), (a, b))| a >= lo - 1e-3 && b <= hi + 1e-3)
    }
}
fn add(into: &mut Option<Bounds>, bounds: Bounds) {
    match into {
        Some(value) => value.include(bounds),
        None => *into = Some(bounds),
    }
}
fn xyz(p: Point3) -> [f64; 3] {
    [p.x, p.y, p.z]
}
fn support(center: Point3, radii: [f64; 3]) -> Bounds {
    let point = xyz(center);
    Bounds {
        low: std::array::from_fn(|i| {
            point.get(i).copied().unwrap_or(0.0) - radii.get(i).copied().unwrap_or(0.0)
        }),
        high: std::array::from_fn(|i| {
            point.get(i).copied().unwrap_or(0.0) + radii.get(i).copied().unwrap_or(0.0)
        }),
    }
}
fn circle(center: Point3, normal: cadmpeg_ir::math::Vector3, radius: f64) -> Bounds {
    support(
        center,
        [normal.x, normal.y, normal.z].map(|n| radius * (1.0 - n * n).max(0.0).sqrt()),
    )
}

#[derive(Clone, Copy, Default)]
struct BodyBounds {
    inner: Option<Bounds>,
    outer: Option<Bounds>,
    unknown: bool,
}

/// Definition-local bounds, keyed by neutral product definition id.
pub(super) fn definitions(model: &Model) -> BTreeMap<&str, (Option<Bounds>, Option<Bounds>)> {
    let ir = &model.ir.model;
    const MAX_BOUND_ENTITIES: usize = 2_000_000;
    let entities = [
        ir.points.len(),
        ir.vertices.len(),
        ir.regions.len(),
        ir.shells.len(),
        ir.faces.len(),
        ir.loops.len(),
        ir.coedges.len(),
        ir.edges.len(),
        ir.curves.len(),
        ir.surfaces.len(),
    ]
    .into_iter()
    .fold(0usize, usize::saturating_add);
    if entities > MAX_BOUND_ENTITIES {
        return BTreeMap::new();
    }
    let mut remaining = MAX_BOUND_ENTITIES;
    let points: BTreeMap<_, _> = ir
        .points
        .iter()
        .map(|p| (p.id.as_str(), xyz(p.position().get())))
        .collect();
    let vertices: BTreeMap<_, _> = ir
        .vertices
        .iter()
        .filter_map(|v| Some((v.id.as_str(), *points.get(v.point.as_str())?)))
        .collect();
    let regions: BTreeMap<_, _> = ir
        .regions
        .iter()
        .map(|r| (r.id.as_str(), r.body.as_str()))
        .collect();
    let shells: BTreeMap<_, _> = ir
        .shells
        .iter()
        .filter_map(|s| Some((s.id.as_str(), *regions.get(s.region.as_str())?)))
        .collect();
    let faces: BTreeMap<_, _> = ir
        .faces
        .iter()
        .filter_map(|f| Some((f.id.as_str(), *shells.get(f.shell.as_str())?)))
        .collect();
    let loops: BTreeMap<_, _> = ir
        .loops
        .iter()
        .filter_map(|l| Some((l.id.as_str(), *faces.get(l.face.as_str())?)))
        .collect();
    let edges: BTreeMap<_, _> = ir.edges.iter().map(|e| (e.id.as_str(), e)).collect();
    let curves: BTreeMap<_, _> = ir.curves.iter().map(|c| (c.id.as_str(), c)).collect();
    let surfaces: BTreeMap<_, _> = ir.surfaces.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut bodies: BTreeMap<String, BodyBounds> = BTreeMap::new();
    let mut edge = |edge: &cadmpeg_ir::topology::Edge, body: &str| {
        let value = bodies.entry(body.to_owned()).or_default();
        for vertex in [&edge.start, &edge.end] {
            if let Some(point) = vertices.get(vertex.as_str()) {
                add(&mut value.inner, Bounds::point(*point));
                add(&mut value.outer, Bounds::point(*point));
            }
        }
        if let Some(curve) = edge
            .curve()
            .and_then(|id| curves.get(id.as_str()))
            .and_then(|c| c.geometry.solved())
        {
            let bound = match curve {
                Curve::Line(_) => None,
                Curve::Circle(c) => Some(circle(
                    c.center().get(),
                    *c.frame().axis().as_raw(),
                    c.radius().get(),
                )),
                Curve::Ellipse(c) => Some(circle(
                    c.center().get(),
                    *c.frame().axis().as_raw(),
                    c.major_radius().get(),
                )),
                Curve::Nurbs(c) => {
                    let points = c.control_points();
                    let size = points.len();
                    if size > remaining {
                        value.unknown = true;
                        return;
                    }
                    remaining -= size;
                    if c.weights()
                        .is_some_and(|weights| weights.iter().any(|w| w.get() <= 0.0))
                    {
                        value.unknown = true;
                    }
                    let mut bound = None;
                    for point in points {
                        add(&mut bound, Bounds::point(xyz(point.get())));
                    }
                    bound
                }
                Curve::Degenerate(c) => {
                    let _ = c;
                    None
                }
                _ => {
                    value.unknown = true;
                    None
                }
            };
            if let Some(bound) = bound {
                add(&mut value.outer, bound);
            }
        } else if edge.curve().is_some() {
            value.unknown = true;
        }
    };
    for coedge in &ir.coedges {
        if let (Some(body), Some(e)) = (
            loops.get(coedge.owner_loop.as_str()),
            edges.get(coedge.edge.as_str()),
        ) {
            edge(e, body);
        }
    }
    for shell in &ir.shells {
        if let Some(body) = shells.get(shell.id.as_str()) {
            for id in shell.wire_edges() {
                if let Some(e) = edges.get(id.as_str()) {
                    edge(e, body);
                }
            }
        }
    }
    for shell in &ir.shells {
        if let Some(body) = shells.get(shell.id.as_str()) {
            let value = bodies.entry((*body).to_owned()).or_default();
            for id in shell.free_vertices() {
                if let Some(point) = vertices.get(id.as_str()) {
                    add(&mut value.inner, Bounds::point(*point));
                    add(&mut value.outer, Bounds::point(*point));
                }
            }
        }
    }
    for coedge in ir
        .coedges
        .iter()
        .filter(|coedge| coedge.use_curve.is_some())
    {
        if let Some(body) = loops.get(coedge.owner_loop.as_str()) {
            bodies.entry((*body).to_owned()).or_default().unknown = true;
        }
    }
    let boundary_bounds: BTreeMap<_, _> = bodies
        .iter()
        .filter_map(|(id, b)| Some((id.clone(), b.outer?)))
        .collect();
    for face in &ir.faces {
        let Some(body) = faces.get(face.id.as_str()) else {
            continue;
        };
        let value = bodies.entry((*body).to_owned()).or_default();
        match surfaces
            .get(face.surface.as_str())
            .and_then(|s| s.geometry.solved())
        {
            Some(Surface::Plane(_)) => {}
            Some(Surface::Cylinder(c)) => {
                if let Some(bounds) = boundary_bounds.get(*body).copied() {
                    // A sphere about each end of the support's axis interval encloses
                    // every trimmed cylinder point, including angular interior extrema.
                    let center = c.origin().get();
                    let axis = *c.frame().axis().as_raw();
                    let d = xyz(center);
                    let n = [axis.x, axis.y, axis.z];
                    let mut ends = [f64::INFINITY, f64::NEG_INFINITY];
                    for mask in 0..8 {
                        let p = std::array::from_fn::<_, 3, _>(|i| {
                            if mask & (1 << i) == 0 {
                                bounds.low.get(i).copied().unwrap_or(0.0)
                            } else {
                                bounds.high.get(i).copied().unwrap_or(0.0)
                            }
                        });
                        let t = p
                            .into_iter()
                            .zip(d)
                            .zip(n)
                            .map(|((p, c), n)| (p - c) * n)
                            .sum::<f64>();
                        ends[0] = ends[0].min(t);
                        ends[1] = ends[1].max(t);
                    }
                    for t in ends {
                        let p = Point3::new(
                            center.x + axis.x * t,
                            center.y + axis.y * t,
                            center.z + axis.z * t,
                        );
                        add(&mut value.outer, support(p, [c.radius().get(); 3]));
                    }
                } else {
                    value.unknown = true;
                }
            }
            Some(Surface::Sphere(s)) => add(
                &mut value.outer,
                support(s.center().get(), [s.radius().get().abs(); 3]),
            ),
            Some(Surface::Torus(s)) => add(
                &mut value.outer,
                support(
                    s.center().get(),
                    [s.major_radius().get() + s.minor_radius().get().abs(); 3],
                ),
            ),
            Some(Surface::Nurbs(s)) => {
                let points = s.poles();
                let size = points.len();
                if size > remaining {
                    value.unknown = true;
                    continue;
                }
                remaining -= size;
                if s.pole_weights()
                    .is_some_and(|weights| weights.iter().any(|w| w.get() <= 0.0))
                {
                    value.unknown = true;
                }
                for point in points {
                    add(&mut value.outer, Bounds::point(xyz(point.get())));
                }
            }
            _ => value.unknown = true,
        }
    }
    let placed: BTreeMap<_, _> = ir
        .bodies
        .iter()
        .filter_map(|body| {
            let mut value = *bodies.get(body.id.as_str())?;
            if let Some(at) = body.transform {
                let [a, b, c, _] = at.rows();
                let rows = [a, b, c];
                value.inner = value.inner.map(|b| b.placed(&rows));
                value.outer = value.outer.map(|b| b.placed(&rows));
            }
            if value.unknown {
                value.outer = None;
            }
            Some((body.id.as_str(), value))
        })
        .collect();
    ir.product_definitions
        .iter()
        .map(|definition| {
            let mut inner = None;
            let mut outer = None;
            let mut known = true;
            for id in &definition.bodies {
                match placed.get(id.as_str()) {
                    Some(bounds) => {
                        if let Some(b) = bounds.inner {
                            add(&mut inner, b);
                        }
                        if let Some(b) = bounds.outer {
                            add(&mut outer, b);
                        } else {
                            known = false;
                        }
                    }
                    None => known = false,
                }
            }
            (
                definition.id.as_str(),
                (inner, known.then_some(outer).flatten()),
            )
        })
        .collect()
}
