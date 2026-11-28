//! Offline REST-response bundles → CADIR. This is an explicit bundle-reader exception
//! to the native-codec rule: STEP decoding remains in cadmpeg, all writer policy is shared.

mod archive;
mod derived;
mod expression;
mod extent;
mod features;
mod intersection;
mod query;
mod regions;
mod sketch;
mod thin;
mod topology;

use serde_json::{json, Value};

#[cfg(test)]
use crate::Format;
use crate::{Error, Model, Progress, Report, Severity};
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

fn loss(report: &mut Report, subject: &str, reason: &str, has_step: bool, code: &str) {
    report.push_coded(
        Severity::Degraded,
        subject,
        code,
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
    use cadmpeg_ir::products::Storage;
    let rows = &model.ir.model;
    if rows.occurrences.len() < 2 || !rows.assembly_joints.is_empty() {
        return;
    }
    let mut roots = rows.product_definitions.iter().filter(|d| d.is_root());
    let Some(root) = roots.next() else {
        return;
    };
    if roots.next().is_some()
        || !root.bodies.is_empty()
        || !root.sketches.is_empty()
        || !root.tessellations.is_empty()
        || !root.instance_bodies.is_empty()
        || !root.representations.is_empty()
        || root.active == Some(false)
        || root.occurrences.len() != rows.occurrences.len()
    {
        return;
    }
    let mut placed = std::collections::BTreeSet::new();
    for occurrence in &rows.occurrences {
        if !root.occurrences.contains(&occurrence.id)
            || occurrence.transform != Some(cadmpeg_ir::transform::Transform::identity())
            || occurrence.suppressed == Some(true)
            || occurrence.visible == Some(false)
            || occurrence.representation.is_some()
            || !occurrence.overrides.is_empty()
            || !placed.insert(occurrence.prototype.as_str())
        {
            return;
        }
    }
    if rows.product_definitions.len() != placed.len().saturating_add(1)
        || rows.product_definitions.iter().any(|definition| {
            definition.id != root.id
                && (!placed.contains(definition.id.as_str())
                    || !matches!(definition.storage, Storage::Embedded)
                    || !definition.occurrences.is_empty()
                    || !definition.instance_bodies.is_empty()
                    || !definition.representations.is_empty()
                    || definition.active == Some(false))
        })
    {
        return;
    }
    // Each placement is named by its row and its owner; any other mention keeps the
    // product structure as decoded. The file wrapper must also have no other users.
    let Ok(text) = serde_json::to_string(&model.ir) else {
        return;
    };
    let named = |id: &str| text.matches(&format!("\"{id}\"")).count();
    if rows
        .occurrences
        .iter()
        .any(|occurrence| named(occurrence.id.as_str()) != 2)
        || named(root.id.as_str()) != 1
    {
        return;
    }
    let root_id = root.id.clone();
    model.ir.model.occurrences.clear();
    model
        .ir
        .model
        .product_definitions
        .retain(|definition| definition.id != root_id);
    // The STEP export names each part on its product. A body that is its product's only
    // body and has no name of its own takes the part's name, which the written body keeps.
    let names: std::collections::BTreeMap<String, String> = model
        .ir
        .model
        .product_definitions
        .iter()
        .filter_map(|definition| {
            let [body] = definition.bodies.as_slice() else {
                return None;
            };
            let name = definition
                .label
                .as_ref()
                .or(definition.source_name.as_ref())
                .filter(|name| !name.trim().is_empty())?;
            Some((body.as_str().to_owned(), name.clone()))
        })
        .collect();
    for body in &mut model.ir.geometry.bodies {
        if body.name.as_ref().is_none_or(|name| name.trim().is_empty()) {
            if let Some(name) = names.get(body.id.as_str()) {
                body.name = Some(name.clone());
            }
        }
    }
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

fn boolean_field(row: &Value, key: &str) -> Mapping<bool> {
    match row.get(key) {
        None => Ok(false),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(format!("{key} must be Boolean")),
    }
}
fn suppression(feature: &Value) -> Mapping<bool> {
    Ok(boolean_field(feature, "suppressed")? | boolean_field(feature, "parentSuppressed")?)
}
fn reserve_history<T>(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    rows: &mut Vec<T>,
    count: usize,
) -> Result<(), Error> {
    rows.try_reserve(count).map_err(|_| {
        let resource = cadmpeg_core::decode::ResourceLimit::allocation_failed(
            cadmpeg_core::decode::ResourceDimension::RetainedBytes,
            ctx.policy().limits.max_retained_bytes,
            cadmpeg_core::decode::u64_from_index(count).saturating_mul(
                cadmpeg_core::decode::u64_from_index(std::mem::size_of::<T>()),
            ),
            "Onshape history allocation",
        );
        Error::Decode(format!(
            "{} during {}",
            cadmpeg_core::CodecError::ResourceLimit(resource),
            resource.operation
        ))
    })
}

fn admit_history(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &Value,
    model: &mut cadmpeg_ir::document::Model,
) -> Result<(), Error> {
    let count = |key| {
        optional_rows(feature, key)
            .map(<[Value]>::len)
            .map_err(Error::Decode)
    };
    let mut entities = count("entities")?;
    let mut constraints = 0usize;
    let mut parameters =
        usize::from(feature.get("featureType").and_then(Value::as_str) != Some("newSketch"));
    let overflow = || Error::Decode("history count overflow".into());
    for row in optional_rows(feature, "constraints").map_err(Error::Decode)? {
        if let Ok(output) = sketch::constraint_output(row) {
            entities = entities.checked_add(output.entities).ok_or_else(overflow)?;
            constraints = constraints
                .checked_add(output.constraints)
                .ok_or_else(overflow)?;
            parameters = parameters
                .checked_add(output.parameters)
                .ok_or_else(overflow)?;
        }
    }
    let total = entities
        .checked_add(constraints)
        .and_then(|n| n.checked_add(parameters))
        .and_then(|n| n.checked_add(2))
        .ok_or_else(overflow)?;
    ctx.charge_entities(
        cadmpeg_core::decode::u64_from_index(total),
        "Onshape history admission",
    )
    .map_err(crate::decode::onshape_failure)?;
    reserve_history(ctx, &mut model.features, 1)?;
    reserve_history(ctx, &mut model.sketches, 1)?;
    reserve_history(ctx, &mut model.sketch_entities, entities)?;
    reserve_history(ctx, &mut model.sketch_constraints, constraints)?;
    reserve_history(ctx, &mut model.parameters, parameters)?;
    reserve_history(ctx, &mut model.feature_input_topologies, 1)?;
    Ok(())
}

pub(super) fn read(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    path: &std::path::Path,
    bytes: &[u8],
    report: &mut Report,
    progress: Option<&Progress<'_>>,
) -> Result<Model, Error> {
    let mut bundle = Bundle::open(ctx, bytes)?;
    let mid = text(
        bundle
            .json("manifest.json")?
            .get("document")
            .unwrap_or(&Value::Null),
        "microversion",
    )
    .map_err(Error::Decode)?
    .to_owned();
    bundle.validate_microversions(&mid);
    bundle.admit_topology(ctx)?;
    bundle.admit_records();
    bundle.report_losses(report);
    let manifest = bundle.json("manifest.json")?;
    let mut reference_budget = 256 << 20;
    read_studio(
        ctx,
        path,
        &bundle,
        manifest,
        "",
        0,
        &mut reference_budget,
        report,
        progress,
    )
}

#[allow(clippy::too_many_arguments)]
fn read_studio(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    path: &std::path::Path,
    bundle: &Bundle,
    manifest: &Value,
    folder: &str,
    depth: usize,
    reference_budget: &mut usize,
    report: &mut Report,
    progress: Option<&Progress<'_>>,
) -> Result<Model, Error> {
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
    let elements = rows(manifest, "elements").map_err(Error::Decode)?;
    let root = manifest
        .get("root_element")
        .map(|value| {
            value
                .as_str()
                .filter(|root| !root.is_empty())
                .ok_or_else(|| {
                    Error::Decode(
                        "Onshape bundle root element has no identity; fetch a new bundle".into(),
                    )
                })
        })
        .transpose()?;
    if elements.is_empty()
        || elements
            .iter()
            .any(|e| e.get("type").and_then(Value::as_str) != Some("PARTSTUDIO"))
        || (elements.len() > 1 && root.is_none())
    {
        return Err(Error::Decode("Onshape bundles with assemblies or no root Part Studio are not supported yet; fetch one Part Studio".into()));
    }
    let mut ids = std::collections::BTreeSet::new();
    for element in elements {
        if !ids.insert(text(element, "id").map_err(Error::Decode)?) {
            return Err(Error::Decode(
                "Onshape element ids repeat; fetch a new bundle".into(),
            ));
        }
    }
    let element = elements
        .iter()
        .find(|e| root.is_none_or(|root| e.get("id").and_then(Value::as_str) == Some(root)))
        .ok_or_else(|| {
            Error::Decode("Onshape bundle has no root Part Studio; fetch a new bundle".into())
        })?;
    let eid = text(element, "id").map_err(Error::Decode)?;
    if !eid
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Error::Decode(
            "Onshape element id cannot name a bundle folder; fetch a new bundle".into(),
        ));
    }
    let prefix = format!("{folder}partstudios/{eid}");
    let features = match bundle
        .json(&format!("{prefix}/features.json"))
        .map_err(|e| e.to_string())
        .and_then(|response| rows(response, "features"))
    {
        Ok(features) => features,
        Err(reason) => {
            loss(
                report,
                "Onshape history",
                &reason,
                false,
                "onshape/history.not-supported",
            );
            &[]
        }
    };
    let inputs = topology::Inputs::new(bundle, &prefix, mid, features).map_err(Error::Decode)?;
    let step_path = format!("{prefix}/export.step");
    let empty = || Model {
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
        merge_bodies: false,
    };
    let mut model = if bundle.contains(&step_path) {
        let source = bundle.bytes(&step_path)?;
        let decoded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            crate::decode::onshape_step(ctx, path, source, report, progress)
        }))
        .unwrap_or_else(|_| {
            Err(Error::Decode(
                "the Onshape STEP member decoder failed unexpectedly; fetch a new bundle".into(),
            ))
        });
        match decoded {
            Ok(mut model) => {
                model.source_step = match String::from_utf8(source.to_vec()) {
                    Ok(text) => Some(text),
                    Err(_) => {
                        loss(
                            report,
                            "Onshape STEP export",
                            "the STEP text is not UTF-8; fetch a new bundle",
                            false,
                            "onshape/member.not-supported",
                        );
                        None
                    }
                };
                part_studio_bodies(&mut model);
                model
            }
            Err(error) => {
                if let Some(resource) = ctx.resource_refusal() {
                    return Err(Error::Decode(format!(
                        "{error} during {}",
                        resource.operation
                    )));
                }
                loss(
                    report,
                    "Onshape STEP export",
                    &error.to_string(),
                    false,
                    "onshape/member.not-supported",
                );
                empty()
            }
        }
    } else {
        empty()
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
                && suppression(feature) == Ok(false)
        })
        .map(|(ordinal, _)| ordinal)
        .collect();
    let has_derived = features
        .iter()
        .any(|feature| derived::is_derived(feature) && suppression(feature) != Ok(true));
    let single_body = (!has_derived && creators.len() == 1 && model.ir.geometry.bodies.len() == 1)
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
                     them; SolidCraft names those parts, and their colors are left out",
                    parts.saturating_sub(bound.len()),
                    parts
                ),
            );
        }
        bound
    } else {
        std::collections::BTreeMap::new()
    };
    if features
        .iter()
        .any(|feature| features::value(feature, "bodyType") == Some("THIN"))
    {
        ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(features.len().saturating_add(2)),
            "Onshape design admission",
        )
        .map_err(crate::decode::onshape_failure)?;
        reserve_history(ctx, &mut model.ir.design.features, features.len())?;
        reserve_history(ctx, &mut model.ir.design.containers, 1)?;
        reserve_history(ctx, &mut model.ir.design.scopes, 1)?;
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut unresolved_variables = std::collections::BTreeSet::new();
    for (ordinal, feature) in features.iter().enumerate() {
        let native = match text(feature, "featureId") {
            Ok(native) if seen.insert(native) => native,
            _ => {
                if feature.get("featureType").and_then(Value::as_str) == Some("assignVariable")
                    && suppression(feature) != Ok(true)
                {
                    unresolved_variables.extend(
                        model
                            .ir
                            .model
                            .parameters
                            .iter()
                            .filter(|parameter| parameter.owner.is_none())
                            .map(|parameter| parameter.name.clone()),
                    );
                    if let Ok(parameters) = optional_rows(feature, "parameters") {
                        unresolved_variables.extend(
                            parameters
                                .iter()
                                .filter(|parameter| {
                                    parameter.get("parameterId").and_then(Value::as_str)
                                        == Some("name")
                                })
                                .filter_map(|parameter| {
                                    parameter.get("value").and_then(Value::as_str)
                                })
                                .map(str::to_owned),
                        );
                    }
                }
                loss(
                    report,
                    &format!("Onshape feature at index {ordinal}"),
                    "the feature id is missing or repeated; fetch a new bundle",
                    model.source_step.is_some(),
                    "onshape/history.not-supported",
                );
                continue;
            }
        };
        let feature_id = id("feature", native);
        let name = feature
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or(native);
        let suppressed = suppression(feature);
        let bindings: Vec<_> =
            if feature.get("featureType").and_then(Value::as_str) == Some("assignVariable") {
                optional_rows(feature, "parameters")
                    .unwrap_or_default()
                    .iter()
                    .filter(|p| p.get("parameterId").and_then(Value::as_str) == Some("name"))
                    .filter_map(|p| p.get("value").and_then(Value::as_str))
                    .collect()
            } else {
                Vec::new()
            };
        if suppressed.as_ref().is_ok_and(|s| !s) {
            for name in &bindings {
                if model
                    .ir
                    .model
                    .parameters
                    .iter()
                    .any(|p| p.owner.is_none() && p.name == *name)
                {
                    unresolved_variables.insert((*name).to_owned());
                }
            }
        }
        // Malformed record counts are local; caller budget/allocation failures stay fatal.
        let admission = ["entities", "constraints", "parameters"]
            .into_iter()
            .try_for_each(|key| optional_rows(feature, key).map(|_| ()));
        if admission.is_ok() {
            admit_history(ctx, feature, &mut model.ir.model)?;
        } else {
            ctx.charge_entities(1, "Onshape native feature admission")
                .map_err(crate::decode::onshape_failure)?;
            reserve_history(ctx, &mut model.ir.model.features, 1)?;
        }
        let sizes = (
            model.ir.model.sketches.len(),
            model.ir.model.sketch_entities.len(),
            model.ir.model.sketch_constraints.len(),
            model.ir.model.parameters.len(),
            model.ir.model.feature_input_topologies.len(),
            model.ir.design.features.len(),
        );
        let mut reference = None;
        let mut dependencies = Vec::<String>::new();
        let mapped = if let Err(reason) = admission {
            Err(reason)
        } else if let Err(reason) = &suppressed {
            Err(reason.clone())
        } else if suppressed == Ok(true) {
            Ok(definition(
                "native",
                json!({"kind": "Suppressed Onshape feature", "parameters": {}}),
            ))
        } else if derived::is_derived(feature) {
            derived::map(
                ctx,
                feature,
                ordinal,
                &inputs,
                bundle,
                folder,
                depth,
                reference_budget,
                report,
            )
            .map(|captured| {
                reference = Some(captured);
                definition("stored_geometry", json!({}))
            })
        } else if has_derived
            && feature.get("featureType").and_then(Value::as_str) == Some("boolean")
        {
            derived::boolean(feature, ordinal, &inputs, &model.ir.model)
        } else {
            features::map(
                feature,
                ordinal,
                &inputs,
                &mut model.ir.model,
                report,
                &unresolved_variables,
            )
            .and_then(|definition| {
                serde_json::from_value::<cadmpeg_ir::features::FeatureDefinition>(
                    definition.clone(),
                )
                .map_err(|e| format!("the resolved construction is not supported yet ({e})"))?;
                dependencies = features::dependencies(
                    feature,
                    &definition,
                    ordinal,
                    &inputs,
                    &model.ir.model,
                )?;
                if features::value(feature, "bodyType") == Some("THIN")
                    && feature.get("featureType").and_then(Value::as_str) == Some("extrude")
                {
                    thin::owner(&model.ir)?;
                    let design = thin::feature(
                        feature,
                        &definition,
                        &model.ir.model,
                        &inputs,
                        ordinal,
                        &unresolved_variables,
                    )?;
                    push(&mut model.ir.design.features, design)?;
                }
                Ok(definition)
            })
        };
        // A referenced studio shares the caller's resource session. Record losses
        // remain local, but an exhausted caller budget must still stop conversion.
        if let Some(resource) = ctx.resource_refusal() {
            return Err(Error::Decode(format!(
                "Onshape resource budget exhausted during {}",
                resource.operation
            )));
        }
        let mut definition = match mapped {
            Ok(definition) => {
                if suppressed != Ok(true)
                    && !matches!(
                        feature.get("featureType").and_then(Value::as_str),
                        Some("newSketch" | "assignVariable")
                    )
                {
                    if let Err(reason) =
                        features::report_evaluated_variables(feature, &model.ir.model, report)
                    {
                        loss(
                            report,
                            name,
                            &reason,
                            model.source_step.is_some(),
                            "onshape/history.not-supported",
                        );
                    }
                }
                definition
            }
            Err(reason) => {
                dependencies.clear();
                model.ir.model.sketches.truncate(sizes.0);
                model.ir.model.sketch_entities.truncate(sizes.1);
                model.ir.model.sketch_constraints.truncate(sizes.2);
                model.ir.model.parameters.truncate(sizes.3);
                model.ir.model.feature_input_topologies.truncate(sizes.4);
                model.ir.design.features.truncate(sizes.5);
                if suppressed != Ok(true) {
                    let region_budget_exhausted = inputs
                        .regions
                        .try_borrow()
                        .is_ok_and(|recovery| recovery.exhausted());
                    let code = if !region_budget_exhausted
                        && feature.get("featureType").and_then(Value::as_str) == Some("extrude")
                        && features::value(feature, "bodyType") == Some("THIN")
                    {
                        "onshape/extrude.thin-not-supported"
                    } else {
                        "onshape/history.not-supported"
                    };
                    loss(report, name, &reason, model.source_step.is_some(), code);
                }
                definition(
                    "native",
                    json!({"kind": format!("Onshape {}: {reason}", feature.get("featureType").and_then(Value::as_str).unwrap_or("feature")), "parameters": {}}),
                )
            }
        };
        if definition.pointer("/value/kind").and_then(Value::as_str) == Some("native")
            && suppressed != Ok(true)
        {
            if bindings.is_empty()
                && feature.get("featureType").and_then(Value::as_str) == Some("assignVariable")
            {
                unresolved_variables.extend(
                    model
                        .ir
                        .model
                        .parameters
                        .iter()
                        .filter(|p| p.owner.is_none())
                        .map(|p| p.name.clone()),
                );
            }
            for name in &bindings {
                unresolved_variables.insert((*name).to_owned());
            }
        }
        if suppressed == Ok(false)
            && definition.pointer("/value/kind").and_then(Value::as_str) == Some("tree_node")
            && feature.get("featureType").and_then(Value::as_str) == Some("assignVariable")
        {
            for name in &bindings {
                unresolved_variables.remove(*name);
            }
        }
        let mut source_properties = match features::source_properties(feature, &definition) {
            Ok(properties) => properties,
            Err(reason) => {
                loss(
                    report,
                    name,
                    &reason,
                    model.source_step.is_some(),
                    "onshape/history.not-supported",
                );
                json!({})
            }
        };
        if definition.pointer("/value/kind").and_then(Value::as_str) == Some("extrude")
            && definition
                .pointer("/value/value/extent/kind")
                .and_then(Value::as_str)
                == Some("symmetric")
        {
            if let Some(properties) = source_properties.as_object_mut() {
                properties.insert("symmetric_distance_parameter".into(), json!("total"));
            }
        }
        let captured = (|| -> Mapping<()> {
            let properties = source_properties
                .as_object_mut()
                .ok_or("the feature source properties are not supported yet")?;
            if let Some(reference) = reference {
                properties.insert("design_reference".into(), json!(reference));
            }
            // Historical bodies can be consumed by a later Boolean and absent from final STEP.
            // Keep their proven rollback identities/names separate from the final geometry.
            if has_derived
                && creators.contains(&ordinal)
                && matches!(
                    definition.pointer("/value/kind").and_then(Value::as_str),
                    Some("extrude" | "revolve")
                )
                && definition
                    .pointer("/value/value/op")
                    .and_then(Value::as_str)
                    == Some("new_body")
            {
                if let (Ok(before), Ok(after)) = (
                    inputs.parts(ordinal),
                    inputs.parts(ordinal.saturating_add(1)),
                ) {
                    let made: Vec<_> = after
                        .iter()
                        .filter(|(key, _)| !before.contains_key(*key))
                        .collect();
                    if let [(native, Some(name))] = made.as_slice() {
                        properties.insert(
                            "historical_bodies".into(),
                            json!(serde_json::to_string(
                                &json!([{"id": id("body", native), "name": name}])
                            )
                            .map_err(|error| error.to_string())?),
                        );
                    }
                }
            }
            let historical = properties
                .get("historical_bodies")
                .or_else(|| properties.get("design_reference"))
                .and_then(Value::as_str)
                .map(serde_json::from_str::<Value>)
                .transpose()
                .map_err(|error| error.to_string())?;
            let bodies = historical.as_ref().and_then(|value| {
                value
                    .as_array()
                    .or_else(|| value.get("bodies").and_then(Value::as_array))
            });
            if let Some(bodies) = bodies {
                let local: Vec<_> = bodies
                    .iter()
                    .filter_map(|body| body.get("id").and_then(Value::as_str))
                    .collect();
                push(
                    &mut model.ir.model.feature_result_topologies,
                    json!({"id":id("result",native),"output_of":feature_id,"members":local.iter().map(|id|json!({"kind":"body","id":id})).collect::<Vec<_>>()}),
                )?;
            }
            Ok(())
        })();
        if let Err(reason) = captured {
            dependencies.clear();
            source_properties = json!({});
            definition = self::definition(
                "native",
                json!({"kind":format!("Onshape feature: {reason}"),"parameters":{}}),
            );
            loss(
                report,
                name,
                &reason,
                model.source_step.is_some(),
                "onshape/history.not-supported",
            );
        }
        if has_derived {
            // Generated body references require direct producer dependencies, even
            // when the timeline's preceding feature already depends on that producer.
            for role in feature
                .get("parameters")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|parameter| {
                    match parameter.get("parameterId").and_then(Value::as_str) {
                        Some("targets") => Some("target"),
                        Some("tools") => Some("tools"),
                        _ => None,
                    }
                })
            {
                if let Some(bodies) = definition
                    .pointer(&format!("/value/value/operands/{role}/value/bodies"))
                    .and_then(Value::as_array)
                {
                    for body in bodies {
                        if let Some(producer) = body.get("feature").and_then(Value::as_str) {
                            if !dependencies.iter().any(|id| id == producer) {
                                dependencies.push(producer.to_owned());
                            }
                        }
                    }
                }
            }
        }
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
        let mut row = json!({"id": feature_id, "ordinal": ordinal, "name": name, "dependencies": dependencies, "source_properties": source_properties, "source_content": [], "outputs": outputs, "definition": definition});
        if let (Ok(suppressed), Some(row)) = (suppressed, row.as_object_mut()) {
            row.insert("suppressed".into(), json!(suppressed));
        }
        if let Err(reason) = push(&mut model.ir.model.features, row) {
            loss(
                report,
                name,
                &reason,
                model.source_step.is_some(),
                "onshape/history.not-supported",
            );
        }
    }
    if !model.ir.design.features.is_empty() {
        let mut refused: std::collections::BTreeSet<_> = model
            .ir
            .design
            .features
            .iter()
            .map(|feature| feature.id.clone())
            .collect();
        if let Err(reason) = thin::install_scope(ctx, &mut model.ir, report) {
            if let Some(resource) = ctx.resource_refusal() {
                return Err(Error::Decode(format!(
                    "{reason} during {}",
                    resource.operation
                )));
            }
            model.ir.design = Default::default();
            for feature in &mut model.ir.model.features {
                if refused.contains(&feature.id)
                    || feature
                        .dependencies
                        .iter()
                        .any(|dependency| refused.contains(dependency))
                {
                    refused.insert(feature.id.clone());
                    if let Ok(native) = serde_json::from_value(definition(
                        "native",
                        json!({"kind":format!("Onshape design scope: {reason}"),"parameters":{}}),
                    )) {
                        feature.evaluation =
                            cadmpeg_ir::features::evaluation::FeatureEvaluation::from_definition(
                                native,
                            );
                    }
                    feature.dependencies = Default::default();
                    loss(
                        report,
                        feature.id.as_str(),
                        &reason,
                        model.source_step.is_some(),
                        "onshape/history.not-supported",
                    );
                }
            }
        }
    }
    annotate(&mut model, features, report);
    model.ir.finalize();
    Ok(model)
}

