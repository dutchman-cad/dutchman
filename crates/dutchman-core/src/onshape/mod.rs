//! Offline REST-response bundles → CADIR. This is an explicit bundle-reader exception
//! to the native-codec rule: STEP decoding remains in cadmpeg, all writer policy is shared.

mod archive;
mod expression;
mod features;
mod query;
mod regions;
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

// CADIR v11 separates the definition wrapper from its operation payload.
fn definition(kind: &str, value: Value) -> Value {
    json!({"kind": "operation", "value": {"kind": kind, "value": value}})
}

fn loss(report: &mut Report, subject: &str, reason: &str, has_step: bool) {
    report.push_coded(
        Severity::Degraded,
        subject,
        "onshape/history.not-supported",
        format!(
            "{reason}; kept {}",
            if has_step {
                "the final STEP geometry"
            } else {
                "the supported sketch geometry"
            }
        ),
    );
}

/// A Part Studio is one design: its parts share one feature list and one frame. Its STEP
/// export still states each part as a root product of its own, which would convert as an
/// assembly whose parts hold none of that history. Parts that are each placed once, at the
/// identity, become the design's own bodies, unless anything else names those placements.
fn part_studio_bodies(model: &mut Model) {
    use cadmpeg_ir::products::{OccurrenceParent, PrototypeReference};
    let rows = &model.ir.model;
    if rows.occurrences.len() < 2 || !rows.assembly_joints.is_empty() {
        return;
    }
    let mut placed = std::collections::BTreeSet::new();
    for occurrence in &rows.occurrences {
        let PrototypeReference::Local { definition } = &occurrence.prototype else {
            return;
        };
        if !matches!(occurrence.parent, OccurrenceParent::Root {})
            || occurrence.transform != cadmpeg_ir::transform::Transform::identity()
            || occurrence.scale.iter().any(|s| s.get() != 1.0)
            || occurrence.linked_prototype.is_some()
            || occurrence.link.is_some()
            || occurrence.visible == Some(false)
            || !placed.insert(definition.as_str())
        {
            return;
        }
    }
    if rows.product_definitions.len() != placed.len()
        || rows
            .product_definitions
            .iter()
            .any(|definition| !placed.contains(definition.id.as_str()))
    {
        return;
    }
    // Each placement is named only by its own row; any other mention keeps the product
    // structure as decoded. The products themselves stay, with their names and bodies.
    let Ok(text) = serde_json::to_string(&model.ir) else {
        return;
    };
    let named = |id: &str| text.matches(&format!("\"{id}\"")).count();
    if rows
        .occurrences
        .iter()
        .any(|occurrence| named(occurrence.id.as_str()) != 1)
    {
        return;
    }
    model.ir.model.occurrences.clear();
}

