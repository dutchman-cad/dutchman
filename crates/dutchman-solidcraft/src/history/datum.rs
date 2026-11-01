//! Nonbuilding datum planes and their resolved frames for later consumers.

use serde_json::{json, Value};

use super::sketch::{dot, number, unit, v3, V3};
use super::{operation, Context};

#[derive(Clone, Debug)]
pub(super) struct Frame {
    pub origin: V3,
    pub normal: V3,
    pub x: V3,
}

fn frame(value: &Value) -> Result<Frame, String> {
    let origin = v3(value.get("origin")).ok_or("its plane origin is not resolved")?;
    let normal = v3(value.get("normal"))
        .and_then(unit)
        .ok_or("its plane normal is not resolved")?;
    let x = v3(value.get("u_axis"))
        .and_then(unit)
        .ok_or("its plane axis is not resolved")?;
    if dot(normal, x).abs() > 1e-9 {
        return Err("its plane axis is not perpendicular to its normal".to_owned());
    }
    Ok(Frame { origin, normal, x })
}

pub(super) fn map(feature: &Value, ctx: &mut Context) -> Result<(), String> {
    let definition = operation(feature);
    let resolved = match definition.get("definition").and_then(Value::as_str) {
        Some("datum_principal_plane") => {
            let (normal, x) = match definition.get("plane").and_then(Value::as_str) {
                Some("top") => ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
                Some("front") => ([0.0, -1.0, 0.0], [0.0, 0.0, -1.0]),
                Some("right") => ([1.0, 0.0, 0.0], [0.0, 0.0, -1.0]),
                _ => return Err("its principal plane is not resolved".to_owned()),
            };
            Ok(Frame {
                origin: [0.0; 3],
                normal,
                x,
            })
        }
        Some("datum_plane") => frame(definition.get("frame").unwrap_or(&Value::Null)),
        Some("datum_offset_plane") => offset(definition, ctx),
        _ => return Err("its datum plane is not supported yet".to_owned()),
    };
    match resolved {
        Ok(frame) => {
            if let Some(id) = feature.get("id").and_then(Value::as_str) {
                ctx.planes.insert(id.to_owned(), frame);
            }
            Ok(())
        }
        Err(_) if independent_sketches(feature, ctx) => Ok(()),
        Err(why) => Err(why),
    }
}

fn offset(definition: &Value, ctx: &Context) -> Result<Frame, String> {
    let reference = definition
        .get("reference")
        .ok_or("its offset plane reference is not resolved")?;
    let mut support = match reference.get("reference").and_then(Value::as_str) {
        Some("feature") => reference
            .get("feature")
            .and_then(Value::as_str)
            .and_then(|id| ctx.planes.get(id))
            .cloned()
            .ok_or("its offset plane does not reference an earlier resolved datum plane")?,
        Some("resolved_plane") => frame(reference.get("frame").unwrap_or(&Value::Null))?,
        Some("face") => {
            let selection = reference.get("face").unwrap_or(&Value::Null);
            let ids = match selection.get("kind").and_then(Value::as_str) {
                Some("faces") => selection.get("value"),
                Some("resolved") => selection.pointer("/value/faces"),
                _ => None,
            }
            .and_then(Value::as_array)
            .filter(|ids| ids.len() == 1)
            .ok_or("its offset plane face is not a unique resolved planar face")?;
            let surface = ids
                .first()
                .and_then(Value::as_str)
                .and_then(|id| ctx.row("faces", id))
                .and_then(|face| face.get("surface"))
                .and_then(Value::as_str)
                .and_then(|id| ctx.row("surfaces", id))
                .and_then(|s| s.get("geometry"))
                .filter(|g| g.get("kind").and_then(Value::as_str) == Some("plane"))
                .ok_or("its offset plane face has no resolved planar surface")?;
            frame(surface)?
        }
        _ => return Err("its offset plane reference is not resolved".to_owned()),
    };
    let distance =
        number(definition.get("distance")).ok_or("its offset distance is not resolved")?;
    for (origin, normal) in support.origin.iter_mut().zip(support.normal) {
        *origin += distance * normal;
        if !origin.is_finite() {
            return Err("its offset plane origin exceeds the supported range".to_owned());
        }
    }
    Ok(support)
}

