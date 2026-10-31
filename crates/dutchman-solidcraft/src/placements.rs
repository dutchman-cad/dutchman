//! Admit occurrence placements and move their reflection into component geometry.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::analytic::{
    CircleCurve, ConeSurface, CylinderSurface, DegenerateCurve, EllipseCurve, HyperbolaCurve,
    LineCurve, ParabolaCurve, PlaneSurface, SphereSurface, TorusSurface,
};
use cadmpeg_ir::geometry::{
    CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::topology::{LoopBoundary, Sense};
use cadmpeg_ir::transform::Transform;
use cadmpeg_ir::units::OrthonormalFrame3;
use dutchman_core::placement::{compose, rigid, MIRROR_X};
use dutchman_core::{Assembly, Error, Model, Report, Severity};

// Bound expansion when both parities of nested component definitions are needed.
const MAX_DEFINITIONS: usize = 100_000;
const MAX_DEPTH: usize = 32;

struct Copies<'a> {
    report: &'a mut Report,
    joints: bool,
    reflected: usize,
    definitions: usize,
}

pub(crate) fn prepare(
    assembly: &Assembly,
    joints: bool,
    report: &mut Report,
) -> Result<Assembly, Error> {
    let mut copies = Copies {
        report,
        joints,
        reflected: 0,
        definitions: 0,
    };
    let assembly = copies.assembly(assembly, false, 0)?;
    if copies.reflected != 0 {
        copies.report.push(Severity::Info, "placements", format!(
            "{} occurrence(s) placed by a reflection were written with a mirrored copy of their component",
            copies.reflected
        ));
    }
    Ok(assembly)
}

impl Copies<'_> {
    fn assembly(
        &mut self,
        source: &Assembly,
        reflected: bool,
        depth: usize,
    ) -> Result<Assembly, Error> {
        if depth > MAX_DEPTH {
            return Err(Error::Encode(
                "assembly placements nest past 32 levels".to_owned(),
            ));
        }
        let transforms: Vec<_> = source
            .placements
            .iter()
            .enumerate()
            .map(|(index, p)| {
                let t = if self.joints {
                    &p.transform
                } else {
                    source.stored_placements.get(index).unwrap_or(&p.transform)
                };
                let t = if reflected { compose(&MIRROR_X, t) } else { *t };
                (t, rigid(&t))
            })
            .collect();
        let mut needed = BTreeSet::new();
        // Retain the original definition order for ordinary assemblies.
        if !reflected {
            needed.extend((0..source.parts.len()).map(|i| (i, false)));
        }
        for (p, (_, admitted)) in source.placements.iter().zip(&transforms) {
            if let Ok((_, mirrored)) = admitted {
                needed.insert((p.part, *mirrored));
            }
        }
        let mut parts = Vec::new();
        let mut definitions = Vec::new();
        let mut indices = BTreeMap::new();
        // All original definitions first, then their mirrored variants.
        for mirrored in [false, true] {
            for &(index, parity) in &needed {
                if parity != mirrored {
                    continue;
                }
                let Some(part) = source.parts.get(index) else {
                    continue;
                };
                self.definitions += 1;
                if self.definitions > MAX_DEFINITIONS {
                    return Err(Error::Encode(
                        "mirrored assembly needs more than 100000 component definitions".to_owned(),
                    ));
                }
                let mut copy = part.clone();
                if mirrored {
                    copy.name = format!("{} (mirrored)", part.name);
                }
                if let Some(sub) = &part.assembly {
                    copy.assembly = Some(self.assembly(sub, mirrored, depth + 1)?);
                } else if mirrored {
                    if let Err(error) = reflect(&mut copy) {
                        // Occurrences of this definition are left out by the writer.
                        self.report.push(
                            Severity::Dropped,
                            "placements",
                            format!(
                                "{}: occurrences placed by a reflection are left out: {error}",
                                part.name
                            ),
                        );
                        continue;
                    }
                    if !part.ir.model.features.is_empty() || !part.ir.model.sketches.is_empty() {
                        self.report.push(Severity::Degraded, "history", format!(
                            "{}: mirrored definition written as imported geometry only; reflecting its feature history is not supported yet",
                            part.name
                        ));
                    }
                    copy.ir.model.features.clear();
                    copy.ir.model.sketches.clear();
                    copy.ir.model.parameters.clear();
                }
                indices.insert((index, mirrored), parts.len());
                definitions.push(if mirrored {
                    None
                } else {
                    source.definitions.get(index).cloned().flatten()
                });
                parts.push(copy);
            }
        }
        let mut placements = source.placements.clone();
        for (p, (t, admitted)) in placements.iter_mut().zip(transforms) {
            match admitted {
                Ok((rotation, mirrored)) => {
                    p.transform = rotation;
                    p.part = indices
                        .get(&(p.part, mirrored))
                        .copied()
                        .unwrap_or(usize::MAX);
                    if mirrored {
                        self.reflected += 1;
                    }
                }
                // The writer drops these while preserving occurrence indices for joint paths.
                Err(_) => {
                    p.transform = t;
                }
            }
        }
        let mut joints = Vec::new();
        for joint in &source.joints {
            if reflected
                || changed_path(source, &joint.a, self.joints)
                || changed_path(source, &joint.b, self.joints)
            {
                self.report.push(Severity::Dropped, "joints", "A joint touching a mirrored component is left out; its stored placement is retained");
            } else {
                joints.push(joint.clone());
            }
        }
        Ok(Assembly {
            own_part: source
                .own_part
                .and_then(|i| indices.get(&(i, reflected)).copied()),
            parts,
            placements,
            joints,
            left_out: source.left_out.clone(),
            held: source.held.clone(),
            joint_notes: source.joint_notes.clone(),
            stored_placements: Vec::new(),
            definitions,
        })
    }
}

