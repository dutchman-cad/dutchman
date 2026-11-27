//! Sheet-metal base and contour flanges, and edge flanges on proven input-state edges. Hems are refused
//! with the reason SolidCraft cannot express them.

use serde_json::{json, Value};

use super::profile::profile;
use super::sketch::{cross, dot, number, v3, Shape, P2, V3};
use super::{name, Context};

/// Maps a base flange whose operation is `definition`.
pub(super) fn base_flange(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let (sketch, selection) = profile(definition.get("profile"), ctx)?;
    let sketch = ctx
        .sketches
        .get(&sketch)
        .map(|mapped| mapped.feature)
        .ok_or("its sketch is not an earlier mapped sketch")?;
    let thickness = number(definition.get("thickness"))
        .filter(|t| *t > 0.0)
        .ok_or("its thickness is not given")?;
    // SolidCraft grows the sheet along the sketch's normal, or against it when flipped.
    let flip = match definition.get("side").and_then(Value::as_str) {
        None | Some("forward") => false,
        Some("reverse") => true,
        Some(_) => {
            return Err(
                "a base flange thick on both sides of its sketch is not mapped yet".to_owned(),
            )
        }
    };
    let rule = ctx.sheet_rule(thickness);
    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("BaseFlange{id}")),
        "type": "sheet_base",
        "suppressed": false,
        "sketch": sketch,
        "profiles": selection,
        "rule": rule,
    });
    if let (true, Some(fields)) = (flip, out.as_object_mut()) {
        fields.insert("flip".to_owned(), Value::Bool(true));
    }
    Ok(out)
}

/// Maps a contour flange: an open chain of sketch lines and arcs swept along the sketch's
/// normal into a sheet, its bends at the chain's corners.
pub(super) fn contour_flange(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let definition = definition
        .get("construction")
        .ok_or("its contour-flange construction is not given")?;
    let profile = definition
        .get("profile")
        .ok_or("its profile is not given")?;
    let kind = profile
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    if kind != "sketch_entities" {
        return Err(format!(
            "a contour flange whose profile is {kind}, not a list of sketch lines and arcs, is not supported yet"
        ));
    }
    let value = profile.get("value").unwrap_or(&Value::Null);
    let sketch_id = value
        .get("sketch")
        .and_then(Value::as_str)
        .ok_or("its profile sketch is malformed")?;
    let entities = value
        .get("entities")
        .and_then(Value::as_array)
        .filter(|entities| !entities.is_empty())
        .ok_or("its profile entities are malformed")?;
    let sketch = ctx
        .sketches
        .get(sketch_id)
        .ok_or("its sketch is not an earlier mapped sketch")?;
    let chain = open_chain(sketch, entities)?;

    let extent = definition.get("extent").ok_or("its extent is not given")?;
    let length = match extent.get("kind").and_then(Value::as_str) {
        Some("one_sided") => {
            let side = extent.get("side").ok_or("its extent is malformed")?;
            if number(side.get("draft")).is_some_and(|draft| draft != 0.0) {
                return Err("a drafted contour flange is not supported yet".to_owned());
            }
            match side.pointer("/termination/kind").and_then(Value::as_str) {
                Some("blind") => number(side.pointer("/termination/length"))
                    .filter(|length| *length != 0.0)
                    .ok_or("its flange length is not given")?,
                Some(other) => {
                    return Err(format!(
                        "a contour flange whose length ends {other} is not supported yet; \
                         SolidCraft takes a distance"
                    ))
                }
                None => return Err("its extent is malformed".to_owned()),
            }
        }
        Some(other) => {
            return Err(format!(
                "a {other} contour flange is not supported yet; SolidCraft sweeps one distance on one side"
            ))
        }
        None => return Err("its extent is malformed".to_owned()),
    };
    let direction = v3(definition.get("direction")).ok_or("its travel direction is not given")?;
    let norm = dot(direction, direction).sqrt();
    if norm < 1e-9 {
        return Err("its travel direction is not given".to_owned());
    }
    let along = dot(direction, sketch.normal) / norm;
    if along.abs() < 1.0 - 1e-6 {
        return Err("a contour flange oblique to its sketch is not supported yet".to_owned());
    }
    // The signed length runs from the profile plane along the stated direction.
    let reverse = length * along < 0.0;
    let flip = match definition.get("side").and_then(Value::as_str) {
        // SolidCraft puts the sheet to the right of the chain's travel by default.
        Some("right") => false,
        Some("left") => true,
        Some("centered") => {
            return Err(
                "a contour flange centered on its profile is not supported yet; SolidCraft puts the sheet on one side"
                    .to_owned(),
            )
        }
        _ => return Err("the side of its profile the sheet lies on is not given".to_owned()),
    };
    let thickness = number(definition.get("thickness"))
        .filter(|t| *t > 0.0)
        .ok_or("its thickness is not given")?;
    let radius = number(definition.get("inside_radius"))
        .filter(|r| *r >= 0.0)
        .ok_or("its inside bend radius is not given")?;
    let k_factor = number(definition.get("neutral_factor"));
    let sketch = sketch.feature;
    let rule = ctx.sheet_rule_of(thickness, Some(radius), k_factor);
    let id = ctx.id();
    ctx.builds = true;
    let mut out = json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("ContourFlange{id}")),
        "type": "sheet_contour",
        "suppressed": false,
        "sketch": sketch,
        "curves": chain,
        "distance": format!("{} mm", super::helpers::clean(length.abs())),
        "rule": rule,
    });
    if let Some(fields) = out.as_object_mut() {
        if flip {
            fields.insert("flip".to_owned(), Value::Bool(true));
        }
        if reverse {
            fields.insert("reverse".to_owned(), Value::Bool(true));
        }
    }
    Ok(out)
}

