//! Cosmetic thread annotations on resolved cylindrical faces.

use dutchman_core::{Report, Severity};
use serde_json::{json, Value};

use super::sketch::{dot, number, unit, v3, V3};
use super::{describe, name, Context};

pub(super) fn map(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
    report: &mut Report,
) -> Result<Value, String> {
    if definition.get("modeled").and_then(Value::as_bool) == Some(true) {
        return Err("a modeled thread is not supported yet".to_owned());
    }
    let diameter = number(definition.get("diameter")).filter(|v| *v > 0.0);
    let pitch = number(definition.get("pitch")).filter(|v| *v > 0.0);
    let (diameter, pitch) = diameter
        .zip(pitch)
        .filter(|(d, p)| p < d)
        .ok_or("cosmetic threads require a resolved diameter and pitch")?;
    if let Some(standard) = definition.get("standard").and_then(Value::as_str) {
        if !matches!(
            standard.trim().to_ascii_lowercase().as_str(),
            "iso" | "metric" | "iso metric" | "iso metric profile"
        ) {
            return Err("only ISO metric cosmetic threads are supported yet".to_owned());
        }
    }
    let designation = format!("M{diameter}x{pitch}");
    if let Some(source) = definition.get("designation").and_then(Value::as_str) {
        if source.chars().any(|c| {
            !(c.is_ascii_digit()
                || c.is_whitespace()
                || matches!(c, 'M' | 'm' | 'x' | 'X' | '×' | '.'))
        }) {
            return Err("a cosmetic thread designation with additional class or hand controls is not supported yet".to_owned());
        }
        if super::hole::metric_thread(source, Some(pitch)) != Some((diameter, pitch)) {
            return Err(
                "its thread designation does not exactly match its diameter and pitch".to_owned(),
            );
        }
    }
    let face = cylinder_point(
        ctx,
        definition.get("face").ok_or("its thread face is missing")?,
    )?;
    let length = match definition.pointer("/extent/kind").and_then(Value::as_str) {
        Some("through") => None,
        Some("blind") => Some(
            number(definition.pointer("/extent/length"))
                .filter(|v| *v > 0.0)
                .ok_or("its thread length is not resolved")?,
        ),
        _ => return Err("its cosmetic thread extent is not resolved".to_owned()),
    };
    let id = ctx.id();
    let mut out = json!({"id": id, "type": "thread", "name": name(feature).unwrap_or("Thread"),
        "suppressed": false, "face": face, "designation": designation});
    if let Some(length) = length {
        if let Some(fields) = out.as_object_mut() {
            fields.insert("length".to_owned(), json!(format!("{length} mm")));
        }
    }
    if definition.get("modeled").and_then(Value::as_bool).is_none() {
        report.push(Severity::Degraded, describe(feature),
            "its source does not resolve whether the thread is modeled; kept as a cosmetic annotation, without reconstructing helical geometry");
    }
    Ok(out)
}

/// A cosmetic annotation does not change topology. Its final face can therefore supply a
/// pick, unlike a cut or blend. Require a complete cylindrical band between two circular
/// rims, so the midpoint is proven inside the selected face, away from its boundaries.
fn cylinder_point(ctx: &Context, selection: &Value) -> Result<V3, String> {
    if selection.get("kind").and_then(Value::as_str) == Some("historical") {
        return Err(
            "its historical thread face has no resolved input surface geometry yet".to_owned(),
        );
    }
    let ids = match selection.get("kind").and_then(Value::as_str) {
        Some("faces") => selection.get("value"),
        Some("resolved") => selection.pointer("/value/faces"),
        _ => None,
    }
    .and_then(Value::as_array)
    .filter(|ids| ids.len() == 1)
    .ok_or("its thread requires exactly one resolved cylindrical face")?;
    let face = ids
        .first()
        .and_then(Value::as_str)
        .and_then(|id| ctx.row("faces", id))
        .ok_or("its thread face is missing")?;
    let geometry = face
        .get("surface")
        .and_then(Value::as_str)
        .and_then(|id| ctx.row("surfaces", id))
        .and_then(|s| s.get("geometry"))
        .ok_or("its thread surface is missing")?;
    if geometry.get("kind").and_then(Value::as_str) != Some("cylinder") {
        return Err("its thread face is not cylindrical".to_owned());
    }
    band_point(ctx, face, geometry).ok_or_else(|| {
        "its thread face needs two resolved complete circular rims on the same cylinder".to_owned()
    })
}

