//! Writes the sheets and views of a drawing into a SolidCraft design, through SolidCraft's own
//! `drawing.*` engine commands run by its command line. SolidCraft recomputes every view from
//! the model, so only the sheet, direction, scale and place are carried.

#![cfg(not(target_family = "wasm"))]

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

use dutchman_core::drawing::{Plan, SheetPlan, ViewPlan};
use serde_json::{json, Value};

/// The first SolidCraft release with drawings (drawing format 5).
pub const FIRST_DRAWING_MINOR: u64 = 47;

/// The drawing format name SolidCraft saves.
pub const DRAWING_FORMAT_PREFIX: &str = "solidcraft-drawing/";

/// What was written.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Written {
    /// The drawing format SolidCraft saved, such as `solidcraft-drawing/7`.
    pub format: String,
    /// Sheets in the saved drawing.
    pub sheets: usize,
    /// Views in the saved drawing.
    pub views: usize,
    /// Views SolidCraft refused, as (view name, sheet name, why).
    pub refused: Vec<(String, String, String)>,
}

const STANDARD: [(&str, f64, f64); 6] = [
    ("a0", 841.0, 1189.0),
    ("a1", 594.0, 841.0),
    ("a2", 420.0, 594.0),
    ("a3", 297.0, 420.0),
    ("a4", 210.0, 297.0),
    ("a5", 148.0, 210.0),
];

/// The `size` and `orientation` parameters for paper of `size_mm` (A4 landscape when unknown).
fn paper(size_mm: Option<[f64; 2]>) -> (Value, &'static str) {
    let Some([w, h]) =
        size_mm.filter(|[w, h]| *w > 0.0 && *h > 0.0 && *w <= 5000.0 && *h <= 5000.0)
    else {
        return (json!({"standard": "a4"}), "landscape");
    };
    let (short, long) = if w <= h { (w, h) } else { (h, w) };
    let orientation = if w >= h { "landscape" } else { "portrait" };
    for (name, s, l) in STANDARD {
        if (short - s).abs() <= 1.0 && (long - l).abs() <= 1.0 {
            return (json!({"standard": name}), orientation);
        }
    }
    (json!({"custom": {"sides_mm": [short, long]}}), orientation)
}

/// Sheet names SolidCraft takes and can address: 1 to 256 bytes, each its own.
fn sheet_names(sheets: &[SheetPlan]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (index, sheet) in sheets.iter().enumerate() {
        let mut base: String = sheet.name.chars().take(80).collect();
        if base.trim().is_empty() {
            base = format!("Sheet {}", index + 1);
        }
        let mut name = base.clone();
        let mut n = 2;
        while names.contains(&name) {
            name = format!("{base} {n}");
            n += 1;
        }
        names.push(name);
    }
    names
}

fn view_command(sheet: &str, view: &ViewPlan) -> Value {
    let mut params = json!({
        "sheet": sheet,
        "view": {"direction": view.direction, "up": view.up},
        "position_mm": view.position_mm,
    });
    if let (Some(scale), Some(map)) = (&view.scale, params.as_object_mut()) {
        map.insert("scale".to_owned(), json!(scale));
    }
    json!({"command": "drawing.view.base", "params": params})
}