fn changed_path(mut assembly: &Assembly, path: &[usize], joints: bool) -> bool {
    for &index in path {
        let Some(p) = assembly.placements.get(index) else {
            return true;
        };
        let t = if joints {
            &p.transform
        } else {
            assembly
                .stored_placements
                .get(index)
                .unwrap_or(&p.transform)
        };
        if !matches!(rigid(t), Ok((_, false))) {
            return true;
        }
        let Some(sub) = assembly.parts.get(p.part).and_then(|p| p.assembly.as_ref()) else {
            break;
        };
        assembly = sub;
    }
    false
}

fn failure(detail: impl std::fmt::Display) -> Error {
    Error::Encode(format!("cannot reflect component geometry: {detail}"))
}

fn reverse(sense: Sense) -> Sense {
    match sense {
        Sense::Forward => Sense::Reversed,
        Sense::Reversed => Sense::Forward,
    }
}

type Reflected<T> = Result<T, Error>;

fn flip(point: Point3) -> Point3 {
    Point3::new(-point.x, point.y, point.z)
}

fn flip_vector(vector: Vector3) -> Vector3 {
    Vector3::new(-vector.x, vector.y, vector.z)
}

fn point(point: FinitePoint3) -> Reflected<FinitePoint3> {
    FinitePoint3::new(flip(point.get())).ok_or_else(|| failure("point is not finite"))
}

/// The frame mirrored across the YZ plane. `reverse_axis` negates the mirrored axis so the
/// frame stays right-handed with the reference direction mirrored: a curve then keeps its
/// parameterisation (the edges that bound it hold parameter ranges). Without it the axis is
/// the mirrored axis, which keeps the outward side of a surface of revolution.
fn frame(source: &OrthonormalFrame3, reverse_axis: bool) -> Reflected<OrthonormalFrame3> {
    let axis = flip_vector(*source.axis().as_raw());
    let axis = if reverse_axis {
        Vector3::new(-axis.x, -axis.y, -axis.z)
    } else {
        axis
    };
    OrthonormalFrame3::new(axis, flip_vector(*source.reference().as_raw()))
        .ok_or_else(|| failure("frame is not orthonormal"))
}

