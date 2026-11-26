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
        // A nominal designation is not a helical construction recipe. In particular,
        // CADIR's CosmeticThread does not carry hand, class or the actual groove profile.
        // SolidCraft's modeled option cannot recover those facts. Do not turn this into
        // an annotation on the unthreaded prefix: that would discard real geometry.
        let designation = definition
            .get("designation")
            .and_then(Value::as_str)
            .filter(|text| text.len() <= 128)
            .map(|text| format!(" {text:?}"))
            .unwrap_or_default();
        report.push_coded(
            Severity::Degraded,
            describe(feature),
            "history/modeled-thread-retained",
            format!(
                "Modeled thread{designation} kept in the imported final geometry; its hand, tolerance class and exact groove profile are not resolved in CADIR, so an exact editable thread is not supported yet"
            ),
        );
        return Err("a modeled thread's hand, tolerance class and exact groove profile are not resolved; an exact editable thread is not supported yet".to_owned());
    }
    let diameter = number(definition.get("diameter")).filter(|v| *v > 0.0);
    let pitch = number(definition.get("pitch")).filter(|v| *v > 0.0);
    let (diameter, pitch) = diameter
        .zip(pitch)
        .ok_or("cosmetic threads require a resolved diameter and pitch")?;
    let standard = definition
        .get("standard")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_ascii_lowercase());
    let unified = matches!(
        standard.as_deref(),
        Some("unc" | "unf" | "ansi unified screw threads")
    );
    if standard.as_deref().is_some_and(|s| {
        !matches!(s, "iso" | "metric" | "iso metric" | "iso metric profile") && !unified
    }) {
        return Err("only ISO metric cosmetic threads are supported yet".to_owned());
    }
    let designation = if let Some(source) = definition.get("designation").and_then(Value::as_str) {
        let (major, lead, inch) = designation_size(source)?;
        if !rounded_value_agrees(diameter, major)
            || !(rounded_value_agrees(pitch, lead)
                || (inch && rounded_value_agrees(pitch, 25.4 / lead)))
        {
            let pitch_units = if inch { "mm or TPI" } else { "mm" };
            return Err(format!(
                "its thread designation {source:?} specifies diameter {major} mm and pitch {lead} mm, but its numbers give diameter {diameter} mm and pitch {pitch} {pitch_units}; check the source thread table"
            ));
        }
        if inch || unified {
            unified_gate(ctx)?;
            unified_name(source, major)?
        } else {
            // Write the table's nominal size, never the rounded secondary numbers.
            format!("M{major}x{lead}")
        }
    } else {
        if unified {
            unified_gate(ctx)?;
            return Err(
                "a UNC/UNF cosmetic thread requires a designation that names its size".to_owned(),
            );
        }
        if pitch >= diameter {
            return Err("cosmetic threads require a resolved diameter and pitch".to_owned());
        }
        format!("M{diameter}x{pitch}")
    };
    let (face, radius, axis) = cylinder_point(
        ctx,
        definition.get("face").ok_or("its thread face is missing")?,
    )?;
    let nominal = definition
        .get("designation")
        .and_then(Value::as_str)
        .and_then(|source| designation_size(source).ok())
        .map_or(diameter, |(major, _, _)| major);
    if let Some(sketched) = resized_by_thread(ctx, axis, radius, nominal) {
        return Err(format!(
            "its face is stored at diameter {} mm, but the sketch that made it is {} mm; the source thread changed the size, which a SolidCraft cosmetic thread cannot do, so the thread is not rebuilt and the stored shape is kept",
            radius * 2.0,
            sketched * 2.0
        ));
    }
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

/// Compare at the precision actually stated by a secondary table value, with a small
/// floating-point allowance for unit conversion. Integer values provide no evidence of
/// decimal rounding, so they must match exactly (apart from arithmetic roundoff).
fn rounded_value_agrees(stated: f64, nominal: f64) -> bool {
    let roundoff = 8.0 * f64::EPSILON * stated.abs().max(nominal.abs());
    if (stated - nominal).abs() <= roundoff {
        return true;
    }
    let text = stated.to_string();
    let Some((_, decimals)) = text.split_once('.') else {
        return false;
    };
    if decimals.contains(['e', 'E']) {
        return false;
    }
    let Ok(places) = i32::try_from(decimals.len()) else {
        return false;
    };
    let scale = 10_f64.powi(places);
    ((nominal * scale).round() / scale - stated).abs() <= roundoff
}

