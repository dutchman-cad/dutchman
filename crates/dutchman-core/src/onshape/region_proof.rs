//! Closed boundaries for complete-profile extent proofs. Containment uses the
//! Jordan boundary theorem after analytic nonintersection, never enclosed area.
use super::*;

type Segment = ([f64; 2], [f64; 2]);
type Polygon = (bool, Vec<Segment>);

pub(crate) struct ClosedProfile {
    rings: Vec<Ring>,
}
impl ClosedProfile {
    pub(crate) fn read(profile: &Value, model: &Model) -> Mapping<Self> {
        let sketch = match text(profile, "kind")? {
            "sketch" => text(profile, "value")?,
            "sketch_regions" => profile
                .pointer("/value/sketch")
                .and_then(Value::as_str)
                .ok_or("the profile sketch is missing")?,
            _ => return Err("the complete profile needs closed sketch regions".into()),
        };
        let mut budget = Budget {
            work: WORK_LIMIT,
            output: OUTPUT_LIMIT,
            bytes: OUTPUT_BYTES,
            exhausted: false,
        };
        let rings = if text(profile, "kind")? == "sketch" {
            let loops = source_loops(sketch, model, &mut budget)?;
            if !loops.open.is_empty() {
                return Err("the complete profile has an open boundary".into());
            }
            loops.rings
        } else {
            let mut rings = Vec::new();
            for region in rows(
                profile
                    .get("value")
                    .ok_or("the profile regions are missing")?,
                "regions",
            )? {
                if text(region, "kind")? != "trimmed" {
                    return Err("the complete profile region has no closed boundary".into());
                }
                let outer = region
                    .get("outer_boundary")
                    .ok_or("the complete profile outer boundary is missing")?;
                for boundary in std::iter::once(outer).chain(rows(region, "hole_boundaries")?) {
                    let uses = boundary
                        .as_array()
                        .ok_or("a complete profile boundary is missing")?;
                    budget.work(uses.len().checked_mul(uses.len()).ok_or(BUDGET_LOSS)?)?;
                    let mut edges = Vec::new();
                    for usage in uses {
                        budget.work(model.sketch_entities.len())?;
                        let entity = model
                            .sketch_entities
                            .iter()
                            .find(|e| {
                                Some(e.id().as_str()) == usage.get("entity").and_then(Value::as_str)
                            })
                            .ok_or("a complete profile curve is missing")?;
                        let wire = serde_json::to_value(entity)
                            .map_err(|_| "a complete profile curve is unreadable")?;
                        let mut edge = Edge::new(entity.id().as_str().into(), &wire)?;
                        let [a, b] = rows(usage, "parameter_range")? else {
                            return Err("the complete profile curve interval is missing".into());
                        };
                        let range = [
                            a.as_f64().ok_or("the profile interval is missing")?,
                            b.as_f64().ok_or("the profile interval is missing")?,
                        ];
                        if range != edge.range {
                            match text(&edge.geometry, "kind")? {
                                "line" => {
                                    let at = |t| {
                                        [
                                            edge.a[0] + t * (edge.b[0] - edge.a[0]),
                                            edge.a[1] + t * (edge.b[1] - edge.a[1]),
                                        ]
                                    };
                                    let a = at(range[0]);
                                    let b = at(range[1]);
                                    edge.geometry = json!({"kind":"line","start":{"u":a[0],"v":a[1]},"end":{"u":b[0],"v":b[1]}});
                                }
                                "circle" | "arc" => {
                                    let geometry = edge
                                        .geometry
                                        .as_object_mut()
                                        .ok_or("the profile arc geometry is missing")?;
                                    geometry.insert("kind".into(), json!("arc"));
                                    geometry.insert("start_angle".into(), json!(range[0]));
                                    geometry.insert("end_angle".into(), json!(range[1]));
                                }
                                "ellipse" => {
                                    edge.geometry
                                        .as_object_mut()
                                        .ok_or("the profile ellipse geometry is missing")?
                                        .insert("bounds".into(), json!(range));
                                }
                                _ => return Err(
                                    "the complete profile interval carrier is not supported yet"
                                        .into(),
                                ),
                            }
                            edge = Edge::new(edge.id.clone(), &json!({"geometry":edge.geometry}))?;
                        }
                        edges.push(edge);
                    }
                    attach_tangencies(&mut edges, model, &mut budget)?;
                    rings.push(closed_ring(edges)?);
                }
            }
            rings
        };
        if rings.is_empty() {
            return Err("the complete profile has no closed boundaries".into());
        }
        for (i, ring) in rings.iter().enumerate() {
            budget.work(
                ring.edges
                    .len()
                    .checked_mul(ring.edges.len())
                    .ok_or(BUDGET_LOSS)?,
            )?;
            admit_ring(ring, roundoff(ring))?;
            for other in rings.iter().take(i) {
                budget.work(
                    ring.edges
                        .len()
                        .checked_mul(other.edges.len())
                        .ok_or(BUDGET_LOSS)?,
                )?;
                admit_separate_rings(ring, other)?;
            }
        }
        Ok(Self { rings })
    }
    pub(crate) fn size(&self) -> usize {
        self.rings.iter().map(|r| r.edges.len()).sum()
    }
    pub(crate) fn validate_polygons(polygons: &[Polygon]) -> Mapping<()> {
        let mut outer = None;
        let mut holes = Vec::new();
        for (is_outer, segments) in polygons {
            let ring = polygon(segments)?;
            if *is_outer {
                if outer.replace(ring).is_some() {
                    return Err("the stored target repeats its outer boundary".into());
                }
            } else {
                holes.push(ring);
            }
        }
        let outer = outer.ok_or("the stored target has no outer boundary")?;
        for (i, hole) in holes.iter().enumerate() {
            admit_separate_rings(&outer, hole)?;
            if !outer.contains(hole.edges.first().ok_or("empty stored hole")?.0.a)? {
                return Err("a stored target hole lies outside its outer boundary".into());
            }
            for other in holes.iter().take(i) {
                admit_separate_rings(hole, other)?;
                if hole.contains(other.edges.first().ok_or("empty stored hole")?.0.a)?
                    || other.contains(hole.edges.first().ok_or("empty stored hole")?.0.a)?
                {
                    return Err("the stored target hole boundaries are nested".into());
                }
            }
        }
        Ok(())
    }
    pub(crate) fn projection(&self, axis: [f64; 2]) -> Mapping<[f64; 2]> {
        let mut result = [f64::INFINITY, f64::NEG_INFINITY];
        for (edge, _) in self.rings.iter().flat_map(|r| &r.edges) {
            let [low, high] = edge.projection(axis, edge.conic()?);
            result = [result[0].min(low), result[1].max(high)];
        }
        if result.iter().all(|n| n.is_finite()) {
            Ok(result)
        } else {
            Err("the complete profile projection is unresolved".into())
        }
    }
    fn contains(&self, point: [f64; 2]) -> Mapping<bool> {
        let count = self
            .rings
            .iter()
            .map(|r| r.contains(point))
            .collect::<Mapping<Vec<_>>>()?
            .into_iter()
            .filter(|x| *x)
            .count();
        Ok(count % 2 == 1)
    }
    /// Exact closed-region overlap after proving all boundaries disjoint. A
    /// witness then classifies an entire connected boundary, not a sampled area.
    pub(crate) fn overlaps(&self, polygons: &[Polygon]) -> Mapping<bool> {
        let mut outer = None;
        let mut holes = Vec::new();
        for (is_outer, segments) in polygons {
            let ring = polygon(segments)?;
            if *is_outer {
                if outer.replace(ring).is_some() {
                    return Err("the stored target repeats its outer boundary".into());
                }
            } else {
                holes.push(ring);
            }
        }
        let outer = outer.ok_or("the stored target has no outer boundary")?;
        for boundary in std::iter::once(&outer).chain(holes.iter()) {
            for ring in &self.rings {
                if !rings_separate(boundary, ring)? {
                    return Ok(true);
                }
            }
        }
        if self.contains(outer.edges.first().ok_or("empty target boundary")?.0.a)? {
            return Ok(true);
        }
        for ring in &self.rings {
            let point = ring.edges.first().ok_or("empty profile boundary")?.0.a;
            if outer.contains(point)? {
                let in_hole = holes
                    .iter()
                    .map(|hole| hole.contains(point))
                    .collect::<Mapping<Vec<_>>>()?
                    .into_iter()
                    .any(|yes| yes);
                if !in_hole {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
    pub(crate) fn covered_by(&self, polygons: &[Polygon]) -> Mapping<bool> {
        let mut outer = None;
        let mut holes = Vec::new();
        for (is_outer, segments) in polygons {
            let ring = polygon(segments)?;
            if *is_outer {
                if outer.replace(ring).is_some() {
                    return Err("the target face repeats its outer boundary".into());
                }
            } else {
                holes.push(ring);
            }
        }
        let outer = outer.ok_or("the stored target face has no outer boundary")?;
        let mut convex = true;
        let mut covered = true;
        for (edge, _) in &outer.edges {
            let axis = [edge.a[1] - edge.b[1], edge.b[0] - edge.a[0]];
            let offset = dot(edge.a, axis);
            let bounds = outer
                .edges
                .iter()
                .map(|(e, _)| dot(e.a, axis) - offset)
                .fold([0.0_f64, 0.0_f64], |[l, h], v| [l.min(v), h.max(v)]);
            let tolerance = roundoff(&outer) * axis[0].hypot(axis[1]);
            let [low, high] = self.projection(axis)?;
            if bounds[0] >= -tolerance && bounds[1] > tolerance {
                covered &= low >= offset - tolerance;
            } else if bounds[1] <= tolerance && bounds[0] < -tolerance {
                covered &= high <= offset + tolerance;
            } else {
                convex = false;
            }
        }
        if convex {
            if !covered {
                return Ok(false);
            }
        } else {
            for ring in &self.rings {
                if !rings_separate(&outer, ring)?
                    || !outer.contains(ring.edges.first().ok_or("empty profile boundary")?.0.a)?
                {
                    return Ok(false);
                }
            }
        }
        for (i, hole) in holes.iter().enumerate() {
            admit_separate_rings(&outer, hole)?;
            if !outer.contains(hole.edges.first().ok_or("empty target hole")?.0.a)? {
                return Err("a stored target hole lies outside its closed outer boundary".into());
            }
            for other in holes.iter().take(i) {
                admit_separate_rings(hole, other)?;
            }
            if self.contains(hole.edges.first().ok_or("empty target hole")?.0.a)? {
                return Ok(false);
            }
            for ring in &self.rings {
                if !rings_separate(hole, ring)?
                    || hole.contains(ring.edges.first().ok_or("empty profile boundary")?.0.a)?
                {
                    return Ok(false);
                }
            }
        }
        Ok(true)
    }
}
fn rings_separate(a: &Ring, b: &Ring) -> Mapping<bool> {
    for (edge, _) in &a.edges {
        for (other, _) in &b.edges {
            if !separated(edge, other)? {
                return Ok(false);
            }
        }
    }
    Ok(true)
}
fn roundoff(ring: &Ring) -> f64 {
    BOUNDARY_ROUNDOFF_FACTOR
        * f64::EPSILON
        * ring
            .edges
            .iter()
            .flat_map(|(e, _)| e.a.into_iter().chain(e.b))
            .map(f64::abs)
            .fold(1.0_f64, f64::max)
}
fn polygon(segments: &[Segment]) -> Mapping<Ring> {
    let mut edges = Vec::new();
    for (a, b) in segments {
        edges.push(Edge::new(String::new(),&json!({"geometry":{"kind":"line","start":{"u":a[0],"v":a[1]},"end":{"u":b[0],"v":b[1]}}}))?);
    }
    closed_ring(edges)
}
fn closed_ring(mut edges: Vec<Edge>) -> Mapping<Ring> {
    let roundoff = boundary_roundoff(&edges);
    let first = edges.pop().ok_or("a closed boundary has no curves")?;
    let start = first.a;
    let mut end = first.b;
    let mut members = vec![(first, false)];
    while distance(start, end) > roundoff {
        let matches: Vec<_> = edges
            .iter()
            .enumerate()
            .flat_map(|(i, e)| {
                [
                    (i, false, distance(end, e.a)),
                    (i, true, distance(end, e.b)),
                ]
            })
            .filter(|(_, _, gap)| *gap <= roundoff)
            .collect();
        let [(i, reversed, _)] = matches.as_slice() else {
            return Err("the stored boundary is open or contradictory".into());
        };
        let edge = edges.remove(*i);
        end = if *reversed { edge.a } else { edge.b };
        members.push((edge, *reversed));
    }
    if !edges.is_empty() {
        return Err("a stored boundary has disconnected curves".into());
    }
    let ring = Ring { edges: members };
    admit_ring(&ring, roundoff)?;
    Ok(ring)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;
    #[test]
    fn concave_stored_face_bounds_do_not_prove_region_overlap() {
        let corners = [
            [-4.0, -4.0],
            [4.0, -4.0],
            [4.0, 0.0],
            [0.0, 0.0],
            [0.0, 4.0],
            [-4.0, 4.0],
        ];
        let segments: Vec<_> = corners
            .into_iter()
            .zip(corners.into_iter().cycle().skip(1))
            .take(corners.len())
            .collect();
        let polygons = vec![(true, segments)];
        ClosedProfile::validate_polygons(&polygons).unwrap();
        for (center, overlap) in [([2.0, 2.0], false), ([0.1, 0.1], true), ([2.0, -2.0], true)] {
            let edge=Edge::new(String::new(),&json!({"geometry":{"kind":"circle","center":{"u":center[0],"v":center[1]},"radius":0.25}})).unwrap();
            let profile = ClosedProfile {
                rings: vec![Ring {
                    edges: vec![(edge, false)],
                }],
            };
            assert_eq!(profile.overlaps(&polygons).unwrap(), overlap, "{center:?}");
        }
        let mut missing = polygons;
        missing[0].1.pop();
        assert!(ClosedProfile::validate_polygons(&missing).is_err());
    }
}
