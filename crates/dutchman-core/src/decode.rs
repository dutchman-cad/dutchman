//! The one place Dutchman calls cadmpeg's decoders.

use std::io::Cursor;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;

use cadmpeg_core::decode::{
    DecodeArena, DecodeContext, DecodeDepth, DecodePolicy, ResourceLimits, View,
};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions};
use cadmpeg_ir::report::Severity as CadmpegSeverity;
use cadmpeg_ir::validate::validate_neutral_for_decode;
use cadmpeg_registry::{
    forced_input, InputCatalog, ResolveSourceError, ResolvedSource, DETECTION_PREFIX_LEN,
};

use crate::{Error, Format, Model, Report, Severity, Stage, FILE_DID_NOT_CONVERT, MAX_INPUT_BYTES};

/// Uses cadmpeg's bounded detector to distinguish Creo and NX part files.
pub(crate) fn detect_part(path: &Path) -> Option<Format> {
    use std::io::Read;
    catch_unwind(AssertUnwindSafe(|| {
        if !std::fs::metadata(path).ok()?.is_file() {
            return None;
        }
        let file = std::fs::File::open(path).ok()?;
        let mut prefix = Vec::new();
        file.take(DETECTION_PREFIX_LEN as u64)
            .read_to_end(&mut prefix)
            .ok()?;
        let arena = DecodeArena::new();
        let policy = policy();
        let (ctx, root) = DecodeContext::from_root_bytes(&prefix, &arena, &policy).ok()?;
        let catalog = InputCatalog::with_builtins();
        let found = match catalog.resolve_source(&ctx, root, None).ok()? {
            ResolvedSource::Native { codec, .. } => Format::from_codec_id(codec.id().as_str()),
            _ => None,
        };
        ctx.finish_session().ok()?;
        found.filter(|f| matches!(f, Format::CreoPart | Format::NxPart))
    }))
    .ok()
    .flatten()
}

/// cadmpeg's desktop ceilings in salvage mode: a successful decode keeps going past a loss
/// and reports it, rather than refusing the whole file. The input cap is Dutchman's own.
fn policy() -> DecodePolicy {
    DecodePolicy {
        limits: ResourceLimits {
            max_input_bytes: MAX_INPUT_BYTES,
            ..ResourceLimits::desktop()
        },
        ..DecodePolicy::desktop()
    }
}

/// Decodes `bytes`, the contents of the `format` file at `path`, into the neutral model.
///
/// cadmpeg is third-party code run on hostile input, so a panic inside it is caught and
/// returned as [`Error::Decode`] instead of taking the process down.
pub(crate) fn decode(
    format: Format,
    path: &Path,
    bytes: &[u8],
    report: &mut Report,
) -> Result<Model, Error> {
    let codec_id = format.codec_id().ok_or(Error::NotSupportedYet { format })?;
    catch_unwind(AssertUnwindSafe(|| {
        with_fallback(report, |depth, report| {
            decode_unguarded(format, codec_id, path, bytes, depth, report)
        })
    }))
    .unwrap_or_else(|_| {
        Err(Error::Decode(format!(
            "the {} decoder failed unexpectedly on this file",
            format.name()
        )))
    })
}

/// Why a decode at one depth gave no model.
#[derive(Debug)]
enum Failed {
    /// cadmpeg decoded the file, but its model fails cadmpeg's checks: why.
    Checks(String),
    /// Anything else.
    Other(Error),
}

/// Decodes with `decode` in full; when cadmpeg's model of the design fails its checks,
/// decodes the geometry alone (B-rep, product structure, placements, appearance: no
/// features, sketches or parameters, where such failures lie) and, when that passes, keeps
/// it with the full model's failure as a loss, rather than losing the file.
fn with_fallback(
    report: &mut Report,
    decode: impl Fn(DecodeDepth, &mut Report) -> Result<Model, Failed>,
) -> Result<Model, Error> {
    let mut full = Report::default();
    let why = match decode(DecodeDepth::Full, &mut full) {
        Ok(model) => {
            report.append(full);
            return Ok(model);
        }
        Err(Failed::Other(err)) => return Err(err),
        Err(Failed::Checks(why)) => why,
    };
    let mut geometry = Report::default();
    match decode(DecodeDepth::Geometry, &mut geometry) {
        Ok(model) => {
            report.append(geometry);
            report.push_at(
                Stage::Project,
                Severity::Degraded,
                "history",
                format!(
                    "the feature history, sketches and parameters are left out and the geometry \
                     is written alone: {why}"
                ),
            );
            Ok(model)
        }
        // Say what each attempt found: the checks may fail the same way twice.
        Err(Failed::Checks(again)) => Err(Error::Decode(format!(
            "{why}; read again as geometry alone, without its history, sketches and parameters, \
             it still fails them {}",
            again.strip_prefix(CHECKS).unwrap_or(&again)
        ))),
        Err(Failed::Other(err)) => Err(Error::Decode(format!(
            "{why}; reading it again as geometry alone failed too: {err}"
        ))),
    }
}

