//! Anonymous, advisory body comparisons measured in the same SolidCraft kernel.
use std::path::Path;
use std::time::Duration;

use dutchman_core::{Model, Report, Writer};
use serde_json::{json, Value};

use crate::{verify, SolidCraftWriter, Verification};

/// Maximum bodies in a comparison; matching work is bounded by its square.
const MAX_BODIES: usize = 4096;
/// Relative tolerance for advisory volume and area comparisons.
const RELATIVE: f64 = 0.01;
/// Absolute placement/extent tolerance in millimetres, before relative size allowance.
const POSITION_MM: f64 = 0.1;

#[derive(Clone, Debug)]
struct Body {
    volume: Option<f64>,
    area: Option<f64>,
    bbox: Option<[f64; 6]>,
    faces: Option<u64>,
}

impl Body {
    fn value(&self) -> Value {
        json!({"volume_mm3": self.volume, "area_mm2": self.area,
            "bbox_mm": self.bbox, "faces": self.faces})
    }
    fn size(&self) -> f64 {
        self.bbox.map_or(0.0, |b| {
            b.iter()
                .skip(3)
                .zip(b.iter())
                .map(|(hi, lo)| (hi - lo).powi(2))
                .sum::<f64>()
                .sqrt()
        })
    }
}

/// Compare stored geometry of two neutral models, never returning names or CLI messages.
/// Missing kernel capabilities are explicitly unavailable. Findings require confirmation.
pub fn models(
    source: &Model,
    reference: &Model,
    verification: &Verification,
    timeout: Duration,
) -> Value {
    let result = (|| {
        let cli = verify::usable_cli(verification).map_err(|_| ())?;
        let writer = SolidCraftWriter::new()
            .import_only(true)
            .sketches(false)
            .joints(false)
            .verification(Verification::Off);
        let encode = |model: &Model| {
            let mut bytes = Vec::new();
            writer
                .write(model, &mut bytes, &mut Report::default())
                .map_err(|_| ())?;
            Ok::<_, ()>(bytes)
        };
        let dir = tempfile::tempdir().map_err(|_| ())?;
        let a = verify::evaluate_json(&cli, timeout, dir.path(), "source", &encode(source)?)
            .map_err(|_| ())?;
        let b = verify::evaluate_json(&cli, timeout, dir.path(), "reference", &encode(reference)?)
            .map_err(|_| ())?;
        compare_values(&a, &b)
    })();
    result.unwrap_or_else(|()| json!({"status": "unavailable", "parts": [],
        "reason": "Stored geometry could not be measured. Install a current SolidCraft CLI and try again."}))
}

/// Compare a written design with the original stored geometry, under one kernel.
/// Preview files carry geometry and stay private. A partial-history design should not be
/// passed here: its intentionally hidden editable bodies are included by CLI eval.
pub fn stored_output(
    source: &Model,
    design: &[u8],
    verification: &Verification,
    timeout: Duration,
    preview: &Path,
) -> Value {
    let result = (|| {
        let cli = verify::usable_cli(verification).map_err(|_| ())?;
        let mut original = Vec::new();
        SolidCraftWriter::new()
            .import_only(true)
            .sketches(false)
            .joints(false)
            .verification(Verification::Off)
            .write(source, &mut original, &mut Report::default())
            .map_err(|_| ())?;
        let dir = tempfile::tempdir().map_err(|_| ())?;
        let a = verify::evaluate_json(&cli, timeout, dir.path(), "original", &original)
            .map_err(|_| ())?;
        let b = verify::evaluate_json(&cli, timeout, dir.path(), "converted", design)
            .map_err(|_| ())?;
        let mut report = compare_values(&a, &b)?;
        if let Some(rows) = report.get_mut("parts").and_then(Value::as_array_mut) {
            for row in rows {
                if let Some(codes) = row.get_mut("findings").and_then(Value::as_array_mut) {
                    for code in codes {
                        if let Some(value) = code.as_str() {
                            *code = json!(value.replacen("step_", "geometry_", 1));
                        }
                    }
                }
            }
        }
        let wrong = report
            .get("parts")
            .and_then(Value::as_array)
            .is_some_and(|rows| {
                rows.iter()
                    .any(|row| row.get("looks_wrong") == Some(&json!(true)))
            });
        if wrong {
            let rendered = render_designs(&cli, timeout, &original, design, preview);
            put(
                &mut report,
                "preview_status",
                json!(if rendered { "available" } else { "unavailable" }),
            );
        }
        Ok::<_, ()>(report)
    })();
    result.unwrap_or_else(|()| json!({"status": "unavailable", "parts": [],
        "reason": "Stored and converted geometry could not be measured. Install a current SolidCraft CLI and retry."}))
}

