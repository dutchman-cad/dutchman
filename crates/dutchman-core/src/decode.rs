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
pub(crate) fn policy() -> DecodePolicy {
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
pub(crate) fn decode_with_progress(
    format: Format,
    path: &Path,
    bytes: &[u8],
    report: &mut Report,
    progress: Option<&crate::Progress<'_>>,
) -> Result<Model, Error> {
    let codec_id = format.codec_id().ok_or(Error::NotSupportedYet { format })?;
    catch_unwind(AssertUnwindSafe(|| {
        decode_unguarded(
            format,
            codec_id,
            path,
            bytes,
            DecodeDepth::Full,
            policy(),
            None,
            false,
            report,
            progress,
        )
    }))
    .unwrap_or_else(|_| {
        Err(Error::Decode(format!(
            "the {} decoder failed unexpectedly on this file",
            format.name()
        )))
    })
}

/// Planning needs design history; this depth includes geometry but skips process data.
pub(crate) fn planned(
    format: Format,
    path: &Path,
    bytes: &[u8],
    max_bytes: u64,
    max_features: usize,
    deadline: Option<std::time::Instant>,
    report: &mut Report,
) -> PlannedRead {
    let Some(codec_id) = format.codec_id() else {
        return PlannedRead {
            model: Err(PlanningError::Decode(Error::NotSupportedYet { format })),
            features: 0,
        };
    };
    let mut policy = policy();
    policy.limits.max_input_bytes = policy.limits.max_input_bytes.min(max_bytes);
    policy.limits.max_decompressed_bytes_total =
        policy.limits.max_decompressed_bytes_total.min(max_bytes);
    policy.limits.max_decompressed_bytes_per_expand = policy
        .limits
        .max_decompressed_bytes_per_expand
        .min(max_bytes);
    policy.limits.max_retained_bytes = policy.limits.max_retained_bytes.min(max_bytes);
    let limits = PlanningLimits {
        max_features,
        deadline,
        features: std::cell::Cell::new(0),
        stop: std::cell::Cell::new(None),
    };
    let result = catch_unwind(AssertUnwindSafe(|| {
        decode_unguarded(
            format,
            codec_id,
            path,
            bytes,
            DecodeDepth::Design,
            policy,
            Some(&limits),
            false,
            report,
            None,
        )
    }))
    .unwrap_or_else(|_| {
        Err(Error::Decode(
            "the decoder failed unexpectedly during planning".to_owned(),
        ))
    });
    let model = match limits.stop.get() {
        Some(PlanningStop::Features) => Err(PlanningError::FeatureBudget),
        Some(PlanningStop::Time) => Err(PlanningError::TimeBudget),
        None => result.map_err(PlanningError::Decode),
    };
    PlannedRead {
        model,
        features: limits.features.get(),
    }
}

pub(crate) struct PlannedRead {
    pub(crate) model: Result<Model, PlanningError>,
    pub(crate) features: usize,
}

pub(crate) enum PlanningError {
    Decode(Error),
    FeatureBudget,
    TimeBudget,
}

#[derive(Clone, Copy)]
enum PlanningStop {
    Features,
    Time,
}

struct PlanningLimits {
    max_features: usize,
    deadline: Option<std::time::Instant>,
    features: std::cell::Cell<usize>,
    stop: std::cell::Cell<Option<PlanningStop>>,
}

impl PlanningLimits {
    fn check_time(&self) -> Result<(), Error> {
        if self
            .deadline
            .is_some_and(|deadline| std::time::Instant::now() >= deadline)
        {
            self.stop.set(Some(PlanningStop::Time));
            return Err(Error::Decode("planning time budget reached".to_owned()));
        }
        Ok(())
    }

    fn check_features(&self, features: usize) -> Result<(), Error> {
        self.features.set(features);
        self.check_time()?;
        if features > self.max_features {
            self.stop.set(Some(PlanningStop::Features));
            return Err(Error::Decode("planning feature budget reached".to_owned()));
        }
        Ok(())
    }
}

#[cfg(feature = "onshape")]
pub(crate) fn onshape_failure(error: CodecError) -> Error {
    let message = match &error {
        CodecError::ResourceLimit(resource) => format!("{error} during {}", resource.operation),
        _ => error.to_string(),
    };
    Error::Decode(message)
}

/// Decode a bundle's STEP member in its parent's resource session and panic guard.
#[cfg(feature = "onshape")]
pub(crate) fn onshape_step(
    ctx: &DecodeContext<'_>,
    path: &Path,
    bytes: &[u8],
    report: &mut Report,
    progress: Option<&crate::Progress<'_>>,
) -> Result<Model, Error> {
    if let Some(progress) = progress {
        progress.stage(Stage::Decode);
    }
    let catalog = InputCatalog::with_builtins();
    let forced = forced_input("step").map_err(|error| Error::Decode(error.to_string()))?;
    let root = View::over_retained(bytes);
    let ResolvedSource::Native { codec, .. } = catalog
        .resolve_source(ctx, root, forced)
        .map_err(|error| Error::Decode(error.to_string()))?
    else {
        return Err(Error::Decode(
            "Onshape STEP decoder is not available".into(),
        ));
    };
    let _depth = ctx
        .enter_nested("Onshape STEP member")
        .map_err(|error| Error::Decode(error.to_string()))?;
    let decoded = codec
        .decode_with_context(ctx, root)
        .map_err(|error| decode_failure(Format::Step, error))?;
    let (ir, decoded_report, fidelity) = decoded.into_parts();
    if !decoded_report.geometry_transferred() {
        return Err(Error::Decode(
            "no geometry could be read from the Onshape STEP member".into(),
        ));
    }
    report.extend_from_cadmpeg(&decoded_report.losses);
    Ok(Model {
        ir,
        fidelity: Some(fidelity),
        name: design_name(path),
        source_file: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        assembly: None,
        source_step: None,
        merge_bodies: false,
    })
}

/// What to say when cadmpeg read a file but found no body in it. Creo parts keep much of their
/// geometry in features cadmpeg does not resolve yet, so they point to a STEP export.
pub(crate) fn no_geometry_message(format: Format) -> String {
    let mut message = format!("no geometry could be read from this {} file", format.name());
    if format == Format::CreoPart {
        message.push_str(
            "; Dutchman reads only the Creo features it can resolve to a solid. Export a STEP file from Creo and convert that instead",
        );
    }
    message
}

/// Full drawing decode uses the same detector, limits, validation and panic guard. A
/// geometry-only retry would discard the drawing graph, so its errors are returned directly.
pub(crate) fn drawings(
    format: Format,
    path: &Path,
    bytes: &[u8],
    report: &mut Report,
) -> Result<Model, Error> {
    let codec_id = format.codec_id().ok_or(Error::NotSupportedYet { format })?;
    catch_unwind(AssertUnwindSafe(|| {
        decode_unguarded(
            format,
            codec_id,
            path,
            bytes,
            DecodeDepth::Full,
            policy(),
            None,
            true,
            report,
            None,
        )
    }))
    .unwrap_or_else(|_| {
        Err(Error::Decode(
            "the drawing decoder failed unexpectedly on this file".into(),
        ))
    })
}

/// Recheck the existing neutral model after discarding only data the retry can remove.
fn with_fallback(
    ir: &mut cadmpeg_ir::CadIr,
    report: &mut Report,
    policy: &DecodePolicy,
    input_bytes: u64,
) -> Result<(), Error> {
    let why = match validate_decoded(ir, DecodeDepth::Full, policy, input_bytes, report) {
        Ok(()) => return Ok(()),
        Err(Error::Checks { message, facts }) if facts.retry => message,
        Err(Error::Checks { message, facts }) => return Err(Error::Checks {
            message: format!("{message}; a geometry-only retry was not needed: the failing checks do not involve history, sketches or parameters"),
            facts,
        }),
        Err(error) => return Err(error),
    };
    match validate_decoded(ir, DecodeDepth::Geometry, policy, input_bytes, report) {
        Ok(()) => {
            report.push_at(Stage::Project, Severity::Degraded, "history", format!(
                "the feature history, sketches and parameters are left out and the geometry is written alone: {why}"
            ));
            Ok(())
        }
        Err(Error::Checks { message, facts }) => Err(Error::Checks {
            message: format!("{why}; checked as geometry alone, without its history, sketches and parameters, it still fails them {}", message.strip_prefix(CHECKS).unwrap_or(&message)),
            facts,
        }),
        Err(error) => Err(error),
    }
}

/// How a failure of cadmpeg's checks begins.
const CHECKS: &str = "cadmpeg's model of it fails its checks ";

#[allow(clippy::too_many_arguments)]
fn decode_unguarded(
    format: Format,
    codec_id: &str,
    path: &Path,
    bytes: &[u8],
    depth: DecodeDepth,
    policy: DecodePolicy,
    planning: Option<&PlanningLimits>,
    drawings: bool,
    report: &mut Report,
    progress: Option<&crate::Progress<'_>>,
) -> Result<Model, Error> {
    if let Some(planning) = planning {
        planning.check_time()?;
    }
    #[cfg(feature = "onshape")]
    if !drawings && format == Format::OnshapeBundle {
        let failure = |error| {
            if planning.is_some() && matches!(&error, CodecError::ResourceLimit(_)) {
                Error::MemoryBudget(format!("planning decoder resource limit reached: {error}"))
            } else {
                onshape_failure(error)
            }
        };
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy).map_err(failure)?;
        let result = crate::onshape::read(&ctx, path, bytes, report, progress);
        ctx.finish_session().map_err(failure)?;
        let mut model = result?;
        if let Some(planning) = planning {
            planning.check_features(
                model
                    .ir
                    .model
                    .features
                    .len()
                    .max(model.ir.design.features.len()),
            )?;
        }
        with_fallback(&mut model.ir, report, &policy, bytes.len() as u64)?;
        if let Some(planning) = planning {
            planning.check_time()?;
        }
        return Ok(model);
    }
    let catalog = InputCatalog::with_builtins();
    let selection = Phase::new(
        progress.and_then(|p| p.members.clone()),
        "codec_selection",
        bytes.len() as u64,
    );
    let codec = select_codec(&catalog, &policy, format, codec_id, bytes)?;
    drop(selection);
    if let Some(planning) = planning {
        planning.check_time()?;
    }

    if let Some(progress) = progress {
        progress.stage(Stage::Decode);
    }
    let reading_name = path.file_name().map(|name| name.to_string_lossy());
    let reading = progress.map(|p| p.stage_step(crate::StepKind::Reading, reading_name.as_deref()));
    let options = DecodeOptions { depth, policy };
    let decode = || codec.decode(&mut Cursor::new(bytes), &options);
    let members = progress.and_then(|p| p.members.clone()).map(|sink| {
        std::rc::Rc::new(Members {
            phase: std::cell::RefCell::new(Some(Phase::new(
                Some(sink.clone()),
                "container_prepare",
                bytes.len() as u64,
            ))),
            step: Default::default(),
            sink,
            bytes: bytes.len() as u64,
            started: Default::default(),
        })
    });
    let decoded = match &members {
        Some(observer) => cadmpeg_codec_f3d::observe_f3z_members(observer.clone(), decode),
        None => decode(),
    };
    // Also closes preparation on a refusal before the first member callback.
    drop(members);
    drop(reading);
    let decoded = decoded.map_err(|err| {
        if depth == DecodeDepth::Design
            && matches!(&err, DecodeFailure::Codec(CodecError::ResourceLimit(_)))
        {
            Error::MemoryBudget(format!("planning decoder resource limit reached: {err}"))
        } else {
            decode_failure(format, err)
        }
    })?;
    let (mut ir, decode_report, fidelity) = decoded.into_parts();
    if let Some(planning) = planning {
        planning.check_features(ir.model.features.len().max(ir.design.features.len()))?;
    }

    if let Some(error) =
        mesh_only_error(format, &ir, decode_report.geometry_transferred()).filter(|_| !drawings)
    {
        return Err(error);
    }
    if let Some(error) =
        undecoded_mesh_only_error(format, &ir, &decode_report.losses).filter(|_| !drawings)
    {
        return Err(error);
    }
    // An assembly's geometry is in its parts' files, read afterwards.
    if !drawings
        && !decode_report.geometry_transferred()
        && !format.is_assembly()
        && !(format == Format::FreeCadDocument && !ir.model.occurrences.is_empty())
    {
        if format == Format::Step && step_holds_no_shapes(bytes) {
            return Err(Error::EmptyExport(EMPTY_STEP_MESSAGE.to_owned()));
        }
        return Err(Error::Decode(no_geometry_message(format)));
    }
    report.extend_from_cadmpeg(&decode_report.losses);
    if format == Format::FreeCadDocument {
        crate::source_errors::record(&ir, report);
    }

    if let Some(progress) = progress {
        progress.stage(Stage::Project);
    }
    let _projecting = progress.map(|p| {
        p.step(
            crate::StepKind::Projecting,
            reading_name.as_deref(),
            None,
            None,
        )
    });
    let _validation = Phase::new(
        progress.and_then(|p| p.members.clone()),
        "model_validation",
        bytes.len() as u64,
    );
    let checked = if drawings {
        validate_decoded(&mut ir, depth, &policy, bytes.len() as u64, report)
    } else {
        with_fallback(&mut ir, report, &policy, bytes.len() as u64)
    };
    if let Some(planning) = planning {
        planning.check_time()?;
    }
    drop_removed_bodies(&mut ir, report);
    drop_inactive_configuration_bodies(&mut ir, report);

    let model = Model {
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
        merge_bodies: false,
    };
    if let Some(sink) = progress.and_then(|p| p.members.as_ref()) {
        sink.model(&model);
    }
    checked?;
    Ok(model)
}