/// Read nominal dimensions from the designation, not from rounded table numbers.
fn designation_size(source: &str) -> Result<(f64, f64, bool), String> {
    if source.len() > 128 {
        return Err("its cosmetic thread designation is too long".to_owned());
    }
    let upper = source.trim().to_ascii_uppercase();
    if let Some(metric) = upper.strip_prefix('M') {
        if metric
            .chars()
            .any(|c| !(c.is_ascii_digit() || c.is_whitespace() || matches!(c, 'X' | '×' | '.')))
        {
            return Err("a cosmetic thread designation with additional class or hand controls is not supported yet".to_owned());
        }
        return super::hole::metric_thread(&upper, None)
            .filter(|(d, p)| d.is_finite() && p.is_finite() && *p > 0.0 && p < d)
            .map(|(d, p)| (d, p, false))
            .ok_or_else(|| {
                "its cosmetic thread designation does not resolve a nominal size".to_owned()
            });
    }
    let (upper, _) = split_class(&upper);
    let unified = upper
        .strip_suffix(" UNC")
        .or_else(|| upper.strip_suffix(" UNF"))
        .or_else(|| upper.strip_suffix(" UNEF"));
    let parsed = unified.and_then(|size| {
        let (diameter, tpi) = size.trim().split_once('-')?;
        let inches = if let Some((n, d)) = diameter.split_once('/') {
            let n = n.parse::<f64>().ok()?;
            let d = d.parse::<f64>().ok()?;
            (d > 0.0).then_some(n / d)?
        } else if let Some(gauge) = numbered_size(diameter) {
            gauge
        } else {
            diameter.parse::<f64>().ok()?
        };
        let tpi = tpi.parse::<f64>().ok()?;
        let major = inches * 25.4;
        let pitch = 25.4 / tpi;
        (major.is_finite() && pitch.is_finite() && pitch > 0.0 && pitch < major)
            .then_some((major, pitch, true))
    });
    parsed.ok_or_else(|| "its cosmetic thread designation is not supported yet".to_owned())
}

/// Refuses a Unified thread on a target without Unified thread support.
fn unified_gate(ctx: &Context) -> Result<(), String> {
    if crate::VersionGate::UnifiedThread.supported(ctx.target_version) {
        return Ok(());
    }
    let [major, minor] = crate::NEWEST_SOLIDCRAFT;
    Err(format!(
        "Unified threads need a SolidCraft release with Unified thread support (newer than {major}.{minor})"
    ))
}

/// Splits a trailing ASME B1.1 class (`-2A` external, `-2B` internal) off an upper-case
/// designation such as `1/4-20 UNC-2A`.
fn split_class(upper: &str) -> (&str, Option<&str>) {
    if let Some((rest, class)) = upper.rsplit_once('-') {
        let series = rest.rsplit(char::is_whitespace).next().unwrap_or_default();
        if matches!(series, "UNC" | "UNF" | "UNEF")
            && matches!(class, "1A" | "2A" | "3A" | "1B" | "2B" | "3B")
        {
            return (rest, Some(class));
        }
    }
    (upper, None)
}

/// A Unified designation in SolidCraft's spelling (solidcraft#20136): `<size>-<TPI> <series>`
/// with a leading hash on a numbered size and the source's class, when it states one. A
/// source with no class gives none; SolidCraft takes the side from the face.
fn unified_name(source: &str, major: f64) -> Result<String, String> {
    let upper = source.trim().to_ascii_uppercase();
    let (body, class) = split_class(&upper);
    let parts = body
        .rsplit_once(char::is_whitespace)
        .and_then(|(size, series)| {
            let (diameter, tpi) = size.trim().split_once('-')?;
            Some((diameter.trim(), tpi.trim(), series))
        });
    let (diameter, tpi, series) =
        parts.ok_or("its cosmetic thread designation is not supported yet")?;
    let size = match numbered_size(diameter) {
        Some(inches) if (inches * 25.4 - major).abs() < 1e-6 => {
            format!("#{}", diameter.trim_start_matches('#'))
        }
        _ => diameter.to_owned(),
    };
    Ok(match class {
        Some(class) => format!("{size}-{tpi} {series}-{class}"),
        None => format!("{size}-{tpi} {series}"),
    })
}

