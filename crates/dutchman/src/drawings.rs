//! Drawing files: every one gets a result and a report, never a silent skip.
//!
//! A FreeCAD document's TechDraw pages are written into the converted SolidCraft design as a
//! SolidCraft drawing. Every other drawing format is accepted, identified by its contents, and
//! reported as not decoded, with the reason codes of [`dutchman_core::drawing::code`].
//! The report's `drawing` section is documented in docs/json.md.

use std::path::Path;

use dutchman_core::drawing::{self, code, Plan};
use dutchman_core::{Format, Report, Severity, Stage};
use serde_json::{json, Value};

/// The `drawing.format` a report names for `format`.
pub fn format_id(format: Format) -> &'static str {
    match format {
        Format::FreeCadDocument => "freecad-techdraw",
        Format::InventorDrawing => "inventor-idw",
        Format::SolidWorksDrawing => "solidworks-slddrw",
        Format::FusionDrawing | Format::FusionMultiDocument => "fusion-f2d",
        Format::SolidEdgeDrawing => "solid-edge-dft",
        Format::DwgDrawing => "autocad-dwg",
        Format::DxfDrawing => "onshape-export",
        _ => "other",
    }
}

/// Whether `report` has a drawing section.
pub fn has_section(report: &Value) -> bool {
    report.get("drawing").is_some_and(Value::is_object)
}

fn loss_rows(report: &Report) -> Vec<Value> {
    report
        .losses()
        .iter()
        .map(|loss| {
            json!({
                "level": match loss.severity {
                    Severity::Info => "Info",
                    Severity::Degraded => "Degraded",
                    Severity::Dropped => "Dropped",
                },
                "code": loss.code,
                "item": loss.subject,
                "message": one_line(&loss.detail),
            })
        })
        .collect()
}

/// A message for the scoreboard: one line, no path.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn counts(read: usize, written: usize) -> Value {
    json!({"read": read, "written": written})
}

