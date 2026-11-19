//! Complete-profile ray/plane intersections against the feature's stored input
//! faces. No midpoint, area, bounding-box response or final-body guess is used.
use super::{
    features,
    regions::ClosedProfile,
    rows, text,
    topology::{self, State},
    Mapping,
};
use cadmpeg_ir::document::Model;
use serde_json::{json, Value};

type Segment = ([f64; 2], [f64; 2]);
struct Face {
    id: String,
    time: Option<Affine>,
    parallel_side: [f64; 2],
    polygons: Vec<(bool, Vec<Segment>)>,
}
#[derive(Clone, Copy)]
struct Affine {
    constant: f64,
    axis: [f64; 2],
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
impl Affine {
    fn bounds(self, profile: &ClosedProfile) -> Mapping<[f64; 2]> {
        let [l, h] = profile.projection(self.axis)?;
        Ok([self.constant + l, self.constant + h])
    }
    fn minus(self, other: Self) -> Self {
        Self {
            constant: self.constant - other.constant,
            axis: [self.axis[0] - other.axis[0], self.axis[1] - other.axis[1]],
        }
    }
}
pub(super) fn next(
    state: &State,
    feature: &Value,
    profile: &Value,
    model: &Model,
) -> Mapping<Value> {
    next_proof(state,feature,profile,model).map_err(|reason| format!("UP_TO_NEXT needs exact coverage and first intersection over the complete profile: {reason}"))
}
fn next_proof(state: &State, feature: &Value, profile: &Value, model: &Model) -> Mapping<Value> {
    if !matches!(
        features::value(feature, "operationType"),
        Some("ADD" | "REMOVE" | "INTERSECT")
    ) {
        return Err("the stored operation does not select an existing target body".into());
    }
    let sketch_id = if text(profile, "kind")? == "sketch" {
        text(profile, "value")?
    } else {
        profile
            .pointer("/value/sketch")
            .and_then(Value::as_str)
            .ok_or("the profile sketch is missing")?
    };
    let sketch = model
        .sketches
        .iter()
        .find(|s| s.id.as_str() == sketch_id)
        .ok_or("the profile sketch is missing")?;
    let sketch = serde_json::to_value(sketch).map_err(|_| "the profile frame is unreadable")?;
    let frame = sketch
        .pointer("/placement/frame")
        .ok_or("the profile frame is missing")?;
    let origin = topology::vector(
        frame.get("origin").ok_or("the profile origin is missing")?,
        1.0,
    )?;
    let x = topology::unit(topology::vector(
        frame.get("u_axis").ok_or("the profile x axis is missing")?,
        1.0,
    )?)?;
    let normal = topology::unit(topology::vector(
        frame.get("normal").ok_or("the profile normal is missing")?,
        1.0,
    )?)?;
    let y = topology::cross(normal, x);
    let reversed = features::enabled(feature, "oppositeDirection")?;
    let direction = normal.map(|n| if reversed { -n } else { n });
    let project = |point: [f64; 3]| {
        let offset = topology::sub(point, origin);
        [dot(offset, x), dot(offset, y)]
    };
    let profile = ClosedProfile::read(profile, model)?;
    let [px0, px1] = profile.projection([1.0, 0.0])?;
    let [py0, py1] = profile.projection([0.0, 1.0])?;
    let tolerance = 64.0
        * f64::EPSILON
        * [px0, px1, py0, py1]
            .into_iter()
            .map(f64::abs)
            .fold(1.0_f64, f64::max);
    let mut faces = Vec::new();
    let mut work = super::regions::WORK_LIMIT;
    for (id, face) in &state.faces {
        let surface = face
            .get("surface")
            .ok_or("a stored target surface is missing")?;
        if text(surface, "type")? != "PLANE" {
            return Err("a stored input surface has no supported analytic intersection yet".into());
        }
        let p = topology::vector(
            surface
                .get("origin")
                .ok_or("a stored target plane origin is missing")?,
            1000.0,
        )?;
        let n = topology::unit(topology::vector(
            surface
                .get("normal")
                .ok_or("a stored target plane normal is missing")?,
            1.0,
        )?)?;
        let denominator = dot(n, direction);
        let signed = Affine {
            constant: dot(n, topology::sub(p, origin)),
            axis: [-dot(n, x), -dot(n, y)],
        };
        if denominator != 0.0 && denominator.abs() <= 64.0 * f64::EPSILON {
            return Err("a stored input plane is nearly parallel to the travel direction; its intersection is unresolved".into());
        }
        let time = if denominator != 0.0 {
            Some(Affine {
                constant: signed.constant / denominator,
                axis: signed.axis.map(|a| a / denominator),
            })
        } else {
            None
        };
        // This is only an algebraic chart for checking the stored face loops;
        // it is never used as a sketch attachment frame.
        let reference = if n[0].abs() < n[1].abs() {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let face_x = topology::unit(topology::cross(n, reference))?;
        let face_y = topology::cross(n, face_x);
        let face_project = |point| {
            let offset = topology::sub(point, p);
            [dot(offset, face_x), dot(offset, face_y)]
        };
        let count = rows(face, "loops")?.iter().try_fold(0usize, |total, lp| {
            total
                .checked_add(rows(lp, "coedges")?.len())
                .ok_or_else(|| "the exact extent boundary count exceeds the limit".to_owned())
        })?;
        let cost = count
            .checked_mul(count)
            .and_then(|cost| cost.checked_mul(3))
            .and_then(|cost| {
                count
                    .checked_mul(profile.size())
                    .and_then(|n| n.checked_mul(4))
                    .and_then(|n| cost.checked_add(n))
            })
            .ok_or("the exact extent work count exceeds the limit")?;
        work = work
            .checked_sub(cost)
            .ok_or("the exact extent work budget is exhausted")?;
        let mut polygons = Vec::new();
        let mut planar_polygons = Vec::new();
        let mut bounds = [
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ];
        for lp in rows(face, "loops")? {
            let is_outer = lp
                .get("isOuter")
                .and_then(Value::as_bool)
                .ok_or("the stored target loop has no outer/hole role")?;
            let mut segments = Vec::new();
            let mut planar_segments = Vec::new();
            for coedge in rows(lp, "coedges")? {
                work = work
                    .checked_sub(1)
                    .ok_or("the exact extent work budget is exhausted")?;
                let edge = state
                    .edges
                    .get(text(coedge, "edgeId")?)
                    .ok_or("a stored target boundary edge is missing")?;
                let carrier = edge
                    .get("curve")
                    .ok_or("a stored target boundary carrier is missing")?;
                if text(carrier, "type")? != "LINE" {
                    return Err(
                        "a stored target boundary needs a supported exact curve intersection"
                            .into(),
                    );
                }
                let geometry = edge
                    .get("geometry")
                    .ok_or("a stored target boundary interval is missing")?;
                let start = topology::vector(
                    geometry
                        .get("startPoint")
                        .ok_or("a stored target boundary start is missing")?,
                    1000.0,
                )?;
                let end = topology::vector(
                    geometry
                        .get("endPoint")
                        .ok_or("a stored target boundary end is missing")?,
                    1000.0,
                )?;
                let carrier_origin = topology::vector(
                    carrier
                        .get("origin")
                        .ok_or("a stored line origin is missing")?,
                    1000.0,
                )?;
                let carrier_direction = topology::unit(topology::vector(
                    carrier
                        .get("direction")
                        .ok_or("a stored line direction is missing")?,
                    1.0,
                )?)?;
                for endpoint in [start, end] {
                    let scale = endpoint
                        .into_iter()
                        .chain(p)
                        .map(f64::abs)
                        .fold(1.0_f64, f64::max);
                    let residual =
                        topology::cross(topology::sub(endpoint, carrier_origin), carrier_direction);
                    if dot(n, topology::sub(endpoint, p)).abs() > 64.0 * f64::EPSILON * scale
                        || residual
                            .iter()
                            .any(|r| r.abs() > 64.0 * f64::EPSILON * scale)
                    {
                        return Err(
                            "a stored target boundary contradicts its plane or line carrier".into(),
                        );
                    }
                    let [u, v] = project(endpoint);
                    bounds = [
                        bounds[0].min(u),
                        bounds[1].max(u),
                        bounds[2].min(v),
                        bounds[3].max(v),
                    ];
                }
                segments.push((project(start), project(end)));
                planar_segments.push((face_project(start), face_project(end)));
            }
            polygons.push((is_outer, segments));
            planar_polygons.push((is_outer, planar_segments));
        }
        ClosedProfile::validate_polygons(&planar_polygons)?;
        if polygons.is_empty() {
            return Err("the stored target face has no closed boundaries".into());
        }
        if bounds[1] < px0 - tolerance
            || bounds[0] > px1 + tolerance
            || bounds[3] < py0 - tolerance
            || bounds[2] > py1 + tolerance
        {
            continue;
        }
        if time.is_some() && !profile.overlaps(&polygons)? {
            continue;
        }
        faces.push(Face {
            id: id.clone(),
            time,
            parallel_side: signed.bounds(&profile)?,
            polygons,
        });
    }
    let comparisons = faces
        .len()
        .checked_mul(faces.len())
        .and_then(|n| n.checked_mul(profile.size()))
        .and_then(|n| n.checked_mul(2))
        .ok_or("the exact extent work count exceeds the limit")?;
    work = work
        .checked_sub(comparisons)
        .ok_or("the exact extent work budget is exhausted")?;
    for candidate in &faces {
        let Some(time) = candidate.time else {
            continue;
        };
        if time.bounds(&profile)?[0] <= tolerance || !profile.covered_by(&candidate.polygons)? {
            continue;
        }
        let mut first = true;
        for other in &faces {
            work = work
                .checked_sub(1)
                .ok_or("the exact extent work budget is exhausted")?;
            if let Some(other_time) = other.time {
                if other_time.bounds(&profile)?[1] > tolerance
                    && other_time.minus(time).bounds(&profile)?[0] < -tolerance
                {
                    first = false;
                    break;
                }
            } else if other.parallel_side[0] < -tolerance && other.parallel_side[1] > tolerance {
                first = false;
                break;
            }
        }
        if first {
            return Ok(
                json!({"kind":"historical","value":{"state":state.row.get("id"),"faces":[candidate.id],"native":"Onshape UP_TO_NEXT exact input-face intersection"}}),
            );
        }
    }
    Err(
        "no stored input face proves closed-boundary coverage and the first positive intersection"
            .into(),
    )
}
