//! Face colours of the bodies a history makes, as SolidCraft face looks.
//!
//! SolidCraft finds a face look's face by its body and a point on the face. A replayed body is
//! built where the source's final body is, so a point inside the source face finds the same
//! face. Points are found for planar faces: the centroid of a triangle of the face's outer
//! loop that lies inside the loop, so concave faces get a point on them too.

use serde_json::{json, Value};

use super::sketch::{dot, v3, V3};
use super::Context;

/// A single resolved planar face on an earlier named body, as a SolidCraft pick.
/// Native, generated and historical selections need their own topology resolver.
pub(super) fn pick(ctx: &Context, selection: Option<&Value>, normal: V3) -> Result<V3, String> {
    let selection = selection.ok_or("its face selection is missing")?;
    let ids = match selection.get("kind").and_then(Value::as_str) {
        Some("faces") => selection.get("value"),
        Some("resolved") => selection.pointer("/value/faces"),
        _ => return Err("its face selection is not resolved to model faces".to_owned()),
    }
    .and_then(Value::as_array)
    .ok_or("its face selection is malformed")?;
    if ids.len() != 1 {
        return Err("a selection without exactly one face is not supported yet".to_owned());
    }
    let id = ids
        .first()
        .and_then(Value::as_str)
        .ok_or("its face is missing")?;
    let face = ctx.row("faces", id).ok_or("its selected face is missing")?;
    let surface = face
        .get("surface")
        .and_then(Value::as_str)
        .and_then(|id| ctx.row("surfaces", id))
        .ok_or("its face surface is missing")?;
    let face_normal = v3(surface.pointer("/geometry/normal"))
        .and_then(super::sketch::unit)
        .ok_or("its face normal is missing")?;
    if surface.pointer("/geometry/kind").and_then(Value::as_str) != Some("plane")
        || dot(face_normal, normal).abs() < 1.0 - 1e-9
    {
        return Err("an extrude face not parallel to its sketch is not supported yet".to_owned());
    }
    let earlier = ctx.named_bodies.iter().any(|(body, _)| {
        ctx.row("bodies", body).is_some_and(|body| {
            faces(ctx, body)
                .iter()
                .any(|face| face.get("id").and_then(Value::as_str) == Some(id))
        })
    });
    if !earlier {
        return Err("its selected face is not on an earlier named body".to_owned());
    }
    // The colour picker approximates loops by their vertices. For a feature end,
    // require a straight polygon without holes so the pick is proven inside it.
    let loops = face
        .pointer("/loops/loops")
        .and_then(Value::as_array)
        .filter(|loops| loops.len() == 1)
        .ok_or("a terminating face with holes is not supported yet")?;
    let lp = loops
        .first()
        .and_then(Value::as_str)
        .and_then(|id| ctx.row("loops", id))
        .ok_or("its face loop is missing")?;
    for coedge in lp
        .pointer("/boundary/coedges")
        .and_then(Value::as_array)
        .ok_or("its face boundary is missing")?
    {
        let edge = coedge
            .as_str()
            .and_then(|id| ctx.row("coedges", id))
            .and_then(|row| row.get("edge"))
            .and_then(Value::as_str)
            .and_then(|id| ctx.row("edges", id))
            .ok_or("its face edge is missing")?;
        let curve = edge
            .pointer("/carrier/curve")
            .and_then(Value::as_str)
            .and_then(|id| ctx.row("curves", id))
            .ok_or("its face curve is missing")?;
        if curve.pointer("/geometry/kind").and_then(Value::as_str) != Some("line") {
            return Err("a terminating face with curved edges is not supported yet".to_owned());
        }
    }
    point_on(ctx, face).ok_or_else(|| "its face has no resolved planar interior point".to_owned())
}

/// A colour of `face` that differs from its body's, as SolidCraft face looks; and how many
/// coloured faces could not be placed.
pub(crate) fn looks(ctx: &Context) -> (Vec<Value>, usize) {
    let mut out = Vec::new();
    let mut unplaced = 0;
    for (body_id, name) in &ctx.named_bodies {
        let Some(body) = ctx.row("bodies", body_id) else {
            continue;
        };
        let body_color = body.get("color");
        for face in faces(ctx, body) {
            let Some(color) = face.get("color").filter(|c| Some(*c) != body_color) else {
                continue;
            };
            let Some(look) = super::extrude::look(color) else {
                continue;
            };
            match point_on(ctx, face) {
                Some(point) => out.push(json!({"body": name, "point": point, "look": look})),
                None => unplaced += 1,
            }
        }
    }
    (out, unplaced)
}