/// An unresolved support cannot affect sketches whose placements are independently resolved.
/// Refuse all other consumers, including later datum constructions needing the missing frame.
fn independent_sketches(feature: &Value, ctx: &Context) -> bool {
    fn contains(value: &Value, id: &str) -> bool {
        match value {
            Value::String(s) => s == id,
            Value::Array(a) => a.iter().any(|v| contains(v, id)),
            Value::Object(o) => o.values().any(|v| contains(v, id)),
            _ => false,
        }
    }
    let Some(id) = feature.get("id").and_then(Value::as_str) else {
        return false;
    };
    let consumers: Vec<_> = ctx
        .arena("features")
        .iter()
        .filter(|f| f.get("id").and_then(Value::as_str) != Some(id))
        .filter(|f| f.get("suppressed").and_then(Value::as_bool) != Some(true))
        .filter(|f| contains(f, id))
        .collect();
    consumers
        .iter()
        .all(|f| super::helpers::resolved_sketch(f, ctx))
}

pub(super) fn value(frame: &Frame) -> Value {
    let point = |p: V3| json!({"x": p[0], "y": p[1], "z": p[2]});
    json!({"origin": point(frame.origin), "normal": point(frame.normal), "u_axis": point(frame.x)})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_offsets_chain_through_earlier_frames() {
        let model = json!({});
        let mut ctx = Context::new(&model);
        let plane = json!({"id": "p", "definition": {"definition": "datum_principal_plane", "plane": "top"}});
        map(&plane, &mut ctx).unwrap();
        for (id, reference, distance, expected) in [("a", "p", -4.0, -4.0), ("b", "a", 7.0, 3.0)] {
            let feature = json!({"id": id, "definition": {"definition": "datum_offset_plane", "distance": distance,
                "reference": {"reference": "feature", "feature": reference}}});
            map(&feature, &mut ctx).unwrap();
            assert_eq!(ctx.planes[id].origin, [0.0, 0.0, expected]);
        }
    }

    #[test]
    fn a_face_offset_requires_one_planar_face_and_normalizes_its_normal() {
        let model = json!({"features": [{"id": "split", "definition": {"definition": "split_body",
            "tool": {"kind": "plane", "value": {"plane": "offset"}}}}],
            "faces": [{"id": "face", "surface": "surface"}],
            "surfaces": [{"id": "surface", "geometry": {"kind": "plane",
                "origin": {"x": 1.0, "y": 3.0, "z": 2.0},
                "normal": {"x": 0.0, "y": 4.0, "z": 0.0},
                "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}}}]});
        let mut ctx = Context::new(&model);
        let mut feature = json!({"id": "offset", "definition": {"definition": "datum_offset_plane",
            "distance": -2.0, "reference": {"reference": "face",
                "face": {"kind": "faces", "value": ["face"]}}}});
        map(&feature, &mut ctx).unwrap();
        assert_eq!(ctx.planes["offset"].origin, [1.0, 1.0, 2.0]);
        feature["definition"]["reference"]["face"]["value"] = json!(["face", "face"]);
        assert!(map(&feature, &mut ctx)
            .unwrap_err()
            .contains("unique resolved planar face"));
    }

    #[test]
    fn an_unused_offset_plane_needs_no_support_frame() {
        let model = json!({});
        let mut ctx = Context::new(&model);
        let feature = json!({"id": "unused", "definition": {"definition": "datum_offset_plane", "distance": 3.0}});
        assert!(map(&feature, &mut ctx).is_ok());
        assert!(ctx.planes.is_empty());
    }

    #[test]
    fn a_missing_reference_cannot_supply_a_frame() {
        let model = json!({"features": [{"id": "split", "definition": {"definition": "split_body",
            "tool": {"kind": "plane", "value": {"plane": "p"}}}}]});
        let mut ctx = Context::new(&model);
        let feature =
            json!({"id": "p", "definition": {"definition": "datum_offset_plane", "distance": 3.0}});
        assert!(map(&feature, &mut ctx)
            .unwrap_err()
            .contains("reference is not resolved"));
        assert!(ctx.planes.is_empty());
    }
}