/// Render two stored models for local visual review. No path enters the anonymous report.
pub fn render_models(
    source: &Model,
    reference: &Model,
    verification: &Verification,
    timeout: Duration,
    preview: &Path,
) -> bool {
    let result = (|| {
        let cli = verify::usable_cli(verification).map_err(|_| ())?;
        let writer = SolidCraftWriter::new()
            .import_only(true)
            .sketches(false)
            .joints(false)
            .verification(Verification::Off);
        let mut a = Vec::new();
        let mut b = Vec::new();
        writer
            .write(source, &mut a, &mut Report::default())
            .map_err(|_| ())?;
        writer
            .write(reference, &mut b, &mut Report::default())
            .map_err(|_| ())?;
        Ok::<_, ()>(render_designs(&cli, timeout, &a, &b, preview))
    })();
    result.unwrap_or(false)
}

fn render_designs(
    cli: &Path,
    timeout: Duration,
    original: &[u8],
    converted: &[u8],
    folder: &Path,
) -> bool {
    use std::process::{Command, Stdio};
    let render = |name: &str, bytes: &[u8]| -> Result<(), ()> {
        std::fs::create_dir_all(folder).map_err(|_| ())?;
        let design = folder.join(format!("{name}.sc"));
        let png = folder.join(format!("{name}.png"));
        std::fs::write(&design, bytes).map_err(|_| ())?;
        let mut child = Command::new(cli)
            .arg("snapshot")
            .arg(&design)
            .arg("--out")
            .arg(&png)
            .args([
                "--width",
                "640",
                "--height",
                "480",
                "--view",
                "iso",
                "--no-grid",
                "--no-axes",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ())?;
        let deadline = std::time::Instant::now() + timeout;
        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    return if status.success() && png.is_file() {
                        Ok(())
                    } else {
                        Err(())
                    }
                }
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(20))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(());
                }
            }
        }
    };
    render("original", original).is_ok() && render("converted", converted).is_ok()
}

fn bodies(value: &Value) -> Result<Vec<Body>, ()> {
    let rows = value.get("bodies").and_then(Value::as_array).ok_or(())?;
    if rows.len() > MAX_BODIES {
        return Err(());
    }
    rows.iter()
        .map(|row| {
            let scalar = |key: &str| {
                row.get(key)
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
            };
            let bbox = (|| {
                let mut bbox = [0.0; 6];
                for (offset, end) in [(0, "min"), (3, "max")] {
                    let values = row.pointer(&format!("/bbox/{end}"))?.as_array()?;
                    if values.len() != 3 {
                        return None;
                    }
                    for (slot, value) in bbox.iter_mut().skip(offset).take(3).zip(values) {
                        *slot = value
                            .as_f64()
                            .filter(|v| v.is_finite() && v.abs() <= 1e12)?;
                    }
                }
                bbox.iter()
                    .take(3)
                    .zip(bbox.iter().skip(3))
                    .all(|(lo, hi)| lo <= hi)
                    .then_some(bbox)
            })();
            Ok(Body {
                volume: scalar("volume_mm3"),
                area: scalar("area_mm2"),
                bbox,
                faces: row
                    .get("faces")
                    .and_then(Value::as_u64)
                    .or_else(|| row.get("face_count").and_then(Value::as_u64)),
            })
        })
        .collect()
}

fn put(row: &mut Value, key: &str, value: Value) {
    if let Some(fields) = row.as_object_mut() {
        fields.insert(key.to_owned(), value);
    }
}

fn relative(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    a.zip(b)
        .map(|(a, b)| (a - b).abs() / a.abs().max(b.abs()).max(1e-6))
}

fn shape_distance(a: &Body, b: &Body) -> f64 {
    let volume = relative(a.volume, b.volume).unwrap_or(1.0);
    let area = relative(a.area, b.area).unwrap_or(0.0);
    let extents = a.bbox.zip(b.bbox).map_or(1.0, |(a, b)| {
        let size = a
            .iter()
            .skip(3)
            .zip(a.iter())
            .chain(b.iter().skip(3).zip(b.iter()))
            .map(|(hi, lo)| (hi - lo).abs())
            .fold(1e-6_f64, f64::max);
        a.iter()
            .skip(3)
            .zip(a.iter())
            .zip(b.iter().skip(3).zip(b.iter()))
            .map(|((hi, lo), (other_hi, other_lo))| ((hi - lo) - (other_hi - other_lo)).abs())
            .sum::<f64>()
            / size
    });
    volume + area + extents
}

