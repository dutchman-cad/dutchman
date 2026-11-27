//! Errors a FreeCAD file already had in FreeCAD, found from what FreeCAD saved.
//!
//! FreeCAD writes an object that failed its last recompute with `Invalid="1"` and, when it
//! has one, `Error="<message>"` on the object's declaration in `Document.xml`. cadmpeg keeps
//! those declaration attributes on each object record, so this reads the decoded records and
//! never the file. `Touched="1"` alone only means a recompute is pending and is not an error.
use crate::{Report, SourceError};
use cadmpeg_ir::CadIr;
use serde_json::Value;
use std::collections::BTreeMap;

/// The most errored objects kept from one file; a hostile file cannot grow the report.
const MAX_OBJECTS: usize = 200;
/// The longest FreeCAD message kept, in characters.
const MAX_MESSAGE: usize = 200;

/// Records the objects the FreeCAD file saved as invalid or errored.
pub(crate) fn record(ir: &CadIr, report: &mut Report) {
    let found = find(ir);
    report.record_source_errors(found.0, found.1);
}

/// The errored objects, at most [`MAX_OBJECTS`], and how many there were in all.
fn find(ir: &CadIr) -> (Vec<SourceError>, u64) {
    let Some(native) = ir.native.namespace("fcstd") else {
        return (Vec::new(), 0);
    };
    let Some(objects) = native.arenas().get("objects") else {
        return (Vec::new(), 0);
    };
    let mut errored: Vec<(String, String, Option<String>)> = Vec::new();
    let mut total = 0_u64;
    for object in objects {
        let Some(Value::Object(attributes)) = object.field("attributes") else {
            continue;
        };
        let flag = |name: &str| attributes.get(name).and_then(Value::as_str) == Some("1");
        let message = attributes
            .get("Error")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|message| !message.is_empty());
        if !flag("Invalid") && message.is_none() {
            continue;
        }
        total += 1;
        if errored.len() >= MAX_OBJECTS {
            continue;
        }
        let name = object
            .field("name")
            .and_then(|name| name.as_str().map(str::to_owned))
            .unwrap_or_default();
        errored.push((
            object.id().to_owned(),
            name,
            message.map(|message| message.chars().take(MAX_MESSAGE).collect()),
        ));
    }
    if errored.is_empty() {
        return (Vec::new(), 0);
    }
    // The label is what FreeCAD's tree shows; the internal name is the fallback.
    let mut labels: BTreeMap<&str, String> = BTreeMap::new();
    if let Some(properties) = native.arenas().get("properties") {
        for property in properties {
            if property.field("name").as_ref().and_then(Value::as_str) != Some("Label") {
                continue;
            }
            let Some(owner) = property.field("owner") else {
                continue;
            };
            let Some(owner) = owner.as_str() else {
                continue;
            };
            let Some((id, _, _)) = errored.iter().find(|(id, _, _)| id == owner) else {
                continue;
            };
            let label = property
                .field("values")
                .and_then(|values| {
                    values.as_array()?.iter().find_map(|value| {
                        value
                            .pointer("/attributes/value")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                })
                .filter(|label| !label.is_empty());
            if let Some(label) = label {
                labels.insert(id.as_str(), label.chars().take(MAX_MESSAGE).collect());
            }
        }
    }
    let rows = errored
        .iter()
        .map(|(id, name, message)| SourceError {
            name: name.clone(),
            label: labels
                .get(id.as_str())
                .cloned()
                .unwrap_or_else(|| name.clone()),
            message: message.clone(),
        })
        .collect();
    (rows, total)
}