/// Count mesh bodies from decoded tessellations and typed mesh selections with known members.
/// An unresolved import or an undecoded-container loss alone does not establish how many mesh
/// bodies exist.
fn mesh_only_error(
    format: Format,
    ir: &cadmpeg_ir::CadIr,
    geometry_transferred: bool,
) -> Option<Error> {
    use cadmpeg_ir::features::mesh::MeshSelection;
    use cadmpeg_ir::FeatureOperation;
    use std::collections::BTreeSet;

    if !matches!(format, Format::FusionArchive | Format::FusionMultiDocument)
        || !ir.geometry.bodies.is_empty()
        || !ir.model.occurrences.is_empty()
    {
        return None;
    }
    let mut bodies = BTreeSet::new();
    let mut suppressed = BTreeSet::new();
    for feature in &ir.model.features {
        let FeatureOperation::MeshImport { tessellations } =
            feature.evaluation.definition().operation()
        else {
            continue;
        };
        if feature.suppressed == Some(true) {
            if let MeshSelection::Tessellations(ids) = tessellations {
                suppressed.extend(ids.iter().map(|id| id.as_str()));
            }
            continue;
        }
        match tessellations {
            MeshSelection::Tessellations(ids) => {
                for id in ids {
                    bodies.insert((None, id.as_str()));
                }
            }
            MeshSelection::Native {
                native,
                members: Some(members),
            } => {
                for member in members.as_slice() {
                    bodies.insert((Some(native.as_str()), member.as_str()));
                }
            }
            _ => return None,
        }
    }
    // Every Fusion tessellation is a decoded mesh body, also one without a mesh-import feature.
    for tessellation in &ir.model.tessellations {
        let id = tessellation.id.as_str();
        if !suppressed.contains(id) {
            bodies.insert((None, id));
        }
    }
    if bodies.is_empty() {
        return None;
    }
    let message = format!(
        "it has mesh bodies ({}), which Dutchman doesn't convert yet, and no solid bodies could be read. Convert the meshes to solids in Fusion, then save a new copy",
        bodies.len()
    );
    Some(if geometry_transferred {
        Error::NoGeometry(message)
    } else {
        Error::Decode(message)
    })
}

/// What to tell the user when a STEP file's writer exported a design with nothing in it.
pub const EMPTY_STEP_MESSAGE: &str = "This STEP file contains no shapes: it has a product but no solids, surfaces or curves, so the program that wrote it exported an empty design. Check the design has bodies, then export it again; mesh bodies are not written to STEP, so turn a design's meshes into solid bodies before exporting (in Fusion: Mesh > Modify > Convert Mesh), or export the mesh as STL and open that in SolidCraft directly.";