/// Unified numbered screw sizes #0 to #12 give a gauge number, not inches: the major
/// diameter is 0.060 + 0.013 x N inches. A bare integer 0..=12 or one with a leading
/// hash is a gauge; fractions, decimals and inch marks are not.
fn numbered_size(text: &str) -> Option<f64> {
    let digits = text.trim().strip_prefix('#').unwrap_or(text.trim());
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let gauge = digits.parse::<u32>().ok().filter(|n| *n <= 12)?;
    Some(0.060 + 0.013 * f64::from(gauge))
}

/// A cosmetic annotation does not change topology. Its final face can therefore supply a
/// pick, unlike a cut or blend. Require a complete cylindrical band between two circular
/// rims, so the midpoint is proven inside the selected face, away from its boundaries.
fn cylinder_point(ctx: &Context, selection: &Value) -> Result<(V3, f64, V3), String> {
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
        .and_then(|s| s.pointer("/geometry/value"))
        .ok_or("its thread surface is missing")?;
    if geometry.get("kind").and_then(Value::as_str) != Some("cylinder") {
        return Err("its thread face is not cylindrical".to_owned());
    }
    band_point(ctx, face, geometry).ok_or_else(|| {
        "its thread face needs two resolved complete circular rims on the same cylinder".to_owned()
    })
}

fn band_point(ctx: &Context, face: &Value, geometry: &Value) -> Option<(V3, f64, V3)> {
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
            .pointer("/geometry/value")?;
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
    Some((point, radius, axis))
}

