//! Sheet-metal base flanges, and edge flanges and attached plates with their bends on proven
//! input-state edges. Hems are refused with the reason SolidCraft cannot express them.

use serde_json::{json, Value};

use super::profile::profile;
use super::sketch::{dot, number, unit, v3, Mapped as MappedSketch, Shape, P2, V3};
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
    let base = base_sheet(ctx, preceding)?;
    let picks: Vec<V3> = picks(feature, selection, ctx, &base.edges, 4)?
        .into_iter()
        .map(|(point, _)| point)
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

/// An attached plate maps together with the bend that joins it to the sheet, as one flange
/// at the bend ([`bend`]); a plate no later bend joins is not mapped.
pub(super) fn attached_plate(feature: &Value, ctx: &Context) -> Result<(), String> {
    let id = feature.get("id").ok_or("it has no id")?;
    let joined = ctx.arena("features").iter().any(|later| {
        later
            .pointer("/definition/definition")
            .and_then(Value::as_str)
            == Some("sheet_metal_bend")
            && later.pointer("/definition/plate") == Some(id)
    });
    if joined {
        Ok(())
    } else {
        Err("an attached plate no bend joins to the sheet is not supported yet".to_owned())
    }
}

/// A bend and the attached plate it joins, as one full-edge flange on a proven perimeter edge
/// of the sole, unchanged rectangular base sheet. The plate must be a rectangle standing at
/// right angles to the sheet, as thick as it, spanning the whole edge, flush with the sheet's
/// side on one of its faces and starting at the sheet's outer face; its height is measured
/// from that face, as SolidCraft measures a flange's. Selection reads only this feature's
/// input state; the final B-rep is never consulted.
pub(super) fn bend(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    preceding: &[Value],
) -> Result<Value, String> {
    let plate_row = definition
        .get("plate")
        .and_then(Value::as_str)
        .and_then(|id| ctx.row("features", id))
        .ok_or("the plate it bends is not in the history")?;
    let plate = plate_row.get("definition").unwrap_or(&Value::Null);
    if plate.get("definition").and_then(Value::as_str) != Some("sheet_metal_attached_plate") {
        return Err("the plate it bends is not an attached plate".to_owned());
    }
    let selection = definition
        .get("attachment")
        .ok_or("its edge selection is missing")?;
    let attachment = plate
        .get("attachment")
        .ok_or("its plate's edge selection is missing")?;
    for edges in [selection, attachment].map(|s| s.pointer("/value/edges")) {
        if edges
            .and_then(Value::as_array)
            .is_some_and(|edges| edges.len() > 1)
        {
            return Err("a plate bent onto more than one edge is not supported yet".to_owned());
        }
    }
    let radius = number(definition.get("inside_radius"))
        .filter(|v| *v > 0.0)
        .ok_or("its bend radius is not given")?;
    let base = base_sheet(ctx, preceding)?;
    let thickness = base.thickness;
    if number(plate.get("thickness")).is_none_or(|t| (t - thickness).abs() > 1e-9) {
        return Err("a plate of another thickness than its sheet is not supported yet".to_owned());
    }
    let [(point, index)] = picks(feature, selection, ctx, &base.edges, 1)?[..] else {
        return Err("it needs one resolved input edge".to_owned());
    };
    // The plate and its bend each select the edge in their own input state, so they agree
    // when both selections resolve to the same edge of the rebuilt sheet.
    let [(_, attached)] = picks(plate_row, attachment, ctx, &base.edges, 1)?[..] else {
        return Err("its plate needs one resolved input edge".to_owned());
    };
    if attached != index {
        return Err("its plate is attached to another edge than it bends".to_owned());
    }
    let (a, b) = *base
        .edges
        .get(index)
        .ok_or("its input edge is not on the sheet")?;
    // `along` runs with the edge, `up` the way the flange rises (away from the sheet face the
    // edge bounds), `out` away from the sheet across the edge.
    let along = unit(difference(b, a)).ok_or("its edge has no length")?;
    let up = if index >= 4 {
        base.up
    } else {
        base.up.map(|c| -c)
    };
    let away = difference(point, base.center);
    let out = unit(difference(away, scaled(up, dot(away, up))))
        .ok_or("its edge is not on the sheet's side")?;

    let (sketch, selected) = profile(plate.get("profile"), ctx)?;
    if selected.get("type").and_then(Value::as_str) != Some("all") {
        return Err("a plate that is not one plain rectangle is not supported yet".to_owned());
    }
    let sketch = ctx
        .sketches
        .get(&sketch)
        .ok_or("its plate's sketch is not an earlier mapped sketch")?;
    let reason = "a plate that is not one plain rectangle is not supported yet";
    let corners = rectangle(sketch, preceding, reason)?;
    if (dot(sketch.normal, out).abs() - 1.0).abs() > 1e-9 {
        return Err("a plate not at right angles to its sheet is not supported yet".to_owned());
    }
    // The plate's two faces, as levels along `out`.
    let plane = dot(sketch.origin, out);
    let toward = dot(sketch.normal, out) * thickness;
    let faces = match plate.get("side").and_then(Value::as_str) {
        None | Some("forward") => [plane, plane + toward],
        Some("reverse") => [plane - toward, plane],
        Some(_) => {
            return Err("a plate thick on both sides of its sketch is not supported yet".to_owned())
        }
    };
    let (near, far) = (faces[0].min(faces[1]), faces[0].max(faces[1]));
    let side = dot(point, out);
    let position = if (far - side).abs() < 1e-6 && (near - (side - thickness)).abs() < 1e-6 {
        "inside"
    } else if (near - side).abs() < 1e-6 && (far - (side + thickness)).abs() < 1e-6 {
        "outside"
    } else {
        return Err("a plate not flush with its sheet's side is not supported yet".to_owned());
    };
    let world: Vec<V3> = corners.iter().map(|c| sketch.world(*c)).collect();
    let span = |axis: V3| {
        world
            .iter()
            .map(|p| dot(*p, axis))
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                (lo.min(v), hi.max(v))
            })
    };
    let (start, end) = span(along);
    let (edge_start, edge_end) = {
        let (x, y) = (dot(a, along), dot(b, along));
        (x.min(y), x.max(y))
    };
    if (start - edge_start).abs() > 1e-6 || (end - edge_end).abs() > 1e-6 {
        return Err("a plate spanning only part of its edge is not supported yet".to_owned());
    }
    let (bottom, top) = span(up);
    let outer = dot(point, up) - thickness;
    if (bottom - outer).abs() > 1e-6 {
        return Err(
            "a plate that does not start at its sheet's outer face is not supported yet".to_owned(),
        );
    }
    let height = top - outer;
    if height <= thickness + radius {
        return Err(
            "a plate too short to leave a flat past its bend is not supported yet".to_owned(),
        );
    }
    let id = ctx.id();
    Ok(json!({
        "id": id,
        "name": name(feature).map(str::to_owned).unwrap_or_else(|| format!("Flange{id}")),
        "type": "sheet_flange", "suppressed": false,
        "edges": [point], "height": format!("{} mm", round(height)),
        "angle": "90 deg",
        "radius": format!("{radius} mm"), "position": position,
    }))
}

