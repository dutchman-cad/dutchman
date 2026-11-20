//! Refuse to publish a design with a known failed timeline feature.
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read, Write};

use dutchman_core::{Check, Error, FeatureStatus, Mode, Report, Severity, Stage};
use serde_json::Value;

use crate::{document, SolidCraftWriter};

impl SolidCraftWriter {
    pub(crate) fn check_opening(
        &self,
        mut bytes: Vec<u8>,
        report: &mut Report,
    ) -> Result<Vec<u8>, Error> {
        if self.memory_refusal().is_some() {
            return Ok(bytes);
        }
        let Ok(cli) = self.selected_evaluator(&self.verification) else {
            return Ok(bytes);
        };
        let started = dutchman_core::time::Instant::now();
        let budget = self.opening_budget();
        let unfinished = || {
            let minutes = budget.as_secs().div_ceil(60);
            format!("SolidCraft didn't finish checking the written design in {minutes} min")
        };
        for _ in 0..64 {
            let remaining = budget.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                unavailable(report, &unfinished());
                return Ok(bytes);
            }
            if self.memory_refusal().is_some() {
                unavailable(report, "the final opening check stopped for memory");
                return Ok(bytes);
            }
            let result = match cli.evaluate(&bytes, remaining, "opening") {
                Ok(result) => result,
                Err(why) if why.contains("timed out") => {
                    unavailable(report, &unfinished());
                    return Ok(bytes);
                }
                Err(why) => {
                    unavailable(report, &why);
                    return Ok(bytes);
                }
            };
            if let Some(why) = result.get("evaluation_failed").and_then(Value::as_str) {
                unavailable(report, why);
                return Ok(bytes);
            }
            let mut unnamed = None;
            let failures: Vec<_> = result
                .get("timeline")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|row| {
                    let why = row.get("error")?.as_str().filter(|why| !why.is_empty())?;
                    if row
                        .get("name")
                        .and_then(Value::as_str)
                        .is_none_or(|name| name.trim().is_empty())
                    {
                        unnamed = Some(why.to_owned());
                        return None;
                    }
                    Some((row.clone(), why.to_owned()))
                })
                .collect();
            if failures.is_empty() {
                if let Some(why) = unnamed {
                    unavailable(
                        report,
                        &format!("SolidCraft did not name a failed feature: {why}"),
                    );
                }
                return Ok(bytes);
            }
            let mut archive =
                zip::ZipArchive::new(Cursor::new(bytes.as_slice())).map_err(failure)?;
            let mut doc: Value =
                serde_json::from_reader(archive.by_name("document.json").map_err(failure)?)
                    .map_err(failure)?;
            let design_name = doc
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Design")
                .to_owned();
            let component_names: BTreeMap<_, _> = doc
                .get("components")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|component| {
                    Some((
                        component.get("id")?.as_u64()?,
                        component.get("name")?.as_str()?.to_owned(),
                    ))
                })
                .collect();
            let component_parents: BTreeMap<_, _> = doc
                .get("components")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|component| {
                    let hidden =
                        component.get("history_hidden").and_then(Value::as_bool) == Some(true);
                    if !hidden {
                        return None;
                    }
                    Some((
                        component.get("id")?.as_u64()?,
                        component.get("parent").and_then(Value::as_u64).unwrap_or(0),
                    ))
                })
                .collect();
            let features = doc
                .get_mut("features")
                .and_then(Value::as_array_mut)
                .ok_or_else(|| failure("the design has no feature list"))?;
            let blobs = repair_imports(&mut archive, features, &failures, report)?;
            if !blobs.is_empty() {
                // Later feature errors can be consequences of the failed import. Rebuild
                // the repaired source before acting on that stale list of errors.
                bytes = replace_document(archive, &doc, &blobs)?;
                continue;
            }
            let mut removed = BTreeSet::new();
            let mut unlocated = Vec::new();
            for (row, why) in failures {
                let name = row.get("name").and_then(Value::as_str).unwrap_or("?");
                let matches = |feature: &Value| {
                    if let Some(id) = row.get("id").and_then(Value::as_u64) {
                        feature.get("id").and_then(Value::as_u64) == Some(id)
                    } else {
                        feature.get("name").and_then(Value::as_str) == Some(name)
                    }
                };
                // A failure that names no feature in the written design can't be repaired
                // here; it is reported, and the design keeps its existing check result.
                let Some(at) = features.iter().position(matches) else {
                    unlocated.push(format!("\"{name}\": {why}"));
                    continue;
                };
                let failed = features.get(at).ok_or_else(|| failure("missing feature"))?;
                let component = failed.get("component").cloned();
                let part_name = component
                    .as_ref()
                    .and_then(Value::as_u64)
                    .and_then(|id| component_names.get(&id))
                    .unwrap_or(&design_name);
                let parent = component
                    .as_ref()
                    .and_then(Value::as_u64)
                    .and_then(|id| component_parents.get(&id))
                    .copied();
                let is_import = failed.get("type").and_then(Value::as_str) == Some("import");
                // The normal replay repair already keeps a source prefix. This last check
                // also covers hidden prefixes, geometry-only designs and assembly imports.
                let original = features
                    .iter()
                    .skip(at + 1)
                    .find(|feature| {
                        let same_component = feature.get("component") == component.as_ref();
                        let visible_parent = parent.is_some_and(|parent| {
                            let in_parent = feature
                                .get("component")
                                .and_then(Value::as_u64)
                                .unwrap_or(0)
                                == parent;
                            let standalone_original = parent == 0
                                && feature.get("name").and_then(Value::as_str)
                                    == Some(document::ORIGINAL);
                            (in_parent || standalone_original)
                                && feature.get("suppressed").and_then(Value::as_bool) != Some(true)
                        });
                        feature.get("type").and_then(Value::as_str) == Some("import")
                            && ((same_component
                                && feature.get("name").and_then(Value::as_str)
                                    == Some(document::ORIGINAL))
                                || (!same_component && visible_parent))
                    })
                    .and_then(|feature| feature.get("id"))
                    .and_then(Value::as_u64);
                if original.is_some() {
                    // Retain the prefix for editing without displaying its unfinished
                    // bodies on top of the final source shape in this component.
                    for feature in features.iter_mut().take(at) {
                        if feature.get("component") == component.as_ref() {
                            feature
                                .as_object_mut()
                                .ok_or_else(|| failure("a feature is not an object"))?
                                .insert("suppressed".to_owned(), Value::Bool(true));
                        }
                    }
                }
                for feature in features.iter_mut().skip(at) {
                    if feature.get("component") != component.as_ref() {
                        continue;
                    }
                    let id = feature
                        .get("id")
                        .and_then(Value::as_u64)
                        .ok_or_else(|| failure("a failed feature has no id"))?;
                    if Some(id) == original {
                        feature
                            .as_object_mut()
                            .ok_or_else(|| failure("a feature is not an object"))?
                            .insert("suppressed".to_owned(), Value::Bool(false));
                    } else {
                        removed.insert(id);
                    }
                }
                if let Some(original) = original {
                    if let Some(source) = features
                        .iter_mut()
                        .find(|feature| feature.get("id").and_then(Value::as_u64) == Some(original))
                    {
                        source
                            .as_object_mut()
                            .ok_or_else(|| failure("a feature is not an object"))?
                            .insert("suppressed".to_owned(), Value::Bool(false));
                    }
                }
                let omitted = features
                    .iter()
                    .filter(|feature| {
                        feature
                            .get("id")
                            .and_then(Value::as_u64)
                            .is_some_and(|id| removed.contains(&id))
                    })
                    .filter_map(|feature| feature.get("name").and_then(Value::as_str))
                    .map(|name| format!("\"{name}\""))
                    .collect::<Vec<_>>()
                    .join(", ");
                if !is_import
                    && original.is_none()
                    && features.iter().all(|feature| {
                        feature
                            .get("id")
                            .and_then(Value::as_u64)
                            .is_none_or(|id| removed.contains(&id))
                    })
                {
                    return Err(failure(format!("feature \"{name}\" failed: {why}; no editable history remains that can open")));
                }
                let reason = if original.is_some() {
                    format!("Feature \"{name}\" in \"{part_name}\" failed to rebuild: {why}; the final source shape is retained")
                } else if !is_import {
                    format!("Feature \"{name}\" in \"{part_name}\" failed to rebuild: {why}; left out {omitted} and retained the earlier editable history")
                } else {
                    format!("Body geometry is MISSING from \"{part_name}\": SolidCraft could not rebuild feature \"{name}\": {why}; the refused source shape was left out as a last resort")
                };
                report.push_coded(
                    if original.is_some() || !is_import {
                        Severity::Degraded
                    } else {
                        Severity::Dropped
                    },
                    name,
                    if original.is_some() || !is_import {
                        "history/rebuild-failed"
                    } else {
                        "geometry/import-failed"
                    },
                    &reason,
                );
                if let Some(outcome) = report.outcome_mut() {
                    for feature in &mut outcome.features {
                        if feature.output.ids.iter().any(|id| removed.contains(id)) {
                            feature.status = FeatureStatus::Blocked;
                            feature.reason = Some(reason.clone());
                            feature.output.ids.clear();
                            feature.output.method = dutchman_core::OutputMatch::NoOutput;
                        }
                    }
                    if outcome.mode == Mode::Import && original.is_none() {
                        outcome.check = Check::Flattened(reason);
                    } else {
                        outcome.mode = Mode::PartialHistory;
                        outcome.check = Check::Partial(reason);
                    }
                }
            }
            if removed.is_empty() {
                not_located(report, &unlocated);
                return Ok(bytes);
            }
            features.retain(|feature| {
                !feature
                    .get("id")
                    .and_then(Value::as_u64)
                    .is_some_and(|id| removed.contains(&id))
            });
            bytes = replace_document(archive, &doc, &BTreeMap::new())?;
        }
        Err(failure("the design exceeded the rebuild repair limit"))
    }
}