fn compare_values(source: &Value, reference: &Value) -> Result<Value, ()> {
    let source = bodies(source)?;
    let reference = bodies(reference)?;
    let mut used = vec![false; reference.len()];
    let mut rows = Vec::new();
    for (index, body) in source.iter().enumerate() {
        // Stable order breaks geometry ties; placement chooses among identical instances.
        let candidate = reference
            .iter()
            .enumerate()
            .filter(|(i, _)| !used.get(*i).copied().unwrap_or(true))
            .min_by(|(i, a), (j, b)| {
                shape_distance(body, a)
                    .total_cmp(&shape_distance(body, b))
                    .then_with(|| {
                        placement(body, a)
                            .unwrap_or(f64::MAX)
                            .total_cmp(&placement(body, b).unwrap_or(f64::MAX))
                    })
                    .then(i.cmp(j))
            });
        let mut findings = Vec::new();
        let mut score = 0.0_f64;
        let mut row = json!({"body": index+1, "reference_body": null,
            "matched": false, "source": body.value(), "reference": null});
        if let Some((other, want)) = candidate {
            if let Some(slot) = used.get_mut(other) {
                *slot = true;
            }
            let volume = relative(body.volume, want.volume);
            let area = relative(body.area, want.area);
            let moved = placement(body, want);
            let box_error = body.bbox.zip(want.bbox).map(|(a, b)| {
                a.iter()
                    .zip(b.iter())
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f64, f64::max)
            });
            let allowed = POSITION_MM.max(0.001 * body.size().max(want.size()));
            for (code, delta, threshold) in [
                ("step_volume", volume, RELATIVE),
                ("step_area", area, RELATIVE),
                ("step_bbox", box_error, allowed),
                ("step_placement", moved, allowed),
            ] {
                if let Some(delta) = delta {
                    score = score.max(delta / threshold);
                    if delta > threshold {
                        findings.push(code);
                    }
                }
            }
            put(&mut row, "reference_body", json!(other + 1));
            put(&mut row, "matched", json!(true));
            put(&mut row, "reference", want.value());
            put(
                &mut row,
                "differences",
                json!({"volume_relative": volume, "area_relative": area,
                "bbox_max_mm": box_error, "placement_mm": moved,
                "faces": body.faces.zip(want.faces).map(|(a,b)| a.abs_diff(b))}),
            );
            if body.volume.is_none()
                || want.volume.is_none()
                || body.area.is_none()
                || want.area.is_none()
                || body.bbox.is_none()
                || want.bbox.is_none()
            {
                put(&mut row, "status", json!("incomplete"));
            } else {
                put(&mut row, "status", json!("measured"));
            }
        } else {
            put(&mut row, "status", json!("unmatched"));
            findings.push("step_unmatched");
            score = f64::MAX;
        }
        put(&mut row, "findings", json!(findings));
        put(&mut row, "looks_wrong", json!(!findings.is_empty()));
        rows.push((score, row));
    }
    for (index, body) in reference
        .iter()
        .enumerate()
        .filter(|(i, _)| !used.get(*i).copied().unwrap_or(true))
    {
        rows.push((f64::MAX, json!({"body": null, "reference_body": index+1, "matched": false,
            "reference": body.value(), "source": null, "status": "unmatched", "looks_wrong": true, "findings": ["step_unmatched"]})));
    }
    rows.sort_by(|(a, _), (b, _)| b.total_cmp(a));
    Ok(json!({"status": "measured", "advisory": true,
        "thresholds": {"volume_relative": RELATIVE, "area_relative": RELATIVE,
            "position_floor_mm": POSITION_MM, "position_diagonal_relative": 0.001,
            "faces": "informational; coplanar faces may merge"},
        "parts": rows.into_iter().map(|(_,row)| row).collect::<Vec<_>>() }))
}

fn placement(a: &Body, b: &Body) -> Option<f64> {
    a.bbox.zip(b.bbox).map(|(a, b)| {
        a.iter()
            .take(3)
            .zip(a.iter().skip(3))
            .zip(b.iter().take(3).zip(b.iter().skip(3)))
            .map(|((lo, hi), (other_lo, other_hi))| ((lo + hi - other_lo - other_hi) / 2.0).powi(2))
            .sum::<f64>()
            .sqrt()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cube(x: f64, volume: f64) -> Value {
        json!({"name": "private", "volume_mm3": volume, "area_mm2": 600.0,
            "bbox": {"min": [x,0,0], "max": [x+10.0,10,10]}, "faces": 6})
    }
    #[test]
    fn matches_reordered_geometry_and_flags_moved_body_worst_first() {
        let result = compare_values(
            &json!({"bodies": [cube(0.0,1000.0),cube(50.0,800.0)]}),
            &json!({"bodies": [cube(60.0,800.0),cube(0.0,1000.0)]}),
        )
        .unwrap();
        assert_eq!(result["parts"][0]["body"], 2);
        assert_eq!(result["parts"][0]["looks_wrong"], true);
        assert_eq!(result["parts"][1]["looks_wrong"], false);
        assert!(!result.to_string().contains("private"));
    }
    #[test]
    fn missing_bodies_and_missing_measures_are_explicit() {
        let result = compare_values(
            &json!({"bodies": [{}]}),
            &json!({"bodies": [cube(0.0,1000.0),cube(20.0,1000.0)]}),
        )
        .unwrap();
        assert_eq!(result["parts"][0]["matched"], false);
        assert!(result.to_string().contains("incomplete"));
    }
    #[test]
    fn mesh_noise_and_face_merging_are_not_findings() {
        let mut reference = cube(0.01, 1000.1);
        reference["faces"] = json!(4);
        let result = compare_values(
            &json!({"bodies": [cube(0.0,1000.0)]}),
            &json!({"bodies": [reference]}),
        )
        .unwrap();
        assert_eq!(result["parts"][0]["looks_wrong"], false);
    }
}
