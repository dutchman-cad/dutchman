//! Offline REST-response bundles → CADIR. This is an explicit bundle-reader exception
//! to the native-codec rule: STEP decoding remains in cadmpeg, all writer policy is shared.

mod archive;
mod features;
mod sketch;
mod topology;

use serde_json::{json, Value};

use crate::{Error, Format, Model, Progress, Report, Severity};
use archive::{Bundle, SCHEMA};

type Mapping<T> = Result<T, String>;
const ROWS: usize = 10_000;

fn text<'a>(row: &'a Value, key: &str) -> Mapping<&'a str> {
    row.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| format!("{key} is missing"))
}

fn rows<'a>(row: &'a Value, key: &str) -> Mapping<&'a [Value]> {
    row.get(key)
        .and_then(Value::as_array)
        .filter(|a| a.len() <= ROWS)
        .map(Vec::as_slice)
        .ok_or_else(|| format!("{key} is missing or exceeds the row limit"))
}

fn number(row: &Value, key: &str) -> Mapping<f64> {
    row.get(key)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or_else(|| format!("{key} has no finite value"))
}

// REST omits numeric zero and empty-list defaults. A present value must still
// have the declared type; null and strings are not geometric zeroes.
fn coordinate(row: &Value, key: &str) -> Mapping<f64> {
    if row.get(key).is_none() {
        Ok(0.0)
    } else {
        number(row, key)
    }
}

fn optional_rows<'a>(row: &'a Value, key: &str) -> Mapping<&'a [Value]> {
    if row.get(key).is_none() {
        Ok(&[])
    } else {
        rows(row, key)
    }
}

fn microversion(row: &Value, key: &str, expected: &str) -> Mapping<()> {
    if let Some(value) = row.get(key) {
        let actual = value
            .as_str()
            .or_else(|| value.get("theId").and_then(Value::as_str));
        if actual != Some(expected) {
            return Err(format!(
                "{key} does not match the pinned microversion; fetch a new bundle"
            ));
        }
    }
    Ok(())
}

fn id(kind: &str, native: &str) -> String {
    format!(
        "onshape:model:{kind}#{}",
        cadmpeg_ir::ids::IdentityKey::encode_key_text(native)
    )
}

fn push<T: serde::de::DeserializeOwned>(arena: &mut Vec<T>, row: Value) -> Mapping<()> {
    arena.push(
        serde_json::from_value(row)
            .map_err(|e| format!("the resolved construction is not supported yet ({e})"))?,
    );
    Ok(())
}

fn loss(report: &mut Report, subject: &str, reason: &str) {
    report.push_coded(
        Severity::Degraded,
        subject,
        "onshape/history.not-supported",
        format!("{reason}; kept the final STEP geometry"),
    );
}