/// Adds a drawing named `name` holding `plan`'s sheets and views to the design at `design`,
/// replacing it. Views SolidCraft refuses are left out and named in the result.
pub fn add(
    cli: &Path,
    design: &Path,
    name: &str,
    plan: &Plan,
    timeout: Duration,
) -> Result<Written, String> {
    if plan.sheets.is_empty() {
        return Err("the drawing has no sheet to write".to_owned());
    }
    let deadline = Instant::now() + timeout;
    let dir = tempfile::tempdir().map_err(|err| format!("no scratch folder: {err}"))?;
    let names = sheet_names(&plan.sheets);
    let mut kept: Vec<(usize, usize)> = plan
        .sheets
        .iter()
        .enumerate()
        .flat_map(|(s, sheet)| (0..sheet.views.len()).map(move |v| (s, v)))
        .collect();
    let mut refused = Vec::new();
    let saved = dir.path().join("saved.sc");
    for _ in 0..=40 {
        let mut script = Vec::new();
        for (index, sheet) in plan.sheets.iter().enumerate() {
            let (size, orientation) = paper(sheet.size_mm);
            let sheet_name = names.get(index).cloned().unwrap_or_default();
            if index == 0 {
                script.push(json!({"command": "drawing.create", "params": {
                    "name": name.chars().take(80).collect::<String>(),
                    "first_sheet_name": sheet_name,
                    "first_sheet_paper": {"size": size, "orientation": orientation},
                }}));
            } else {
                script.push(json!({"command": "drawing.sheet.add", "params": {
                    "name": sheet_name, "size": size, "orientation": orientation,
                }}));
            }
        }
        let first_view = script.len();
        for (s, v) in &kept {
            if let (Some(sheet), Some(view)) = (
                names.get(*s),
                plan.sheets.get(*s).and_then(|sheet| sheet.views.get(*v)),
            ) {
                script.push(view_command(sheet, view));
            }
        }
        // The design may have no drawing name of its own; give the drawing the file's.
        let result =
            crate::verify::run_and_save(cli, deadline, dir.path(), design, &saved, &script)?;
        let Some(error) = result.get("error").filter(|e| !e.is_null()) else {
            let written = summarize(&saved, refused)?;
            std::fs::copy(&saved, design)
                .map_err(|err| format!("cannot replace the design: {err}"))?;
            return Ok(written);
        };
        let step = error.get("step").and_then(Value::as_u64).unwrap_or(0) as usize;
        let why = error
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("refused")
            .to_owned();
        // Steps are numbered from 1, counting the model's own commands: here the script alone.
        let index = step.saturating_sub(1);
        match index.checked_sub(first_view) {
            Some(view_index) if view_index < kept.len() => {
                let (s, v) = kept.remove(view_index);
                let sheet = plan.sheets.get(s);
                refused.push((
                    sheet
                        .and_then(|sheet| sheet.views.get(v))
                        .map(|view| view.name.clone())
                        .unwrap_or_default(),
                    names.get(s).cloned().unwrap_or_default(),
                    why,
                ));
            }
            _ => return Err(why),
        }
    }
    Err("SolidCraft refused too many views of the drawing".to_owned())
}

/// Counts what the saved design holds.
fn summarize(saved: &Path, refused: Vec<(String, String, String)>) -> Result<Written, String> {
    let (format, sheets, views) = inspect(saved)?;
    Ok(Written {
        format,
        sheets,
        views,
        refused,
    })
}

/// The drawing format, sheet count and view count of the first drawing in the design `.sc`.
pub fn inspect(design: &Path) -> Result<(String, usize, usize), String> {
    let file =
        std::fs::File::open(design).map_err(|err| format!("cannot read the design: {err}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|err| format!("not a SolidCraft design: {err}"))?;
    let names: Vec<String> = (0..archive.len())
        .filter_map(|index| {
            let member = archive.by_index(index).ok()?;
            member.name().ok().map(|name| name.into_owned())
        })
        .collect();
    let entry = names
        .iter()
        .find(|n| n.starts_with("drawings/") && n.ends_with(".json"))
        .ok_or_else(|| "the saved design holds no drawing".to_owned())?;
    let mut text = String::new();
    archive
        .by_name(entry)
        .map_err(|err| format!("cannot read the drawing: {err}"))?
        .take(256 << 20)
        .read_to_string(&mut text)
        .map_err(|err| format!("cannot read the drawing: {err}"))?;
    let value: Value =
        serde_json::from_str(&text).map_err(|err| format!("the drawing is not readable: {err}"))?;
    let count = |key: &str| value.get(key).and_then(Value::as_array).map_or(0, Vec::len);
    let format = value
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or(DRAWING_FORMAT_PREFIX)
        .to_owned();
    Ok((format, count("sheets"), count("views")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_matches_standard_sizes_either_way_round() {
        assert_eq!(
            paper(Some([297.0, 210.0])),
            (json!({"standard": "a4"}), "landscape")
        );
        assert_eq!(
            paper(Some([420.0, 594.0])),
            (json!({"standard": "a2"}), "portrait")
        );
        assert_eq!(paper(None).1, "landscape");
        let (size, _) = paper(Some([300.0, 123.0]));
        assert_eq!(size["custom"]["sides_mm"], json!([123.0, 300.0]));
    }

    #[test]
    fn sheet_names_are_unique_and_never_blank() {
        let sheet = |name: &str| SheetPlan {
            name: name.to_owned(),
            size_mm: None,
            views: vec![],
        };
        let names = sheet_names(&[sheet("Page"), sheet("Page"), sheet("  ")]);
        assert_eq!(names, ["Page", "Page 2", "Sheet 3"]);
    }
}
