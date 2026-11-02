//! Local, proven reproduction bundles. Previews contain only the inventory and sizes.
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::report_send::{self, BundlePart, BundleSource, Prepared};
use dutchman_core::{repro, MemberEvent, MemberObserver, Model};
use serde_json::{json, Value};

pub const SCHEMA: &str = "dutchman.repro-bundle/1";
pub const BUDGET: Duration = Duration::from_secs(10);

struct Source(repro::Bundle);
impl BundleSource for Source {
    fn parts(&self) -> Result<Vec<BundlePart>, String> {
        Ok(vec![BundlePart {
            role: "CADIR fragment with numeric geometry and reproduction contract".into(),
            bytes: serde_json::to_vec(&self.0)
                .map_err(|_| "The bundle could not be serialized.")?,
        }])
    }
    fn manifest(&self) -> Value {
        serde_json::to_value(&self.0.manifest).unwrap_or(json!([]))
    }
}

/// Saves only a bundle that reproduces its original signature. No network work.
pub fn save(
    document: &repro::Document,
    folder: &Path,
    format: &str,
    context: serde_json::Map<String, Value>,
    budget: Duration,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, String> {
    if folder.join("prepared.json").exists() {
        return Ok(Prepared::load(folder)?.preview());
    }
    let options = repro::CutOptions {
        context,
        ..Default::default()
    };
    let Some(bundle) = repro::bundle(document, &options, budget, cancel)? else {
        return Ok(no_bundle());
    };
    let signature = bundle.signature.hash.clone();
    let mut random = [0u8; 8];
    getrandom::fill(&mut random).map_err(|_| "A report id could not be created.")?;
    let id: String = random.iter().map(|b| format!("{b:02x}")).collect();
    let source = Source(bundle);
    let prepared = Prepared::new(&id, format, &signature, Some(&source), folder)?;
    report_send::private_write(
        &folder.join("manifest.json"),
        &serde_json::to_vec_pretty(&prepared.manifest)
            .map_err(|_| "The manifest could not be saved.")?,
    )?;
    Ok(prepared.preview())
}

pub(super) fn no_bundle() -> Value {
    json!({"schema": SCHEMA, "ok": true, "bundle_available": false,
        "summary": "No proven bundle is available. A cut can exceed the current fragment limits. Decode refusals have no bundle yet. Choose an anonymous summary or the private-send card.",
        "private_send": crate::json::DISCORD_URL})
}

#[derive(Default)]
struct Capture(Mutex<Vec<repro::Document>>);
impl MemberObserver for Capture {
    fn member_event(&self, _: &str, _: MemberEvent, _: u64, _: Option<&'static str>) {}
    fn model(&self, model: &Model) {
        // Check in the observer, before a refused model is discarded. Retain one failure.
        if let Ok(mut docs) = self.0.lock() {
            if docs.is_empty() && repro::check(model).is_err() {
                docs.push(model.ir.clone());
            }
        }
    }
}

/// Read through the existing bounded decoder and retain only a real check failure.
/// The cancel hook is checked between shrink candidates; it never publishes partial cuts.
pub fn extract(
    input: &Path,
    out: &Path,
    settings: &super::Settings,
    feature: Option<usize>,
    operation: Option<&str>,
    cancel: &dyn Fn() -> bool,
) -> Result<Value, String> {
    let folder = out.join(super::FOLDER).join("repro");
    let metadata = std::fs::metadata(input).map_err(|_| "The source file could not be opened.")?;
    if !metadata.is_file() || metadata.len() > dutchman_core::MAX_INPUT_BYTES {
        return Err("Choose a regular design file within the local input size limit.".into());
    }
    let key = serde_json::to_vec(&super::narrow::Fingerprint::of(input)?)
        .map_err(|_| "The local source key could not be made.")?;
    let key_file = folder.join("source-key.json");
    if folder.join("prepared.json").exists() {
        if report_send::bounded_read(&key_file, 1024)? != key {
            return Err("This bundle is about another file. Choose a fresh output folder.".into());
        }
        return Ok(Prepared::load(&folder)?.preview());
    }
    let capture = Arc::new(Capture::default());
    let progress = dutchman_core::Progress::new(&|_| {}).with_members(Some(capture.clone()));
    let format =
        dutchman_core::Format::from_path(input).ok_or("Choose a supported native design file.")?;
    let model = dutchman_core::read_with_progress(
        format,
        input,
        &settings.options.part_search(),
        &mut dutchman_core::Report::default(),
        Some(&progress),
    );
    let doc = capture
        .0
        .lock()
        .map_err(|_| "The local model could not be retained.")?
        .pop();
    let standalone = !format.is_assembly() && format != dutchman_core::Format::FusionMultiDocument;
    let (doc, part) = match model {
        Ok(model) => {
            let number = settings.target_part.unwrap_or(1);
            let selected = dutchman_core::parts::select(&model, number, false)
                .map_err(|_| "Choose a part listed in the diagnose report.")?;
            (selected.ir, Some(number))
        }
        Err(_) => {
            let Some(doc) = doc else {
                return Ok(no_bundle());
            };
            // A refused assembly member has no proven Dutchman part ordinal yet.
            (doc, standalone.then_some(1))
        }
    };
    let context = json!({"dutchman_part": part,
        "feature_index": feature, "failed_operation_kind": operation});
    let preview = save(
        &doc,
        &folder,
        format.id(),
        context.as_object().cloned().unwrap_or_default(),
        settings.time_limit.min(BUDGET),
        &|| cancel() || out.join(super::FOLDER).join("cancel-bundle").exists(),
    )?;
    if preview.get("bundle_available").and_then(Value::as_bool) == Some(true) {
        report_send::private_write(&key_file, &key)?;
    }
    Ok(preview)
}

/// Recheck the saved fragment, bounded both before and after gzip decompression.
pub fn verify(folder: &Path) -> Result<Value, String> {
    let prepared = Prepared::load(folder)?;
    let compressed = report_send::bounded_read(&folder.join("bundle.json.gz"), 128 * 1024 * 1024)?;
    if prepared.bundle_sha256.as_deref() != Some(report_send::hash(&compressed).as_str()) {
        return Err("The bundle changed. Extract and review it again.".into());
    }
    let mut bytes = Vec::new();
    flate2::read::GzDecoder::new(compressed.as_slice())
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The saved gzip bundle could not be read.")?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("The bundle exceeds the replay size limit.".into());
    }
    let bundle: repro::Bundle =
        serde_json::from_slice(&bytes).map_err(|_| "The saved fragment could not be read.")?;
    Ok(
        json!({"schema": SCHEMA, "ok": true, "reproduces": repro::verifies(&bundle),
        "signature": bundle.signature.hash, "manifest": bundle.manifest,
        "compressed_bytes": compressed.len(), "uncompressed_bytes": bytes.len()}),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing)]
    use super::*;

    fn synthetic_failure() -> repro::Document {
        let model = dutchman_core::read(
            dutchman_core::Format::FusionArchive,
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../dutchman-core/tests/fixtures/generated_unit_cube.f3d"),
            &mut dutchman_core::Report::default(),
        )
        .unwrap();
        let mut doc = model.ir;
        doc.model.points[0]
            .set_position(serde_json::from_value(json!({"x": 50.0, "y": 0.0, "z": 0.0})).unwrap());
        doc
    }

    #[test]
    fn synthetic_cut_gzip_manifest_replay_and_cancel() {
        let doc = synthetic_failure();
        let folder = tempfile::tempdir().unwrap();
        let context =
            json!({"dutchman_part": 1, "feature_index": 3, "failed_operation_kind": "extrude"});
        let preview = save(
            &doc,
            folder.path(),
            "f3d",
            context.as_object().unwrap().clone(),
            Duration::from_secs(1),
            &|| false,
        )
        .unwrap();
        assert_eq!(preview["bundle_available"], true);
        assert_eq!(preview["default_choice"], "bundle");
        assert!(preview["manifest"]
            .as_array()
            .unwrap()
            .iter()
            .all(
                |m| m["serialized_bytes"].as_u64().is_some() && m["description"].as_str().is_some()
            ));
        let verified = verify(folder.path()).unwrap();
        assert_eq!(verified["reproduces"], true);
        assert!(!verified.to_string().contains("claim_secret"));
        let mut raw = Vec::new();
        flate2::read::GzDecoder::new(
            std::fs::File::open(folder.path().join("bundle.json.gz")).unwrap(),
        )
        .read_to_end(&mut raw)
        .unwrap();
        let bundle: repro::Bundle = serde_json::from_slice(&raw).unwrap();
        assert_eq!(bundle.context, context.as_object().unwrap().clone());
        let cancelled = repro::bundle(
            &doc,
            &repro::CutOptions::default(),
            Duration::from_secs(1),
            &|| true,
        )
        .unwrap()
        .unwrap();
        assert!(repro::verifies(&cancelled));
        eprintln!(
            "Synthetic bundle: {} gzip bytes, {} JSON bytes",
            preview["bundle_size"],
            raw.len()
        );
        // Optional local artifact for end-to-end review, containing only synthetic geometry.
        if let Ok(out) = std::env::var("DUTCHMAN_SYNTHETIC_BUNDLE_OUT") {
            std::fs::create_dir_all(&out).unwrap();
            for file in ["bundle.json.gz", "manifest.json", "prepared.json"] {
                std::fs::copy(folder.path().join(file), Path::new(&out).join(file)).unwrap();
            }
        }
    }

    #[test]
    fn decode_refusal_never_fabricates_bundle() {
        let out = tempfile::tempdir().unwrap();
        let input = out.path().join("synthetic.f3d");
        std::fs::write(&input, b"no readable design").unwrap();
        let result = extract(
            &input,
            out.path(),
            &super::super::Settings::default(),
            None,
            None,
            &|| false,
        )
        .unwrap();
        assert_eq!(result["bundle_available"], false);
        assert!(result["summary"]
            .as_str()
            .unwrap()
            .contains("Decode refusals"));
        assert!(!out.path().join("diagnose/repro/bundle.json.gz").exists());
    }
}
