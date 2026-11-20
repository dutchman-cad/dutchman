//! Bounded prefix evidence and budgeted design decoding for host planners.

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{Format, MAX_INPUT_BYTES};

/// Work ceilings, not estimates of how long conversion will take.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub max_bytes: u64,
    pub max_time: Duration,
    pub max_features: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_bytes: 64 * 1024 * 1024,
            max_time: Duration::from_millis(750),
            max_features: 10_000,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Readability {
    Readable,
    NotReadable,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocumentKind {
    Part,
    Assembly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Prediction {
    FullHistory,
    Partial,
    SolidOnly,
    Refused,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    ReaderUnavailable,
    Planned,
    DecodeFailed,
    TargetPlannerUnavailable,
    ByteBudget,
    TimeBudget,
    FeatureBudget,
    ResourceLimit,
    InputUnavailable,
    NotAFile,
    EmptyInput,
    InputTooLarge,
    FormatNotSupported,
    FormatMismatch,
}

impl Reason {
    pub fn message(self) -> &'static str {
        match self {
            Self::Planned => "The design was read and its history planned without writing output.",
            Self::DecodeFailed => "The design could not be read; review the decoding reason before converting.",
            Self::TargetPlannerUnavailable => "A planning-only forecast for this target is not supported yet; convert for its report.",
            Self::ReaderUnavailable => "A bounded history reader is not supported yet; convert the file for a complete report.",
            Self::ByteBudget => "The scan byte budget was reached; readability and history remain unknown.",
            Self::TimeBudget => "The scan time budget was reached; history forecasts remain unknown.",
            Self::ResourceLimit => "A decoder resource limit was reached; readability and history remain unknown.",
            Self::FeatureBudget => "The scan feature budget was reached; history remains unknown.",
            Self::InputUnavailable => "The input could not be opened; check the path and read permissions.",
            Self::NotAFile => "The input is not a regular file; choose a design file.",
            Self::EmptyInput => "The input is empty; choose a saved design file.",
            Self::InputTooLarge => "The input exceeds the conversion byte limit; choose a smaller design file.",
            Self::FormatMismatch => "The detected format does not match this suffix; use the file's original suffix.",
            Self::FormatNotSupported => "Conversion of this file suffix is not supported yet in this build; choose a supported design file.",
        }
    }
}

/// All absent values mean unknown, including an absent feature-kind list.
#[derive(Clone, Debug, Serialize)]
pub struct Scan {
    pub format_hint: Option<&'static str>,
    pub detected_codec: Option<&'static str>,
    pub source_version: Option<String>,
    pub assembly_hint: Option<bool>,
    pub document_kind: Option<DocumentKind>,
    pub size_bytes: Option<u64>,
    pub max_input_bytes: u64,
    pub bytes_read: u64,
    pub readable: Readability,
    pub reason: Reason,
    pub feature_count: Option<usize>,
    pub feature_kinds: Option<Vec<String>>,
}

/// A target is supplied by the frontend, keeping target knowledge outside the core.
#[derive(Clone, Debug, Serialize)]
pub struct TargetPrediction {
    pub target: String,
    pub target_version: Option<String>,
    pub outcome: Prediction,
    pub reason: Reason,
    pub driving_feature_kinds: Option<Vec<String>>,
    pub mapped_features: Option<usize>,
    pub total_features: Option<usize>,
    pub first_blocking_feature: Option<BlockingFeature>,
    pub detail: Option<String>,
}

impl Scan {
    pub fn predict(&self, target: &str) -> TargetPrediction {
        TargetPrediction {
            target: target.to_owned(),
            target_version: None,
            outcome: if self.readable == Readability::NotReadable {
                Prediction::Refused
            } else {
                Prediction::Unknown
            },
            reason: self.reason,
            driving_feature_kinds: None,
            mapped_features: None,
            total_features: None,
            first_blocking_feature: None,
            detail: None,
        }
    }
}

fn hint(path: &Path) -> Option<Format> {
    // Do not use from_path: shared suffix resolution opens the file outside our budget.
    let ext = Format::source_extension(path)?;
    if ext.eq_ignore_ascii_case("prt") || Format::part_revision(path).is_some() {
        return Some(Format::UnclassifiedPart);
    }
    Format::ALL.into_iter().find(|format| {
        format
            .extensions()
            .iter()
            .any(|suffix| ext.eq_ignore_ascii_case(suffix))
    })
}

fn initial(path: &Path) -> Scan {
    let format = hint(path);
    Scan {
        format_hint: format.map(Format::id),
        detected_codec: None,
        source_version: None,
        assembly_hint: format.map(Format::is_assembly),
        document_kind: None,
        size_bytes: None,
        max_input_bytes: MAX_INPUT_BYTES,
        bytes_read: 0,
        readable: Readability::Unknown,
        reason: Reason::ReaderUnavailable,
        feature_count: None,
        feature_kinds: None,
    }
}

fn refuse(mut scan: Scan, reason: Reason) -> Scan {
    scan.readable = Readability::NotReadable;
    scan.reason = reason;
    scan
}

fn unsupported_hint(scan: &Scan) -> bool {
    scan.format_hint
        .and_then(|id| Format::ALL.into_iter().find(|format| format.id() == id))
        .is_some_and(|format| format != Format::UnclassifiedPart && !format.can_convert())
}

fn budget_reason(budget: Budget, started: Instant) -> Option<Reason> {
    if started.elapsed() >= budget.max_time {
        Some(Reason::TimeBudget)
    } else if budget.max_bytes == 0 {
        Some(Reason::ByteBudget)
    } else if budget.max_features == 0 {
        Some(Reason::FeatureBudget)
    } else {
        None
    }
}

/// Scans a regular file, reading at most the smaller of the byte budget and the
/// detector prefix ceiling. No archive member is expanded and no decoder is run.
/// The time limit is cooperative between operations; filesystem calls and prefix
/// detection cannot be interrupted. Hosts needing a hard deadline must isolate the call.
pub fn scan(path: &Path, budget: Budget) -> Scan {
    use std::io::Read;

    let started = Instant::now();
    let mut result = initial(path);
    if let Some(reason) = budget_reason(budget, started) {
        result.reason = reason;
        return result;
    }
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return refuse(result, Reason::InputUnavailable),
    };
    if !metadata.is_file() {
        return refuse(result, Reason::NotAFile);
    }
    result.size_bytes = Some(metadata.len());
    if metadata.len() == 0 {
        return refuse(result, Reason::EmptyInput);
    }
    if metadata.len() > MAX_INPUT_BYTES {
        return refuse(result, Reason::InputTooLarge);
    }
    if unsupported_hint(&result) {
        return refuse(result, Reason::FormatNotSupported);
    }
    if let Some(reason) = budget_reason(budget, started) {
        result.reason = reason;
        return result;
    }
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return refuse(result, Reason::InputUnavailable),
    };
    let mut prefix = Vec::new();
    if file
        .take(prefix_limit(budget))
        .read_to_end(&mut prefix)
        .is_err()
    {
        result.bytes_read = prefix.len() as u64;
        return refuse(result, Reason::InputUnavailable);
    }
    finish(result, &prefix, budget, started)
}