/// How a failure of cadmpeg's checks begins.
const CHECKS: &str = "cadmpeg's model of it fails its checks ";

fn decode_unguarded(
    format: Format,
    codec_id: &str,
    path: &Path,
    bytes: &[u8],
    depth: DecodeDepth,
    report: &mut Report,
) -> Result<Model, Failed> {
    let other = |err: Error| Failed::Other(err);
    let policy = policy();
    let catalog = InputCatalog::with_builtins();
    let codec = select_codec(&catalog, &policy, format, codec_id, bytes).map_err(other)?;

    let options = DecodeOptions { depth, policy };
    let decoded = codec
        .decode(&mut Cursor::new(bytes), &options)
        .map_err(|err| decode_failure(format, err))
        .map_err(other)?;
    let (ir, decode_report, fidelity) = decoded.into_parts();
    // An assembly's geometry is in its parts' files, read afterwards.
    if !decode_report.geometry_transferred() && !format.is_assembly() {
        return Err(other(Error::Decode(format!(
            "no geometry could be read from this {} file",
            format.name()
        ))));
    }
    report.extend_from_cadmpeg(&decode_report.losses);

    // Check the model the way cadmpeg's own conversion does before anything is built on it.
    let arena = DecodeArena::new();
    let decode_error = |err: &dyn std::fmt::Display| other(Error::Decode(err.to_string()));
    let ctx = DecodeContext::for_loaded_input(&arena, &policy, bytes.len() as u64)
        .map_err(|err| decode_error(&err))?;
    let validation =
        validate_neutral_for_decode(&ctx, &ir, Vec::new()).map_err(|err| decode_error(&err))?;
    ctx.finish_session().map_err(|err| decode_error(&err))?;
    if !validation.is_ok() {
        let first = validation
            .findings
            .iter()
            .find(|finding| finding.severity >= CadmpegSeverity::Error)
            .map(|finding| format!(": {}", finding.message))
            .unwrap_or_default();
        return Err(Failed::Checks(format!(
            "{CHECKS}({} errors){first}",
            validation.error_count()
        )));
    }
    for finding in &validation.findings {
        report.push_at(
            Stage::Project,
            finding.severity.into(),
            "validation",
            finding.message.clone(),
        );
    }

    Ok(Model {
        ir,
        fidelity: Some(fidelity),
        name: design_name(path),
        source_file: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        assembly: None,
        source_step: (format == Format::Step)
            .then(|| String::from_utf8(bytes.to_vec()).ok())
            .flatten(),
    })
}

/// Keep cadmpeg's cause, and give a recovery step for unreadable containers.
fn decode_failure(format: Format, err: DecodeFailure) -> Error {
    let recovery = matches!(
        &err,
        DecodeFailure::Codec(
            CodecError::Malformed(_) | CodecError::WrongFormat(_) | CodecError::Truncated { .. }
        )
    );
    let cause = match err {
        DecodeFailure::Codec(err) => err.to_string(),
        other => other.to_string(),
    };
    if recovery {
        Error::Decode(format!(
            "{cause}. The file may be incomplete or not a {} file; download or copy it again, \
             or check its extension. If the original opens in {}, report it with the file's \
             anonymous report: {FILE_DID_NOT_CONVERT}",
            format.name(),
            format.family()
        ))
    } else {
        Error::Decode(cause)
    }
}