/// The SolidCraft curve ids of `entities` as one open chain of lines and arcs, each curve
/// reaching the next, in the order the source lists them (so the first curve is where the
/// chain's travel starts, which SolidCraft follows and the sheet's side is judged against).
fn open_chain(sketch: &super::sketch::Mapped, entities: &[Value]) -> Result<Vec<String>, String> {
    // Each source curve occurs once. Bound allocation by the already mapped sketch,
    // including for a hostile selection that repeats a curve many times.
    if entities.len() > sketch.shapes.len() {
        return Err("its profile repeats sketch curves".to_owned());
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut curves = Vec::new();
    for entity in entities {
        let id = entity
            .as_str()
            .ok_or("its profile entities are malformed")?;
        if !seen.insert(id) {
            return Err("its profile repeats sketch curves".to_owned());
        }
        let curve = sketch
            .curve(id)
            .ok_or("its profile uses a curve the sketch does not carry")?;
        let pieces = if curve.pieces.is_empty() {
            std::slice::from_ref(curve)
        } else {
            curve.pieces.as_slice()
        };
        if pieces
            .iter()
            .any(|piece| !matches!(piece.kind, "line" | "arc"))
        {
            return Err(
                "a contour flange along anything but sketch lines and arcs is not supported yet"
                    .to_owned(),
            );
        }
        let (Some(start), Some(end)) = (curve.start, curve.end) else {
            return Err("its profile uses a curve whose ends are not known".to_owned());
        };
        curves.push((curve, start, end));
    }
    let position = |index: usize| sketch.points.get(index).map(|(_, at)| *at);
    let same = |a: usize, b: usize| {
        a == b
            || matches!((position(a), position(b)), (Some(p), Some(q))
                if (p[0] - q[0]).abs() < 1e-7 && (p[1] - q[1]).abs() < 1e-7)
    };
    // Which end of each curve the travel enters it by.
    let mut forward = Vec::new();
    if let [first, second, ..] = curves.as_slice() {
        let (_, a, b) = *first;
        let (_, c, d) = *second;
        let touches = |p: usize| same(p, c) || same(p, d);
        forward.push(match (touches(b), touches(a)) {
            (true, false) => true,
            (false, true) => false,
            _ => return Err("its profile curves do not form an open chain".to_owned()),
        });
    } else {
        forward.push(true);
    }
    let first_forward = forward.first().copied().unwrap_or(true);
    let mut at = curves
        .first()
        .map(|&(_, a, b)| if first_forward { b } else { a });
    for &(_, a, b) in curves.iter().skip(1) {
        let here = at.ok_or("its profile is empty")?;
        if same(here, a) {
            forward.push(true);
            at = Some(b);
        } else if same(here, b) {
            forward.push(false);
            at = Some(a);
        } else {
            return Err("its profile curves do not form an open chain".to_owned());
        }
    }
    let first = curves
        .first()
        .map(|&(_, a, b)| if first_forward { a } else { b });
    if let (Some(first), Some(last)) = (first, at) {
        if same(first, last) {
            return Err("a contour flange along a closed profile is not supported yet".to_owned());
        }
    }
    let mut ids = Vec::new();
    for ((curve, ..), forward) in curves.iter().zip(forward) {
        let mut each = curve.ids();
        if !forward {
            each.reverse();
        }
        ids.extend(each);
    }
    Ok(ids)
}

/// A flange on a proven perimeter edge of the sole, unchanged rectangular base sheet.
/// Selection reads only this feature's input state; the final B-rep is never consulted.
pub(super) fn edge_flange(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    preceding: &[Value],
) -> Result<Value, String> {
    let selection = definition
        .get("edges")
        .ok_or("its edge selection is missing")?;
    let (state, selected) = selected_edges(feature, selection, ctx)?;
    if definition.pointer("/height/kind").and_then(Value::as_str) != Some("distance") {
        return Err("a flange height ending at an object is not supported yet".to_owned());
    }
    if definition.get("height_datum").and_then(Value::as_str) != Some("outer_faces") {
        return Err("a flange height measured from inner faces is not supported yet".to_owned());
    }
    if definition.pointer("/width/kind").and_then(Value::as_str) != Some("full_edge") {
        return Err("a flange spanning only part of its edge is not supported yet".to_owned());
    }
    let position = match definition.get("bend_position").and_then(Value::as_str) {
        Some("inside") => "inside",
        Some("outside") => "outside",
        _ => return Err("this flange bend position is not supported yet".to_owned()),
    };
    let height = number(definition.pointer("/height/value"))
        .filter(|v| *v > 0.0)
        .ok_or("its flange height is not given")?;
    let angle = number(definition.get("angle"))
        .filter(|v| *v > 0.0 && *v < std::f64::consts::PI)
        .ok_or("a flange angle outside zero to 180 degrees is not supported yet")?;
    let radius = number(definition.get("bend_radius"))
        .filter(|v| *v > 0.0)
        .ok_or("its flange bend radius is not given")?;
    let base = base_edges(ctx, preceding, false)?;
    let picks: Vec<V3> = prove_picks(state, selected, &base)?
        .into_iter()
        .map(|pick| pick.point)
        .collect();
    let id = ctx.id();
    Ok(json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Flange{id}")),
        "type": "sheet_flange", "suppressed": false,
        "edges": picks, "height": format!("{height} mm"),
        "angle": format!("{} deg", angle.to_degrees()),
        "radius": format!("{radius} mm"), "position": position,
    }))
}