/// Try the lossless topology repair before dropping any named failed import.
fn repair_imports(
    archive: &mut zip::ZipArchive<Cursor<&[u8]>>,
    features: &mut [Value],
    failures: &[(Value, String)],
    report: &mut Report,
) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let mut blobs = BTreeMap::new();
    for (row, _) in failures {
        let feature = features.iter_mut().find(|feature| {
            if let Some(id) = row.get("id").and_then(Value::as_u64) {
                feature.get("id").and_then(Value::as_u64) == Some(id)
            } else {
                feature.get("name") == row.get("name")
            }
        });
        let Some(feature) =
            feature.filter(|feature| feature.get("type").and_then(Value::as_str) == Some("import"))
        else {
            continue;
        };
        let Some(path) = feature.get("step_blob").and_then(Value::as_str) else {
            continue;
        };
        let mut step = String::new();
        let mut entry = archive.by_name(path).map_err(failure)?;
        if entry.size() > crate::step::MAX_STEP_BYTES as u64 {
            continue;
        }
        entry
            .by_ref()
            .take(crate::step::MAX_STEP_BYTES as u64 + 1)
            .read_to_string(&mut step)
            .map_err(failure)?;
        let Some((repaired, count)) = crate::step::split_sheet_edges(&step) else {
            continue;
        };
        let digest: String = Sha256::digest(repaired.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        let path = format!("blobs/{digest}.step");
        let name = feature
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Import")
            .to_owned();
        feature
            .as_object_mut()
            .ok_or_else(|| failure("an import is not an object"))?
            .insert("step_blob".to_owned(), Value::String(path.clone()));
        blobs.insert(path, repaired.into_bytes());
        report.push_coded(Severity::Info, &name, "geometry/sheet-edges-split",
            format!("Feature \"{name}\": split {count} excess shared edge uses without changing the source faces, curves, or vertices; checking the repaired source shape"));
    }
    Ok(blobs)
}

