//! Exact planar loops selected from a spatial sketch for extrusion.

use std::collections::BTreeSet;

use serde_json::{json, Value};

use super::super::sketch::{
    cross, distance, dot, number, synthesized, unit, v3, COINCIDENT, PARALLEL, V3,
};
use super::Context;

fn sub(a: V3, b: V3) -> V3 {
    std::array::from_fn(|i| a.get(i).copied().unwrap_or(0.0) - b.get(i).copied().unwrap_or(0.0))
}

struct Frame {
    origin: V3,
    normal: V3,
    x: V3,
    y: V3,
}

impl Frame {
    fn point(&self, value: Option<&Value>) -> Result<Value, String> {
        let point = v3(value).ok_or("its 3D profile has a missing point")?;
        let d = sub(point, self.origin);
        if dot(d, self.normal).abs() > COINCIDENT {
            return Err("its 3D sketch profile boundary is not planar".to_owned());
        }
        Ok(json!({"u":dot(d,self.x),"v":dot(d,self.y)}))
    }

    /// An exact 2D geometry and whether its stored traversal reverses the source curve.
    fn geometry(&self, geometry: &Value) -> Result<(Value, bool), String> {
        let kind = geometry
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let out = match kind {
            "line" => {
                json!({"kind":"line","start":self.point(geometry.get("start"))?,"end":self.point(geometry.get("end"))?})
            }
            "circle" | "arc" => {
                let normal = v3(geometry.get("normal"))
                    .and_then(unit)
                    .ok_or("its 3D profile curve has no normal")?;
                let sign = dot(normal, self.normal);
                if sign.abs() < 1.0 - PARALLEL {
                    return Err("its 3D sketch profile boundary is not planar".to_owned());
                }
                let center = self.point(geometry.get("center"))?;
                let radius =
                    number(geometry.get("radius")).ok_or("its 3D profile curve has no radius")?;
                if kind == "circle" {
                    return Ok((
                        json!({"kind":"circle","center":center,"radius":radius}),
                        false,
                    ));
                }
                let reference = v3(geometry.get("reference_direction"))
                    .and_then(unit)
                    .ok_or("its 3D profile arc has no reference direction")?;
                let zero = dot(reference, self.y).atan2(dot(reference, self.x));
                let a = number(geometry.get("start_angle"))
                    .ok_or("its 3D profile arc has no start angle")?;
                let b = number(geometry.get("end_angle"))
                    .ok_or("its 3D profile arc has no end angle")?;
                let turn = std::f64::consts::TAU;
                if a == b || ((b - a).abs() - turn).abs() <= PARALLEL {
                    return Ok((
                        json!({"kind":"circle","center":center,"radius":radius}),
                        false,
                    ));
                }
                if (b - a).abs() > turn + PARALLEL {
                    return Err("multi-turn 3D profile arcs are not supported yet".to_owned());
                }
                let (start, end) = if sign > 0.0 {
                    (zero + a, zero + b)
                } else {
                    (zero - b, zero - a)
                };
                return Ok((
                    json!({"kind":"arc","center":center,"radius":radius,"start_angle":start,"end_angle":end}),
                    sign < 0.0,
                ));
            }
            "nurbs" => {
                let mut curve = geometry
                    .get("curve")
                    .cloned()
                    .ok_or("its 3D profile spline has no curve")?;
                let rational =
                    curve.pointer("/poles/kind").and_then(Value::as_str) == Some("rational");
                let points = curve
                    .pointer_mut("/poles/points")
                    .and_then(Value::as_array_mut)
                    .ok_or("its 3D profile spline has no poles")?;
                for point in points {
                    if rational {
                        let at = self.point(point.get("point"))?;
                        point
                            .as_object_mut()
                            .ok_or("its 3D profile spline pole is missing")?
                            .insert("point".to_owned(), at);
                    } else {
                        *point = self.point(Some(point))?;
                    }
                }
                json!({"kind":"nurbs","curve":curve})
            }
            _ => {
                return Err(format!(
                    "a 3D sketch extrude profile with {kind} geometry is not supported yet"
                ))
            }
        };
        Ok((out, false))
    }
}

/// SolidCraft extrudes planar sketch curves, so keep the 3D source and make an exact planar
/// profile copy. Its body must pass the existing source comparison before it is retained.
pub(in crate::history) fn map(
    reference: &Value,
    ctx: &mut Context,
) -> Result<(String, Value), String> {
    map_limited(reference, ctx, super::constraints::MAX_CONSTRAINTS)
}