/// The sole base sheet before a flange: its twelve edges (bottom ring, top ring, then the
/// four thickness edges), thickness, centre, and the unit direction its thickness grows in.
struct Base {
    edges: Vec<(V3, V3)>,
    thickness: f64,
    center: V3,
    up: V3,
}

/// The pre-feature body from the mapped base sketch. Refuse other histories rather than
/// assume their final edges survived earlier operations.
fn base_sheet(ctx: &Context, preceding: &[Value]) -> Result<Base, String> {
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
    let reason =
        "an edge flange on a base sheet that is not one plain rectangle is not supported yet";
    if base.pointer("/profiles/type").and_then(Value::as_str) != Some("all") {
        return Err(reason.to_owned());
    }
    let sketch = ctx
        .sketches
        .values()
        .find(|sketch| Some(sketch.feature) == base.get("sketch").and_then(Value::as_u64))
        .ok_or(reason)?;
    let corners = rectangle(sketch, preceding, reason)?;
    let ring: Vec<_> = corners
        .iter()
        .copied()
        .zip(corners.iter().copied().cycle().skip(1))
        .collect();
    let rule = base.get("rule").and_then(Value::as_str).ok_or(reason)?;
    let thickness = ctx
        .sheet_rules
        .iter()
        .find(|(_, name)| name == rule)
        .map(|(thickness, _)| *thickness)
        .ok_or(reason)?;
    let sign = if base.get("flip").and_then(Value::as_bool) == Some(true) {
        -1.0
    } else {
        1.0
    };
    let up = sketch.normal.map(|c| sign * c);
    let shift = |p: V3| -> V3 {
        let [x, y, z] = p;
        let [nx, ny, nz] = up;
        [x + thickness * nx, y + thickness * ny, z + thickness * nz]
    };
    let mut edges = Vec::new();
    for top in [false, true] {
        for (a, b) in &ring {
            let (a, b) = (sketch.world(*a), sketch.world(*b));
            edges.push(if top { (shift(a), shift(b)) } else { (a, b) });
        }
    }
    for p in corners {
        let p = sketch.world(p);
        edges.push((p, shift(p)));
    }
    let [x0, y0] = corners[0];
    let [x1, y1] = corners[2];
    let middle = sketch.world([(x0 + x1) / 2.0, (y0 + y1) / 2.0]);
    Ok(Base {
        edges,
        thickness,
        center: difference(middle, scaled(up, -thickness / 2.0)),
        up,
    })
}