fn annotation_result<T, E: std::fmt::Display>(
    result: Result<T, E>,
    report: &mut Report,
    subject: &str,
    has_step: bool,
) {
    if let Err(error) = result {
        loss(
            report,
            subject,
            &error.to_string(),
            has_step,
            "onshape/history.not-supported",
        );
    }
}

fn annotate(model: &mut Model, features: &[Value], report: &mut Report) {
    use cadmpeg_ir::annotations::AnnotationBuilder;
    use cadmpeg_ir::Exactness;
    let has_step = model.source_step.is_some();
    let Some(fidelity) = model.fidelity.as_mut() else {
        return;
    };
    let mut annotations = AnnotationBuilder::resume(std::mem::take(&mut fidelity.annotations));
    for topology in &model.ir.model.feature_input_topologies {
        annotations.exactness(&topology.id, Exactness::Derived);
    }
    for entity in &model.ir.model.sketch_entities {
        if !matches!(
            entity.geometry.definition(),
            cadmpeg_ir::sketches::SketchGeometryDefinition::Point { .. }
        ) {
            annotation_result(
                annotations.field_exactness(entity.id(), "geometry", Exactness::Derived),
                report,
                entity.id().as_str(),
                has_step,
            );
        }
    }
    for parameter in &model.ir.model.parameters {
        if !parameter.dependencies.is_empty() || parameter.expression.contains('(') {
            annotation_result(
                annotations.field_exactness(&parameter.id, "value", Exactness::Derived),
                report,
                parameter.id.as_str(),
                has_step,
            );
        }
    }
    for feature in &model.ir.model.features {
        annotation_result(
            annotations.field_exactness(&feature.id, "definition", Exactness::Derived),
            report,
            feature.id.as_str(),
            has_step,
        );
        annotation_result(
            annotations.field_exactness(&feature.id, "dependencies", Exactness::Derived),
            report,
            feature.id.as_str(),
            has_step,
        );
        annotation_result(
            annotations.field_exactness(&feature.id, "outputs", Exactness::Derived),
            report,
            feature.id.as_str(),
            has_step,
        );
    }
    for (ordinal, source) in features.iter().enumerate() {
        let Ok(native) = text(source, "featureId") else {
            continue;
        };
        if !model.ir.model.features.iter().any(|feature| {
            feature.id.as_str() == id("feature", native) && feature.ordinal == ordinal as u64
        }) {
            continue;
        }
        let sketch_id = id("sketch", native);
        if model
            .ir
            .model
            .sketches
            .iter()
            .any(|s| s.id.as_str() == sketch_id)
        {
            let queries = match features::queries(source, "sketchPlane") {
                Ok(queries) => queries,
                Err(reason) => {
                    loss(
                        report,
                        native,
                        &reason,
                        has_step,
                        "onshape/history.not-supported",
                    );
                    continue;
                }
            };
            let inferred = queries.iter().all(|q| {
                q.get("deterministicIds")
                    .and_then(Value::as_array)
                    .is_none_or(Vec::is_empty)
                    && q.get("queryString")
                        .and_then(Value::as_str)
                        .is_none_or(|text| {
                            !text.contains("qCompressed(")
                                || query::resolved(q).is_ok_and(|v| {
                                    v.get("queryType").and_then(Value::as_str) == Some("DUMMY")
                                })
                        })
            });
            annotation_result(
                annotations.field_exactness(
                    &sketch_id,
                    "placement",
                    if inferred {
                        Exactness::Inferred
                    } else {
                        Exactness::Derived
                    },
                ),
                report,
                sketch_id.as_str(),
                has_step,
            );
        }
        if source.get("suppressed").is_none() && source.get("parentSuppressed").is_none() {
            annotation_result(
                annotations.field_exactness(
                    id("feature", native),
                    "suppressed",
                    Exactness::Inferred,
                ),
                report,
                id("feature", native).as_str(),
                has_step,
            );
        }
    }
    fidelity.annotations = annotations.build();
}

#[cfg(test)]
mod review;
#[cfg(test)]
mod tests;

#[cfg(test)]
mod rereview;
