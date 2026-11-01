//! Matching exports are read through the same guarded decoder as every native input.
use std::path::{Path, PathBuf};
use std::time::Duration;

use dutchman_core::{Format, Model, Report};
use serde_json::{json, Value};

/// Find an unambiguous same-name STEP beside a native design, case-insensitively.
pub fn matching(input: &Path) -> Option<PathBuf> {
    if Format::from_path(input) == Some(Format::Step) {
        return None;
    }
    let stem = input.file_stem()?.to_string_lossy().to_lowercase();
    let mut found = std::fs::read_dir(
        input
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )
    .ok()?
    .take(dutchman_core::MAX_SEARCHED)
    .filter_map(Result::ok)
    .map(|entry| entry.path())
    .filter(|path| {
        Format::from_path(path) == Some(Format::Step)
            && path
                .file_stem()
                .is_some_and(|s| s.to_string_lossy().to_lowercase() == stem)
            && path.is_file()
    })
    .collect::<Vec<_>>();
    found.sort();
    if found.len() == 1 {
        found.pop()
    } else {
        None
    }
}

pub(super) fn compare(
    model: &Model,
    step: &Path,
    options: &crate::ConvertOptions,
    timeout: Duration,
    preview: Option<&Path>,
) -> Value {
    if Format::from_path(step) != Some(Format::Step) {
        return json!({"status": "unavailable", "parts": [], "reason": "Choose a STEP export of the same design."});
    }
    let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dutchman_core::read_in(
            Format::Step,
            step,
            &options.part_search(),
            &mut Report::default(),
        )
    }));
    match read {
        Ok(Ok(reference)) => {
            let verification = options.solidcraft.clone().map_or(
                dutchman_solidcraft::Verification::Auto,
                dutchman_solidcraft::Verification::With,
            );
            let mut report =
                dutchman_solidcraft::compare::models(model, &reference, &verification, timeout);
            let wrong = report
                .get("parts")
                .and_then(Value::as_array)
                .is_some_and(|rows| {
                    rows.iter()
                        .any(|row| row.get("looks_wrong") == Some(&json!(true)))
                });
            if let Some(preview) = preview.filter(|_| wrong) {
                let rendered = dutchman_solidcraft::compare::render_models(
                    model,
                    &reference,
                    &verification,
                    timeout,
                    preview,
                );
                if let Some(fields) = report.as_object_mut() {
                    fields.insert(
                        "preview_status".into(),
                        json!(if rendered { "available" } else { "unavailable" }),
                    );
                }
            }
            report
        }
        _ => {
            json!({"status": "unavailable", "parts": [], "reason": "The STEP export could not be read. Export it again and retry."})
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discovers_case_insensitively_and_refuses_ambiguous_exports() {
        let dir = tempfile::tempdir().unwrap();
        let input = dir.path().join("Part.f3d");
        let step = dir.path().join("pART.STEP");
        std::fs::write(&step, b"synthetic placeholder").unwrap();
        assert_eq!(matching(&input), Some(step.clone()));
        std::fs::write(dir.path().join("part.stp"), b"synthetic placeholder").unwrap();
        assert_eq!(matching(&input), None);
        assert_eq!(matching(&step), None);
    }
}
