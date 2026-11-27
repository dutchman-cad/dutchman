//! Synthetic product fixtures described as placed trees, emitted as definition-owned CADIR.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Keep fixture recipes concise while constructing the pinned product schema. Each
/// recipe row names its parent placement; shared definitions own the union of their
/// children, with per-insertion suppression overrides selecting each stored layout.
pub(crate) fn read(wire: &Value) -> cadmpeg_ir::CadIr {
    let mut wire = wire.clone();
    let model = &mut wire["model"];
    let mut definitions = model["product_definitions"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    let mut rows = model["occurrences"].as_array().cloned().unwrap_or_default();
    let legacy = rows.iter().any(|row| row["prototype"].is_object());
    let file_id = "synthetic:fixture:definition#file";
    let mut parents = BTreeMap::new();
    let mut prototypes = BTreeMap::new();
    for row in &mut rows {
        let id = row["id"].as_str().unwrap().to_owned();
        if let Some(prototype) = row["prototype"].as_object() {
            let definition = if prototype["kind"] == "local" {
                prototype["definition"].as_str().unwrap().to_owned()
            } else {
                let definition = format!("{id}/external");
                let mut storage = prototype
                    .get("document")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                storage["kind"] = json!("external");
                if let Some(object) = prototype.get("object") {
                    storage["object"] = object.clone();
                }
                definitions.push(json!({"id":definition, "storage":storage}));
                definition
            };
            row["prototype"] = json!(definition);
        }
        prototypes.insert(id.clone(), row["prototype"].as_str().unwrap().to_owned());
        if legacy || row.get("parent").is_some() {
            parents.insert(
                id,
                row.pointer("/parent/occurrence")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            );
        }
        if let Some(scale) = row.get("scale") {
            assert!(
                scale
                    .as_array()
                    .is_some_and(|axes| axes.len() == 3
                        && axes.iter().all(|axis| axis.as_f64() == Some(1.0))),
                "fixture placements must have unit scale"
            );
        }
        for field in ["parent", "ordinal", "scale"] {
            row.as_object_mut().unwrap().remove(field);
        }
        row.as_object_mut()
            .unwrap()
            .entry("overrides")
            .or_insert(json!([]));
    }
    if !parents.is_empty() {
        if !definitions.iter().any(|d| d["id"] == file_id) {
            definitions.push(json!({"id":file_id, "storage":{"kind":"file", "document":{}}}));
        }
        let mut children: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let mut per_placement: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for row in &rows {
            let id = row["id"].as_str().unwrap();
            let Some(parent) = parents.get(id) else {
                continue;
            };
            let owner = parent
                .as_ref()
                .map_or(file_id, |id| prototypes.get(id).unwrap().as_str());
            children
                .entry(owner.to_owned())
                .or_default()
                .push(json!(id));
            if let Some(parent) = parent {
                per_placement
                    .entry(parent.clone())
                    .or_default()
                    .insert(id.to_owned());
            }
        }
        for definition in &mut definitions {
            let id = definition["id"].as_str().unwrap();
            if let Some(children) = children.get(id) {
                definition["occurrences"] = json!(children);
            }
        }
        for row in &mut rows {
            let id = row["id"].as_str().unwrap();
            let Some(children) = children.get(row["prototype"].as_str().unwrap()) else {
                continue;
            };
            let owned = per_placement.get(id);
            let suppressed: Vec<_> = children.iter().filter(|child| !owned.is_some_and(|owned| owned.contains(child.as_str().unwrap())))
                .map(|child| json!({"path":[{"occurrence":{"kind":"occurrence", "value":child}}], "suppressed":true})).collect();
            row["overrides"].as_array_mut().unwrap().extend(suppressed);
        }
    }
    for definition in &mut definitions {
        let fields = definition.as_object_mut().unwrap();
        fields
            .entry("storage")
            .or_insert(json!({"kind":"embedded"}));
        fields.entry("bom_properties").or_insert(json!({}));
        for field in [
            "bodies",
            "sketches",
            "tessellations",
            "occurrences",
            "instance_bodies",
            "representations",
        ] {
            fields.entry(field).or_insert(json!([]));
        }
    }
    model["product_definitions"] = json!(definitions);
    model["occurrences"] = json!(rows);
    cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap()
}