/// Copy our own generated payloads byte for byte, replacing the document and repaired blobs.
fn replace_document(
    mut archive: zip::ZipArchive<Cursor<&[u8]>>,
    doc: &Value,
    blobs: &BTreeMap<String, Vec<u8>>,
) -> Result<Vec<u8>, Error> {
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..archive.len() {
        let entry = archive.by_index(index).map_err(failure)?;
        let name = entry.name().map_err(failure)?;
        if name == "document.json"
            || blobs.contains_key(name.as_ref())
            || (!blobs.is_empty() && name.starts_with("cache/"))
        {
            continue;
        }
        output.raw_copy_file(entry).map_err(failure)?;
    }
    for (name, data) in blobs {
        output
            .start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated),
            )
            .map_err(failure)?;
        output.write_all(data).map_err(failure)?;
    }
    output
        .start_file(
            "document.json",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .map_err(failure)?;
    output
        .write_all(&serde_json::to_vec(doc).map_err(failure)?)
        .map_err(failure)?;
    Ok(output.finish().map_err(failure)?.into_inner())
}

/// SolidCraft named failed features that the written design doesn't contain under that name.
fn not_located(report: &mut Report, failures: &[String]) {
    let why = format!(
        "SolidCraft reported failed features that could not be matched to the written design: {}",
        failures.join("; ")
    );
    report.push_at(Stage::SolidCraft, Severity::Degraded, crate::HISTORY, &why);
    if let Some(outcome) = report.outcome_mut() {
        if matches!(outcome.check, Check::Verified) {
            outcome.check = Check::Differs(why);
        }
    }
}

