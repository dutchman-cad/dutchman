//! Pinned Part Studio references. Only neutral geometry crosses the writer seam.
use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use super::{archive::Bundle, definition, features, id, rows, text, topology, Mapping};
use crate::Report;

pub(super) fn is_derived(feature: &Value) -> bool {
    matches!(
        feature.get("featureType").and_then(Value::as_str),
        Some("derived" | "importDerived")
    )
}

/// A structured REST reference, never an evaluated FeatureScript query or a guessed URL.
pub(super) fn reference(value: &Value, depth: usize) -> Mapping<Option<(&str, &str, &str)>> {
    if depth > 64 {
        return Err("the derived reference exceeds the nesting limit".into());
    }
    if let (Some(d), Some(v), Some(e)) = (
        value.get("documentId").and_then(Value::as_str),
        value.get("versionId").and_then(Value::as_str),
        value.get("elementId").and_then(Value::as_str),
    ) {
        for part in [d, v, e] {
            if part.len() != 24 || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("the derived reference has no pinned document/version/element identity; fetch a new bundle".into());
            }
        }
        if let Some(object) = value.as_object() {
            for (key, value) in object {
                match key.as_str() {
                    "documentId" | "versionId" | "elementId" | "btType" => {},
                    "configuration" if value.as_str() == Some("") => {},
                    _ => return Err(format!("derived reference field {key} is not supported yet; its configuration and placement must be resolved")),
                }
            }
        }
        return Ok(Some((d, v, e)));
    }
    let children: Vec<&Value> = match value {
        Value::Object(map) => map.values().collect(),
        Value::Array(list) if list.len() <= super::ROWS => list.iter().collect(),
        Value::Array(_) => return Err("the derived reference exceeds the row limit".into()),
        _ => Vec::new(),
    };
    let mut found = None;
    for child in children {
        if let Some(next) = reference(child, depth + 1)? {
            if found.is_some_and(|previous| previous != next) {
                return Err(
                    "deriving from several Part Studios in one feature is not supported yet".into(),
                );
            }
            found = Some(next);
        }
    }
    Ok(found)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn map(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    bundle: &Bundle,
    folder: &str,
    depth: usize,
    reference_budget: &mut usize,
    report: &mut Report,
) -> Mapping<String> {
    if *reference_budget == 0 {
        return Err("the captured neutral designs exceed the 256 MiB reference limit; convert a smaller studio".into());
    }
    if feature
        .get("namespace")
        .and_then(Value::as_str)
        .is_some_and(|namespace| !namespace.is_empty())
    {
        return Err("custom FeatureScript derived features are not supported yet".into());
    }
    if depth >= 7 {
        return Err("derived Part Studios exceed the reference depth limit".into());
    }
    let mut selected = BTreeSet::new();
    for parameter in rows(feature, "parameters")? {
        let key = text(parameter, "parameterId")?;
        if !matches!(
            key,
            "parts" | "partStudio" | "buildFunction" | "newParts" | "includeProperties"
        ) {
            return Err(format!("derived parameter {key} is not supported yet; transformed or configured inserts need a resolved placement"));
        }
        if parameter
            .get("configuration")
            .is_some_and(|v| v.as_str() != Some(""))
        {
            return Err("configured derived Part Studios are not supported yet".into());
        }
        if parameter.get("queries").is_some() {
            for query in rows(parameter, "queries")? {
                let ids = rows(query, "deterministicIds")?;
                if ids.is_empty()
                    || query.get("queryString").is_some()
                    || query
                        .get("namespace")
                        .and_then(Value::as_str)
                        .is_some_and(|namespace| !namespace.is_empty())
                {
                    return Err("derived part queries are not fully resolved; select the whole referenced Part Studio".into());
                }
                for value in ids {
                    let native = value
                        .as_str()
                        .filter(|id| !id.trim().is_empty())
                        .ok_or("a selected derived part identity is missing")?;
                    if !selected.insert(native) || selected.len() > super::ROWS {
                        return Err("derived part identities repeat or exceed the row limit".into());
                    }
                }
            }
        }
    }
    let (d, v, e) = reference(feature, 0)?
        .ok_or("the derived Part Studio reference is not resolved; fetch a new bundle")?;
    let prefix = format!("{folder}references/{d}/{v}/{e}/");
    let manifest = bundle
        .json(&format!("{prefix}manifest.json"))
        .map_err(|_| "the referenced Part Studio is missing; fetch a new bundle")?;
    let document = manifest.get("document").unwrap_or(&Value::Null);
    if text(document, "id")? != d
        || text(document, "wvid")? != v
        || text(document, "workspace_or_version")? != "v"
        || text(manifest, "root_element")? != e
    {
        return Err("the captured Part Studio does not match the pinned derived reference; fetch a new bundle".into());
    }
    let before = inputs.solid_parts(ordinal)?;
    let after = inputs.solid_parts(ordinal.saturating_add(1))?;
    if before.iter().any(|(id, name)| after.get(id) != Some(name)) {
        return Err("the derived feature changes an existing body; only an unchanged-placement Part Studio insert is supported yet".into());
    }
    let mut source = super::read_studio(
        ctx,
        std::path::Path::new("Referenced studio"),
        bundle,
        manifest,
        &prefix,
        depth + 1,
        reference_budget,
        report,
        None,
    )
    .map_err(|error| error.to_string())?;
    if let Some(name) = manifest
        .get("elements")
        .and_then(Value::as_array)
        .and_then(|elements| {
            elements
                .iter()
                .find(|element| element.get("id").and_then(Value::as_str) == Some(e))
        })
        .and_then(|element| element.get("name"))
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
    {
        source.name = name.into();
    }
    // A STEP export can omit its sole solid's name. One final rollback part and one
    // neutral body give a unique correspondence without relying on STEP body order.
    let missing_single_name = matches!(source.ir.geometry.bodies.as_slice(), [body]
        if body.name.as_ref().is_none_or(|name| name.trim().is_empty()));
    let source_parts = if missing_single_name || !selected.is_empty() {
        let source_prefix = format!("{prefix}partstudios/{e}");
        let source_features = rows(
            bundle
                .json(&format!("{source_prefix}/features.json"))
                .map_err(|e| e.to_string())?,
            "features",
        )?;
        let source_inputs = topology::Inputs::new(
            bundle,
            &source_prefix,
            text(document, "microversion")?,
            source_features,
        )?;
        Some(source_inputs.solid_parts(source_features.len()).map_err(|reason| {
            format!("the referenced final parts snapshot cannot be used ({reason}); fetch a new bundle")
        })?)
    } else {
        None
    };
    if !selected.is_empty()
        && !source_parts.as_ref().is_some_and(|parts| {
            selected.len() == parts.len() && selected.iter().all(|id| parts.contains_key(*id))
        })
    {
        return Err(
            "derived part subsets are not supported yet; select the whole referenced Part Studio"
                .into(),
        );
    }
    if missing_single_name {
        if let (Some(body), Some(parts)) =
            (source.ir.geometry.bodies.first_mut(), source_parts.as_ref())
        {
            let names: Vec<_> = parts.values().collect();
            if let [Some(name)] = names.as_slice() {
                body.name = Some(name.clone());
            }
        }
    }
    let step = source
        .source_step
        .as_ref()
        .ok_or("the derived Part Studio has no solid bodies")?;
    let made: BTreeMap<_, _> = after
        .iter()
        .filter(|(key, _)| !before.contains_key(*key))
        .collect();
    if made.len() != source.ir.geometry.bodies.len() || made.is_empty() {
        return Err(
            "the derived bodies cannot be matched to the whole referenced Part Studio".into(),
        );
    }
    let mut bodies = Vec::new();
    let mut names = BTreeSet::new();
    for body in &source.ir.geometry.bodies {
        let name = body
            .name
            .as_deref()
            .ok_or("a referenced body has no unique name")?;
        let matches: Vec<_> = made
            .iter()
            .filter(|(_, n)| n.as_deref() == Some(name))
            .collect();
        let [(native, _)] = matches.as_slice() else {
            return Err("derived body names do not match the captured Part Studio uniquely".into());
        };
        if !names.insert(name) {
            return Err("referenced body names are not unique".into());
        }
        bodies.push(json!({"id": id("body", native), "name": name}));
    }
    let payload = serde_json::to_string(&json!({"schema":"dutchman.design-reference/1","ir":source.ir,"step":step,"name":source.name,"bodies":bodies})).map_err(|e|e.to_string())?;
    if payload.len() > *reference_budget {
        *reference_budget = 0;
        return Err("the captured neutral designs exceed the 256 MiB reference limit; convert a smaller studio".into());
    }
    *reference_budget -= payload.len();
    Ok(payload)
}