/// The corners of a mapped sketch that is exactly one axis-aligned rectangle of four lines,
/// counter-clockwise from its lowest corner, or `reason`.
fn rectangle(sketch: &MappedSketch, preceding: &[Value], reason: &str) -> Result<[P2; 4], String> {
    let mapped = preceding
        .iter()
        .find(|f| f.get("id").and_then(Value::as_u64) == Some(sketch.feature))
        .ok_or(reason)?;
    let curves = mapped
        .pointer("/sketch/curves")
        .and_then(Value::as_array)
        .ok_or(reason)?;
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
    for (a, b) in corners
        .iter()
        .copied()
        .zip(corners.iter().copied().cycle().skip(1))
    {
        if lines
            .iter()
            .filter(|(c, d)| (a == *c && b == *d) || (a == *d && b == *c))
            .count()
            != 1
        {
            return Err(reason.to_owned());
        }
    }
    Ok(corners)
}

/// The points picking each edge of `selection`, a historical selection of this feature's own
/// input edges, on the `rebuilt` base sheet, with the index of the rebuilt edge each picks.
/// At most `most` edges, each an outline edge of the sheet picked once, whose stored
/// vertices and tangent are the rebuilt edge's.
fn picks(
    feature: &Value,
    selection: &Value,
    ctx: &Context,
    rebuilt: &[(V3, V3)],
    most: usize,
) -> Result<Vec<(V3, usize)>, String> {
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
        .filter(|edges| !edges.is_empty() && edges.len() <= most)
        .ok_or_else(|| {
            if most == 1 {
                "it needs one resolved input edge".to_owned()
            } else {
                format!("a flange needs one to {most} resolved input edges")
            }
        })?;
    let geometry = state
        .get("edge_geometry")
        .and_then(Value::as_array)
        .ok_or("its input topology has no edge geometry")?;
    let members = state
        .get("edges")
        .and_then(Value::as_array)
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
        let matches: Vec<_> = rebuilt
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
        // The last four edges are the plate's thickness edges, which cannot grow a flange.
        if index >= 8 || taken.contains(&(index % 4)) {
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
        taken.push(index % 4);
        picks.push((point, index));
    }
    Ok(picks)
}

fn scaled(v: V3, by: f64) -> V3 {
    v.map(|c| c * by)
}

/// `value` without floating-point noise below a nanometre.
fn round(value: f64) -> f64 {
    (value * 1e6).round() / 1e6
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

/// A SolidCraft sheet rule of `thickness` mm, named after it, with SolidCraft's own defaults
/// for everything else.
pub(super) fn rule(name: &str, thickness: f64) -> Value {
    json!({
        "name": name,
        "thickness": format!("{thickness} mm"),
        "bend_radius": "Thickness",
        "k_factor": "0.44",
        "relief_width": "Thickness",
        "relief_depth": "Thickness * 0.5",
        "corner_relief": "Thickness * 4.0",
        "corner_relief_shape": "notch",
        "hem_gap": "0.01 mm",
        "gap": "Thickness",
    })
}