/// True when `bytes` is a STEP exchange text with a product and no geometric entity at all: no
/// solid, shell, face, surface, curve, edge, vertex or tessellation. Presentation styles
/// (`SURFACE_STYLE_USAGE`, `CURVE_STYLE`) and units (`SOLID_ANGLE_UNIT`) are not geometry. A
/// file that is not STEP text, or that has no product, is not judged empty.
fn step_holds_no_shapes(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    if !text.trim_start().starts_with("ISO-10303-21") {
        return false;
    }
    let mut product = false;
    let mut in_string = false;
    let mut in_comment = false;
    let mut word = String::new();
    let mut previous = '\0';
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_comment {
            if previous == '*' && c == '/' {
                in_comment = false;
            }
            previous = c;
            continue;
        }
        if in_string {
            // A doubled quote inside a string is an escaped quote.
            if c == '\'' {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                } else {
                    in_string = false;
                }
            }
            continue;
        }
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c.to_ascii_uppercase());
            previous = c;
            continue;
        }
        if !word.is_empty() {
            let mut next = c;
            if c.is_whitespace() {
                while let Some(&n) = chars.peek() {
                    if !n.is_whitespace() {
                        break;
                    }
                    chars.next();
                }
                next = chars.peek().copied().unwrap_or(' ');
            }
            if next == '(' {
                if word == "PRODUCT" {
                    product = true;
                } else if is_geometric_entity(&word) {
                    return false;
                }
            }
            word.clear();
        }
        if c == '\'' {
            in_string = true;
        } else if previous == '/' && c == '*' {
            in_comment = true;
        }
        previous = c;
    }
    product
}

fn is_geometric_entity(name: &str) -> bool {
    if name.contains("STYLE") || name.contains("PRESENTATION") || name.ends_with("_UNIT") {
        return false;
    }
    matches!(
        name,
        "LINE" | "CIRCLE" | "ELLIPSE" | "PARABOLA" | "HYPERBOLA" | "POLYLINE" | "POINT_CLOUD"
    ) || [
        "BREP",
        "SHELL",
        "FACE",
        "SURFACE",
        "CURVE",
        "EDGE",
        "SOLID",
        "TESSELLATED",
        "VERTEX",
    ]
    .iter()
    .any(|part| name.contains(part))
}

/// cadmpeg's loss for a Fusion part whose stored mesh bodies, its only bodies, did not decode.
const MESH_ONLY_UNTRANSFERRED: (&str, &str) = ("f3d", "design.mesh-only-untransferred");

/// Name a Fusion design whose only bodies are meshes that cadmpeg could not read. The loss
/// carries no structured count, so the message names none.
fn undecoded_mesh_only_error(
    format: Format,
    ir: &cadmpeg_ir::CadIr,
    losses: &[cadmpeg_ir::report::loss::LossNote],
) -> Option<Error> {
    if !matches!(format, Format::FusionArchive | Format::FusionMultiDocument)
        || !ir.geometry.bodies.is_empty()
        || !ir.model.occurrences.is_empty()
    {
        return None;
    }
    let (namespace, code) = MESH_ONLY_UNTRANSFERRED;
    losses
        .iter()
        .any(|note| note.code.namespace() == namespace && note.code.local_code() == code)
        .then(|| {
            Error::Decode(
                "it holds only mesh bodies, which Dutchman doesn't convert yet, and none of them could be read. Convert the meshes to solids in Fusion, then save a new copy"
                    .to_owned(),
            )
        })
}

/// Visit independent F3Z members through cadmpeg's bounded session API. Models are released
/// after the callback; the caller may retain the first matching member. This is local only.
pub fn repro_members(
    input: &Path,
    visit: &mut crate::repro::MemberVisitor<'_>,
) -> Result<(), Error> {
    catch_unwind(AssertUnwindSafe(|| {
        crate::check_input(input)?;
        let bytes = std::fs::read(input).map_err(|source| Error::Io {
            path: input.into(),
            source,
        })?;
        let arena = DecodeArena::new();
        let policy = policy();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy)
            .map_err(|e| Error::Decode(e.to_string()))?;
        let session = cadmpeg_codec_f3d::F3dCodec
            .open_archive(&ctx, root)
            .map_err(|e| Error::Decode(e.to_string()))?;
        let count = session.members().len();
        for (index, member) in session.members().iter().enumerate().take(256) {
            use cadmpeg_codec_f3d::F3zMemberKind;
            if !matches!(
                member.kind(),
                F3zMemberKind::Design | F3zMemberKind::PlacementOnly
            ) {
                continue;
            }
            let result = session
                .decode_member(member.id())
                .map_err(|e| Error::Decode(e.to_string()))?;
            let model = match result.decoded() {
                Some(Ok(decoded)) => {
                    let mut ir = decoded.ir.clone();
                    // Apply the same in-memory recovery as the original conversion. A
                    // failed check still leaves its model available to the local visitor.
                    let _ =
                        with_fallback(&mut ir, &mut Report::default(), &policy, member.byte_size());
                    Ok(Model {
                        ir,
                        fidelity: None,
                        name: member.id().into(),
                        source_file: Some(member.id().into()),
                        assembly: None,
                        source_step: None,
                        merge_bodies: false,
                    })
                }
                Some(Err(error)) => Err(Error::Decode(error.to_string())),
                None => continue,
            };
            if visit(index + 1, count, member.id(), model) {
                break;
            }
        }
        ctx.finish_session()
            .map_err(|e| Error::Decode(e.to_string()))?;
        Ok(())
    }))
    .unwrap_or_else(|_| {
        Err(Error::Decode(
            "the archive member decoder failed unexpectedly".into(),
        ))
    })
}

/// Scoped phase records survive error returns and caught decoder panics.
struct Phase {
    sink: Option<std::sync::Arc<dyn crate::MemberObserver>>,
    name: &'static str,
    bytes: u64,
    started: crate::time::Instant,
}

impl Phase {
    fn new(
        sink: Option<std::sync::Arc<dyn crate::MemberObserver>>,
        name: &'static str,
        bytes: u64,
    ) -> Self {
        if let Some(sink) = &sink {
            sink.phase_event(name, crate::MemberEvent::Start, 0, bytes);
        }
        Self {
            sink,
            name,
            bytes,
            started: crate::time::Instant::now(),
        }
    }
}

impl Drop for Phase {
    fn drop(&mut self) {
        if let Some(sink) = &self.sink {
            sink.phase_event(
                self.name,
                crate::MemberEvent::Finish,
                u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX),
                self.bytes,
            );
        }
    }
}

struct Members {
    phase: std::cell::RefCell<Option<Phase>>,
    /// The preparation step running inside `container_prepare`.
    step: std::cell::RefCell<Option<Phase>>,
    bytes: u64,
    sink: std::sync::Arc<dyn crate::MemberObserver>,
    started: std::cell::RefCell<std::collections::BTreeMap<String, crate::time::Instant>>,
}

impl cadmpeg_codec_f3d::F3zMemberObserver for Members {
    fn phase_started(&self, phase: cadmpeg_codec_f3d::F3zPhase) {
        let name = match phase {
            cadmpeg_codec_f3d::F3zPhase::Scan => "archive_scan",
            cadmpeg_codec_f3d::F3zPhase::Classify => "archive_classify",
            cadmpeg_codec_f3d::F3zPhase::SessionOpen => "archive_session_open",
        };
        *self.step.borrow_mut() = Some(Phase::new(Some(self.sink.clone()), name, self.bytes));
    }

    fn phase_finished(
        &self,
        _phase: cadmpeg_codec_f3d::F3zPhase,
        _outcome: Result<(), &CodecError>,
    ) {
        self.step.borrow_mut().take();
    }

    fn member_classified(
        &self,
        path: &str,
        elapsed: std::time::Duration,
        nested_scan: std::time::Duration,
        layers: std::time::Duration,
        metadata: std::time::Duration,
    ) {
        let ms =
            |duration: std::time::Duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX);
        self.sink
            .member_classified(path, ms(elapsed), ms(nested_scan), ms(layers), ms(metadata));
    }

    fn member_started(&self, path: &str) {
        // Test builds only: a named design that never finishes, for the stuck-design limit.
        #[cfg(debug_assertions)]
        if std::env::var_os("DUTCHMAN_TEST_STUCK_DESIGN").is_some_and(|stuck| stuck == path) {
            self.sink
                .member_event(path, crate::MemberEvent::Start, 0, None);
            loop {
                std::thread::sleep(std::time::Duration::from_secs(1));
            }
        }
        self.phase.borrow_mut().take();
        let mut started = self.started.borrow_mut();
        // The decoder also caps archive members; bound diagnostics independently.
        if started.len() < 100_000 {
            started.insert(path.to_owned(), crate::time::Instant::now());
        }
        self.sink
            .member_event(path, crate::MemberEvent::Start, 0, None);
    }

    fn member_finished(&self, path: &str, outcome: Result<(), &CodecError>) {
        let ms = self.started.borrow_mut().remove(path).map_or(0, |start| {
            u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
        });
        let (event, code) = match outcome {
            Ok(()) => (crate::MemberEvent::Finish, None),
            Err(error) => (crate::MemberEvent::Fail, Some(member_error_code(error))),
        };
        self.sink.member_event(path, event, ms, code);
        *self.phase.borrow_mut() = Some(Phase::new(
            Some(self.sink.clone()),
            "container_compose",
            self.bytes,
        ));
    }
}