fn prefix_limit(budget: Budget) -> u64 {
    budget
        .max_bytes
        .min(cadmpeg_registry::DETECTION_PREFIX_LEN as u64)
}

/// Memory-only entry point for hosts supplying uploaded bytes, including the library's
/// wasm build. The caller owns and bounds the upload; this function copies no input.
pub fn scan_bytes(name: &Path, bytes: &[u8], budget: Budget) -> Scan {
    let started = Instant::now();
    let mut result = initial(name);
    result.size_bytes = Some(bytes.len() as u64);
    if let Some(reason) = budget_reason(budget, started) {
        result.reason = reason;
        return result;
    }
    if bytes.is_empty() {
        return refuse(result, Reason::EmptyInput);
    }
    if bytes.len() as u64 > MAX_INPUT_BYTES {
        return refuse(result, Reason::InputTooLarge);
    }
    if unsupported_hint(&result) {
        return refuse(result, Reason::FormatNotSupported);
    }
    let length = usize::try_from(prefix_limit(budget))
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    finish(
        result,
        bytes.get(..length).unwrap_or_default(),
        budget,
        started,
    )
}

fn finish(mut result: Scan, prefix: &[u8], budget: Budget, started: Instant) -> Scan {
    result.bytes_read = prefix.len() as u64;
    if let Some(reason) = budget_reason(budget, started) {
        result.reason = reason;
        return result;
    }
    result.detected_codec = crate::decode::preflight_codec(prefix);
    if started.elapsed() >= budget.max_time {
        result.detected_codec = None;
        result.reason = Reason::TimeBudget;
    } else if (prefix.len() as u64) < result.size_bytes.unwrap_or_default()
        && budget.max_bytes < cadmpeg_registry::DETECTION_PREFIX_LEN as u64
    {
        result.reason = Reason::ByteBudget;
    }
    // An absent detector match is inconclusive, especially for archive containers.
    // Shared suffixes need content evidence, so their hint never refuses conversion.
    result
}