fn band_point(ctx: &Context, face: &Value, geometry: &Value) -> Option<V3> {
    let origin = v3(geometry.get("origin"))?;
    let axis = unit(v3(geometry.get("axis"))?)?;
    let radial = unit(v3(geometry.get("ref_direction"))?)?;
    let radius = number(geometry.get("radius"))?;
    let loops = face.pointer("/loops/loops")?.as_array()?;
    if loops.len() != 2 {
        return None;
    }
    let mut heights = Vec::new();
    for lp in loops {
        let coedges = ctx
            .row("loops", lp.as_str()?)?
            .pointer("/boundary/coedges")?
            .as_array()?;
        if coedges.len() != 1 {
            return None;
        }
        let edge = ctx
            .row("coedges", coedges.first()?.as_str()?)?
            .get("edge")?
            .as_str()?;
        let edge = ctx.row("edges", edge)?;
        let range = edge.pointer("/carrier/param_range")?.as_array()?;
        if ((number(range.get(1))? - number(range.first())?).abs() - std::f64::consts::TAU).abs()
            > 1e-6
        {
            return None;
        }
        let circle = ctx
            .row("curves", edge.pointer("/carrier/curve")?.as_str()?)?
            .get("geometry")?;
        if circle.get("kind")?.as_str()? != "circle"
            || (number(circle.get("radius"))? - radius).abs() > 1e-6
            || dot(unit(v3(circle.get("axis"))?)?, axis).abs() < 1.0 - 1e-9
        {
            return None;
        }
        let center = v3(circle.get("center"))?;
        let delta = [
            center[0] - origin[0],
            center[1] - origin[1],
            center[2] - origin[2],
        ];
        let height = dot(delta, axis);
        if delta
            .iter()
            .zip(axis)
            .any(|(d, a)| (d - height * a).abs() > 1e-6)
        {
            return None;
        }
        heights.push(height);
    }
    let a = *heights.first()?;
    let b = *heights.get(1)?;
    if (a - b).abs() <= 1e-9 {
        return None;
    }
    let mut point = [0.0; 3];
    for (i, coordinate) in point.iter_mut().enumerate() {
        *coordinate = origin.get(i)? + axis.get(i)? * ((a + b) * 0.5) + radial.get(i)? * radius;
    }
    Some(point)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn model() -> Value {
        json!({
            "faces": [{"id": "f", "surface": "s", "loops": {"loops": ["lo", "hi"]}}],
            "surfaces": [{"id": "s", "geometry": {"kind": "cylinder", "origin": {"x": 0,"y": 0,"z": 0},
                "axis": {"x": 0,"y": 0,"z": 1}, "ref_direction": {"x": 1,"y": 0,"z": 0}, "radius": 1.5}}],
            "loops": [{"id": "lo", "boundary": {"coedges": ["cl"]}}, {"id": "hi", "boundary": {"coedges": ["ch"]}}],
            "coedges": [{"id": "cl", "edge": "el"}, {"id": "ch", "edge": "eh"}],
            "edges": [{"id": "el", "carrier": {"curve": "a", "param_range": [0, std::f64::consts::TAU]}},
                {"id": "eh", "carrier": {"curve": "b", "param_range": [0, std::f64::consts::TAU]}}],
            "curves": [
                {"id": "a", "geometry": {"kind": "circle", "center": {"x": 0,"y": 0,"z": 0}, "axis": {"x": 0,"y": 0,"z": 1}, "radius": 1.5}},
                {"id": "b", "geometry": {"kind": "circle", "center": {"x": 0,"y": 0,"z": 8}, "axis": {"x": 0,"y": 0,"z": 1}, "radius": 1.5}}
            ]
        })
    }

    fn definition() -> Value {
        json!({"modeled": false, "face": {"kind": "faces", "value": ["f"]}, "diameter": 3.0,
            "pitch": 0.5, "extent": {"kind": "blind", "length": 6.0}})
    }

    #[test]
    fn cosmetic_thread_without_standard_keeps_exact_size_and_length() {
        let model = model();
        let mut ctx = Context::new(&model);
        let out = map(
            &json!({"name": "Thread 1"}),
            &definition(),
            &mut ctx,
            &mut Report::default(),
        )
        .unwrap();
        assert_eq!(out["type"], "thread");
        assert_eq!(out["designation"], "M3x0.5");
        assert_eq!(out["length"], "6 mm");
        assert_eq!(out["face"], json!([1.5, 0.0, 4.0]));
        let mut through = definition();
        through["extent"] = json!({"kind": "through"});
        assert!(map(&json!({}), &through, &mut ctx, &mut Report::default())
            .unwrap()
            .get("length")
            .is_none());
    }

    #[test]
    fn unspecified_construction_keeps_an_annotation_and_reports_the_gap() {
        let model = model();
        let mut d = definition();
        d.as_object_mut().unwrap().remove("modeled");
        let mut report = Report::default();
        let out = map(&json!({}), &d, &mut Context::new(&model), &mut report).unwrap();
        assert_eq!(out["type"], "thread");
        assert!(report.losses()[0]
            .detail
            .contains("does not resolve whether the thread is modeled"));
    }

    #[test]
    fn cosmetic_thread_refuses_unproven_or_conflicting_data() {
        let model = model();
        let mut ctx = Context::new(&model);
        for (field, value, reason) in [
            ("modeled", json!(true), "modeled thread"),
            ("pitch", Value::Null, "resolved diameter and pitch"),
            ("designation", json!("M3x0.6"), "exactly match"),
            ("standard", json!("UNC"), "ISO metric"),
            ("standard", json!("Trapezoidal metric"), "ISO metric"),
            ("designation", json!("M3x0.5-LH"), "class or hand"),
            ("extent", Value::Null, "extent is not resolved"),
            (
                "face",
                json!({"kind": "native", "value": "face"}),
                "resolved cylindrical face",
            ),
            (
                "face",
                json!({"kind": "historical", "value": {"state": "s", "faces": ["f"]}}),
                "no resolved input surface geometry",
            ),
        ] {
            let mut d = definition();
            d[field] = value;
            assert!(map(&json!({}), &d, &mut ctx, &mut Report::default())
                .unwrap_err()
                .contains(reason));
        }
        let mut incomplete = model.clone();
        incomplete["edges"][0]["carrier"]["param_range"] = json!([0, 1]);
        assert!(map(
            &json!({}),
            &definition(),
            &mut Context::new(&incomplete),
            &mut Report::default()
        )
        .unwrap_err()
        .contains("complete circular rims"));
    }
}