/// Fusion can store a cosmetic thread's face at the thread class's diameter (Ø9.85 for an M10
/// thread) while the sketch that made the cylinder keeps the nominal size (Ø10). A SolidCraft
/// cosmetic thread changes nothing, so the sketch, not the stored face, would be rebuilt.
/// The sketch's radius when an earlier mapped sketch along the thread's axis has a circle of
/// the nominal size and the stored face is slightly smaller (within a class allowance of 3 %).
fn resized_by_thread(ctx: &Context, axis: V3, radius: f64, nominal: f64) -> Option<f64> {
    let wanted = nominal / 2.0;
    let shrink = wanted - radius;
    if !(shrink > 1e-4 && shrink <= 0.03 * wanted) {
        return None;
    }
    ctx.sketches
        .iter()
        .filter(|(_, mapped)| dot(mapped.normal, axis).abs() >= 1.0 - 1e-9)
        .find_map(|(id, _)| {
            ctx.arena("sketch_entities")
                .iter()
                .filter(|entity| {
                    entity.get("sketch").and_then(Value::as_str) == Some(id.as_str())
                        && entity.get("construction").and_then(Value::as_bool) != Some(true)
                        && entity.pointer("/geometry/kind").and_then(Value::as_str)
                            == Some("circle")
                })
                .find_map(|entity| {
                    number(entity.pointer("/geometry/radius"))
                        .filter(|sketched| (sketched - wanted).abs() <= 1e-6)
                })
        })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn model() -> Value {
        json!({
            "faces": [{"id": "f", "surface": "s", "loops": {"loops": ["lo", "hi"]}}],
            "surfaces": [{"id": "s", "geometry": {"kind":"solved", "value": {"kind": "cylinder", "origin": {"x": 0,"y": 0,"z": 0},
                "axis": {"x": 0,"y": 0,"z": 1}, "ref_direction": {"x": 1,"y": 0,"z": 0}, "radius": 1.5}}}],
            "loops": [{"id": "lo", "boundary": {"coedges": ["cl"]}}, {"id": "hi", "boundary": {"coedges": ["ch"]}}],
            "coedges": [{"id": "cl", "edge": "el"}, {"id": "ch", "edge": "eh"}],
            "edges": [{"id": "el", "carrier": {"curve": "a", "param_range": [0, std::f64::consts::TAU]}},
                {"id": "eh", "carrier": {"curve": "b", "param_range": [0, std::f64::consts::TAU]}}],
            "curves": [
                {"id": "a", "geometry": {"kind":"solved", "value": {"kind": "circle", "center": {"x": 0,"y": 0,"z": 0}, "axis": {"x": 0,"y": 0,"z": 1}, "radius": 1.5}}},
                {"id": "b", "geometry": {"kind":"solved", "value": {"kind": "circle", "center": {"x": 0,"y": 0,"z": 8}, "axis": {"x": 0,"y": 0,"z": 1}, "radius": 1.5}}}
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
        let mut ctx = Context::testing(&model);
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
    fn rounded_metric_table_values_keep_the_designations_nominal_size() {
        let model = model();
        for (designation, diameter, pitch, expected) in [
            ("M3.175x0.5", 3.18, 0.5, "M3.175x0.5"),
            (
                "M3.5x0.6",
                3.5000000000000004,
                0.6000000000000001,
                "M3.5x0.6",
            ),
            ("M3", 3.0, 0.5, "M3x0.5"),
        ] {
            let mut d = definition();
            d["designation"] = json!(designation);
            d["diameter"] = json!(diameter);
            d["pitch"] = json!(pitch);
            let out = map(
                &json!({}),
                &d,
                &mut Context::testing(&model),
                &mut Report::default(),
            )
            .unwrap();
            assert_eq!(out["designation"], expected);
        }
    }

    fn unified_definition(designation: &str, diameter: f64, pitch: f64) -> Value {
        let mut d = definition();
        d["standard"] = json!("ANSI Unified Screw Threads");
        d["designation"] = json!(designation);
        d["diameter"] = json!(diameter);
        d["pitch"] = json!(pitch);
        d
    }

    fn unified_result(designation: &str, diameter: f64, pitch: f64) -> Result<Value, String> {
        let model = model();
        map(
            &json!({}),
            &unified_definition(designation, diameter, pitch),
            &mut Context::testing(&model),
            &mut Report::default(),
        )
    }

    const UNIFIED_CASES: [(&str, f64, f64); 8] = [
        ("2-56 UNC", 2.1844, 0.4536),
        ("#2-56 UNC", 2.1844, 0.4536),
        ("#10-32 UNF", 4.826, 0.79375),
        ("#0-80 UNF", 1.524, 0.3175),
        ("12-24 UNC", 5.4864, 1.0583),
        ("1/4-20 UNC", 6.35, 1.27),
        ("1/4-28 UNF", 6.35, 0.907),
        ("1/4-20 UNC-2B", 6.35, 1.27),
    ];

    #[test]
    fn unified_threads_are_refused_while_no_release_supports_them() {
        for (designation, diameter, pitch) in UNIFIED_CASES {
            let reason = unified_result(designation, diameter, pitch).unwrap_err();
            assert_eq!(
                reason,
                "Unified threads need a SolidCraft release with Unified thread support (newer than 0.49)",
                "{designation}"
            );
        }
        // Every known target refuses; so does a thread with no designation.
        let model = model();
        for minor in 19..=49 {
            let mut ctx = Context::testing(&model);
            ctx.target_version = format!("0.{minor}.0").parse().unwrap();
            let reason = map(
                &json!({}),
                &unified_definition("1/4-20 UNC", 6.35, 1.27),
                &mut ctx,
                &mut Report::default(),
            )
            .unwrap_err();
            assert!(
                reason.contains("Unified thread support"),
                "0.{minor}: {reason}"
            );
        }
        let mut d = unified_definition("1/4-20 UNC", 6.35, 1.27);
        d.as_object_mut().unwrap().remove("designation");
        let reason = map(
            &json!({}),
            &d,
            &mut Context::testing(&model),
            &mut Report::default(),
        )
        .unwrap_err();
        assert!(reason.contains("Unified thread support"), "{reason}");
    }

    #[test]
    fn a_supporting_target_gets_solidcrafts_unified_designation() {
        let _forced = crate::verify::tests_force::force_unified_thread();
        for (designation, diameter, pitch, expected) in [
            ("2-56 UNC", 2.1844, 0.4536, "#2-56 UNC"),
            ("#2-56 UNC-2A", 2.1844, 0.4536, "#2-56 UNC-2A"),
            ("#10-32 UNF", 4.826, 0.79375, "#10-32 UNF"),
            ("1/4-20 UNC", 6.35, 1.27, "1/4-20 UNC"),
            ("1/4-20 unc-2a", 6.35, 1.27, "1/4-20 UNC-2A"),
            ("1/4-28 UNF-3A", 6.35, 0.907, "1/4-28 UNF-3A"),
            ("1/4-20 UNC-2B", 6.35, 1.27, "1/4-20 UNC-2B"),
            ("5/16-18 UNC-1B", 7.94, 1.411, "5/16-18 UNC-1B"),
        ] {
            let out = unified_result(designation, diameter, pitch)
                .unwrap_or_else(|reason| panic!("{designation}: {reason}"));
            assert_eq!(out["type"], "thread");
            assert_eq!(out["designation"], expected, "{designation}");
        }
        // A wrong size is still refused with the forced capability on.
        let reason = unified_result("#2-56 UNC-2A", 6.35, 0.4536).unwrap_err();
        assert!(reason.contains("specifies diameter 2.18"), "{reason}");
    }

    #[test]
    fn numbered_unified_sizes_use_the_gauge_diameter() {
        for (designation, diameter, pitch) in UNIFIED_CASES {
            let (major, lead, inch) = designation_size(designation).unwrap();
            assert!(inch && (major - diameter).abs() < 1e-9, "{designation}");
            assert!((lead - pitch).abs() < 1e-3, "{designation}");
        }
        assert!(designation_size("13-20 UNC").is_ok_and(|(major, _, _)| major > 300.0));
        assert!(designation_size("#13-20 UNC").is_err());
    }

    #[test]
    fn a_wrong_size_for_a_numbered_thread_is_still_refused() {
        let model = model();
        let mut d = definition();
        d["standard"] = json!("ANSI Unified Screw Threads");
        d["designation"] = json!("2-56 UNC");
        d["diameter"] = json!(50.8);
        d["pitch"] = json!(0.4536);
        let reason = map(
            &json!({}),
            &d,
            &mut Context::testing(&model),
            &mut Report::default(),
        )
        .unwrap_err();
        assert!(reason.contains("specifies diameter 2.18"), "{reason}");
    }

    #[test]
    fn contradictory_table_values_name_both_sizes() {
        let model = model();
        for (designation, diameter, pitch) in [
            ("M3x0.6", 3.0, 0.5),
            ("M3x0.5", 4.0, 0.5),
            ("M3", 3.0, 0.6),
            ("1/4-20 UNC", 6.35, 1.5),
        ] {
            let mut d = definition();
            d["designation"] = json!(designation);
            d["diameter"] = json!(diameter);
            d["pitch"] = json!(pitch);
            let reason = map(
                &json!({}),
                &d,
                &mut Context::testing(&model),
                &mut Report::default(),
            )
            .unwrap_err();
            assert!(reason.contains(designation), "{reason}");
            assert!(reason.contains("specifies diameter"), "{reason}");
            assert!(
                reason.contains(&format!(
                    "numbers give diameter {diameter} mm and pitch {pitch}"
                )),
                "{reason}"
            );
        }
        assert!(!rounded_value_agrees(3.1, 3.0));
        assert!(!rounded_value_agrees(4.0, 3.5));
    }

    fn class_sized_model(face_radius: f64, sketch_radius: f64) -> Value {
        let mut model = model();
        model["surfaces"][0]["geometry"]["value"]["radius"] = json!(face_radius);
        for curve in model["curves"].as_array_mut().unwrap() {
            curve["geometry"]["value"]["radius"] = json!(face_radius);
        }
        model["sketch_entities"] = json!([{"id": "e", "sketch": "s", "construction": false,
            "geometry": {"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": sketch_radius}}]);
        model
    }

    fn flat_sketch() -> super::super::sketch::Mapped {
        super::super::sketch::Mapped {
            feature: 1,
            normal: [0.0, 0.0, 1.0],
            origin: [0.0, 0.0, 0.0],
            x_dir: [1.0, 0.0, 0.0],
            y_dir: [0.0, 1.0, 0.0],
            points: Vec::new(),
            point_displacement: 0.0,
            shapes: Default::default(),
        }
    }

    #[test]
    fn a_thread_whose_face_is_smaller_than_the_nominal_sketch_is_not_rebuilt() {
        // An M10 thread: the sketch is the nominal 10 mm, Fusion's stored face the class's 9.85 mm.
        let mut d = definition();
        d["designation"] = json!("M10x1.5");
        d["diameter"] = json!(10.0);
        d["pitch"] = json!(1.5);
        let model = class_sized_model(4.925, 5.0);
        let mut ctx = Context::testing(&model);
        ctx.sketches.insert("s".to_owned(), flat_sketch());
        let reason = map(&json!({}), &d, &mut ctx, &mut Report::default()).unwrap_err();
        assert!(reason.contains("stored at diameter 9.85 mm"), "{reason}");
        assert!(reason.contains("sketch that made it is 10 mm"), "{reason}");
        // The same thread on a face that matches the sketch, or with no such sketch, is kept.
        let model = class_sized_model(5.0, 5.0);
        let mut ctx = Context::testing(&model);
        ctx.sketches.insert("s".to_owned(), flat_sketch());
        assert!(map(&json!({}), &d, &mut ctx, &mut Report::default()).is_ok());
        let model = class_sized_model(4.925, 5.0);
        assert!(map(
            &json!({}),
            &d,
            &mut Context::testing(&model),
            &mut Report::default()
        )
        .is_ok());
        // A tapped hole's much smaller face is not a class allowance.
        let model = class_sized_model(4.2, 5.0);
        let mut ctx = Context::testing(&model);
        ctx.sketches.insert("s".to_owned(), flat_sketch());
        assert!(map(&json!({}), &d, &mut ctx, &mut Report::default()).is_ok());
    }

    #[test]
    fn unspecified_construction_keeps_an_annotation_and_reports_the_gap() {
        let model = model();
        let mut d = definition();
        d.as_object_mut().unwrap().remove("modeled");
        let mut report = Report::default();
        let out = map(&json!({}), &d, &mut Context::testing(&model), &mut report).unwrap();
        assert_eq!(out["type"], "thread");
        assert!(report.losses()[0]
            .detail
            .contains("does not resolve whether the thread is modeled"));
    }

    #[test]
    fn modeled_thread_keeps_its_designation_in_a_degraded_loss_without_picking_a_face() {
        let model = json!({});
        for designation in [Some("M3x0.5"), None] {
            let mut d = definition();
            d["modeled"] = json!(true);
            d["designation"] = json!(designation);
            let mut report = Report::default();
            let reason =
                map(&json!({}), &d, &mut Context::testing(&model), &mut report).unwrap_err();
            assert!(reason.contains("exact groove profile"));
            let loss = &report.losses()[0];
            assert_eq!(loss.severity, Severity::Degraded);
            assert_eq!(
                loss.code.as_deref(),
                Some("history/modeled-thread-retained")
            );
            assert!(loss.detail.contains("imported final geometry"));
            if let Some(designation) = designation {
                assert!(loss.detail.contains(designation));
            }
        }
    }

    #[test]
    fn cosmetic_thread_refuses_unproven_or_conflicting_data() {
        let model = model();
        let mut ctx = Context::testing(&model);
        for (field, value, reason) in [
            ("pitch", Value::Null, "resolved diameter and pitch"),
            (
                "designation",
                json!("M3x0.6"),
                "specifies diameter 3 mm and pitch 0.6 mm",
            ),
            ("standard", json!("UNC"), "Unified thread support"),
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
            &mut Context::testing(&incomplete),
            &mut Report::default()
        )
        .unwrap_err()
        .contains("complete circular rims"));
    }
}
