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
/// are checked against the cap too. `part_positions` applies before resolving any
/// assembly, including nested external assemblies.
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
    read_decoded(
        format,
        name,
        part_positions,
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
        resolve,
        report,
    )
}

#[cfg(feature = "source-reader")]
fn read_decoded(
    format: Format,
    name: &Path,
    part_positions: crate::PartPositions,
    decode: &dyn Fn(Format, &Path, &mut Report) -> Result<Model, Error>,
    resolve: &dyn Fn(&str, &Path) -> Option<PathBuf>,
    report: &mut Report,
) -> Result<Model, Error> {
    let mut finder = crate::locate::Finder::source(resolve);
    finder.part_positions = part_positions;
    let mut model = assembly::read(format, name, report, decode, &finder, &mut Vec::new())?;
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
    let model = crate::decode::decode_with_progress(format, input, bytes, report, progress)?;
    finish_decoded(format, model, include_hidden, report)
}

pub(crate) fn finish_decoded(
    format: Format,
    mut model: Model,
    include_hidden: bool,
    report: &mut Report,
) -> Result<Model, Error> {
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

#[cfg(all(test, feature = "source-reader", feature = "solidworks"))]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::PartPositions;
    use serde_json::json;

    #[test]
    fn source_positions_reach_external_and_nested_assembly_placements() {
        // Synthetic neutral assembly records, as in assembly::solidworks tests. The
        // decoder seam avoids inventing native bytes for displayed pose metadata.
        let decode = |format, path: &Path, report: &mut Report| {
            let mut model = decode_source_bytes(
                Format::SolidWorksPart,
                path,
                include_bytes!("../tests/fixtures/solidworks_cube.sldprt"),
                false,
                report,
                None,
            )?;
            if format == Format::SolidWorksPart {
                return Ok(model);
            }
            let mut wire = serde_json::to_value(&model.ir).unwrap();
            let nested = path == Path::new("nested.sldasm");
            let member = if nested {
                "cube.sldprt"
            } else {
                "nested.sldasm"
            };
            wire["geometry"]["bodies"] = json!([]);
            wire["model"]["occurrences"] = json!([{
                "id":"synthetic:assembly:occurrence#0", "ordinal":0,
                "prototype":{"kind":"external","document":{"kind":"path","path":member}},
                "parent":{"kind":"root"}, "name":"Part", "scale":[1,1,1],
                "transform":[[1,0,0,5],[0,1,0,0],[0,0,1,0]]
            }]);
            wire["native"]["f3d"] = json!({"display_poses":[{
                "id":"f3d:scene:display-pose#0", "stream":"world", "byte_offset":0,
                "path":[1,0], "binding":{"state":"bound","occurrence":"synthetic:assembly:occurrence#0"},
                "transform":[[1,0,0,25],[0,1,0,0],[0,0,1,0],[0,0,0,1]]
            }]});
            model.ir = crate::test_products::read(&wire);
            Ok(model)
        };
        fn leaf_x(model: &Model) -> f64 {
            model.assembly.as_ref().map_or(0.0, |assembly| {
                let placement = &assembly.placements[0];
                placement.transform[0][3] + leaf_x(&assembly.parts[placement.part])
            })
        }
        for (positions, expected) in [
            (PartPositions::Smart, 50.0),
            (PartPositions::LastShown, 50.0),
            (PartPositions::FirstPlaced, 10.0),
        ] {
            let mut report = Report::default();
            let model = read_decoded(
                Format::SolidWorksAssembly,
                Path::new("outer.sldasm"),
                positions,
                &decode,
                &|stored, _| Some(PathBuf::from(stored)),
                &mut report,
            )
            .unwrap();
            assert_eq!(leaf_x(&model), expected, "{positions:?}: {report:?}");
            assert_eq!(report.part_positions().unwrap().0, positions);
        }
    }
}