/// The final STEP body each part-making feature made, by the part's name. A feature binds a
/// body only when exactly one named part appears between its rollback states and exactly
/// one STEP body, claimed by no other feature, carries that name. Anything else is left
/// unbound rather than guessed.
fn part_bodies(
    creators: &[usize],
    inputs: &topology::Inputs<'_>,
    model: &Model,
) -> std::collections::BTreeMap<usize, cadmpeg_ir::ids::BodyId> {
    let mut named: std::collections::BTreeMap<&str, Vec<&cadmpeg_ir::ids::BodyId>> =
        std::collections::BTreeMap::new();
    for body in &model.ir.geometry.bodies {
        if let Some(name) = body.name.as_deref() {
            named.entry(name).or_default().push(&body.id);
        }
    }
    let mut claims: std::collections::BTreeMap<usize, &cadmpeg_ir::ids::BodyId> =
        std::collections::BTreeMap::new();
    for &ordinal in creators {
        let (Ok(before), Ok(after)) = (
            inputs.parts(ordinal),
            inputs.parts(ordinal.saturating_add(1)),
        ) else {
            continue;
        };
        let made: Vec<&Option<String>> = after
            .iter()
            .filter(|(id, _)| !before.contains_key(*id))
            .map(|(_, name)| name)
            .collect();
        let [Some(name)] = made.as_slice() else {
            continue;
        };
        if after.values().filter(|n| n.as_ref() == Some(name)).count() != 1 {
            continue;
        }
        if let Some([body]) = named.get(name.as_str()).map(Vec::as_slice) {
            claims.insert(ordinal, body);
        }
    }
    claims
        .iter()
        .filter(|(_, body)| claims.values().filter(|b| b == body).count() == 1)
        .map(|(ordinal, body)| (*ordinal, (*body).clone()))
        .collect()
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
    let response = bundle.json(&format!("{prefix}/features.json"))?;
    microversion(&response, "sourceMicroversion", mid).map_err(Error::Decode)?;
    let features = rows(&response, "features").map_err(Error::Decode)?;
    let step_path = format!("{prefix}/export.step");
    let mut model = if bundle.contains(&step_path) {
        let source = bundle.bytes(&step_path)?;
        // Exact final bodies and comparison reference, never historical picks.
        let mut model =
            crate::decode::decode_with_progress(Format::Step, path, source, report, progress)?;
        model.source_step = Some(String::from_utf8(source.to_vec()).map_err(|_| {
            Error::Decode("Onshape STEP export is not UTF-8; fetch a new bundle".into())
        })?);
        part_studio_bodies(&mut model);
        model
    } else if !features.is_empty()
        && features.iter().all(|feature| {
            feature.get("suppressed").and_then(Value::as_bool) == Some(true)
                || feature.get("parentSuppressed").and_then(Value::as_bool) == Some(true)
                || matches!(
                    feature.get("featureType").and_then(Value::as_str),
                    Some("newSketch" | "assignVariable")
                )
        })
    {
        Model {
            ir: cadmpeg_ir::CadIr::empty(),
            fidelity: None,
            name: path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Onshape sketches")
                .to_owned(),
            source_file: path.file_name().map(|s| s.to_string_lossy().into_owned()),
            assembly: None,
            source_step: None,
        }
    } else {
        return Err(Error::Decode(
            "Onshape bundle has body-building features but no STEP export; fetch a new bundle"
                .into(),
        ));
    };
    // One body creator followed by modifying operations has an unambiguous final
    // body identity. This binds writer body names, including through-all cuts.
    // Several creators cannot be paired with final STEP bodies by list order.
    let creators: Vec<usize> = features
        .iter()
        .enumerate()
        .filter(|(_, feature)| {
            matches!(
                feature.get("featureType").and_then(Value::as_str),
                Some("extrude" | "revolve")
            ) && features::value(feature, "operationType") == Some("NEW")
                && feature.get("suppressed").and_then(Value::as_bool) != Some(true)
                && feature.get("parentSuppressed").and_then(Value::as_bool) != Some(true)
        })
        .map(|(ordinal, _)| ordinal)
        .collect();
    let single_body = (creators.len() == 1 && model.ir.geometry.bodies.len() == 1)
        .then(|| model.ir.geometry.bodies.first().map(|body| body.id.clone()))
        .flatten();
    let parts = model.ir.geometry.bodies.len();
    let part_bodies = if single_body.is_none() && parts > 1 && !creators.is_empty() {
        let bound = part_bodies(&creators, &inputs, &model);
        if bound.len() < parts {
            report.push_coded(
                Severity::Degraded,
                "part names",
                "onshape/history.part-not-matched",
                format!(
                    "{} of {} parts could not be matched by name to the feature that made \
                     them; SolidCraft names those parts, and their colours are left out",
                    parts.saturating_sub(bound.len()),
                    parts
                ),
            );
        }
        bound
    } else {
        std::collections::BTreeMap::new()
    };
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
            Ok(definition(
                "native",
                json!({"kind": "Suppressed Onshape feature", "parameters": {}}),
            ))
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
                    loss(report, name, &reason, model.source_step.is_some());
                }
                definition(
                    "native",
                    json!({"kind": format!("Onshape {}: {reason}", feature.get("featureType").and_then(Value::as_str).unwrap_or("feature")), "parameters": {}}),
                )
            }
        };
        let dependencies: Vec<_> = previous.iter().cloned().collect();
        let outputs: Vec<_> = single_body
            .iter()
            .chain(part_bodies.get(&ordinal))
            .filter(|_| {
                definition
                    .pointer("/value/value/op")
                    .and_then(Value::as_str)
                    == Some("new_body")
                    && matches!(
                        definition.pointer("/value/kind").and_then(Value::as_str),
                        Some("extrude" | "revolve")
                    )
            })
            .cloned()
            .collect();
        push(&mut model.ir.model.features, json!({"id": feature_id, "ordinal": ordinal, "name": name, "suppressed": suppressed, "dependencies": dependencies, "source_properties": {}, "source_content": [], "outputs": outputs, "definition": definition})).map_err(Error::Decode)?;
        if !suppressed {
            previous = Some(feature_id);
        }
    }
    model.ir.finalize();
    Ok(model)
}

#[cfg(test)]
mod tests;