/// The section for a drawing that was not decoded: nothing read, nothing written.
fn not_decoded(format: Format, input: &Path) -> Value {
    let mut notes = Report::default();
    match drawing::sniff(format, input) {
        Ok(sniff) => {
            if !sniff.matches_suffix {
                notes.push_coded(
                    Severity::Info,
                    "file",
                    code::FORMAT_CONTENT_MISMATCH,
                    format!(
                        "the contents look like {} rather than a {} file",
                        match sniff.family {
                            "compound_document" => "a compound document",
                            "zip" => "a zip archive",
                            "dwg" => "a DWG drawing",
                            "dxf" => "a DXF drawing",
                            "text" => "a text file",
                            _ => "an unrecognized file",
                        },
                        format.name()
                    ),
                );
            }
            let detail = match (sniff.version_name, &sniff.version) {
                (Some(name), _) => format!(" ({name})"),
                (None, Some(version)) => format!(" (version {version})"),
                _ => String::new(),
            };
            notes.push_coded(
                Severity::Dropped,
                "file",
                code::FORMAT_NOT_DECODED,
                format!(
                    "{} is recognized{detail}, but reading its sheets and views isn't supported yet",
                    format.name()
                ),
            );
        }
        Err(_) => notes.push_coded(
            Severity::Dropped,
            "file",
            code::FORMAT_UNREADABLE,
            "the file could not be read to identify it; check that it exists and can be opened",
        ),
    }
    section(
        format,
        "not_decoded",
        &Plan::default(),
        (0, 0),
        None,
        &notes,
        None,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn section(
    format: Format,
    status: &str,
    plan: &Plan,
    written: (usize, usize),
    output: Option<&Path>,
    notes: &Report,
    model_source: Option<&str>,
    model_converted: bool,
) -> Value {
    let losses = loss_rows(notes);
    let first_stop = losses.iter().find(|loss| loss["level"] == "Dropped").map(
        |loss| json!({"code": loss["code"], "item": loss["item"], "message": loss["message"]}),
    );
    let [base, projected, section, detail, other] = plan.by_kind;
    json!({
        "schema": drawing::SCHEMA,
        "format": format_id(format),
        "status": status,
        "sheets": counts(plan.sheets_read, written.0),
        "views": {
            "read": plan.views_read, "written": written.1,
            "by_kind": {"base": base, "projected": projected, "section": section,
                        "detail": detail, "other": other},
        },
        "dimensions": counts(plan.dimensions_read, 0),
        "annotations": counts(plan.annotations_read, 0),
        "model": {"source": model_source, "converted": model_converted},
        "first_stop": first_stop,
        "losses": losses,
        "output": output.map(|path| path.display().to_string()),
    })
}

/// Converts a drawing file whose sheets cannot be read yet: nothing is written but the
/// report, which is a failed conversion carrying the `drawing` section.
pub fn convert_unread(input_path: &Path) -> Value {
    let input = crate::json::input(input_path);
    let Some(format) = Format::from_path(input_path) else {
        return crate::json::failure(crate::json::CONVERT, input, "not a drawing file");
    };
    let mut report = crate::json::failure_at(
        crate::json::CONVERT,
        input,
        &drawing::not_decoded_message(format),
        Stage::Decode,
        Some("cadmpeg"),
    );
    if let Some(error) = report.get_mut("error").and_then(Value::as_object_mut) {
        error.insert("kind".into(), json!("not_supported_yet"));
    }
    if let Some(fields) = report.as_object_mut() {
        fields.insert("drawing".into(), not_decoded(format, input_path));
    }
    report
}

/// Adds the `drawing` section to the report of a model conversion, for the drawings the
/// input holds: FreeCAD TechDraw pages are written into the converted design; the drawings
/// of a Fusion multi-document archive are listed as not decoded.
pub fn attach(input: &Path, options: &crate::ConvertOptions, report: &mut Value) {
    if report.get("ok").and_then(Value::as_bool) != Some(true) || has_section(report) {
        return;
    }
    let section = match Format::from_path(input) {
        Some(Format::FreeCadDocument) => freecad_section(input, options, report),
        Some(Format::FusionMultiDocument) => fusion_section(input),
        _ => None,
    };
    if let (Some(section), Some(fields)) = (section, report.as_object_mut()) {
        fields.insert("drawing".into(), section);
    }
}

fn fusion_section(input: &Path) -> Option<Value> {
    let file = std::fs::File::open(input).ok()?;
    let mut archive = zip::ZipArchive::new(file).ok()?;
    let drawings = (0..archive.len().min(100_000))
        .filter(|index| {
            archive.by_index(*index).ok().is_some_and(|member| {
                member
                    .name()
                    .is_ok_and(|name| name.to_ascii_lowercase().ends_with(".f2d"))
            })
        })
        .count();
    if drawings == 0 {
        return None;
    }
    let mut notes = Report::default();
    notes.push_coded(
        Severity::Dropped,
        "archive",
        code::FORMAT_NOT_DECODED,
        format!("the archive holds {drawings} Fusion drawings (.f2d); reading their sheets and views isn't supported yet"),
    );
    let plan = Plan::default();
    Some(section(
        Format::FusionDrawing,
        "not_decoded",
        &plan,
        (0, 0),
        None,
        &notes,
        None,
        true,
    ))
}

fn freecad_section(input: &Path, options: &crate::ConvertOptions, report: &Value) -> Option<Value> {
    let mut notes = Report::default();
    let model = dutchman_core::read_drawings(input, &mut Report::default()).ok()?;
    let plan = drawing::plan(&model, &mut notes);
    if plan.sheets_read == 0 {
        return None;
    }
    let output = report
        .pointer("/output/path")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from);
    let source = input.file_name().map(|n| n.to_string_lossy().into_owned());
    let design = output.filter(|path| {
        path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            ["sc", "solidcraft"]
                .iter()
                .any(|k| e.eq_ignore_ascii_case(k))
        })
    });
    let Some(design) = design else {
        notes.push_coded(
            Severity::Dropped,
            "drawing",
            code::WRITE_NO_DESIGN,
            "the converted model is not a SolidCraft design, so the drawing is not written",
        );
        return Some(section(
            Format::FreeCadDocument,
            "failed",
            &plan,
            (0, 0),
            None,
            &notes,
            source.as_deref(),
            true,
        ));
    };
    let verification = options
        .solidcraft
        .as_ref()
        .map_or(dutchman_solidcraft::Verification::Auto, |path| {
            dutchman_solidcraft::Verification::With(path.clone())
        });
    let cli = match dutchman_solidcraft::find_cli(&verification) {
        Ok(cli) => cli,
        Err(_) => {
            notes.push_coded(
                Severity::Dropped,
                "drawing",
                code::WRITE_UNAVAILABLE,
                "the drawing is not written: SolidCraft's command line was not found (install SolidCraft 0.47 or newer, or set SOLIDCRAFT_CLI)",
            );
            return Some(section(
                Format::FreeCadDocument,
                "failed",
                &plan,
                (0, 0),
                None,
                &notes,
                source.as_deref(),
                true,
            ));
        }
    };
    let minor = dutchman_solidcraft::solidcraft_version(&verification)
        .and_then(|line| dutchman_solidcraft::solidcraft_minor(&line))
        .and_then(|m| m.split('.').nth(1).and_then(|n| n.parse::<u64>().ok()));
    if minor.is_some_and(|minor| minor < dutchman_solidcraft::drawing::FIRST_DRAWING_MINOR) {
        notes.push_coded(
            Severity::Dropped,
            "drawing",
            code::WRITE_UNAVAILABLE,
            "the drawing is not written: SolidCraft drawings need SolidCraft 0.47 or newer",
        );
        return Some(section(
            Format::FreeCadDocument,
            "failed",
            &plan,
            (0, 0),
            None,
            &notes,
            source.as_deref(),
            true,
        ));
    }
    let name = input
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Drawing".to_owned());
    let timeout = std::time::Duration::from_secs(options.verify_timeout.max(60));
    match dutchman_solidcraft::drawing::add(&cli, &design, &name, &plan, timeout) {
        Ok(written) => {
            for (view, _sheet, _why) in &written.refused {
                notes.push_coded(
                    Severity::Dropped,
                    view,
                    code::WRITE_REFUSED,
                    "SolidCraft refused this view; it is left out",
                );
            }
            let complete = written.sheets == plan.sheets_read
                && written.views == plan.views_read
                && plan.dimensions_read == 0
                && plan.annotations_read == 0
                && !notes
                    .losses()
                    .iter()
                    .any(|l| l.severity == Severity::Dropped);
            Some(section(
                Format::FreeCadDocument,
                if complete { "converted" } else { "partial" },
                &plan,
                (written.sheets, written.views),
                Some(&design),
                &notes,
                source.as_deref(),
                true,
            ))
        }
        Err(_) => {
            notes.push_coded(
                Severity::Dropped,
                "drawing",
                code::WRITE_REFUSED,
                "SolidCraft refused the drawing commands; the design keeps its model without the drawing",
            );
            Some(section(
                Format::FreeCadDocument,
                "failed",
                &plan,
                (0, 0),
                None,
                &notes,
                source.as_deref(),
                true,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unread_drawing_report_has_codes_and_no_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Frame Assembly.idw");
        std::fs::write(
            &path,
            [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0, 0],
        )
        .unwrap();
        let report = convert_unread(&path);
        assert_eq!(report["ok"], false);
        let section = &report["drawing"];
        assert_eq!(section["format"], "inventor-idw");
        assert_eq!(section["status"], "not_decoded");
        assert_eq!(section["first_stop"]["code"], "drawing.format.not_decoded");
        let separators = dir.path().to_string_lossy().into_owned();
        for loss in section["losses"].as_array().unwrap() {
            let message = loss["message"].as_str().unwrap();
            assert!(!message.contains('\n') && !message.contains('/') && !message.contains('\\'));
            assert!(!message.contains(&separators) && !message.contains("Frame Assembly"));
        }
    }
}
