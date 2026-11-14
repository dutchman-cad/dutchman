//! Capability-based reading for front ends that have no ambient filesystem.
use crate::{assembly, hidden, names, Error, Format, Model, Progress, Report, Severity, Stage};
use std::path::Path;
#[cfg(feature = "source-reader")]
use std::path::PathBuf;

/// A caller-owned file reader that refuses inputs above the supplied byte cap.
#[cfg(feature = "source-reader")]
pub type SourceReader<'a> = dyn Fn(&Path, u64) -> Result<Vec<u8>, Error> + 'a;

/// Read through caller-owned capabilities, retaining the normal assembly and placement pipeline.
/// `resolve` maps stored part paths to host-relative names; it must never grant ambient access.
/// The reader and resolver run serially. The reader receives the byte cap and must refuse
/// larger files before producing their bytes (returning [`Error::TooLarge`]). Returned bytes
/// are checked against the cap too.
#[cfg(feature = "source-reader")]
pub fn read(
    format: Format,
    name: &Path,
    include_hidden: bool,
    part_positions: crate::PartPositions,
    reader: &SourceReader<'_>,
    resolve: &dyn Fn(&str, &Path) -> Option<PathBuf>,
    report: &mut Report,
) -> Result<Model, Error> {
    let finder = crate::locate::Finder::source(resolve);
    let mut model = assembly::read(
        format,
        name,
        report,
        &|format, path, report| {
            if !format.can_convert() {
                return Err(Error::NotSupportedYet { format });
            }
            let bytes = reader(path, crate::MAX_INPUT_BYTES)?;
            if bytes.len() as u64 > crate::MAX_INPUT_BYTES {
                return Err(Error::TooLarge {
                    path: path.into(),
                    bytes: bytes.len() as u64,
                    limit: crate::MAX_INPUT_BYTES,
                });
            }
            names::saw_path(path, report);
            decode_source_bytes(format, path, &bytes, include_hidden, report, None)
        },
        &finder,
        &mut Vec::new(),
    )?;
    crate::local_components(&mut model, 1, part_positions, report);
    report.note_part_positions(part_positions);
    Ok(model)
}

pub(crate) fn decode_source_bytes(
    format: Format,
    input: &Path,
    bytes: &[u8],
    include_hidden: bool,
    report: &mut Report,
    progress: Option<&Progress<'_>>,
) -> Result<Model, Error> {
    if let Some(progress) = progress {
        progress.stage(Stage::Decode);
    }
    let mut model = crate::decode::decode_with_progress(format, input, bytes, report, progress)?;
    if matches!(format, Format::SolidWorksPart | Format::SolidWorksAssembly) {
        assembly::release_solidworks_display(&mut model, report);
    }
    names::saw_model(&model, report);
    if include_hidden {
        return Ok(model);
    }
    if matches!(format, Format::FusionArchive | Format::FusionMultiDocument) {
        return Ok(hidden::fusion(model, report));
    }
    if format != Format::FreeCadDocument {
        return Ok(model);
    }
    match hidden::shown(&model) {
        Ok(None) => Ok(model),
        Ok(Some((shown, hidden))) => {
            if hidden > 0 {
                let body = if hidden == 1 { "body" } else { "bodies" };
                report.push(
                    Severity::Info,
                    "bodies",
                    format!("{hidden} hidden {body} not converted: FreeCAD hides them or a container above them"),
                );
            }
            Ok(shown)
        }
        Err(why) => {
            report.push(
                Severity::Degraded,
                "bodies",
                format!("hidden bodies converted too, as {why}"),
            );
            Ok(model)
        }
    }
}