/// Returns the surface mirrored across the YZ plane, built from its own parameters so no
/// carrier holds a transform that reverses handedness. The second value says whether the
/// carrier's own normal flips relative to the mirrored solid (free-form surfaces only).
fn surface(source: &SolvedSurfaceGeometry) -> Reflected<(SolvedSurfaceGeometry, bool)> {
    Ok(match source {
        SolvedSurfaceGeometry::Plane(s) => (
            SolvedSurfaceGeometry::Plane(PlaneSurface::new(
                point(s.origin())?,
                frame(s.frame(), false)?,
            )),
            false,
        ),
        SolvedSurfaceGeometry::Cylinder(s) => (
            SolvedSurfaceGeometry::Cylinder(CylinderSurface::new(
                point(s.origin())?,
                frame(s.frame(), false)?,
                s.radius(),
            )),
            false,
        ),
        SolvedSurfaceGeometry::Cone(s) => (
            SolvedSurfaceGeometry::Cone(ConeSurface::new(
                point(s.origin())?,
                frame(s.frame(), false)?,
                s.radius(),
                s.ratio(),
                s.half_angle(),
            )),
            false,
        ),
        SolvedSurfaceGeometry::Sphere(s) => (
            SolvedSurfaceGeometry::Sphere(SphereSurface::new(
                point(s.center())?,
                frame(s.frame(), false)?,
                s.radius(),
            )),
            false,
        ),
        SolvedSurfaceGeometry::Torus(s) => (
            SolvedSurfaceGeometry::Torus(TorusSurface::new(
                point(s.center())?,
                frame(s.frame(), false)?,
                s.major_radius(),
                s.minor_radius(),
            )),
            false,
        ),
        SolvedSurfaceGeometry::Nurbs(s) => {
            let mut s = s.clone();
            s.try_map_control_points(|_, p| point(p))?;
            (SolvedSurfaceGeometry::Nurbs(s), true)
        }
        _ => return Err(failure("a surface kind without an exact mirrored form")),
    })
}

fn curve(source: &SolvedCurveGeometry) -> Reflected<SolvedCurveGeometry> {
    Ok(match source {
        SolvedCurveGeometry::Line(c) => SolvedCurveGeometry::Line(
            LineCurve::try_new(flip(c.origin().get()), flip_vector(*c.direction().as_raw()))
                .map_err(failure)?,
        ),
        SolvedCurveGeometry::Circle(c) => SolvedCurveGeometry::Circle(CircleCurve::new(
            point(c.center())?,
            frame(c.frame(), true)?,
            c.radius(),
        )),
        SolvedCurveGeometry::Ellipse(c) => SolvedCurveGeometry::Ellipse(EllipseCurve::new(
            point(c.center())?,
            frame(c.frame(), true)?,
            c.radii(),
        )),
        SolvedCurveGeometry::Parabola(c) => SolvedCurveGeometry::Parabola(ParabolaCurve::new(
            point(c.vertex())?,
            frame(c.frame(), true)?,
            c.focal_distance(),
        )),
        SolvedCurveGeometry::Hyperbola(c) => SolvedCurveGeometry::Hyperbola(HyperbolaCurve::new(
            point(c.center())?,
            frame(c.frame(), true)?,
            c.major_radius(),
            c.minor_radius(),
        )),
        SolvedCurveGeometry::Degenerate(c) => {
            SolvedCurveGeometry::Degenerate(DegenerateCurve::new(point(c.point())?))
        }
        SolvedCurveGeometry::Nurbs(c) => {
            let mut c = c.clone();
            c.try_map_control_points(|_, p| point(p))?;
            SolvedCurveGeometry::Nurbs(c)
        }
        // Composite curves refer to other arena carriers, which are mirrored individually.
        SolvedCurveGeometry::Composite { .. } => source.clone(),
        _ => return Err(failure("a curve kind without an exact mirrored form")),
    })
}