/// Share the spatial relation allocation budget with the separate planar copy.
fn map_limited(
    reference: &Value,
    ctx: &mut Context,
    constraint_limit: usize,
) -> Result<(String, Value), String> {
    if ctx.target_version < crate::SolidCraftVersion::V0_44_0 {
        return Err("its sketch is not an earlier mapped sketch".to_owned());
    }
    let value = reference
        .get("value")
        .ok_or("its 3D profile selection is missing")?;
    let sketch = value
        .get("sketch")
        .and_then(Value::as_str)
        .ok_or("its 3D profile sketch is missing")?;
    if !ctx.spatial.contains_key(sketch) {
        return Err("its sketch is not an earlier mapped 3D sketch".to_owned());
    }
    let profiles = ctx
        .row("spatial_sketches", sketch)
        .and_then(|s| s.get("profiles"))
        .and_then(Value::as_array)
        .ok_or("its 3D sketch has no resolved profiles")?;
    let selected = value
        .get("profiles")
        .and_then(Value::as_array)
        .filter(|p| !p.is_empty() && p.len() <= 256)
        .ok_or("its 3D profile selection is empty or exceeds the profile limit")?;
    let first_profile = selected
        .first()
        .and_then(Value::as_u64)
        .and_then(|i| usize::try_from(i).ok())
        .and_then(|i| profiles.get(i))
        .ok_or("its 3D profile is missing")?;
    let origin = v3(first_profile.get("origin")).ok_or("its 3D profile origin is missing")?;
    let normal = v3(first_profile.get("normal"))
        .and_then(unit)
        .ok_or("its 3D profile normal is missing")?;
    let x = v3(first_profile.get("u_axis"))
        .and_then(unit)
        .ok_or("its 3D profile axis is missing")?;
    let y = unit(cross(normal, x)).ok_or("its 3D profile axes are parallel")?;
    let mut rings = Vec::new();
    let mut ids = BTreeSet::new();
    let frame = Frame {
        origin,
        normal,
        x,
        y,
    };
    let mut entities = Vec::new();
    for index in selected {
        let ring = index
            .as_u64()
            .and_then(|i| usize::try_from(i).ok())
            .and_then(|i| profiles.get(i))
            .and_then(|profile| profile.get("boundary"))
            .and_then(Value::as_array)
            .filter(|r| !r.is_empty() && r.len() <= 2000)
            .ok_or("its 3D profile is not a resolved bounded loop")?;
        // SolidCraft 0.44 profile detection admits at most 2,000 lifted edges.
        if entities.len().saturating_add(ring.len()) > 2000 {
            return Err("its 3D profile selection exceeds SolidCraft’s 2000-edge limit".to_owned());
        }
        let mut boundary = Vec::new();
        for use_ in ring {
            let entity = use_
                .get("entity")
                .and_then(Value::as_str)
                .ok_or("its 3D profile boundary is missing")?;
            let row = ctx
                .row("spatial_sketch_entities", entity)
                .filter(|e| e.get("sketch").and_then(Value::as_str) == Some(sketch))
                .ok_or("its 3D profile uses a missing sketch curve")?;
            let geometry = row
                .get("geometry")
                .ok_or("its 3D profile curve has no geometry")?;
            let (geometry, flipped) = frame.geometry(geometry)?;
            boundary.push((
                entity.to_owned(),
                use_.get("reversed")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    ^ flipped,
            ));
            if ids.insert(entity.to_owned()) {
                entities.push(json!({"id":entity,"construction":false,"geometry":geometry}));
            }
        }
        rings.push(boundary);
    }
    let id = ctx.id();
    let (mut feature, mapped) =
        synthesized(id, "3D sketch extrude profile", (origin, x, y), &entities)?;
    // Check the source traversal after exact curve conversion, including chains of spline pieces.
    for ring in &rings {
        let mut first = None;
        let mut previous = None;
        for (entity, reversed) in ring {
            let curve = mapped
                .curve(entity)
                .ok_or("its 3D profile curve cannot be expressed exactly")?;
            if curve.kind == "circle" && ring.len() == 1 {
                continue;
            }
            let (a, b) = curve
                .start
                .zip(curve.end)
                .and_then(|(a, b)| mapped.points.get(a).zip(mapped.points.get(b)))
                .ok_or("its 3D profile curve has no resolved ends")?;
            let (from, to) = if *reversed { (b.1, a.1) } else { (a.1, b.1) };
            if previous.is_some_and(|p| distance(from, p) > COINCIDENT) {
                return Err("its 3D profile boundary is not closed".to_owned());
            }
            first.get_or_insert(from);
            previous = Some(to);
        }
        if first
            .zip(previous)
            .is_some_and(|(a, b)| distance(a, b) > COINCIDENT)
        {
            return Err("its 3D profile boundary is not closed".to_owned());
        }
    }
    let mut constraints = Vec::new();
    for row in ctx
        .arena("spatial_sketch_constraints")
        .iter()
        .filter(|r| r.get("sketch").and_then(Value::as_str) == Some(sketch))
    {
        let Some(definition) = row.get("definition") else {
            continue;
        };
        let kind = definition.get("kind").and_then(Value::as_str);
        // Borrow a slice: repeated relations may contain arbitrarily many occurrences.
        let entities = match kind {
            Some("line_length" | "parallel_to_direction") => definition
                .get("entity")
                .map(std::slice::from_ref)
                .unwrap_or_default(),
            Some("repeated_line_length") => definition
                .get("entities")
                .and_then(Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default(),
            _ => continue,
        };
        for entity in entities {
            let Some(curve) = entity.as_str().and_then(|e| mapped.curve(e)) else {
                continue;
            };
            if curve.kind != "line" {
                continue;
            }
            let constraint = if kind == Some("parallel_to_direction") {
                let Some(direction) = v3(definition.get("direction")).and_then(unit) else {
                    continue;
                };
                let kind = if dot(direction, x).abs() >= 1.0 - 1e-9 {
                    "horizontal"
                } else if dot(direction, y).abs() >= 1.0 - 1e-9 {
                    "vertical"
                } else {
                    continue;
                };
                json!({"type": kind, "l": curve.index})
            } else {
                let Some(param) = definition
                    .get("parameter")
                    .and_then(Value::as_str)
                    .and_then(|p| ctx.params.get(p))
                else {
                    continue;
                };
                let Some((a, b)) = curve
                    .start
                    .zip(curve.end)
                    .and_then(|(a, b)| mapped.points.get(a).zip(mapped.points.get(b)))
                else {
                    continue;
                };
                json!({"type": "length", "l": curve.index, "value": distance(a.1,b.1), "param": param})
            };
            if constraints.len() == constraint_limit {
                return Err(format!(
                    "its planar 3D sketch profile exceeds the {constraint_limit}-constraint copy limit; reduce repeated relations and convert again"
                ));
            }
            constraints.push(constraint);
        }
    }
    for (index, constraint) in constraints.iter_mut().enumerate() {
        if let Some(fields) = constraint.as_object_mut() {
            fields.insert("id".to_owned(), json!(format!("k{}", index + 1)));
        }
    }
    if let Some(sketch) = feature.get_mut("sketch").and_then(Value::as_object_mut) {
        sketch.insert("constraints".to_owned(), json!(constraints));
    }
    let loops = mapped.boundaries(
        rings
            .into_iter()
            .map(|ring| ring.into_iter().map(|(entity, _)| entity).collect())
            .collect(),
    )?;
    let key = format!("spatial-profile-{id}");
    ctx.extra.push(feature);
    ctx.sketches.insert(key.clone(), mapped);
    Ok((key, json!({"type": "curves", "loops": loops})))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame() -> Frame {
        Frame {
            origin: [2.0, 3.0, 4.0],
            normal: [0.0, 1.0, 0.0],
            x: [1.0, 0.0, 0.0],
            y: [0.0, 0.0, -1.0],
        }
    }

    fn square(relations: Vec<Value>) -> Value {
        let corners = [
            [0.0, 0.0, 0.0],
            [5.0, 0.0, 0.0],
            [5.0, 5.0, 0.0],
            [0.0, 5.0, 0.0],
        ];
        let xyz = |[x, y, z]: V3| json!({"x":x,"y":y,"z":z});
        json!({
            "spatial_sketches":[{"id":"s","profiles":[{
                "origin":xyz(corners[0]),"normal":xyz([0.0,0.0,1.0]),"u_axis":xyz([1.0,0.0,0.0]),
                "boundary":(0..4).map(|i| json!({"entity":format!("e{i}"),"reversed":false})).collect::<Vec<_>>()
            }]}],
            "spatial_sketch_entities":(0..4).map(|i| json!({"id":format!("e{i}"),"sketch":"s","geometry":{
                "kind":"line","start":xyz(corners[i]),"end":xyz(corners[(i+1)%4])
            }})).collect::<Vec<_>>(),
            "spatial_sketch_constraints":relations
        })
    }

    #[test]
    fn profile_copy_bounds_repeated_entities_and_the_total_across_rows() {
        let repeated = |entities: Vec<Value>| {
            json!({"sketch":"s","definition":{
                "kind":"repeated_line_length","entities":entities,"parameter":"p"
            }})
        };
        let four: Vec<_> = (0..4).map(|i| json!(format!("e{i}"))).collect();
        let reference = json!({"value":{"sketch":"s","profiles":[0]}});
        for (rows, accepted) in [
            (vec![repeated(four.clone())], true),
            (vec![repeated(vec![json!("e0"); 100_000])], false),
            (vec![repeated(four.clone()), repeated(four.clone())], false),
            (
                vec![
                    repeated(four),
                    json!({"sketch":"s","definition":{
                        "kind":"parallel_to_direction","entity":"e0","direction":{"x":1.0,"y":0.0,"z":0.0}
                    }}),
                ],
                false,
            ),
        ] {
            let source = square(rows);
            let mut ctx = Context::testing(&source);
            ctx.target_version = crate::SolidCraftVersion::V0_44_0;
            ctx.spatial.insert(
                "s".to_owned(),
                super::super::Mapped {
                    feature: 1,
                    wires: Default::default(),
                },
            );
            ctx.params.insert("p".to_owned(), "length".to_owned());
            let result = map_limited(&reference, &mut ctx, 4);
            if accepted {
                assert!(result.is_ok(), "{result:?}");
                let constraints = ctx.extra[0]["sketch"]["constraints"].as_array().unwrap();
                assert_eq!(constraints.len(), 4);
                assert_eq!(constraints[3]["id"], "k4");
                assert_eq!(constraints[3]["param"], "length");
            } else {
                assert!(result.unwrap_err().contains("4-constraint copy limit"));
                assert!(ctx.extra.is_empty());
                assert!(ctx.sketches.is_empty());
            }
        }
    }

    #[test]
    fn tilted_profiles_preserve_positions_and_refuse_out_of_plane_points() {
        assert_eq!(
            frame()
                .point(Some(&json!({"x":7.0,"y":3.0,"z":-2.0})))
                .unwrap(),
            json!({"u":5.0,"v":6.0})
        );
        assert!(frame()
            .point(Some(&json!({"x":7.0,"y":4.0,"z":-2.0})))
            .unwrap_err()
            .contains("not planar"));
    }

    #[test]
    fn opposite_arc_normals_reverse_the_boundary_use() {
        let (arc,reversed) = frame().geometry(&json!({"kind":"arc","center":{"x":2.0,"y":3.0,"z":4.0},"normal":{"x":0.0,"y":-1.0,"z":0.0},"reference_direction":{"x":1.0,"y":0.0,"z":0.0},"radius":5.0,"start_angle":0.0,"end_angle":std::f64::consts::FRAC_PI_2})).unwrap();
        assert!(reversed);
        assert_eq!(arc["center"], json!({"u":0.0,"v":0.0}));
        assert_eq!(arc["start_angle"], -std::f64::consts::FRAC_PI_2);
        assert_eq!(arc["end_angle"], 0.0);
    }

    #[test]
    fn projected_rational_splines_keep_weights_and_refuse_nonplanar_poles() {
        let mut source = json!({"kind":"nurbs","curve":{"degree":2,"knots":[0.0,0.0,0.0,1.0,1.0,1.0],"poles":{"kind":"rational","points":[{"point":{"x":2.0,"y":3.0,"z":4.0},"weight":1.0},{"point":{"x":5.0,"y":3.0,"z":0.0},"weight":0.5},{"point":{"x":8.0,"y":3.0,"z":4.0},"weight":1.0}]}}});
        let (curve, reversed) = frame().geometry(&source).unwrap();
        assert!(!reversed);
        assert_eq!(
            curve["curve"]["poles"]["points"][1],
            json!({"point":{"u":3.0,"v":4.0},"weight":0.5})
        );
        assert_eq!(curve["curve"]["knots"], source["curve"]["knots"]);
        source["curve"]["poles"]["points"][1]["point"]["y"] = json!(4.0);
        assert!(frame()
            .geometry(&source)
            .unwrap_err()
            .contains("not planar"));
    }
}