pub(super) fn read(
    path: &std::path::Path,
    bytes: &[u8],
    report: &mut Report,
    progress: Option<&Progress<'_>>,
) -> Result<Model, Error> {
    let bundle = Bundle::open(bytes)?;
    let manifest = bundle.json("manifest.json")?;
    if manifest.get("schema").and_then(Value::as_str) != Some(SCHEMA) {
        return Err(Error::Decode(
            "Onshape bundle schema is not supported yet; fetch a format 1 bundle".into(),
        ));
    }
    match manifest.get("consistent") {
        Some(Value::Bool(false)) => return Err(Error::Decode(
            "Onshape bundle is marked inconsistent: its files do not reflect one microversion; fetch it again, or use a version link".into(),
        )),
        Some(Value::Bool(true)) | None => {},
        Some(_) => return Err(Error::Decode(
            "Onshape bundle consistency flag must be true or false; fetch a new bundle".into(),
        )),
    }
    let document = manifest.get("document").unwrap_or(&Value::Null);
    let mid = text(document, "microversion").map_err(Error::Decode)?;
    let elements = rows(&manifest, "elements").map_err(Error::Decode)?;
    if elements.len() != 1
        || elements
            .first()
            .and_then(|e| e.get("type"))
            .and_then(Value::as_str)
            != Some("PARTSTUDIO")
    {
        return Err(Error::Decode("Onshape bundles with assemblies or multiple elements are not supported yet; fetch one Part Studio".into()));
    }
    let element = elements
        .first()
        .ok_or_else(|| Error::Decode("Onshape bundle has no Part Studio".into()))?;
    let eid = text(element, "id").map_err(Error::Decode)?;
    if !eid
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Error::Decode(
            "Onshape element id cannot name a bundle folder; fetch a new bundle".into(),
        ));
    }
    let prefix = format!("partstudios/{eid}");
    let inputs = topology::Inputs::new(&bundle, &prefix, mid).map_err(Error::Decode)?;
    let source = bundle.bytes(&format!("{prefix}/export.step"))?;
    // This supplies exact final bodies and the comparison reference, never historical picks.
    let mut model =
        crate::decode::decode_with_progress(Format::Step, path, source, report, progress)?;
    model.source_step = Some(String::from_utf8(source.to_vec()).map_err(|_| {
        Error::Decode("Onshape STEP export is not UTF-8; fetch a new bundle".into())
    })?);
    let response = bundle.json(&format!("{prefix}/features.json"))?;
    microversion(&response, "sourceMicroversion", mid).map_err(Error::Decode)?;
    let features = rows(&response, "features").map_err(Error::Decode)?;
    // One body creator followed by modifying operations has an unambiguous final
    // body identity. This binds writer body names, including through-all cuts.
    // Several creators cannot be paired with final STEP bodies by list order.
    let creators = features
        .iter()
        .filter(|feature| {
            matches!(
                feature.get("featureType").and_then(Value::as_str),
                Some("extrude" | "revolve")
            ) && features::value(feature, "operationType") == Some("NEW")
                && feature.get("suppressed").and_then(Value::as_bool) != Some(true)
                && feature.get("parentSuppressed").and_then(Value::as_bool) != Some(true)
        })
        .count();
    let single_body = (creators == 1 && model.ir.model.bodies.len() == 1)
        .then(|| model.ir.model.bodies.first().map(|body| body.id.clone()))
        .flatten();
    let mut seen = std::collections::BTreeSet::new();
    let mut previous = None;
    for (ordinal, feature) in features.iter().enumerate() {
        let native = text(feature, "featureId").map_err(Error::Decode)?;
        if !seen.insert(native) {
            return Err(Error::Decode(
                "Onshape feature ids repeat; fetch a new bundle".into(),
            ));
        }
        let feature_id = id("feature", native);
        let name = feature
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(native);
        let suppressed = feature
            .get("suppressed")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            || feature
                .get("parentSuppressed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
        // Roll back appended rows so a refused sketch cannot leave dangling references.
        let sizes = (
            model.ir.model.sketches.len(),
            model.ir.model.sketch_entities.len(),
            model.ir.model.sketch_constraints.len(),
            model.ir.model.parameters.len(),
            model.ir.model.feature_input_topologies.len(),
        );
        let mapped = if suppressed {
            Ok(json!({"definition": "native", "kind": "Suppressed Onshape feature"}))
        } else {
            features::map(feature, ordinal, &inputs, &mut model.ir.model, report).and_then(
                |definition| {
                    // Refuse a feature locally instead of letting one inadmissible scalar
                    // discard the entire history during the neutral-model check.
                    serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(
                        definition.clone(),
                    )
                    .map_err(|e| format!("the resolved construction is not supported yet ({e})"))?;
                    Ok(definition)
                },
            )
        };
        let definition = match mapped {
            Ok(definition) => definition,
            Err(reason) => {
                model.ir.model.sketches.truncate(sizes.0);
                model.ir.model.sketch_entities.truncate(sizes.1);
                model.ir.model.sketch_constraints.truncate(sizes.2);
                model.ir.model.parameters.truncate(sizes.3);
                model.ir.model.feature_input_topologies.truncate(sizes.4);
                if !suppressed {
                    loss(report, name, &reason);
                }
                json!({"definition": "native", "kind": format!("Onshape {}: {reason}", feature.get("featureType").and_then(Value::as_str).unwrap_or("feature"))})
            }
        };
        let dependencies: Vec<_> = previous.iter().cloned().collect();
        let outputs: Vec<_> = single_body
            .iter()
            .filter(|_| {
                definition.get("op").and_then(Value::as_str) == Some("new_body")
                    && matches!(
                        definition.get("definition").and_then(Value::as_str),
                        Some("extrude" | "revolve")
                    )
            })
            .cloned()
            .collect();
        push(&mut model.ir.model.features, json!({"id": feature_id, "ordinal": ordinal, "name": name, "suppressed": suppressed, "dependencies": dependencies, "outputs": outputs, "definition": definition})).map_err(Error::Decode)?;
        if !suppressed {
            previous = Some(feature_id);
        }
    }
    model.ir.finalize();
    Ok(model)
}
