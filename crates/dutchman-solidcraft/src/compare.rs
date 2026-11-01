//! Anonymous, advisory body comparisons measured in the same SolidCraft kernel.
use std::path::Path;
use std::time::Duration;

use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use dutchman_core::{Model, Report, Writer};
use serde_json::{json, Value};

use crate::{verify, SolidCraftWriter, Verification};

/// Maximum bodies in a comparison; matching work is bounded by its square.
const MAX_BODIES: usize = 4096;
/// Relative tolerance for advisory volume and area comparisons.
const RELATIVE: f64 = 0.01;
/// Absolute placement/extent tolerance in millimetres, before relative size allowance.
const POSITION_MM: f64 = 0.1;
/// Keep a one millimetre move detectable even on a large body.
const POSITION_CEILING_MM: f64 = 0.5;
const POSITION_DIAGONAL_RELATIVE: f64 = 0.001;

/// Find untrimmed tori sharing a shell with other faces. A whole torus is valid as
/// a shell by itself, including a separate cavity shell; in a joined shell it
/// suggests a trimmed patch has lost its boundary. Return anonymous entity numbers.
pub fn topology(model: &Model) -> Vec<Value> {
    let topology = &model.ir.model;
    let surfaces: std::collections::BTreeSet<_> = topology
        .surfaces
        .iter()
        .filter(|s| s.geometry.solved().is_some_and(is_torus))
        .map(|s| &s.id)
        .collect();
    let shells: std::collections::BTreeMap<_, _> = topology
        .shells
        .iter()
        .filter(|s| s.faces().len() > 1)
        .map(|s| (&s.id, &s.region))
        .collect();
    let regions: std::collections::BTreeMap<_, _> =
        topology.regions.iter().map(|r| (&r.id, &r.body)).collect();
    let bodies: std::collections::BTreeMap<_, _> = topology
        .bodies
        .iter()
        .enumerate()
        .map(|(i, b)| (&b.id, i + 1))
        .collect();
    topology
        .faces
        .iter()
        .enumerate()
        .filter(|(_, face)| face.loops.is_empty() && surfaces.contains(&face.surface))
        .filter_map(|(index, face)| {
            let region = shells.get(&face.shell)?;
            let body = bodies.get(regions.get(region)?)?;
            Some(json!({"body": body, "face": index + 1, "code": "geometry_untrimmed_torus"}))
        })
        .collect()
}