/// One proven input edge of the rebuilt base sheet.
struct Pick {
    /// The input edge's midpoint.
    point: V3,
    /// Its index among the rebuilt edges: sketch perimeter, then far perimeter, then thickness.
    index: usize,
    /// The rebuilt edge's end points.
    ends: (V3, V3),
}

/// The input topology row and the selected edge ids of a historical selection.
fn selected_edges<'a, 'b>(
    feature: &Value,
    selection: &'b Value,
    ctx: &Context<'a>,
) -> Result<(&'a Value, &'b [Value]), String> {
    if selection.get("kind").and_then(Value::as_str) != Some("historical") {
        return Err("its edge selection needs this feature's input edge geometry".to_owned());
    }
    let state = selection
        .pointer("/value/state")
        .and_then(Value::as_str)
        .and_then(|id| ctx.row("feature_input_topologies", id))
        .filter(|state| state.get("input_of") == feature.get("id"))
        .ok_or("its selected input topology does not belong to this feature")?;
    let selected = selection
        .pointer("/value/edges")
        .and_then(Value::as_array)
        .filter(|edges| !edges.is_empty() && edges.len() <= 4)
        .ok_or("a flange needs one to four resolved input edges")?;
    Ok((state, selected))
}

/// Proves each selected input edge is exactly one rebuilt edge of the base sheet.
fn prove_picks(state: &Value, selected: &[Value], base: &Base) -> Result<Vec<Pick>, String> {
    let geometry = state
        .get("edge_geometry")
        .and_then(Value::as_array)
        .filter(|geometry| !geometry.is_empty())
        .ok_or("its input topology has no edge geometry")?;
    let members = state
        .get("edges")
        .and_then(Value::as_array)
        .filter(|members| !members.is_empty())
        .ok_or("its input topology has no edges")?;
    let mut picks = Vec::new();
    let mut taken = Vec::new();
    for edge in selected {
        if !members.contains(edge) {
            return Err("its selected edge is absent from its input topology".to_owned());
        }
        let mut rows = geometry.iter().filter(|row| row.get("edge") == Some(edge));
        let row = rows
            .next()
            .ok_or("its selected input edge has no geometry")?;
        if rows.next().is_some() {
            return Err("its selected input edge has ambiguous geometry".to_owned());
        }
        let point = v3(row.get("interior")).ok_or("its input edge midpoint is missing")?;
        let start = v3(row.get("start")).ok_or("its input edge start vertex is missing")?;
        let end = v3(row.get("end")).ok_or("its input edge end vertex is missing")?;
        let tangent = v3(row.get("tangent")).ok_or("its input edge tangent is missing")?;
        let matches: Vec<_> = base
            .edges
            .iter()
            .enumerate()
            .filter(|(_, (a, b))| on_segment(point, *a, *b))
            .collect();
        let (index, (a, b)) = match matches.as_slice() {
            [only] => *only,
            _ => {
                return Err(
                    "its input edge midpoint does not identify exactly one rebuilt edge".to_owned(),
                )
            }
        };
        // The final edges join the two faces through the thickness and cannot grow a flange.
        if index >= 2 * base.perimeter || taken.contains(&(index % base.perimeter)) {
            return Err("its input edges repeat a pick or select a thickness edge".to_owned());
        }
        let direction = difference(*b, *a);
        let length = dot(direction, direction).sqrt();
        if !((near(start, *a) && near(end, *b)) || (near(start, *b) && near(end, *a)))
            || (dot(tangent, direction).abs() / length - 1.0).abs() > 1e-9
        {
            return Err(
                "its input edge vertices or tangent differ from the rebuilt edge".to_owned(),
            );
        }
        taken.push(index % base.perimeter);
        picks.push(Pick {
            point,
            index,
            ends: (*a, *b),
        });
    }
    Ok(picks)
}

/// The rebuilt base sheet and the numbers a flange on it needs.
struct Base {
    edges: Vec<(V3, V3)>,
    thickness: f64,
    /// Unit direction the sheet grows from its sketch face.
    growth: V3,
    /// A point on the sketch face.
    origin: V3,
    /// Number of edges on each face.
    perimeter: usize,
    /// Outward unit direction across each sketch perimeter edge.
    outward: Vec<V3>,
}