fn unavailable(report: &mut Report, why: &str) {
    let why = format!("Not verified: {why}");
    report.push_at(Stage::SolidCraft, Severity::Info, crate::HISTORY, &why);
    if let Some(outcome) = report.outcome_mut() {
        let why = match &outcome.check {
            Check::Partial(previous) | Check::Flattened(previous) => format!("{previous}; {why}"),
            _ => why,
        };
        outcome.check = Check::Unverified(why);
    }
}

fn failure(why: impl std::fmt::Display) -> Error {
    Error::Encode(format!(
        "cannot write a design that fails to open in SolidCraft: {why}"
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    #[derive(Debug)]
    struct RefusedImport;

    impl crate::SolidCraftEvaluator for RefusedImport {
        fn evaluate(&self, design: &[u8], _: Duration, _: &str) -> Result<Value, String> {
            let mut archive = zip::ZipArchive::new(Cursor::new(design)).unwrap();
            let doc: Value =
                serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
            let errors: Vec<_> = doc["features"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|feature| feature["id"] == 1)
                .map(|feature| {
                    json!({"id": feature["id"], "name": feature["name"],
                    "error": "Synthetic STEP refusal"})
                })
                .collect();
            Ok(json!({"bodies": [], "timeline": errors}))
        }

        fn run(&self, _: &[u8], _: &[Value], _: Duration, _: &str) -> Result<Vec<Value>, String> {
            Err("No commands expected".to_owned())
        }
    }

    #[derive(Debug)]
    struct RepairableSheet;
    impl crate::SolidCraftEvaluator for RepairableSheet {
        fn evaluate(&self, design: &[u8], _: Duration, _: &str) -> Result<Value, String> {
            let mut archive = zip::ZipArchive::new(Cursor::new(design)).unwrap();
            let doc: Value =
                serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
            let mut step = String::new();
            archive
                .by_name(doc["features"][0]["step_blob"].as_str().unwrap())
                .unwrap()
                .read_to_string(&mut step)
                .unwrap();
            let error = step.contains("#4=ORIENTED_EDGE('',*,*,#1,.T.)");
            Ok(
                json!({"bodies": [], "timeline": if error {json!([{"id":1,"name":"Sheet","error":"a sheet touching itself along an edge"}])} else {json!([])}}),
            )
        }
        fn run(&self, _: &[u8], _: &[Value], _: Duration, _: &str) -> Result<Vec<Value>, String> {
            Err("No commands expected".to_owned())
        }
    }

    #[test]
    fn a_refused_sheet_is_repaired_and_rechecked_before_omitting_any_feature() {
        let source = "ISO-10303-21;DATA;#1=EDGE_CURVE('',#10,#11,#12,.T.);#2=ORIENTED_EDGE('',*,*,#1,.T.);#3=ORIENTED_EDGE('',*,*,#1,.F.);#4=ORIENTED_EDGE('',*,*,#1,.T.);ENDSEC;END-ISO-10303-21;";
        let container = crate::container::Container::new(source.as_bytes()).unwrap();
        let blob = container.step_blob().to_owned();
        let doc = json!({"features":[{"id":1,"type":"import","name":"Sheet","component":10,"body_names":["Sheet body"],"step_blob":blob}],"occurrences":[{"id":11,"component":10,"parent":0}]});
        let bytes = container.finish(&doc).unwrap();
        let writer = SolidCraftWriter::new().evaluator(std::sync::Arc::new(RepairableSheet));
        let mut report = Report::default();
        let bytes = writer.check_opening(bytes, &mut report).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let repaired: Value =
            serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
        assert_eq!(repaired["features"].as_array().unwrap().len(), 1);
        assert_eq!(repaired["features"][0]["id"], 1);
        assert_eq!(repaired["features"][0]["component"], 10);
        assert_eq!(
            repaired["features"][0]["body_names"],
            doc["features"][0]["body_names"]
        );
        assert_eq!(repaired["occurrences"], doc["occurrences"]);
        assert_ne!(
            repaired["features"][0]["step_blob"],
            doc["features"][0]["step_blob"]
        );
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.code.as_deref() == Some("geometry/sheet-edges-split")));
        assert!(!report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("MISSING")));
    }

    #[test]
    fn a_late_rebuild_failure_retains_the_prefix_and_activates_the_final_source() {
        for source_component in [10, 20, 30] {
            let bytes = crate::container::Container::empty().finish(&json!({"features": [
            {"id": 0, "name": "Stored base", "type": "import", "component": 10, "suppressed": false},
            {"id": 1, "name": "Edit", "type": "offset_faces", "component": 10, "suppressed": false},
            {"id": 2, "name": document::ORIGINAL, "type": "import", "component": source_component, "suppressed": source_component == 10}
        ], "components": [{"id": 10, "parent": if source_component == 30 { 0 } else { 20 }, "name": "Editable history (partial)", "history_hidden": true}]})).unwrap();
            let writer = SolidCraftWriter::new().evaluator(std::sync::Arc::new(RefusedImport));
            let mut report = Report::default();
            let bytes = writer.check_opening(bytes, &mut report).unwrap();
            let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
            let doc: Value =
                serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
            assert_eq!(doc["features"].as_array().unwrap().len(), 2);
            assert_eq!(doc["features"][0]["name"], "Stored base");
            assert_eq!(doc["features"][0]["suppressed"], true);
            assert_eq!(doc["features"][1]["name"], document::ORIGINAL);
            assert_eq!(doc["features"][1]["suppressed"], false);
            assert!(report.losses().iter().any(|loss| loss.code.as_deref()
                == Some("history/rebuild-failed")
                && loss.detail.contains("Edit")
                && loss.detail.contains("Synthetic STEP refusal")));
        }
    }

    #[test]
    fn a_failed_edit_without_source_keeps_and_rechecks_the_earlier_history() {
        let bytes = crate::container::Container::empty()
            .finish(&json!({"features": [
                {"id":0,"name":"Base sketch","type":"sketch","component":10},
                {"id":1,"name":"Hole","type":"hole","component":10},
                {"id":2,"name":"Dependent fillet","type":"fillet","component":10}
            ]}))
            .unwrap();
        let writer = SolidCraftWriter::new().evaluator(std::sync::Arc::new(RefusedImport));
        let mut report = Report::default();
        let bytes = writer.check_opening(bytes, &mut report).unwrap();
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let doc: Value =
            serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
        assert_eq!(doc["features"].as_array().unwrap().len(), 1);
        assert_eq!(doc["features"][0]["name"], "Base sketch");
        assert_ne!(doc["features"][0]["suppressed"], true);
        assert!(report.losses().iter().any(|loss| loss.code.as_deref()
            == Some("history/rebuild-failed")
            && loss.detail.contains("Hole")
            && loss.detail.contains("Dependent fillet")
            && loss.detail.contains("Synthetic STEP refusal")
            && loss.detail.contains("earlier editable history")));
    }

    #[test]
    fn a_failed_edit_with_no_usable_prefix_is_refused() {
        let bytes = crate::container::Container::empty()
            .finish(&json!({"features": [
                {"id":1,"name":"Hole","type":"hole","component":10}
            ]}))
            .unwrap();
        let writer = SolidCraftWriter::new().evaluator(std::sync::Arc::new(RefusedImport));
        let error = writer
            .check_opening(bytes, &mut Report::default())
            .unwrap_err();
        assert!(error.to_string().contains("no editable history remains"));
    }

    #[test]
    fn a_failed_import_removes_only_its_component_suffix_and_retains_other_parts() {
        let mut container = crate::container::Container::new(b"synthetic STEP payload").unwrap();
        let blob = container.step_blob().to_owned();
        container
            .add_companion("synthetic.sc", b"synthetic companion")
            .unwrap();
        let bytes = container
            .finish(&json!({"features": [
                {"id": 0, "name": "Before", "type": "sketch", "component": 10},
                {"id": 1, "name": "Body", "type": "import", "component": 10, "step_blob": blob},
                {"id": 2, "name": "Later", "type": "extrude", "component": 10},
                {"id": 3, "name": "Body", "type": "import", "component": 20, "step_blob": blob}
            ]}))
            .unwrap();
        let writer = SolidCraftWriter::new().evaluator(std::sync::Arc::new(RefusedImport));
        let mut report = Report::default();
        let bytes = writer.check_opening(bytes, &mut report).unwrap();
        assert_eq!(
            writer.check_opening(bytes.clone(), &mut report).unwrap(),
            bytes
        );
        let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let doc: Value =
            serde_json::from_reader(archive.by_name("document.json").unwrap()).unwrap();
        let ids: Vec<_> = doc["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["id"].clone())
            .collect();
        assert_eq!(ids, [json!(0), json!(3)]);
        assert!(archive.by_name(&blob).is_ok());
        assert!(archive.by_name("companions/synthetic.sc").is_ok());
        assert!(report
            .losses()
            .iter()
            .any(|loss| loss.detail.contains("Body")
                && loss.detail.contains("MISSING")
                && loss.detail.contains("Synthetic STEP refusal")));
    }
}