pub(super) fn boolean(
    feature: &Value,
    ordinal: usize,
    inputs: &topology::Inputs<'_>,
    model: &cadmpeg_ir::document::Model,
) -> Mapping<Value> {
    if feature
        .get("namespace")
        .and_then(Value::as_str)
        .is_some_and(|namespace| !namespace.is_empty())
    {
        return Err("custom FeatureScript Boolean features are not supported yet".into());
    }
    let parameters = rows(feature, "parameters")?;
    for parameter in parameters {
        let key = text(parameter, "parameterId")?;
        if !matches!(key, "targets" | "tools" | "operationType" | "keepTools") {
            return Err(format!("Boolean parameter {key} is not supported yet"));
        }
    }
    let keep = features::parameter(feature, "keepTools")?
        .get("value")
        .and_then(Value::as_bool)
        .ok_or("the Boolean keep-tools choice has no stored boolean value")?;
    let parts = inputs.solid_parts(ordinal)?;
    let selected = |key| -> Mapping<Vec<(String, String)>> {
        let mut picked = Vec::new();
        for query in features::queries(feature, key)? {
            let ids = rows(query, "deterministicIds")?;
            if query.get("queryString").is_some()
                || ids.is_empty()
                || query
                    .get("namespace")
                    .is_some_and(|namespace| namespace.as_str() != Some(""))
            {
                return Err("Boolean body queries are not resolved".into());
            }
            for value in ids {
                let native = value.as_str().ok_or("a Boolean body identity is missing")?;
                if !parts.contains_key(native)
                    || picked.iter().any(|(local, _)| local == &id("body", native))
                {
                    return Err(
                        "a Boolean body is missing or repeats in its input rollback state".into(),
                    );
                }
                let producer = inputs.producer(ordinal, native, topology::OperandKind::Body)?;
                inputs.unique_feature(&producer, ordinal)?;
                picked.push((id("body", native), id("feature", &producer)));
            }
        }
        Ok(picked)
    };
    let target = selected("targets")?;
    let tools = selected("tools")?;
    if target.len() != 1
        || tools.is_empty()
        || target
            .iter()
            .any(|(local, _)| tools.iter().any(|(tool, _)| tool == local))
    {
        return Err("this Boolean needs one target and disjoint tool bodies".into());
    }
    let op = match features::value(feature, "operationType") {
        Some("UNION" | "ADD") => "join",
        Some("SUBTRACTION" | "REMOVE") => "cut",
        Some("INTERSECTION" | "INTERSECT") => "intersect",
        _ => return Err("this Boolean operation is not supported yet".into()),
    };
    let generated = |selected: Vec<(String, String)>| -> Mapping<Value> {
        let mut references = Vec::new();
        for (local, producer) in selected {
            let owners: Vec<_> = model
                .feature_result_topologies
                .iter()
                .filter(|state| state.bodies().iter().any(|body| body.as_str() == local))
                .collect();
            let [owner] = owners.as_slice() else {
                return Err(
                    "a Boolean body cannot be tied uniquely to an earlier feature result".into(),
                );
            };
            if owner.output_of.as_str() != producer {
                return Err(
                    "the Boolean body producer disagrees with its stored rollback transition"
                        .into(),
                );
            }
            references.push(json!({"feature":owner.output_of,"local_id":local}));
        }
        Ok(
            json!({"kind":"generated","value":{"bodies":references,"native":serde_json::to_string(feature).map_err(|e|e.to_string())?}}),
        )
    };
    Ok(definition(
        "combine",
        json!({"operands": {"target": generated(target)?, "tools":generated(tools)?}, "op":op, "keep_tools":keep, "destination":"target_component"}),
    ))
}