/// Enumerates the pre-feature body edges from the mapped base sketch.
/// Refuse other histories rather than assume their final edges survived earlier operations.
fn base_edges(ctx: &Context, preceding: &[Value], attached: bool) -> Result<Base, String> {
    let mut solids = preceding
        .iter()
        .filter(|f| f.get("type").and_then(Value::as_str) != Some("sketch"));
    let base = solids
        .next()
        .filter(|f| f.get("type").and_then(Value::as_str) == Some("sheet_base"))
        .ok_or("an edge flange on a body that is not a base sheet is not supported yet")?;
    if solids.next().is_some() {
        return Err(
            "an edge flange on a base sheet that earlier features changed is not supported yet"
                .to_owned(),
        );
    }
    let reason = if attached {
        "an attached plate needs one closed, non-crossing base perimeter with distinct straight edges; curved, disconnected, or collinear base edges are not supported yet"
    } else {
        "an edge flange on a base sheet that is not one plain rectangle is not supported yet"
    };
    if !attached && base.pointer("/profiles/type").and_then(Value::as_str) != Some("all") {
        return Err(reason.to_owned());
    }
    let sketch = ctx
        .sketches
        .values()
        .find(|sketch| Some(sketch.feature) == base.get("sketch").and_then(Value::as_u64))
        .ok_or(reason)?;
    let mapped = preceding
        .iter()
        .find(|f| f.get("id") == base.get("sketch"))
        .ok_or(reason)?;
    let curves = mapped
        .pointer("/sketch/curves")
        .and_then(Value::as_array)
        .ok_or(reason)?;
    let ring = if attached {
        profile_ring(sketch, curves, base.get("profiles").ok_or(reason)?).ok_or(reason)?
    } else {
        rectangular_ring(sketch, curves, reason)?
    };
    let rule = base.get("rule").and_then(Value::as_str).ok_or(reason)?;
    let thickness = ctx
        .sheet_rules
        .iter()
        .find(|known| known.name == rule)
        .map(|known| known.thickness)
        .ok_or(reason)?;
    let sign = if base.get("flip").and_then(Value::as_bool) == Some(true) {
        -1.0
    } else {
        1.0
    };
    let shift = |p: V3| -> V3 {
        let [x, y, z] = p;
        let [nx, ny, nz] = sketch.normal;
        [
            x + sign * thickness * nx,
            y + sign * thickness * ny,
            z + sign * thickness * nz,
        ]
    };
    let perimeter = ring.len();
    let area = super::crossing::signed_area(ring.iter().copied());
    let mut outward = Vec::new();
    for (a, b) in &ring {
        let direction = difference(sketch.world(*b), sketch.world(*a));
        outward.push(scale(
            cross(direction, sketch.normal),
            area.signum() / dot(direction, direction).sqrt(),
        ));
    }
    let mut edges = Vec::new();
    for top in [false, true] {
        for (a, b) in &ring {
            let (a, b) = (sketch.world(*a), sketch.world(*b));
            edges.push(if top { (shift(a), shift(b)) } else { (a, b) });
        }
    }
    for (p, _) in &ring {
        let p = sketch.world(*p);
        edges.push((p, shift(p)));
    }
    let [nx, ny, nz] = sketch.normal;
    Ok(Base {
        edges,
        thickness,
        growth: [sign * nx, sign * ny, sign * nz],
        origin: sketch.origin,
        perimeter,
        outward,
    })
}

/// Preserve the existing rectangular edge-flange proof and edge ordering.
fn rectangular_ring(
    sketch: &super::sketch::Mapped,
    curves: &[Value],
    reason: &str,
) -> Result<Vec<(P2, P2)>, String> {
    if curves.len() != 4
        || curves
            .iter()
            .any(|c| c.get("construction").and_then(Value::as_bool) == Some(true))
    {
        return Err(reason.to_owned());
    }
    let mut lines = Vec::new();
    for shape in sketch.shapes.values() {
        let Shape::Curve(curve) = shape else {
            continue;
        };
        if curve.kind != "line" {
            return Err(reason.to_owned());
        }
        let a = sketch
            .points
            .get(curve.start.ok_or(reason)?)
            .ok_or(reason)?
            .1;
        let b = sketch.points.get(curve.end.ok_or(reason)?).ok_or(reason)?.1;
        lines.push((a, b));
    }
    if lines.len() != 4 {
        return Err(reason.to_owned());
    }
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for (a, b) in &lines {
        for (i, (lo, hi)) in min.iter_mut().zip(max.iter_mut()).enumerate() {
            for p in [a, b] {
                let v = p.get(i).copied().ok_or(reason)?;
                *lo = lo.min(v);
                *hi = hi.max(v);
            }
        }
    }
    let [x0, y0] = min;
    let [x1, y1] = max;
    if x1 - x0 < 1e-6 || y1 - y0 < 1e-6 {
        return Err(reason.to_owned());
    }
    let corners = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
    let ring: Vec<_> = corners
        .iter()
        .copied()
        .zip(corners.iter().copied().cycle().skip(1))
        .collect();
    for (a, b) in &ring {
        if lines
            .iter()
            .filter(|(c, d)| (a == c && b == d) || (a == d && b == c))
            .count()
            != 1
        {
            return Err(reason.to_owned());
        }
    }
    Ok(ring)
}