/// The first source feature that prevents further history retention.
#[derive(Clone, Debug, Serialize)]
pub struct BlockingFeature {
    pub kind: String,
    pub reason: String,
}

/// Prefix evidence plus a decoded model for a target-aware host. No output is written.
pub struct Prepared {
    pub scan: Scan,
    pub model: Option<crate::Model>,
    pub report: crate::Report,
    pub stop_reason: Option<Reason>,
    pub detail: Option<String>,
}

/// Read design history with existing validation, panic guards and assembly handling.
/// The byte ceiling charges prefix and all part-file reads. Decode expansion and
/// retention retain the existing limits, additionally capped per decode by max_bytes.
/// Native decoding and filesystem calls are cooperative deadline boundaries.
pub fn prepare(path: &Path, budget: Budget, started: Instant) -> Prepared {
    use std::io::Read;
    let mut scan = if started.elapsed() >= budget.max_time {
        let mut scan = initial(path);
        scan.reason = Reason::TimeBudget;
        scan
    } else {
        scan(path, budget)
    };
    let mut report = crate::Report::default();
    let mut stop_reason = match scan.reason {
        Reason::TimeBudget | Reason::ByteBudget | Reason::FeatureBudget => Some(scan.reason),
        _ => None,
    };
    if started.elapsed() >= budget.max_time {
        stop_reason = Some(Reason::TimeBudget);
    }
    if let Some(reason) = stop_reason {
        scan.reason = reason;
        scan.readable = Readability::Unknown;
    }
    if scan.readable == Readability::NotReadable || stop_reason.is_some() {
        return Prepared {
            scan,
            model: None,
            report,
            stop_reason,
            detail: None,
        };
    }
    let state = std::sync::Mutex::new((scan.bytes_read, 0usize, None));
    let search = crate::PartSearch {
        threads: 1,
        ..Default::default()
    };
    let finder = crate::locate::Finder::new(&search);
    let source_hint = hint(path);
    let format = source_hint.and_then(|format| {
        if format == Format::UnclassifiedPart {
            scan.detected_codec
                .and_then(Format::from_codec_id)
                .filter(|format| matches!(*format, Format::CreoPart | Format::NxPart))
        } else {
            Some(format)
        }
    });
    if source_hint == Some(Format::UnclassifiedPart) && format.is_none() {
        if scan.detected_codec.is_some() {
            scan.readable = Readability::NotReadable;
            scan.reason = Reason::FormatMismatch;
        }
        return Prepared {
            scan,
            model: None,
            report,
            stop_reason,
            detail: None,
        };
    }
    let result = format
        .ok_or_else(|| crate::Error::UnknownFormat {
            path: path.to_path_buf(),
        })
        .and_then(|format| {
            crate::assembly::read(
                format,
                path,
                &mut report,
                &|format, input, report| {
                    let mut state = state.lock().map_err(|_| {
                        crate::Error::Decode(
                            "planning budget accounting could not continue".to_owned(),
                        )
                    })?;
                    if state.2.is_some() {
                        return Err(crate::Error::Decode(
                            "planning budget already reached".to_owned(),
                        ));
                    }
                    if started.elapsed() >= budget.max_time {
                        state.2 = Some(Reason::TimeBudget);
                        return Err(crate::Error::Decode(
                            "planning time budget reached".to_owned(),
                        ));
                    }
                    let remaining = budget
                        .max_bytes
                        .saturating_sub(state.0)
                        .min(MAX_INPUT_BYTES);
                    let file = std::fs::File::open(input).map_err(|source| crate::Error::Io {
                        path: input.to_path_buf(),
                        source,
                    })?;
                    if file
                        .metadata()
                        .map_err(|source| crate::Error::Io {
                            path: input.to_path_buf(),
                            source,
                        })?
                        .len()
                        > remaining
                    {
                        state.2 = Some(Reason::ByteBudget);
                        return Err(crate::Error::Decode(
                            "planning byte budget reached".to_owned(),
                        ));
                    }
                    let mut bytes = Vec::new();
                    (&file)
                        .take(remaining)
                        .read_to_end(&mut bytes)
                        .map_err(|source| crate::Error::Io {
                            path: input.to_path_buf(),
                            source,
                        })?;
                    state.0 = state.0.saturating_add(bytes.len() as u64);
                    if file
                        .metadata()
                        .map_err(|source| crate::Error::Io {
                            path: input.to_path_buf(),
                            source,
                        })?
                        .len()
                        > remaining
                    {
                        state.2 = Some(Reason::ByteBudget);
                        return Err(crate::Error::Decode(
                            "planning byte budget reached".to_owned(),
                        ));
                    }
                    if started.elapsed() >= budget.max_time {
                        state.2 = Some(Reason::TimeBudget);
                        return Err(crate::Error::Decode(
                            "planning time budget reached".to_owned(),
                        ));
                    }
                    let decoded = crate::decode::planned(
                        format,
                        input,
                        &bytes,
                        budget.max_bytes,
                        budget.max_features.saturating_sub(state.1),
                        started.checked_add(budget.max_time),
                        report,
                    );
                    state.1 = state.1.saturating_add(decoded.features);
                    if started.elapsed() >= budget.max_time {
                        state.2 = Some(Reason::TimeBudget);
                        return Err(crate::Error::Decode(
                            "planning time budget reached".to_owned(),
                        ));
                    }
                    let decoded = decoded.model.map_err(|error| match error {
                        crate::decode::PlanningError::TimeBudget => {
                            state.2 = Some(Reason::TimeBudget);
                            crate::Error::Decode("planning time budget reached".to_owned())
                        }
                        crate::decode::PlanningError::FeatureBudget => {
                            state.2 = Some(Reason::FeatureBudget);
                            crate::Error::Decode("planning feature budget reached".to_owned())
                        }
                        crate::decode::PlanningError::Decode(error) => {
                            if matches!(error, crate::Error::MemoryBudget(_)) {
                                state.2 = Some(Reason::ResourceLimit);
                            }
                            error
                        }
                    })?;
                    let model = crate::source::finish_decoded(format, decoded, false, report)?;
                    Ok(model)
                },
                &finder,
                &mut Vec::new(),
            )
        });
    if let Ok(state) = state.lock() {
        scan.bytes_read = state.0;
        stop_reason = state.2;
    }
    if started.elapsed() >= budget.max_time {
        stop_reason = Some(Reason::TimeBudget);
    }
    let mut detail = None;
    let model = match result {
        Ok(mut model) if stop_reason.is_none() => {
            crate::local_components(&mut model, 1, crate::PartPositions::Smart, &mut report);
            let count = feature_count(&model);
            if started.elapsed() >= budget.max_time {
                stop_reason = Some(Reason::TimeBudget);
                None
            } else if count > budget.max_features {
                stop_reason = Some(Reason::FeatureBudget);
                None
            } else {
                scan.feature_count = Some(count);
                scan.readable = Readability::Readable;
                scan.reason = Reason::Planned;
                scan.document_kind = Some(if model.assembly.is_some() {
                    DocumentKind::Assembly
                } else {
                    DocumentKind::Part
                });
                Some(model)
            }
        }
        Ok(_) => None,
        Err(error) => {
            detail = Some(error.to_string());
            if stop_reason.is_none() {
                scan.readable = Readability::NotReadable;
                scan.reason = Reason::DecodeFailed;
            }
            None
        }
    };
    if let Some(reason) = stop_reason {
        scan.reason = reason;
        scan.readable = Readability::Unknown;
    }
    Prepared {
        scan,
        model,
        report,
        stop_reason,
        detail,
    }
}

fn feature_count(model: &crate::Model) -> usize {
    let own = model
        .ir
        .model
        .features
        .len()
        .max(model.ir.design.features.len());
    model.assembly.as_ref().map_or(own, |assembly| {
        assembly
            .parts
            .iter()
            .fold(own, |count, part| count.saturating_add(feature_count(part)))
    })
}