fn member_error_code(error: &CodecError) -> &'static str {
    match error {
        CodecError::WrongFormat(_) => "wrong_format",
        CodecError::Malformed(_) => "malformed",
        CodecError::InvalidInput(_) => "invalid_input",
        CodecError::Truncated { .. } => "truncated",
        CodecError::ResourceLimit(_) => "resource_limit",
        CodecError::UnsupportedDialect { .. } => "unsupported_dialect",
        CodecError::UnsupportedTarget(_) => "unsupported_target",
        CodecError::InsufficientDepth { .. } => "insufficient_depth",
        CodecError::NotImplemented(_) => "not_implemented",
        CodecError::Io(_) => "io",
        _ => "other",
    }
}

/// Keep only the arenas needed to restate geometry and its assembly structure.
/// Rebuilding the model also removes the private feature-regeneration parent graph.
/// Leaves out the bodies an active Remove feature deletes. Fusion keeps a removed body in its
/// B-rep and only stops showing it, and cadmpeg still lists it among the model's bodies, so it
/// would otherwise be converted as part of the design. Only a Remove whose bodies cadmpeg
/// resolves can say which bodies those are; any other is left as it is.
fn drop_removed_bodies(ir: &mut cadmpeg_ir::CadIr, report: &mut Report) {
    use cadmpeg_ir::{BodyRetentionMode, BodySelection, FeatureOperation};
    let mut removed: Vec<(cadmpeg_ir::ids::BodyId, String)> = Vec::new();
    for feature in &ir.model.features {
        if feature.suppressed == Some(true) {
            continue;
        }
        let FeatureOperation::DeleteBody {
            bodies,
            mode: BodyRetentionMode::DeleteSelected,
        } = feature.evaluation.definition().operation()
        else {
            continue;
        };
        let ids: Vec<&cadmpeg_ir::ids::BodyId> = match bodies {
            BodySelection::Bodies(bodies) | BodySelection::Resolved { bodies, .. } => {
                bodies.iter().collect()
            }
            _ => continue,
        };
        let name = feature
            .name
            .clone()
            .unwrap_or_else(|| "a Remove feature".to_owned());
        for id in ids {
            if ir.geometry.bodies.iter().any(|body| &body.id == id)
                && !removed.iter().any(|(known, _)| known == id)
            {
                removed.push((id.clone(), name.clone()));
            }
        }
    }
    if removed.is_empty() {
        return;
    }
    let gone = |id: &cadmpeg_ir::ids::BodyId| removed.iter().any(|(known, _)| known == id);
    ir.geometry.bodies.retain(|body| !gone(&body.id));
    for definition in &mut ir.model.product_definitions {
        definition.bodies.retain(|id| !gone(id));
    }
    let mut by_feature: Vec<(&str, usize)> = Vec::new();
    for (_, name) in &removed {
        match by_feature
            .iter_mut()
            .find(|(known, _)| *known == name.as_str())
        {
            Some((_, count)) => *count += 1,
            None => by_feature.push((name.as_str(), 1)),
        }
    }
    for (name, count) in by_feature {
        let bodies = if count == 1 { "body" } else { "bodies" };
        report.push(
            Severity::Info,
            format!("feature \"{name}\""),
            format!("{count} {bodies} it removes left out of the converted design"),
        );
    }
}

/// Leaves out bodies known only to inactive configurations. Only one active configuration
/// with resolved bodies can say which to keep; a body no configuration lists stays because
/// unknown membership does not mean absence from the saved configuration.
fn drop_inactive_configuration_bodies(ir: &mut cadmpeg_ir::CadIr, report: &mut Report) {
    use std::collections::BTreeSet;
    let mut active = ir
        .model
        .configurations
        .iter()
        .filter(|config| config.active);
    let Some(config) = active.next() else {
        return;
    };
    if active.next().is_some() {
        return;
    }
    let Some(bodies) = &config.bodies else {
        return;
    };
    let kept: BTreeSet<_> = bodies.iter().collect();
    let removed: BTreeSet<_> = ir
        .model
        .configurations
        .iter()
        .filter(|config| !config.active)
        .filter_map(|config| config.bodies.as_ref())
        .flat_map(|bodies| bodies.iter())
        .filter(|id| !kept.contains(id))
        .cloned()
        .collect();
    let name = config.name.as_deref().unwrap_or("unnamed").to_owned();
    let before = ir.geometry.bodies.len();
    ir.geometry
        .bodies
        .retain(|body| !removed.contains(&body.id));
    for definition in &mut ir.model.product_definitions {
        definition.bodies.retain(|id| !removed.contains(id));
    }
    let count = before - ir.geometry.bodies.len();
    if count != 0 {
        let bodies = if count == 1 { "body" } else { "bodies" };
        report.push_coded(
            Severity::Info,
            "configurations",
            "dutchman/configuration.inactive-bodies-left-out",
            format!(
                "{count} {bodies} of other configurations left out; converted the saved configuration \"{name}\""
            ),
        );
    }
}

fn geometry_only(ir: &mut cadmpeg_ir::CadIr) {
    ir.design = Default::default();
    ir.process = Default::default();
    let mut source = std::mem::take(&mut ir.model);
    macro_rules! keep {
        ($($arena:ident),+ $(,)?) => {
            $(ir.model.$arena = std::mem::take(&mut source.$arena);)+
        };
    }
    keep!(
        assets,
        product_definitions,
        occurrences,
        assembly_joints,
        tessellations,
        appearances,
        appearance_bindings,
        attributes,
    );
    for joint in &mut ir.model.assembly_joints {
        joint.driving_parameter = None;
    }
    // The sketches are gone, so what a definition owned must not name them.
    for definition in &mut ir.model.product_definitions {
        definition.sketches.clear();
    }
}

pub(crate) fn validate_decoded(
    ir: &mut cadmpeg_ir::CadIr,
    depth: DecodeDepth,
    policy: &DecodePolicy,
    input_bytes: u64,
    report: &mut Report,
) -> Result<(), Error> {
    if depth == DecodeDepth::Geometry {
        geometry_only(ir);
    }

    // Check the model the way cadmpeg's own conversion does before anything is built on it.
    let arena = DecodeArena::new();
    let decode_error = |err: &dyn std::fmt::Display| Error::Decode(err.to_string());
    let ctx = DecodeContext::for_loaded_input(&arena, policy, input_bytes)
        .map_err(|err| decode_error(&err))?;
    let validation =
        validate_neutral_for_decode(&ctx, ir, Vec::new()).map_err(|err| decode_error(&err))?;
    ctx.finish_session().map_err(|err| decode_error(&err))?;
    if !validation.is_ok() {
        let first = validation
            .findings
            .iter()
            .find(|finding| finding.severity >= CadmpegSeverity::Error)
            .map(|finding| format!(": {}", finding.message))
            .unwrap_or_default();
        return Err(Error::Checks {
            message: format!("{CHECKS}({} errors){first}", validation.error_count()),
            facts: check_facts(ir, &validation.findings),
        });
    }
    for finding in &validation.findings {
        report.push_at(
            Stage::Project,
            finding.severity.into(),
            "validation",
            finding.message.clone(),
        );
    }

    Ok(())
}