/// Read actual emitted profile lines, ignoring construction geometry. Only a single simple
/// perimeter is admitted: holes, disconnected regions, curves, and merged collinear edges
/// need a different proof of which edges SolidCraft builds.
fn profile_ring(
    sketch: &super::sketch::Mapped,
    curves: &[Value],
    selection: &Value,
) -> Option<Vec<(P2, P2)>> {
    if curves
        .iter()
        .filter(|curve| curve.get("construction").and_then(Value::as_bool) != Some(true))
        .count()
        > super::crossing::MAX_LINES
    {
        return None;
    }
    // Explicit selections are safe here only when they take the same single perimeter as
    // the whole emitted sketch. Extra solid curves could subdivide or merge its edges.
    let selected = match selection.get("type")?.as_str()? {
        "all" => None,
        "curves" => {
            let [boundary] = selection.get("loops")?.as_array()?.as_slice() else {
                return None;
            };
            Some(boundary.as_array()?)
        }
        "whole_boundaries" => {
            let [region] = selection.get("profiles")?.as_array()?.as_slice() else {
                return None;
            };
            if !region.get("holes")?.as_array()?.is_empty() {
                return None;
            }
            Some(region.get("outer")?.as_array()?)
        }
        _ => return None,
    };
    if selected.is_some_and(|selected| selected.len() > super::crossing::MAX_LINES) {
        return None;
    }
    let by_id: std::collections::BTreeMap<_, _> = sketch
        .shapes
        .values()
        .filter_map(|shape| match shape {
            Shape::Curve(curve) => Some((curve.id.as_str(), curve)),
            _ => None,
        })
        .collect();
    let mut lines = Vec::new();
    for curve in curves {
        if curve.get("construction").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        let id = curve.get("id")?.as_str()?;
        if selected.is_some_and(|selected| {
            selected
                .iter()
                .filter(|pick| pick.as_str() == Some(id))
                .count()
                != 1
        }) {
            return None;
        }
        let mapped = by_id.get(id)?;
        if mapped.kind != "line" {
            return None;
        }
        lines.push((
            sketch.points.get(mapped.start?)?.1,
            sketch.points.get(mapped.end?)?.1,
        ));
    }
    if selected.is_some_and(|selected| selected.len() != lines.len()) {
        return None;
    }
    let ring = super::crossing::line_ring(&lines)?;
    Some(
        ring.iter()
            .copied()
            .zip(ring.iter().copied().cycle().skip(1))
            .collect(),
    )
}

fn difference(a: V3, b: V3) -> V3 {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ax - bx, ay - by, az - bz]
}

fn near(a: V3, b: V3) -> bool {
    let d = difference(a, b);
    dot(d, d) < 1e-12
}

fn on_segment(p: V3, a: V3, b: V3) -> bool {
    let d = difference(b, a);
    let t = dot(difference(p, a), d) / dot(d, d);
    let [x, y, z] = a;
    let [dx, dy, dz] = d;
    (0.0..=1.0).contains(&t) && near(p, [x + t * dx, y + t * dy, z + t * dz])
}

/// CADIR hems retain developed lengths; SolidCraft accepts an outer-face length.
pub(super) fn hem_refusal(definition: &Value) -> String {
    let reason = match definition.get("direction").and_then(Value::as_str) {
        Some("unresolved") => "a hem with unresolved fold direction is not supported yet",
        _ => match definition.pointer("/form/kind").and_then(Value::as_str) {
            Some("flat" | "open") => {
                "a hem specified by developed length is not supported yet; SolidCraft requires length to the outer face"
            }
            Some("gap_length") => "an unresolved flat-or-open hem is not supported yet",
            Some("rolled") => "a rolled hem is not supported yet",
            Some("teardrop") => "a teardrop hem is not supported yet",
            _ => "this hem form is not supported yet",
        },
    };
    reason.to_owned()
}