fn is_torus(mut surface: &SolvedSurfaceGeometry) -> bool {
    // Transformed carriers can nest; stop at the same depth as assembly placements.
    for _ in 0..32 {
        match surface {
            SolvedSurfaceGeometry::Torus(_) => return true,
            SolvedSurfaceGeometry::Transformed(placed) => surface = placed.basis(),
            _ => return false,
        }
    }
    false
}

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
/// `joints` must match the conversion choice so both sides use the same assembly pose.
pub fn stored_output(
    source: &Model,
    design: &[u8],
    joints: bool,
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
            .joints(joints)
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
            let allowed = (POSITION_DIAGONAL_RELATIVE * body.size().max(want.size()))
                .clamp(POSITION_MM, POSITION_CEILING_MM);
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
            "position_floor_mm": POSITION_MM, "position_diagonal_relative": POSITION_DIAGONAL_RELATIVE,
            "position_ceiling_mm": POSITION_CEILING_MM,
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

    #[test]
    fn one_millimetre_move_and_floating_part_are_detected_at_every_scale() {
        for size in [10.0, 1000.0, 10_000.0] {
            let source = json!({"volume_mm3": size * 100.0, "area_mm2": size * 40.0 + 200.0,
                "bbox": {"min": [0.0, 0.0, 0.0], "max": [size, 10.0, 10.0]}});
            for shift in [1.0, 1000.0] {
                let mut moved = source.clone();
                moved["bbox"]["min"][0] = json!(shift);
                moved["bbox"]["max"][0] = json!(size + shift);
                let report =
                    compare_values(&json!({"bodies": [source]}), &json!({"bodies": [moved]}))
                        .unwrap();
                assert_eq!(
                    report["parts"][0]["findings"],
                    json!(["step_bbox", "step_placement"])
                );
            }
        }
    }

    #[test]
    fn untrimmed_torus_in_joined_shell_is_advisory_but_whole_torus_is_allowed() {
        use cadmpeg_ir::geometry::{analytic::TorusSurface, SurfaceGeometry};
        use cadmpeg_ir::math::{Point3, Vector3};
        use cadmpeg_ir::topology::{FaceLoops, Shell};

        let mut model = dutchman_core::read_in(
            dutchman_core::Format::FusionArchive,
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d"),
            &dutchman_core::PartSearch::default(),
            &mut Report::default(),
        )
        .unwrap();
        let face = model.ir.model.faces.first().unwrap();
        let surface_id = face.surface.clone();
        let face_id = face.id.clone();
        let shell_id = face.shell.clone();
        let surface = model
            .ir
            .model
            .surfaces
            .iter_mut()
            .find(|s| s.id == surface_id)
            .unwrap();
        surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
                5.0,
                1.0,
            )
            .unwrap(),
        ));
        // A toroidal patch with trims is ordinary geometry.
        assert!(topology(&model).is_empty());
        model.ir.model.faces.first_mut().unwrap().loops = FaceLoops::unspecified(Vec::new());
        let findings = topology(&model);
        assert_eq!(
            findings,
            vec![json!({"body": 1, "face": 1, "code": "geometry_untrimmed_torus"})]
        );
        // A shell consisting only of that closed face is a legitimate whole torus.
        let shell = model
            .ir
            .model
            .shells
            .iter_mut()
            .find(|s| s.id == shell_id)
            .unwrap();
        *shell = Shell::new(
            shell.id.clone(),
            shell.region.clone(),
            vec![face_id],
            vec![],
            vec![],
        )
        .unwrap();
        assert!(topology(&model).is_empty());
    }

    #[test]
    fn kernel_comparison_detects_planted_assembly_moves() {
        let Some(cli) = std::env::var_os(crate::CLI_ENV) else {
            return;
        };
        let mut source = dutchman_core::read_in(
            dutchman_core::Format::FusionArchive,
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d"),
            &dutchman_core::PartSearch::default(),
            &mut Report::default(),
        )
        .unwrap();
        let part = source.clone();
        let origin = dutchman_core::Placement {
            name: None,
            part: 0,
            transform: dutchman_core::placement::IDENTITY,
            hidden: false,
        };
        let mut second = origin.clone();
        second.transform[0][3] = 50.0;
        source.assembly = Some(dutchman_core::Assembly {
            own_part: None,
            parts: vec![part],
            definitions: vec![],
            identities: vec![],
            placements: vec![origin, second],
            joints: vec![],
            left_out: vec![],
            held: vec![],
            joint_notes: vec![],
            stored_placements: vec![],
        });
        for shift in [0.0, 1.0, 1000.0] {
            let mut moved = source.clone();
            moved.assembly.as_mut().unwrap().placements[1].transform[0][3] += shift;
            let report = models(
                &source,
                &moved,
                &Verification::With(cli.clone().into()),
                Duration::from_secs(30),
            );
            assert_eq!(report["status"], "measured", "{report}");
            let flags: Vec<_> = report["parts"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|r| r["looks_wrong"] == true)
                .collect();
            assert_eq!(flags.len(), usize::from(shift != 0.0), "{report}");
            if let Some(flag) = flags.first() {
                assert_eq!(flag["findings"], json!(["step_bbox", "step_placement"]));
            }
        }
        // Joint solving can deliberately move an occurrence from its stored pose.
        // The reference must follow the same choice as the converted design.
        let assembly = source.assembly.as_mut().unwrap();
        assembly.stored_placements = assembly.placements.iter().map(|p| p.transform).collect();
        assembly.placements[1].transform[0][3] = 75.0;
        let preview = tempfile::tempdir().unwrap();
        for joints in [false, true] {
            let mut design = Vec::new();
            SolidCraftWriter::new()
                .import_only(true)
                .sketches(false)
                .joints(joints)
                .verification(Verification::Off)
                .write(&source, &mut design, &mut Report::default())
                .unwrap();
            let report = stored_output(
                &source,
                &design,
                joints,
                &Verification::With(cli.clone().into()),
                Duration::from_secs(30),
                preview.path(),
            );
            assert_eq!(report["status"], "measured", "{report}");
            assert!(
                report["parts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|r| r["looks_wrong"] == false),
                "{report}"
            );
        }
    }
}