/// Classify structured findings while the private model is still available.
pub(crate) fn check_facts(
    ir: &cadmpeg_ir::CadIr,
    findings: &[cadmpeg_ir::report::check::Finding],
) -> crate::CheckFacts {
    use cadmpeg_ir::geometry::pcurve::PcurveGeometry;
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::report::check::Check;
    use std::collections::{BTreeMap, BTreeSet};
    let mut design = BTreeSet::new();
    macro_rules! design_ids {
        ($($arena:ident),+ $(,)?) => { $(
            design.extend(ir.model.$arena.iter().map(|entity| entity.id.as_str()));
        )+ };
    }
    design_ids!(
        features,
        feature_input_topologies,
        feature_result_topologies,
        configurations,
        parameters,
        sketches,
        sketch_constraints,
        spatial_sketches,
        spatial_sketch_constraints,
        spreadsheets
    );
    design.extend(
        ir.model
            .sketch_entities
            .iter()
            .map(|entity| entity.id().as_str()),
    );
    design.extend(
        ir.model
            .spatial_sketch_entities
            .iter()
            .map(|entity| entity.id().as_str()),
    );
    let curves: BTreeMap<_, _> = ir
        .geometry
        .curves
        .iter()
        .map(|c| (c.id.as_str(), &c.geometry))
        .collect();
    let pcurves: BTreeMap<_, _> = ir
        .geometry
        .pcurves
        .iter()
        .map(|c| (c.id.as_str(), &c.geometry))
        .collect();
    let edges: BTreeMap<_, _> = ir
        .geometry
        .edges
        .iter()
        .map(|e| (e.id.as_str(), e))
        .collect();
    let coedges: BTreeMap<_, _> = ir
        .geometry
        .coedges
        .iter()
        .map(|c| (c.id.as_str(), c))
        .collect();
    let mut facts = crate::CheckFacts::default();
    for finding in findings
        .iter()
        .filter(|f| f.severity >= CadmpegSeverity::Error)
    {
        *facts.findings.entry(finding.check.to_string()).or_default() += 1;
        let ids = finding
            .entity
            .iter()
            .chain(&finding.related)
            .map(String::as_str);
        facts.retry |= ids.clone().any(|id| design.contains(id));
        if !matches!(
            finding.check,
            Check::ParameterDomain | Check::CarrierReachability | Check::GeometricConsistency
        ) {
            continue;
        }
        // Count each referenced carrier once per finding, even if reached by two references.
        let mut referenced = BTreeSet::new();
        for id in ids {
            if curves.contains_key(id) || pcurves.contains_key(id) {
                referenced.insert(id);
            }
            if let Some(edge) = edges.get(id) {
                if let Some(curve) = edge.curve() {
                    referenced.insert(curve.as_str());
                }
            }
            if let Some(coedge) = coedges.get(id) {
                referenced.extend(coedge.pcurves.iter().map(|use_| use_.pcurve.as_str()));
                if let Some(curve) = &coedge.use_curve {
                    referenced.insert(curve.curve.as_str());
                }
            }
        }
        for id in referenced {
            let kind = match curves.get(id) {
                Some(CurveGeometry::Procedural { .. }) => "procedural",
                Some(CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(_))) => "nurbs",
                Some(CurveGeometry::Solved(
                    SolvedCurveGeometry::Line(_)
                    | SolvedCurveGeometry::Circle(_)
                    | SolvedCurveGeometry::Ellipse(_)
                    | SolvedCurveGeometry::Parabola(_)
                    | SolvedCurveGeometry::Hyperbola(_),
                )) => "analytic",
                Some(_) => "other",
                None => match pcurves.get(id) {
                    Some(PcurveGeometry::Nurbs { .. } | PcurveGeometry::PolarNurbs { .. }) => {
                        "nurbs"
                    }
                    Some(
                        PcurveGeometry::Transformed(_)
                        | PcurveGeometry::Trimmed(_)
                        | PcurveGeometry::Offset(_),
                    ) => "other",
                    Some(_) => "analytic",
                    None => "unavailable",
                },
            };
            *facts.curve_kinds.entry(kind.into()).or_default() += 1;
        }
    }
    facts
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