/// Picks the codec for `bytes`: the one cadmpeg recognises from the contents, or, when the
/// contents are not conclusive, the one the extension named. Contents that clearly belong to
/// a different format are refused rather than decoded with the wrong codec.
fn select_codec<'a>(
    catalog: &'a InputCatalog,
    policy: &DecodePolicy,
    format: Format,
    codec_id: &str,
    bytes: &[u8],
) -> Result<&'a dyn Codec, Error> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, policy)
        .map_err(|err| Error::Decode(err.to_string()))?;
    let prefix = bytes.get(..DETECTION_PREFIX_LEN).unwrap_or(bytes);
    let detected = catalog.resolve_seekable_source(
        &ctx,
        View::over_retained(prefix),
        &mut Cursor::new(bytes),
        None,
    );
    let resolved = match detected {
        Ok(native @ ResolvedSource::Native { .. }) => Ok(native),
        Ok(ResolvedSource::Cadir | ResolvedSource::Unrecognized)
        | Err(ResolveSourceError::Ambiguous(_)) => {
            let forced = forced_input(codec_id).map_err(|err| Error::Decode(err.to_string()))?;
            catalog.resolve_source(&ctx, View::over_retained(prefix), forced)
        }
        Err(err) => Err(err),
    };
    ctx.finish_session()
        .map_err(|err| Error::Decode(err.to_string()))?;
    match resolved.map_err(|err| Error::Decode(err.to_string()))? {
        ResolvedSource::Native { codec, .. } if codec.id().as_str() == codec_id => Ok(codec),
        ResolvedSource::Native { codec, .. } => {
            let found = Format::from_codec_id(codec.id().as_str()).map_or_else(
                || format!("{} data", codec.id().as_str()),
                |found| found.name().to_owned(),
            );
            Err(Error::Decode(format!(
                "its contents are {found}, not {}; rename it to match",
                format.name()
            )))
        }
        ResolvedSource::Cadir | ResolvedSource::Unrecognized => Err(Error::Decode(format!(
            "its contents are not {}",
            format.name()
        ))),
    }
}

/// The design's name: the input file's stem, or `Design` when it has none.
fn design_name(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .filter(|stem| !stem.trim().is_empty())
        .unwrap_or_else(|| "Design".to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn recovery_advice_is_for_unreadable_containers_not_missing_capabilities() {
        for cause in [
            CodecError::Malformed("ZIP end record is absent".to_owned()),
            CodecError::WrongFormat("not a Fusion archive".to_owned()),
        ] {
            let message = decode_failure(Format::FusionArchive, cause.into());
            assert_eq!(message.stage(), Stage::Decode);
            assert_eq!(message.upstream(), Some("cadmpeg"));
            let message = message.to_string();
            assert!(message.contains("download or copy it again"), "{message}");
            assert!(
                message.contains("If the original opens in Fusion"),
                "{message}"
            );
        }
        let gap = decode_failure(
            Format::FusionArchive,
            CodecError::NotImplemented("a new container version".to_owned()).into(),
        );
        assert_eq!(
            gap.to_string(),
            "cannot read this design: not implemented yet: a new container version"
        );
    }

    fn cube() -> Model {
        crate::read(
            Format::InventorPart,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap()
    }

    #[test]
    fn a_design_failing_its_checks_is_written_as_its_geometry() {
        let mut report = Report::default();
        let model = with_fallback(&mut report, |depth, report| match depth {
            DecodeDepth::Full => {
                report.push(Severity::Info, "full", "not kept");
                Err(Failed::Checks(
                    "parameter scope None repeats a name".to_owned(),
                ))
            }
            _ => {
                report.push(Severity::Info, "geometry", "kept");
                Ok(cube())
            }
        })
        .unwrap();
        assert_eq!(model.ir.model.bodies.len(), 1);
        let losses: Vec<String> = report.losses().iter().map(ToString::to_string).collect();
        assert_eq!(losses.len(), 2, "{losses:?}");
        assert!(losses[0].contains("geometry: kept"), "{losses:?}");
        assert!(
            losses[1].contains("geometry is written alone: parameter scope None repeats a name"),
            "{losses:?}"
        );
        assert_eq!(report.losses()[1].stage, Stage::Project);
    }

    #[test]
    fn when_the_geometry_fails_too_both_failures_are_said() {
        let err = with_fallback(&mut Report::default(), |depth, _| {
            Err(Failed::Checks(match depth {
                DecodeDepth::Full => format!("{CHECKS}(10 errors): scope repeats `x`"),
                _ => format!("{CHECKS}(3 errors): scope repeats `y`"),
            }))
        })
        .unwrap_err();
        assert_eq!(
            err.to_string(),
            Error::Decode(format!(
                "{CHECKS}(10 errors): scope repeats `x`; read again as geometry alone, without \
                 its history, sketches and parameters, it still fails them (3 errors): scope \
                 repeats `y`"
            ))
            .to_string()
        );
        let other = with_fallback(&mut Report::default(), |_, _| {
            Err(Failed::Other(Error::Decode("unreadable".to_owned())))
        })
        .unwrap_err();
        assert!(other.to_string().contains("unreadable"));
    }
}