/// Mirrors the part across the YZ plane in its own carriers: points, curves and surfaces are
/// rebuilt from mirrored parameters (no carrier holds a reflecting transform, which STEP
/// readers refuse), and loops and faces are reoriented so every solid keeps outward faces
/// and positive volume. Parameter-space curves are dropped; the 3D edges carry the shape.
fn reflect(part: &mut Model) -> Result<(), Error> {
    for position in &mut part.ir.model.points {
        position.set_position(
            FinitePoint3::new(flip(position.position().get()))
                .ok_or_else(|| failure("point is not finite"))?,
        );
    }
    let mut flipped = BTreeSet::new();
    for s in &mut part.ir.model.surfaces {
        let basis = s
            .geometry
            .solved()
            .ok_or_else(|| failure(format!("surface {} has no solved carrier", s.id)))?;
        let (mirrored, reversed) = surface(basis)?;
        if reversed {
            flipped.insert(s.id.clone());
        }
        s.geometry = SurfaceGeometry::Solved(mirrored);
    }
    for c in &mut part.ir.model.curves {
        let basis = c
            .geometry
            .solved()
            .ok_or_else(|| failure(format!("curve {} has no solved carrier", c.id)))?;
        c.geometry = CurveGeometry::Solved(curve(basis)?);
    }
    part.ir.model.pcurves.clear();
    for coedge in &mut part.ir.model.coedges {
        coedge.pcurves.clear();
    }
    for face in &mut part.ir.model.faces {
        // A mirrored analytic surface keeps its outward side; a free-form one is mirrored
        // pole by pole, which turns its parametric normal against the solid.
        if flipped.contains(&face.surface) {
            face.sense = reverse(face.sense);
        }
    }
    for coedge in &mut part.ir.model.coedges {
        coedge.sense = reverse(coedge.sense);
    }
    for boundary in &mut part.ir.model.loops {
        if let LoopBoundary::Vertex { pcurves, .. } = &mut boundary.boundary {
            pcurves.clear();
        }
        if boundary.coedges().is_empty() {
            continue;
        }
        let mut uses = boundary.anchored_vertex_uses().to_vec();
        for vertex in &mut uses {
            vertex.pcurves.clear();
        }
        // A pole after one edge is before that edge in the reversed ring.
        for vertex in &mut uses {
            vertex.after = boundary
                .next_coedge(&vertex.after)
                .cloned()
                .ok_or_else(|| failure("loop anchor has no next coedge"))?;
        }
        uses.reverse();
        let ring = boundary.coedges().iter().rev().cloned().collect();
        boundary.replace_ring(ring, uses).map_err(failure)?;
    }
    for body in &mut part.ir.model.bodies {
        if let Some(transform) = body.transform {
            // M * B * p = (M * B * M) * (M * p).
            body.transform = Transform::affine(compose(
                &compose(&MIRROR_X, &transform.affine_rows()),
                &MIRROR_X,
            ));
        }
    }
    part.source_step = None;
    part.fidelity = None;
    // Appearance face selectors are found on the reflected carriers during restatement.
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    // Signed volume from the oriented planar polygons of our synthetic cube. This check
    // does not depend on a kernel repairing the imported solid's orientation.
    fn signed_volume(model: &Model) -> f64 {
        let m = &model.ir.model;
        let mut volume = 0.0;
        for boundary in &m.loops {
            let polygon: Vec<_> = boundary
                .coedges()
                .iter()
                .map(|id| {
                    let coedge = m.coedges.iter().find(|c| c.id == *id).unwrap();
                    let edge = m.edges.iter().find(|e| e.id == coedge.edge).unwrap();
                    let vertex = if coedge.sense == Sense::Forward {
                        &edge.start
                    } else {
                        &edge.end
                    };
                    let vertex = m.vertices.iter().find(|v| v.id == *vertex).unwrap();
                    let point = m
                        .points
                        .iter()
                        .find(|p| p.id == vertex.point)
                        .unwrap()
                        .position()
                        .get();
                    [point.x, point.y, point.z]
                })
                .collect();
            let a = polygon[0];
            for triangle in polygon[1..].windows(2) {
                let [b, c] = [triangle[0], triangle[1]];
                volume += (a[0] * (b[1] * c[2] - b[2] * c[1])
                    + a[1] * (b[2] * c[0] - b[0] * c[2])
                    + a[2] * (b[0] * c[1] - b[1] * c[0]))
                    / 6.0;
            }
        }
        volume
    }

    fn unit(v: Vector3) -> [f64; 3] {
        [v.x, v.y, v.z]
    }

    fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    }

    #[test]
    fn mirrored_curved_carriers_are_right_handed_and_trace_the_mirrored_shape() {
        let frame =
            OrthonormalFrame3::new(Vector3::new(0.0, 0.6, 0.8), Vector3::new(1.0, 0.0, 0.0))
                .unwrap();
        let center = FinitePoint3::new(Point3::new(3.0, 4.0, 5.0)).unwrap();
        let radius = cadmpeg_ir::scalar::PositiveLength::new(2.0).unwrap();
        let circle = CircleCurve::new(center, frame, radius);
        let SolvedCurveGeometry::Circle(mirrored) =
            curve(&SolvedCurveGeometry::Circle(circle)).unwrap()
        else {
            panic!("a circle stays a circle");
        };
        let at = |c: &CircleCurve, t: f64| {
            let x = unit(*c.frame().reference().as_raw());
            let a = unit(*c.frame().axis().as_raw());
            let y = cross(a, x);
            let o = c.center().get();
            [0, 1, 2].map(|i| [o.x, o.y, o.z][i] + 2.0 * (t.cos() * x[i] + t.sin() * y[i]))
        };
        for t in [0.0, 0.7, 2.0, 4.1] {
            let (a, b) = (at(&circle, t), at(&mirrored, t));
            assert!((a[0] + b[0]).abs() < 1e-12);
            assert!((a[1] - b[1]).abs() < 1e-12 && (a[2] - b[2]).abs() < 1e-12);
        }
        // A mirrored cylinder keeps its radial outward side: same radius, mirrored axis.
        let cylinder = CylinderSurface::new(center, frame, radius);
        let SolvedSurfaceGeometry::Cylinder(mirrored) =
            surface(&SolvedSurfaceGeometry::Cylinder(cylinder))
                .unwrap()
                .0
        else {
            panic!("a cylinder stays a cylinder");
        };
        assert_eq!(mirrored.radius(), radius);
        assert_eq!(unit(*mirrored.frame().axis().as_raw()), [0.0, 0.6, 0.8]);
        assert_eq!(mirrored.origin().get().x, -3.0);
    }

    #[test]
    fn reflection_preserves_positive_signed_volume_and_reverses_loops() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures/synthetic_cube.ipt");
        let original = dutchman_core::read(
            dutchman_core::Format::InventorPart,
            &path,
            &mut Report::default(),
        )
        .unwrap();
        let volume = signed_volume(&original);
        assert!((volume - 1000.0).abs() < 1e-7, "{volume}");
        let mut mirrored = original.clone();
        reflect(&mut mirrored).unwrap();
        assert!((signed_volume(&mirrored) - volume).abs() < 1e-7);
        for (before, after) in original.ir.model.faces.iter().zip(&mirrored.ir.model.faces) {
            // Mirrored planes keep their outward normal.
            assert_eq!(after.sense, before.sense);
        }
        for (before, after) in original.ir.model.loops.iter().zip(&mirrored.ir.model.loops) {
            assert_eq!(
                before.coedges().iter().rev().collect::<Vec<_>>(),
                after.coedges().iter().collect::<Vec<_>>()
            );
        }
        for (before, after) in original
            .ir
            .model
            .coedges
            .iter()
            .zip(&mirrored.ir.model.coedges)
        {
            assert_eq!(after.sense, reverse(before.sense));
        }
    }
}
