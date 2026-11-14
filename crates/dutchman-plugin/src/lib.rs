//! The same CADIR and SolidCraft writer pipeline as the CLI, with host-owned file access.
#[cfg(target_arch = "wasm32")]
mod adapter;
// The guest's evaluator over the host's `evaluate` import, and its tests.
#[cfg(any(test, target_arch = "wasm32"))]
mod evaluator;
mod report;
mod stand_up;

use dutchman_core::{Error, Format, Report, Writer};
use dutchman_solidcraft::{SolidCraftEvaluator, SolidCraftVersion, SolidCraftWriter, Verification};
use serde::Deserialize;
use std::path::{Path, PathBuf};

mod limits;
pub use limits::{CHUNK_BYTES, MAX_OUTPUT_BYTES, MAX_REPORT_BYTES};
#[cfg(test)]
mod tests;

#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpAxis {
    #[default]
    AsIs,
    StandUp,
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Options {
    pub editable_history: bool,
    pub up_axis: UpAxis,
    pub include_hidden_bodies: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            editable_history: true,
            up_axis: UpAxis::AsIs,
            include_hidden_bodies: false,
        }
    }
}

pub fn validate_name(name: &str) -> Result<(), String> {
    if name.is_empty()
        || name.len() > 255
        || matches!(name, "." | "..")
        || name.contains(['/', '\\', ':', '\0'])
    {
        return Err("Choose a named CAD file without folder components".into());
    }
    Ok(())
}

/// Resolve stored part paths within the input folder, preferring the longest matching
/// path suffix and nearest referring folder. The host enforces the folder grant.
pub fn resolve(stored: &str, folder: &Path, exists: &dyn Fn(&Path) -> bool) -> Option<PathBuf> {
    const MAX_COMPONENTS: usize = 64;
    const MAX_PATH_BYTES: usize = 4096;
    let safe = |name: &str| {
        !name.is_empty()
            && name.len() <= MAX_PATH_BYTES
            && !name.contains('\\')
            && name.split('/').count() <= MAX_COMPONENTS
            && name
                .split('/')
                .all(|p| !p.is_empty() && !matches!(p, "." | "..") && !p.contains([':', '\0']))
    };
    let normalized = stored.replace('\\', "/");
    let mut components: Vec<_> = normalized.rsplit('/').take(MAX_COMPONENTS).collect();
    components.reverse();
    let folder = PathBuf::from(folder.to_string_lossy().replace('\\', "/"));
    let folders: Vec<_> = folder
        .ancestors()
        .take(MAX_COMPONENTS)
        .filter(|p| p.as_os_str().is_empty() || safe(&p.to_string_lossy()))
        .collect();
    for start in 0..components.len() {
        let tail = components.get(start..).unwrap_or_default().join("/");
        if !safe(&tail) {
            continue;
        }
        for folder in &folders {
            let joined = folder.join(&tail);
            let candidate = PathBuf::from(joined.to_string_lossy().replace('\\', "/"));
            if safe(&candidate.to_string_lossy()) && exists(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

pub fn convert(
    name: &str,
    options: Options,
    reader: &dutchman_core::source::SourceReader<'_>,
    resolve: &dyn Fn(&str, &Path) -> Option<PathBuf>,
    evaluator: Option<std::sync::Arc<dyn SolidCraftEvaluator>>,
) -> Result<(Vec<u8>, String), Error> {
    validate_name(name).map_err(Error::Decode)?;
    let path = Path::new(name);
    let format =
        Format::from_path(path).ok_or_else(|| Error::UnknownFormat { path: path.into() })?;
    if !matches!(
        format,
        Format::FusionArchive
            | Format::FusionMultiDocument
            | Format::InventorPart
            | Format::InventorAssembly
            | Format::SolidWorksPart
            | Format::SolidWorksAssembly
            | Format::FreeCadDocument
    ) {
        return Err(Error::NotSupportedYet { format });
    }
    let mut report = Report::default();
    let model = dutchman_core::source::read(
        format,
        path,
        options.include_hidden_bodies,
        dutchman_core::PartPositions::Smart,
        reader,
        resolve,
        &mut report,
    )?;
    let mut design = Vec::new();
    // With the host's evaluator, the history is checked in SolidCraft itself, in a private
    // session apart from every open design, before stand-up below; without one it stays
    // unverified and hidden, as before. The trait doesn't discover the host's release.
    let writer = SolidCraftWriter::new()
        .threads(1)
        .target_version(Some(SolidCraftVersion::default()))
        .hide_unverified_history(true)
        .import_only(!options.editable_history);
    let writer = match evaluator {
        Some(evaluator) => writer.evaluator(evaluator).verification(Verification::Auto),
        None => writer.verification(Verification::Off),
    };
    writer.write(&model, &mut design, &mut report)?;
    if matches!(options.up_axis, UpAxis::StandUp) {
        design = stand_up::apply(&design)?;
    }
    if design.len() > MAX_OUTPUT_BYTES {
        return Err(Error::Encode(
            "Converted design exceeds 256 MiB; convert fewer parts at once".into(),
        ));
    }
    let mut host_report = report::host(&report);
    let mut text = serde_json::to_string(&host_report).map_err(|e| Error::Encode(e.to_string()))?;
    if text.len() > MAX_REPORT_BYTES {
        // The items already carry each loss; keep the rest of the details.
        if let Some(details) = host_report
            .get_mut("details")
            .and_then(serde_json::Value::as_object_mut)
        {
            details.remove("losses");
        }
        text = serde_json::to_string(&host_report).map_err(|e| Error::Encode(e.to_string()))?;
    }
    if text.len() > MAX_REPORT_BYTES {
        return Err(Error::Encode(
            "Conversion report exceeds 1 MiB; convert fewer parts at once".into(),
        ));
    }
    Ok((design, text))
}