/// A plate attached to a sheet edge together with its paired bend, written as one 90 degree
/// edge flange. SolidCraft has no feature that joins a separate plate, so only a rectangular
/// plate that spans the whole edge, stands square to the sheet and starts where the bend ends
/// is the same shape as an edge flange. The bend feature is consumed with its plate.
pub(super) fn attached_plate(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    preceding: &[Value],
) -> Result<Value, String> {
    const TOL: f64 = 1e-4;
    let selection = definition
        .get("attachment")
        .ok_or("an attached plate has no selected sheet edge to attach to")?;
    let (state, selected) = selected_edges(feature, selection, ctx)?;
    let plate_id = feature.get("id").and_then(Value::as_str);
    let bend = ctx
        .model
        .get("features")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find(|f| {
            let op = super::operation(f);
            super::operation_kind(f) == Some("sheet_metal_bend")
                && op.get("plate").and_then(Value::as_str) == plate_id
        })
        .ok_or("its paired bend is missing, so the plate has no joining region")?;
    let bend_id = bend
        .get("id")
        .and_then(Value::as_str)
        .ok_or("its paired bend has no id")?
        .to_owned();
    let radius = number(super::operation(bend).get("inside_radius"))
        .filter(|r| *r > 0.0)
        .ok_or("its bend radius is not given")?;
    let thickness = number(definition.get("thickness"))
        .filter(|t| *t > 0.0)
        .ok_or("its thickness is not given")?;
    let base = base_edges(ctx, preceding, true)?;
    if (thickness - base.thickness).abs() > TOL {
        return Err("its thickness differs from the base sheet's".to_owned());
    }
    let picks = prove_picks(state, selected, &base)?;
    let (pick, others) = picks
        .split_first()
        .ok_or("a plate needs at least one attached edge")?;
    let (sketch_id, _) = profile(definition.get("profile"), ctx)?;
    let sketch = ctx
        .sketches
        .get(&sketch_id)
        .ok_or("its sketch is not an earlier mapped sketch")?;
    let corners = rectangle(sketch)
        .ok_or("an attached plate whose profile is not one rectangle is not supported yet")?;
    let (a, b) = pick.ends;
    let along = difference(b, a);
    let length = dot(along, along).sqrt();
    // Inventor builds the plate once on each attached edge: the same rectangle, standing
    // square on the same face of the sheet. A further edge is the same flange only when it
    // is as long as the first and on the same face.
    for other in others {
        let (c, d) = other.ends;
        let long = difference(d, c);
        if (dot(long, long).sqrt() - length).abs() > TOL
            || (other.index >= 4) != (pick.index >= 4)
            || dot(long, along).abs() / (length * length) < 1.0 - 1e-6
        {
            return Err(
                "a plate attached along edges of different lengths or faces is not supported yet"
                    .to_owned(),
            );
        }
    }
    let e = scale(along, 1.0 / length);
    let growth = base.growth;
    let normal = sketch.normal;
    if dot(normal, e).abs() > 1e-6 || dot(normal, growth).abs() > 1e-6 {
        return Err("a plate not square to the base sheet is not supported yet".to_owned());
    }
    // The profile's winding supplies the local outward direction even at a concave edge.
    let out = *base
        .outward
        .get(pick.index % base.perimeter)
        .ok_or("its picked edge has no outward direction")?;
    let offset = dot(difference(sketch.origin, a), out);
    if offset.abs() > TOL && (offset + thickness).abs() > TOL {
        return Err(
            "a plate not flush with the end face of the sheet is not supported yet".to_owned(),
        );
    }
    // The flange rises from the picked face: up from the far face, down from the sketch face.
    let top = pick.index >= base.perimeter;
    let rise = if top { growth } else { scale(growth, -1.0) };
    let outer = if top {
        base.origin
    } else {
        let [x, y, z] = base.origin;
        let [gx, gy, gz] = growth;
        [x + thickness * gx, y + thickness * gy, z + thickness * gz]
    };
    let mut near = f64::INFINITY;
    let mut far = f64::NEG_INFINITY;
    for corner in corners {
        let at = dot(difference(corner, a), e);
        if at.abs() > TOL && (at - length).abs() > TOL {
            return Err(
                "a plate that does not span the whole edge is not supported yet".to_owned(),
            );
        }
        let up = dot(difference(corner, outer), rise);
        near = near.min(up);
        far = far.max(up);
    }
    // The plate starts where its bend ends, or, as Inventor stores a flange whose height is
    // measured from where the outer faces meet, at the outer face itself; either way its far
    // end is the flange's height from the outer face.
    if (near - (radius + thickness)).abs() > TOL && near.abs() > TOL {
        return Err(format!(
            "a plate that does not start where its 90 degree bend ends is not supported yet \
             (it starts {near:.4} mm from the sheet's outer face, the bend ends at {:.4} mm)",
            radius + thickness
        ));
    }
    ctx.consumed_bends.insert(bend_id);
    let id = ctx.id();
    Ok(json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Flange{id}")),
        "type": "sheet_flange", "suppressed": false,
        "edges": picks.iter().map(|pick| pick.point).collect::<Vec<_>>(),
        "height": format!("{far} mm"),
        "angle": "90 deg", "radius": format!("{radius} mm"), "position": "inside",
    }))
}

/// The four world-space corners of a sketch that is exactly one rectangle of four lines.
fn rectangle(sketch: &super::sketch::Mapped) -> Option<[V3; 4]> {
    let mut corners: Vec<V3> = Vec::new();
    let mut lines = Vec::new();
    for shape in sketch.shapes.values() {
        let Shape::Curve(curve) = shape else {
            continue;
        };
        if curve.kind != "line" {
            return None;
        }
        let a = sketch.world(sketch.points.get(curve.start?)?.1);
        let b = sketch.world(sketch.points.get(curve.end?)?.1);
        for p in [a, b] {
            if !corners.iter().any(|c| near(*c, p)) {
                corners.push(p);
            }
        }
        lines.push((a, b));
    }
    let [c0, c1, c2, c3] = <[V3; 4]>::try_from(corners).ok()?;
    if lines.len() != 4 {
        return None;
    }
    // The corner farthest from the first is its diagonal; the other two are its neighbours.
    let mut rest = [c1, c2, c3];
    rest.sort_by(|p, q| {
        let (dp, dq) = (difference(*p, c0), difference(*q, c0));
        dot(dp, dp).total_cmp(&dot(dq, dq))
    });
    let [n1, n2, diagonal] = rest;
    let (s1, s2) = (difference(n1, c0), difference(n2, c0));
    let scale_of = dot(s1, s1).max(dot(s2, s2));
    if scale_of < 1e-12
        || dot(s1, s2).abs() > 1e-9 * scale_of
        || !near(
            diagonal,
            [
                c0[0] + s1[0] + s2[0],
                c0[1] + s1[1] + s2[1],
                c0[2] + s1[2] + s2[2],
            ],
        )
    {
        return None;
    }
    let (l1, l2) = (dot(s1, s1).sqrt(), dot(s2, s2).sqrt());
    let sides = lines.iter().all(|(a, b)| {
        let d = difference(*b, *a);
        let len = dot(d, d).sqrt();
        (len - l1).abs() < 1e-6 || (len - l2).abs() < 1e-6
    });
    sides.then_some([c0, n1, diagonal, n2])
}