/// The faces of `body`.
fn faces<'a>(ctx: &Context<'a>, body: &'a Value) -> Vec<&'a Value> {
    let ids = |row: &'a Value, key: &str| -> Vec<&'a str> {
        row.get(key)
            .and_then(Value::as_array)
            .map(|ids| ids.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default()
    };
    let mut out = Vec::new();
    for region in ids(body, "regions") {
        let Some(region) = ctx.row("regions", region) else {
            continue;
        };
        for shell in ids(region, "shells") {
            let Some(shell) = ctx.row("shells", shell) else {
                continue;
            };
            for member in shell
                .get("members")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if member.get("kind").and_then(Value::as_str) != Some("face") {
                    continue;
                }
                if let Some(face) = member
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| ctx.row("faces", id))
                {
                    out.push(face);
                }
            }
        }
    }
    out
}

/// A point inside a planar face, from its outer loop.
fn point_on(ctx: &Context, face: &Value) -> Option<V3> {
    let surface = ctx.row("surfaces", face.get("surface")?.as_str()?)?;
    let geometry = surface.get("geometry")?;
    if geometry.get("kind")?.as_str()? != "plane" {
        return None;
    }
    let normal = v3(geometry.get("normal"))?;
    let u = v3(geometry.get("u_axis"))?;
    let v = [
        normal[1] * u[2] - normal[2] * u[1],
        normal[2] * u[0] - normal[0] * u[2],
        normal[0] * u[1] - normal[1] * u[0],
    ];
    let outer = face
        .pointer("/loops/loops")?
        .as_array()?
        .first()?
        .as_str()?;
    let lp = ctx.row("loops", outer)?;
    let mut corners: Vec<V3> = Vec::new();
    for coedge in lp.pointer("/boundary/coedges")?.as_array()? {
        let coedge = ctx.row("coedges", coedge.as_str()?)?;
        let edge = ctx.row("edges", coedge.get("edge")?.as_str()?)?;
        let reversed = coedge.get("sense").and_then(Value::as_str) == Some("reversed");
        let vertex = edge.get(if reversed { "end" } else { "start" })?.as_str()?;
        let point = ctx.row(
            "points",
            ctx.row("vertices", vertex)?.get("point")?.as_str()?,
        )?;
        corners.push(v3(point.get("position"))?);
    }
    if corners.len() < 3 {
        return None;
    }
    let flat: Vec<[f64; 2]> = corners.iter().map(|p| [dot(*p, u), dot(*p, v)]).collect();
    let n = flat.len();
    (0..n)
        .find_map(|i| {
            let a = *corners.get((i + n - 1) % n)?;
            let b = *corners.get(i)?;
            let c = *corners.get((i + 1) % n)?;
            let centre = [
                (a[0] + b[0] + c[0]) / 3.0,
                (a[1] + b[1] + c[1]) / 3.0,
                (a[2] + b[2] + c[2]) / 3.0,
            ];
            let degenerate = {
                let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
                let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
                let cross = [
                    ab[1] * ac[2] - ab[2] * ac[1],
                    ab[2] * ac[0] - ab[0] * ac[2],
                    ab[0] * ac[1] - ab[1] * ac[0],
                ];
                dot(cross, cross) < 1e-18
            };
            (!degenerate && inside(&flat, [dot(centre, u), dot(centre, v)])).then_some(centre)
        })
        .filter(|p| p.iter().all(|c| c.is_finite()))
}

/// Whether `p` lies inside the polygon `poly` (even-odd rule).
fn inside(poly: &[[f64; 2]], p: [f64; 2]) -> bool {
    let mut inside = false;
    let n = poly.len();
    for i in 0..n {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + n - 1) % n)) else {
            continue;
        };
        if (a[1] > p[1]) != (b[1] > p[1])
            && p[0] < (b[0] - a[0]) * (p[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_inside_a_concave_outline() {
        // An L shape: its corner-triangle at (2, 2) is a notch; others lie inside.
        let l = [
            [0.0, 0.0],
            [4.0, 0.0],
            [4.0, 2.0],
            [2.0, 2.0],
            [2.0, 4.0],
            [0.0, 4.0],
        ];
        assert!(inside(&l, [1.0, 1.0]));
        assert!(!inside(&l, [3.0, 3.0]));
    }
}