/// Prefix evidence only: a weak container signature never selects a format.
pub(crate) fn preflight_codec(prefix: &[u8]) -> Option<&'static str> {
    catch_unwind(AssertUnwindSafe(|| {
        let arena = DecodeArena::new();
        let mut policy = policy();
        policy.limits.max_input_bytes = prefix.len() as u64;
        policy.limits.max_retained_bytes = prefix.len() as u64;
        policy.limits.max_entities = prefix.len() as u64;
        policy.limits.max_decompressed_bytes_total = 0;
        policy.limits.max_decompressed_bytes_per_expand = 0;
        let (ctx, root) = DecodeContext::from_root_bytes(prefix, &arena, &policy).ok()?;
        let catalog = InputCatalog::with_builtins();
        let found = match catalog.detect(&ctx, root).ok()? {
            cadmpeg_registry::DetectionOutcome::Detected {
                codec,
                confidence: cadmpeg_ir::codec::Confidence::High,
            } => Format::from_codec_id(codec.id().as_str()).and_then(Format::codec_id),
            _ => None,
        };
        ctx.finish_session().ok()?;
        found
    }))
    .ok()
    .flatten()
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_creo_part_with_no_body_points_to_a_step_export() {
        let creo = super::no_geometry_message(Format::CreoPart);
        assert!(creo.starts_with("no geometry could be read from this Creo part"));
        assert!(creo.contains("Export a STEP file from Creo"), "{creo}");
        let other = super::no_geometry_message(Format::CatiaPart);
        assert!(!other.contains("STEP"), "{other}");
    }

    #[test]
    fn step_shape_scan_tells_empty_from_geometry() {
        let head = "ISO-10303-21;\nHEADER;\nENDSEC;\nDATA;\n#1 = PRODUCT('a','a','',());\n";
        let tail = "ENDSEC;\nEND-ISO-10303-21;\n";
        let make = |body: &str| format!("{head}{body}{tail}");
        let styled = make(
            "#2 = SURFACE_STYLE_USAGE(.BOTH.,#3);\n#4 = ( NAMED_UNIT(*) SOLID_ANGLE_UNIT() );\n",
        );
        assert!(step_holds_no_shapes(styled.as_bytes()));
        for body in [
            "#2 = MANIFOLD_SOLID_BREP('',#3);\n",
            "#2 = ADVANCED_FACE('',(#3),#4,.T.);\n",
            "#2 = LINE('',#3,#4);\n",
            "#2 = SHELL_BASED_SURFACE_MODEL('',(#3));\n",
            "#2 = B_SPLINE_CURVE_WITH_KNOTS('',1,(#3),.UNSPECIFIED.,.F.,.F.,(2,2),(0.,1.),.UNSPECIFIED.);\n",
        ] {
            assert!(!step_holds_no_shapes(make(body).as_bytes()), "{body}");
        }
        // A name inside a string or comment is not an entity.
        let quoted = make("#2 = DESCRIPTION('MANIFOLD_SOLID_BREP(');\n/* FACE( */\n");
        assert!(step_holds_no_shapes(quoted.as_bytes()));
        // Not STEP text, or no product: not judged empty.
        assert!(!step_holds_no_shapes(b"\x00\x01 PRODUCT("));
        assert!(!step_holds_no_shapes(b"ISO-10303-21;\nDATA;\nENDSEC;\n"));
    }

    #[cfg(feature = "onshape")]
    #[test]
    fn onshape_resource_refusal_keeps_the_caller_operation() {
        let mut policy = policy();
        policy.limits.max_entities = 2;
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        assert!(ctx
            .charge_entities(3, "synthetic Onshape asset admission")
            .is_err());
        let error = onshape_failure(ctx.finish_session().unwrap_err()).to_string();
        assert!(
            error.contains("Entities") && error.contains("synthetic Onshape asset admission"),
            "{error}"
        );
    }

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

    fn mesh_import(selection: serde_json::Value) -> cadmpeg_ir::features::Feature {
        serde_json::from_value(serde_json::json!({"id": "test:model:feature#mesh", "ordinal": 0, "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "mesh_import", "value": {"tessellations": selection}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}))
        .unwrap()
    }

    #[test]
    fn mesh_only_diagnostic_uses_decoded_members_before_the_geometry_gate() {
        let mut model = cube();
        model.ir.model.features =
            vec![mesh_import(serde_json::json!({"kind": "native", "value": {
                "native": "test:model:source#mesh", "members": ["17", "23"]
            }}))];
        // A mixed design keeps its existing conversion and losses.
        assert!(mesh_only_error(Format::FusionArchive, &model.ir, false).is_none());
        model.ir.geometry.bodies.clear();
        let error = mesh_only_error(Format::FusionArchive, &model.ir, false).unwrap();
        assert_eq!(error.stage(), Stage::Decode);
        assert_eq!(error.upstream(), Some("cadmpeg"));
        let transferred = mesh_only_error(Format::FusionArchive, &model.ir, true).unwrap();
        assert_eq!(transferred.stage(), Stage::Project);
        assert_eq!(transferred.kind(), "no_geometry");
        assert!(transferred
            .to_string()
            .starts_with("cannot convert this design:"));
        assert_eq!(error.to_string(), "cannot read this design: it has mesh bodies (2), which Dutchman doesn't convert yet, and no solid bodies could be read. Convert the meshes to solids in Fusion, then save a new copy");
        assert!(mesh_only_error(Format::InventorPart, &model.ir, false).is_none());
        assert!(mesh_only_error(Format::FusionMultiDocument, &model.ir, false).is_some());
        model.ir.model.features[0].suppressed = Some(true);
        assert!(mesh_only_error(Format::FusionArchive, &model.ir, false).is_none());
    }

    #[test]
    fn mesh_only_diagnostic_does_not_invent_counts_for_unresolved_sources() {
        let mut model = cube();
        model.ir.geometry.bodies.clear();
        for selection in [
            serde_json::json!({"kind": "native", "value": {"native": "test:model:source#mesh"}}),
            serde_json::json!({"kind": "unresolved", "value": {"feature": "test:model:feature#mesh"}}),
        ] {
            model.ir.model.features = vec![mesh_import(selection)];
            assert!(mesh_only_error(Format::FusionArchive, &model.ir, false).is_none());
        }
        model.ir.model.features.clear();
        assert!(mesh_only_error(Format::FusionArchive, &model.ir, false).is_none());
    }

    fn mesh_only_untransferred(namespace: &str, code: &str) -> cadmpeg_ir::report::loss::LossNote {
        use cadmpeg_ir::report::loss::{LossNote, LossTaxonomy, NamespacedLossKind};
        LossNote::new(
            NamespacedLossKind::new_owned(
                namespace.to_owned(),
                code.to_owned(),
                LossTaxonomy::GeometryNotTransferred,
            )
            .unwrap(),
            "mesh-only design: the Design asset stores no BREP entry, and 1 of its mesh geometry container(s) transferred no mesh body",
        )
    }

    #[test]
    fn undecoded_mesh_only_design_is_named_from_cadmpeg_loss() {
        let mut model = cube();
        let losses = vec![mesh_only_untransferred(
            "f3d",
            "design.mesh-only-untransferred",
        )];
        // A design with B-rep bodies keeps its conversion.
        assert!(undecoded_mesh_only_error(Format::FusionArchive, &model.ir, &losses).is_none());
        model.ir.geometry.bodies.clear();
        let error = undecoded_mesh_only_error(Format::FusionArchive, &model.ir, &losses).unwrap();
        assert_eq!(error.stage(), Stage::Decode);
        assert_eq!(error.upstream(), Some("cadmpeg"));
        assert_eq!(error.to_string(), "cannot read this design: it holds only mesh bodies, which Dutchman doesn't convert yet, and none of them could be read. Convert the meshes to solids in Fusion, then save a new copy");
        assert!(
            undecoded_mesh_only_error(Format::FusionMultiDocument, &model.ir, &losses).is_some()
        );
        assert!(undecoded_mesh_only_error(Format::InventorPart, &model.ir, &losses).is_none());
        for (namespace, code) in [
            ("f3d", "mesh.container-undecoded"),
            ("inventor", "design.mesh-only-untransferred"),
        ] {
            let other = vec![mesh_only_untransferred(namespace, code)];
            assert!(undecoded_mesh_only_error(Format::FusionArchive, &model.ir, &other).is_none());
        }
    }

    #[test]
    fn mesh_only_diagnostic_deduplicates_resolved_tessellation_identities() {
        let mut model = cube();
        model.ir.geometry.bodies.clear();
        let feature = mesh_import(
            serde_json::json!({"kind": "tessellations", "value": ["test:model:tessellation#mesh"]}),
        );
        model.ir.model.features = vec![feature.clone(), feature];
        let error = mesh_only_error(Format::FusionArchive, &model.ir, false).unwrap();
        assert!(error.to_string().contains("has mesh bodies (1)"));
    }

    fn tessellation(id: &str) -> cadmpeg_ir::tessellation::Tessellation {
        use cadmpeg_ir::math::Point3;
        use cadmpeg_ir::tessellation::{Tessellation, TessellationId, TessellationMesh};
        let point = |x: f64, y: f64, z: f64| Point3 { x, y, z };
        let mesh = TessellationMesh::List {
            vertices: vec![
                point(0.0, 0.0, 0.0),
                point(10.0, 0.0, 0.0),
                point(0.0, 10.0, 0.0),
            ],
            triangles: vec![[0, 1, 2]],
        };
        Tessellation::new(TessellationId::mint(id).unwrap(), mesh, Vec::new()).unwrap()
    }

    #[test]
    fn mesh_only_diagnostic_counts_decoded_meshes_without_a_mesh_import() {
        let mut model = cube();
        model.ir.model.features.clear();
        model.ir.model.tessellations = vec![
            tessellation("test:model:tessellation#a"),
            tessellation("test:model:tessellation#b"),
        ];
        // A design with B-rep bodies keeps its conversion.
        assert!(mesh_only_error(Format::FusionArchive, &model.ir, true).is_none());
        model.ir.geometry.bodies.clear();
        let error = mesh_only_error(Format::FusionArchive, &model.ir, true).unwrap();
        assert_eq!(error.stage(), Stage::Project);
        assert_eq!(error.kind(), "no_geometry");
        assert_eq!(error.to_string(), "cannot convert this design: it has mesh bodies (2), which Dutchman doesn't convert yet, and no solid bodies could be read. Convert the meshes to solids in Fusion, then save a new copy");
        assert!(mesh_only_error(Format::InventorPart, &model.ir, true).is_none());
    }

    #[test]
    fn mesh_only_diagnostic_counts_an_imported_mesh_once() {
        let mut model = cube();
        model.ir.geometry.bodies.clear();
        model.ir.model.tessellations = vec![tessellation("test:model:tessellation#mesh")];
        model.ir.model.features = vec![mesh_import(
            serde_json::json!({"kind": "tessellations", "value": ["test:model:tessellation#mesh"]}),
        )];
        let error = mesh_only_error(Format::FusionArchive, &model.ir, true).unwrap();
        assert!(error.to_string().contains("has mesh bodies (1)"));
        model.ir.model.features[0].suppressed = Some(true);
        assert!(mesh_only_error(Format::FusionArchive, &model.ir, true).is_none());
    }

    #[test]
    fn mesh_only_diagnostic_adds_decoded_meshes_to_native_members() {
        let mut model = cube();
        model.ir.geometry.bodies.clear();
        model.ir.model.tessellations = vec![tessellation("test:model:tessellation#decoded")];
        model.ir.model.features =
            vec![mesh_import(serde_json::json!({"kind": "native", "value": {
                "native": "test:model:source#mesh", "members": ["17", "23"]
            }}))];
        let error = mesh_only_error(Format::FusionArchive, &model.ir, false).unwrap();
        assert!(error.to_string().contains("has mesh bodies (3)"));
    }

    #[test]
    fn duplicate_parameters_are_removed_before_geometry_validation() {
        let mut source = cube();
        for ordinal in 0..2 {
            source.ir.model.parameters.push(
                serde_json::from_value(serde_json::json!({"id": format!("test:design:parameter#{ordinal}"), "ordinal": ordinal, "name": "HoleOffset", "expression": "2 mm", "dependencies": [], "properties": {}}))
                .unwrap(),
            );
        }
        let failure = validate_decoded(
            &mut source.ir.clone(),
            DecodeDepth::Full,
            &policy(),
            0,
            &mut Report::default(),
        )
        .unwrap_err();
        assert!(
            matches!(failure, Error::Checks { ref message, .. }
            if message.contains("repeats parameter name `HoleOffset`")),
            "{failure:?}"
        );

        source.ir.model.sketches.push(
            serde_json::from_value(serde_json::json!({"id": "test:design:sketch#1", "placement": {"kind": "unresolved"}, "profiles": []}))
            .unwrap(),
        );
        for ordinal in 0..2 {
            source.ir.model.features.push(
                serde_json::from_value(serde_json::json!({"id": format!("test:design:feature#{ordinal}"), "ordinal": ordinal, "suppressed": false, "definition": {"kind": "operation", "value": {"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": "test:design:sketch#1"}}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}))
                .unwrap(),
            );
        }
        let parent = source.ir.model.features[0].id.clone();
        let child = source.ir.model.features[1].id.clone();
        source
            .ir
            .model
            .set_feature_regeneration_parent(&child, &parent)
            .unwrap();
        source.ir.model.configurations.push(
            serde_json::from_value(serde_json::json!({"id": "test:design:configuration#1", "parameter_overrides": {"test:design:parameter#0": "HoleOffset + 1 mm"}, "properties": {"HoleOffset": "2 mm"}, "ordinal": 0, "active": false, "parameter_values": {}, "feature_states": {}}))
            .unwrap(),
        );
        let mut joint = cadmpeg_ir::products::AssemblyJoint::grounded(
            cadmpeg_ir::products::JointId::mint("test:assembly:joint#1").unwrap(),
            cadmpeg_ir::products::JointConnector {
                operand: cadmpeg_ir::products::JointOperand::root(
                    source.ir.geometry.bodies[0].id.as_str(),
                    Vec::new(),
                ),
                frame: cadmpeg_ir::transform::Transform::identity(),
                detached: false,
            },
            None,
        );
        joint.driving_parameter = Some(source.ir.model.parameters[0].id.clone());
        source.ir.model.assembly_joints.push(joint.clone());

        let mut report = Report::default();
        let mut model = source.clone();
        with_fallback(&mut model.ir, &mut report, &policy(), 0).unwrap();
        assert_eq!(model.ir.geometry.bodies.len(), 1);
        assert_eq!(model.ir.geometry.bodies, source.ir.geometry.bodies);
        assert_eq!(
            model.ir.model.product_definitions,
            source.ir.model.product_definitions
        );
        assert_eq!(model.ir.model.occurrences, source.ir.model.occurrences);
        assert_eq!(model.ir.model.appearances, source.ir.model.appearances);
        assert!(model.ir.model.parameters.is_empty());
        assert!(model
            .ir
            .populated_arena(cadmpeg_core::decode::ArenaClass::Design)
            .is_none());
        assert!(model.ir.model.feature_regeneration_parent(&child).is_none());
        joint.driving_parameter = None;
        assert_eq!(model.ir.model.assembly_joints, vec![joint]);
        assert!(report.losses().iter().any(|loss| {
            loss.severity == Severity::Degraded
                && loss.stage == Stage::Project
                && loss
                    .to_string()
                    .contains("sketches and parameters are left out")
                && loss.to_string().contains("HoleOffset")
        }));
    }

    #[test]
    fn geometry_validation_still_rejects_duplicate_bodies() {
        let mut model = cube();
        model
            .ir
            .geometry
            .bodies
            .push(model.ir.geometry.bodies[0].clone());
        let failure = validate_decoded(
            &mut model.ir,
            DecodeDepth::Geometry,
            &policy(),
            0,
            &mut Report::default(),
        )
        .unwrap_err();
        assert!(matches!(failure, Error::Checks { .. }), "{failure:?}");
    }

    #[test]
    fn a_body_an_active_remove_deletes_is_left_out() {
        let remove = |suppressed: bool| -> cadmpeg_ir::Feature {
            serde_json::from_value(serde_json::json!({"id": format!("test:model:feature#{suppressed}"), "ordinal": 9, "name": "RemoveBody 1", "suppressed": suppressed, "definition": {"kind": "operation", "value": {"kind": "delete_body", "value": {"bodies": {"kind": "bodies", "value": [cube().ir.geometry.bodies[0].id.clone()]}, "mode": "delete_selected"}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []}))
            .unwrap()
        };
        // A suppressed Remove deletes nothing.
        let mut model = cube();
        model.ir.model.features.push(remove(true));
        let mut report = Report::default();
        drop_removed_bodies(&mut model.ir, &mut report);
        assert_eq!(model.ir.geometry.bodies.len(), 1);
        assert!(report.losses().is_empty());

        let mut model = cube();
        model.ir.model.features.push(remove(false));
        let mut report = Report::default();
        drop_removed_bodies(&mut model.ir, &mut report);
        assert!(model.ir.geometry.bodies.is_empty());
        let loss = report.losses().first().unwrap();
        assert_eq!(loss.subject, "feature \"RemoveBody 1\"");
        assert_eq!(
            loss.detail,
            "1 body it removes left out of the converted design"
        );
    }

    fn configuration_bodies() -> cadmpeg_ir::CadIr {
        let mut ir = cadmpeg_ir::CadIr::empty();
        for id in ["test:model:body#A", "test:model:body#B"] {
            ir.geometry.bodies.push(
                serde_json::from_value(serde_json::json!({"id": id, "kind": "solid"})).unwrap(),
            );
        }
        ir.model.product_definitions.push(
            serde_json::from_value(serde_json::json!({"id": "test:product:definition#0", "kind": "part", "bodies": ["test:model:body#A", "test:model:body#B"], "bom_properties": {}, "sketches": []}))
            .unwrap(),
        );
        for (ordinal, name, body, active) in [(0, "Short", "A", true), (1, "Tall", "B", false)] {
            ir.model.configurations.push(
                serde_json::from_value(serde_json::json!({"id": format!("test:design:configuration#{ordinal}"), "ordinal": ordinal, "name": name, "active": active, "bodies": [format!("test:model:body#{body}")], "properties": {}, "parameter_overrides": {}, "parameter_values": {}, "feature_states": {}}))
                .unwrap(),
            );
        }
        ir
    }

    #[test]
    fn inactive_configuration_bodies_are_left_out_and_reported() {
        let mut ir = configuration_bodies();
        let mut report = Report::default();
        drop_inactive_configuration_bodies(&mut ir, &mut report);
        assert_eq!(ir.geometry.bodies.len(), 1);
        assert_eq!(ir.geometry.bodies[0].id.as_str(), "test:model:body#A");
        assert_eq!(ir.model.product_definitions[0].bodies.len(), 1);
        assert_eq!(
            ir.model.product_definitions[0].bodies[0],
            ir.geometry.bodies[0].id
        );
        assert_eq!(report.losses().len(), 1);
        let loss = &report.losses()[0];
        assert_eq!(loss.severity, Severity::Info);
        assert_eq!(loss.subject, "configurations");
        assert_eq!(
            loss.code.as_deref(),
            Some("dutchman/configuration.inactive-bodies-left-out")
        );
        assert_eq!(
            loss.detail,
            "1 body of other configurations left out; converted the saved configuration \"Short\""
        );
        drop_inactive_configuration_bodies(&mut ir, &mut report);
        assert_eq!(report.losses().len(), 1);
    }

    #[test]
    fn bodies_with_unknown_configuration_membership_are_kept() {
        let mut ir = configuration_bodies();
        ir.geometry.bodies.push(
            serde_json::from_value(serde_json::json!({"id": "test:model:body#C", "kind": "solid"}))
                .unwrap(),
        );
        drop_inactive_configuration_bodies(&mut ir, &mut Report::default());
        assert_eq!(ir.geometry.bodies.len(), 2);
        assert_eq!(ir.geometry.bodies[0].id.as_str(), "test:model:body#A");
        assert_eq!(ir.geometry.bodies[1].id.as_str(), "test:model:body#C");
    }

    #[test]
    fn active_configuration_bodies_are_kept_even_when_shared() {
        let mut ir = configuration_bodies();
        ir.model.configurations[1].bodies = ir.model.configurations[0].bodies.clone();
        let source = ir.clone();
        let mut report = Report::default();
        drop_inactive_configuration_bodies(&mut ir, &mut report);
        assert_eq!(ir, source);
        assert!(report.losses().is_empty());
    }

    #[test]
    fn unresolved_active_configuration_bodies_leave_every_body() {
        let mut ir = configuration_bodies();
        ir.model.configurations[0].bodies = None;
        let source = ir.clone();
        let mut report = Report::default();
        drop_inactive_configuration_bodies(&mut ir, &mut report);
        assert_eq!(ir, source);
        assert!(report.losses().is_empty());
    }

    #[test]
    fn two_active_configurations_leave_every_body() {
        let mut ir = configuration_bodies();
        ir.model.configurations[1].active = true;
        let source = ir.clone();
        let mut report = Report::default();
        drop_inactive_configuration_bodies(&mut ir, &mut report);
        assert_eq!(ir, source);
        assert!(report.losses().is_empty());
    }

    #[test]
    fn no_active_configuration_leaves_every_body() {
        let mut ir = configuration_bodies();
        ir.model.configurations[0].active = false;
        let source = ir.clone();
        let mut report = Report::default();
        drop_inactive_configuration_bodies(&mut ir, &mut report);
        assert_eq!(ir, source);
        assert!(report.losses().is_empty());
        ir.model.configurations.clear();
        let source = ir.clone();
        drop_inactive_configuration_bodies(&mut ir, &mut report);
        assert_eq!(ir, source);
        assert!(report.losses().is_empty());
    }

    #[test]
    fn geometry_alone_leaves_no_definition_naming_a_removed_sketch() {
        let mut model = cube();
        model.ir.model.product_definitions.push(
            serde_json::from_value(serde_json::json!({"id": "test:product:definition#0", "kind": "part", "sketches": ["test:model:sketch#0"], "bom_properties": {}, "bodies": []}))
            .unwrap(),
        );
        geometry_only(&mut model.ir);
        assert!(model
            .ir
            .model
            .product_definitions
            .iter()
            .all(|definition| definition.sketches.is_empty()));
    }

    #[test]
    fn geometry_errors_stop_without_discarding_history() {
        let mut model = cube();
        model
            .ir
            .geometry
            .bodies
            .push(model.ir.geometry.bodies[0].clone());
        let error = with_fallback(&mut model.ir, &mut Report::default(), &policy(), 0).unwrap_err();
        assert!(error.to_string().contains("retry was not needed"));
        assert_eq!(error.stage(), Stage::Project);
        let Error::Checks { facts, .. } = error else {
            panic!("expected checks")
        };
        assert!(!facts.retry);
        assert!(facts.findings.values().sum::<u64>() > 0);
    }

    #[test]
    fn mixed_findings_recheck_only_the_existing_geometry() {
        let mut model = cube();
        for ordinal in 0..2 {
            model.ir.model.parameters.push(
                serde_json::from_value(serde_json::json!({"id": format!("test:design:parameter#{ordinal}"), "ordinal": ordinal, "name": "PrivateParameter", "expression": "2 mm", "dependencies": [], "properties": {}}))
                .unwrap(),
            );
        }
        model
            .ir
            .geometry
            .bodies
            .push(model.ir.geometry.bodies[0].clone());
        let error = with_fallback(&mut model.ir, &mut Report::default(), &policy(), 0).unwrap_err();
        assert!(error.to_string().contains("checked as geometry alone"));
        assert!(error.to_string().contains("it still fails them"));
        assert!(model.ir.model.parameters.is_empty());
        let Error::Checks { facts, .. } = error else {
            panic!("expected checks")
        };
        assert!(!facts.retry);
    }

    #[test]
    fn structured_findings_classify_references_and_never_keep_private_text() {
        use cadmpeg_ir::report::check::{Check, Finding};
        let mut model = cube();
        model.ir.geometry.coedges[0].pcurves.clear();
        let curve_id = model.ir.geometry.curves[0].id.clone();
        model.ir.geometry.coedges[0].use_curve = Some(
            serde_json::from_value(serde_json::json!({
                "curve": curve_id, "parameter_range": [0.0, 1.0]
            }))
            .unwrap(),
        );
        let findings = vec![Finding {
            check: Check::ParameterDomain,
            severity: CadmpegSeverity::Error,
            entity: Some(model.ir.geometry.coedges[0].id.to_string()),
            related: Vec::new(),
            message: "PrivateName 123.456 /private/design.f3z".into(),
        }];
        let facts = check_facts(&model.ir, &findings);
        assert!(!facts.retry);
        assert_eq!(facts.findings.get("parameter_domain"), Some(&1));
        assert_eq!(facts.curve_kinds.get("analytic"), Some(&1));
        model.ir.geometry.curves[0].geometry = serde_json::from_value(serde_json::json!({
            "kind": "procedural", "value": {"construction": "test:model:procedural_curve#0"}
        }))
        .unwrap();
        assert_eq!(
            check_facts(&model.ir, &findings)
                .curve_kinds
                .get("procedural"),
            Some(&1)
        );
        model.ir.geometry.curves[0].geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(serde_json::from_value(serde_json::json!({
            "kind": "nurbs", "degree": 1, "knots": [0.0, 0.0, 1.0, 1.0],
            "poles": {"kind": "polynomial", "points": [{"x": 0.0, "y": 0.0, "z": 0.0}, {"x": 1.0, "y": 0.0, "z": 0.0}]},
            "periodic": false
        })).unwrap());
        assert_eq!(
            check_facts(&model.ir, &findings).curve_kinds.get("nurbs"),
            Some(&1)
        );
        assert!(!format!("{facts:?}").contains("PrivateName"));
        let mut warning = findings[0].clone();
        warning.severity = CadmpegSeverity::Warning;
        assert!(check_facts(&model.ir, &[warning]).findings.is_empty());
    }

    #[test]
    fn phase_events_close_on_refusal_and_member_boundaries() {
        use cadmpeg_codec_f3d::F3zMemberObserver;
        use std::sync::{Arc, Mutex};
        type Event = (&'static str, crate::MemberEvent, u64, u64);
        #[derive(Default)]
        struct Events(Mutex<Vec<Event>>);
        impl crate::MemberObserver for Events {
            fn member_event(
                &self,
                _: &str,
                _: crate::MemberEvent,
                _: u64,
                _: Option<&'static str>,
            ) {
            }
            fn phase_event(
                &self,
                phase: &'static str,
                event: crate::MemberEvent,
                ms: u64,
                bytes: u64,
            ) {
                self.0.lock().unwrap().push((phase, event, ms, bytes));
            }
        }
        let events = Arc::new(Events::default());
        let make = || Members {
            phase: std::cell::RefCell::new(Some(Phase::new(
                Some(events.clone()),
                "container_prepare",
                42,
            ))),
            step: Default::default(),
            bytes: 42,
            sink: events.clone(),
            started: Default::default(),
        };
        drop(make()); // No member callback on a container refusal.
        let members = make();
        members.member_started("PrivateMember.f3d");
        members.member_finished("PrivateMember.f3d", Ok(()));
        drop(members);
        let events = events.0.lock().unwrap();
        assert_eq!(events.len(), 6);
        for pair in events.as_chunks::<2>().0 {
            assert_eq!(pair[0].0, pair[1].0);
            assert_eq!(pair[0].1, crate::MemberEvent::Start);
            assert_eq!(pair[1].1, crate::MemberEvent::Finish);
            assert_eq!(pair[1].3, 42);
        }
        assert_eq!(events[4].0, "container_compose");
    }

    #[test]
    fn member_observer_times_repeated_decodes_and_sends_categories_only() {
        use crate::{MemberEvent, MemberObserver};
        use cadmpeg_codec_f3d::F3zMemberObserver;
        use std::sync::{Arc, Mutex};
        type Event = (String, MemberEvent, u64, Option<&'static str>);
        #[derive(Default)]
        struct Events(Mutex<Vec<Event>>);
        impl MemberObserver for Events {
            fn member_event(
                &self,
                path: &str,
                event: MemberEvent,
                ms: u64,
                code: Option<&'static str>,
            ) {
                self.0.lock().unwrap().push((path.into(), event, ms, code));
            }
        }
        let events = Arc::new(Events::default());
        let observer = Members {
            phase: Default::default(),
            step: Default::default(),
            bytes: 100,
            sink: events.clone(),
            started: Default::default(),
        };
        observer.member_started("ZebraWidget.f3d");
        std::thread::sleep(std::time::Duration::from_millis(2));
        observer.member_finished(
            "ZebraWidget.f3d",
            Err(&CodecError::Malformed("Secret decoder message".into())),
        );
        observer.member_started("ZebraWidget.f3d");
        observer.member_finished("ZebraWidget.f3d", Ok(()));
        let events = events.0.lock().unwrap();
        assert_eq!(events.len(), 4);
        assert_eq!(events[0].1, MemberEvent::Start);
        assert_eq!(events[1].1, MemberEvent::Fail);
        assert!(events[1].2 >= 2);
        assert_eq!(events[1].3, Some("malformed"));
        assert_eq!(events[2].1, MemberEvent::Start);
        assert_eq!(events[3].1, MemberEvent::Finish);
        assert_eq!(events[3].3, None);
        assert!(observer.started.borrow().is_empty());
    }
}