fn scale(a: V3, k: f64) -> V3 {
    let [x, y, z] = a;
    [x * k, y * k, z * k]
}

/// The reason a bend not consumed by its plate cannot be mapped.
pub(super) fn bend_refusal() -> String {
    "a bend whose attached plate was not written with it as an edge flange is not supported yet"
        .to_owned()
}

/// A sheet rule a sheet-metal feature uses: its thickness, and the bend radius and K-factor
/// when the feature states them.
#[derive(Clone)]
pub(super) struct SheetRule {
    pub(super) thickness: f64,
    pub(super) radius: Option<f64>,
    pub(super) k_factor: Option<f64>,
    pub(super) name: String,
}

impl SheetRule {
    /// The SolidCraft rule.
    pub(super) fn value(&self) -> Value {
        rule(&self.name, self.thickness, self.radius, self.k_factor)
    }
}

/// A SolidCraft sheet rule of `thickness` mm, named after it, with SolidCraft's own defaults
/// for everything else but the bend radius and K-factor, where given.
pub(super) fn rule(
    name: &str,
    thickness: f64,
    radius: Option<f64>,
    k_factor: Option<f64>,
) -> Value {
    json!({
        "name": name,
        "thickness": format!("{thickness} mm"),
        "bend_radius": radius.map_or_else(|| "Thickness".to_owned(), |r| format!("{r} mm")),
        "k_factor": k_factor.map_or_else(|| "0.44".to_owned(), |k| k.to_string()),
        "relief_width": "Thickness",
        "relief_depth": "Thickness * 0.5",
        "corner_relief": "Thickness * 4.0",
        "corner_relief_shape": "notch",
        "hem_gap": "0.01 mm",
        "gap": "Thickness",
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use std::collections::BTreeMap;

    use super::super::sketch::{Curve, Mapped, Shape};
    use super::*;

    fn line(index: usize, a: usize, b: usize) -> Shape {
        Shape::Curve(Curve {
            pieces: Vec::new(),
            index,
            id: format!("l{}", index + 1),
            kind: "line",
            start: Some(a),
            end: Some(b),
            center: None,
        })
    }

    /// A sketch on XY with the open chain (0,0) - (20,0) - (20,15) as entities e1, e2, a
    /// detached line e3 and the closing line e4 back to the start.
    fn sketch() -> Mapped {
        let shapes = [
            ("e1", line(0, 1, 2)),
            ("e2", line(1, 2, 3)),
            ("e3", line(2, 4, 5)),
            ("e4", line(3, 3, 1)),
        ];
        Mapped {
            feature: 7,
            normal: [0.0, 0.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            x_dir: [1.0, 0.0, 0.0],
            y_dir: [0.0, 1.0, 0.0],
            point_displacement: 0.0,
            points: vec![
                ("origin".to_owned(), [0.0, 0.0]),
                ("l1.start".to_owned(), [0.0, 0.0]),
                ("l1.end".to_owned(), [20.0, 0.0]),
                ("l2.end".to_owned(), [20.0, 15.0]),
                ("l3.start".to_owned(), [50.0, 0.0]),
                ("l3.end".to_owned(), [60.0, 0.0]),
            ],
            shapes: shapes
                .into_iter()
                .map(|(id, shape)| (id.to_owned(), shape))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn definition(change: Value) -> Value {
        let mut full = json!({
            "profile": chain(&["e1", "e2"]),
            "direction": {"x": 0.0, "y": 0.0, "z": 1.0},
            "extent": travel(10.0),
            "thickness": 2.0,
            "inside_radius": 2.0,
            "side": "right",
            "neutral_factor": 0.4,
        });
        for (key, value) in change.as_object().unwrap() {
            full[key] = value.clone();
        }
        json!({"definition": "sheet_metal_contour_flange", "construction": full})
    }

    fn chain(entities: &[&str]) -> Value {
        json!({"kind": "sketch_entities", "value": {"sketch": "s", "entities": entities}})
    }

    fn travel(length: f64) -> Value {
        json!({"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": length}}})
    }

    fn mapped(change: Value) -> Result<(Value, Vec<Value>), String> {
        let model = json!({});
        let mut ctx = Context::new(&model);
        ctx.sketches.insert("s".to_owned(), sketch());
        let out = contour_flange(&json!({"name": "Contour"}), &definition(change), &mut ctx)?;
        Ok((out, ctx.sheet_rules.iter().map(SheetRule::value).collect()))
    }

    #[test]
    fn a_contour_flange_is_a_sheet_contour_under_a_rule_of_its_bend() {
        let (flange, rules) = mapped(json!({})).unwrap();
        assert_eq!(flange["type"], "sheet_contour");
        assert_eq!(flange["sketch"], 7);
        assert_eq!(flange["curves"], json!(["l1", "l2"]));
        assert_eq!(flange["distance"], "10 mm");
        assert_eq!(flange["rule"], "Sheet 2 mm R2 K0.4");
        assert!(flange.get("flip").is_none());
        assert!(flange.get("reverse").is_none());
        assert_eq!(rules[0]["thickness"], "2 mm");
        assert_eq!(rules[0]["bend_radius"], "2 mm");
        assert_eq!(rules[0]["k_factor"], "0.4");
    }

    #[test]
    fn a_contour_flange_keeps_its_side_and_travel() {
        // CADIR's left of the chain is SolidCraft's flip; travel against the direction reverses.
        assert_eq!(mapped(json!({"side": "left"})).unwrap().0["flip"], true);
        let back = mapped(json!({"extent": travel(-10.0)})).unwrap().0;
        assert_eq!(back["reverse"], true);
        assert_eq!(back["distance"], "10 mm");
        let down = json!({"x": 0.0, "y": 0.0, "z": -1.0});
        assert_eq!(
            mapped(json!({"direction": down})).unwrap().0["reverse"],
            true
        );
        let both = mapped(json!({"direction": down, "extent": travel(-10.0)}))
            .unwrap()
            .0;
        assert!(both.get("reverse").is_none());
        // The chain runs in the order the source lists it, each curve entered by its shared end.
        let backwards = mapped(json!({"profile": chain(&["e2", "e1"])})).unwrap().0;
        assert_eq!(backwards["curves"], json!(["l2", "l1"]));
        let one = mapped(json!({"profile": chain(&["e2"])})).unwrap().0;
        assert_eq!(one["curves"], json!(["l2"]));
    }

    #[test]
    fn a_contour_flange_solidcraft_cannot_hold_is_refused_with_its_reason() {
        let blind = json!({"termination": {"kind": "blind", "length": 10.0}});
        for (change, reason) in [
            (
                json!({"side": "centered"}),
                "a contour flange centered on its profile is not supported yet",
            ),
            (
                json!({"extent": {"kind": "symmetric", "side": blind}}),
                "a symmetric contour flange is not supported yet",
            ),
            (
                json!({"extent": {"kind": "one_sided",
                    "side": {"termination": {"kind": "through_all"}}}}),
                "a contour flange whose length ends through_all is not supported yet",
            ),
            (
                json!({"extent": {"kind": "one_sided", "side": {
                    "termination": {"kind": "blind", "length": 10.0}, "draft": 0.1}}}),
                "a drafted contour flange is not supported yet",
            ),
            (
                json!({"direction": {"x": 1.0, "y": 0.0, "z": 1.0}}),
                "a contour flange oblique to its sketch is not supported yet",
            ),
            (
                json!({"profile": chain(&["e1", "e3"])}),
                "its profile curves do not form an open chain",
            ),
            (
                json!({"profile": chain(&["e1", "e2", "e4"])}),
                "a contour flange along a closed profile is not supported yet",
            ),
            (
                json!({"profile": {"kind": "sketch_profiles",
                    "value": {"sketch": "s", "profiles": [0]}}}),
                "a contour flange whose profile is sketch_profiles",
            ),
            (json!({"thickness": null}), "its thickness is not given"),
            (
                json!({"inside_radius": null}),
                "its inside bend radius is not given",
            ),
        ] {
            let error = mapped(change.clone()).unwrap_err();
            assert!(error.contains(reason), "{change}: {error}");
        }
    }

    #[test]
    fn contour_flanges_of_one_bend_share_a_rule() {
        let model = json!({});
        let mut ctx = Context::new(&model);
        ctx.sketches.insert("s".to_owned(), sketch());
        let default = ctx.sheet_rule(2.0);
        for change in [
            json!({}),
            json!({}),
            json!({"inside_radius": 3.0}),
            json!({"neutral_factor": 0.5}),
        ] {
            contour_flange(&json!({}), &definition(change), &mut ctx).unwrap();
        }
        assert_eq!(ctx.sheet_rules.len(), 4);
        assert_eq!(ctx.sheet_rule(2.0), default);
        let base = ctx.sheet_rules[0].value();
        assert_eq!(base["name"], "Sheet 2 mm");
        assert_eq!(base["bend_radius"], "Thickness");
        assert_eq!(base["k_factor"], "0.44");
    }

    #[test]
    fn contour_profiles_reject_repeated_curves_and_single_closed_curves() {
        assert!(mapped(json!({"profile": chain(&["e1", "e2", "e1"])}))
            .unwrap_err()
            .contains("repeats sketch curves"));
        let mut sketch = sketch();
        if let Some(Shape::Curve(curve)) = sketch.shapes.get_mut("e1") {
            curve.end = curve.start;
        }
        assert!(open_chain(&sketch, &[json!("e1")])
            .unwrap_err()
            .contains("closed profile"));
    }
}
