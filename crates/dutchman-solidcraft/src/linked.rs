//! Insert Design links and their companion designs, as saved by SolidCraft 0.44.
use std::io::{Cursor, Read};

use dutchman_core::Error;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::container::Container;

const IDENTITY: [[f64; 4]; 4] = [
    [1., 0., 0., 0.],
    [0., 1., 0., 0.],
    [0., 0., 1., 0.],
    [0., 0., 0., 1.],
];

fn failure(detail: impl std::fmt::Display) -> Error {
    Error::Encode(format!("Could not write a linked design: {detail}"))
}

/// Complete the cached Import with the component, occurrence and link Insert Design saves.
pub(crate) fn store(document: &mut Value, container: &mut Container) -> Result<(), Error> {
    let document = document
        .as_object_mut()
        .ok_or_else(|| failure("document fields are missing"))?;
    let mut next = document
        .get("next_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| failure("next id is missing"))?;
    let mut components = Vec::new();
    let mut occurrences = Vec::new();
    let mut links = serde_json::Map::new();
    if let Some(features) = document.get_mut("features").and_then(Value::as_array_mut) {
        for feature in features {
            let Some(fields) = feature.as_object_mut() else {
                continue;
            };
            let Some(linked) = fields.remove("_linked_design") else {
                continue;
            };
            let hex = linked
                .get("hex")
                .and_then(Value::as_str)
                .filter(|hex| hex.len() <= 512 << 20 && hex.len().is_multiple_of(2))
                .ok_or_else(|| failure("source bytes exceed the companion limit"))?;
            let bytes: Vec<u8> = hex
                .as_bytes()
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| {
                    let pair = std::str::from_utf8(pair).map_err(failure)?;
                    u8::from_str_radix(pair, 16).map_err(failure)
                })
                .collect::<Result<_, _>>()?;
            let path = fields
                .get("file")
                .and_then(Value::as_str)
                .ok_or_else(|| failure("source path is missing"))?
                .to_owned();
            container.add_companion(&path, &bytes)?;
            for (path, bytes) in companions(&bytes)? {
                container.add_companion(&path, &bytes)?;
            }
            let component = next;
            let occurrence = next
                .checked_add(1)
                .ok_or_else(|| failure("id limit exceeded"))?;
            next = next
                .checked_add(2)
                .ok_or_else(|| failure("id limit exceeded"))?;
            let parent = fields.get("component").and_then(Value::as_u64).unwrap_or(0);
            let name = fields
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("Insert Design")
                .to_owned();
            fields.insert("component".into(), json!(component));
            components.push(json!({"id":component,"name":name,"parent":parent}));
            occurrences.push(json!({"id":occurrence,"component":component,"name":format!("{name}:1"),"parent":parent,"grounded":true,"transform":IDENTITY}));
            links.insert(
                component.to_string(),
                json!({"feature":fields.get("id"),"path":path,"file_sha256":linked.get("sha256")}),
            );
        }
    }
    if links.is_empty() {
        return Ok(());
    }
    for (key, extra) in [("components", components), ("occurrences", occurrences)] {
        if document.get(key).is_none() {
            document.insert(key.into(), json!([]));
        }
        document
            .get_mut(key)
            .and_then(Value::as_array_mut)
            .ok_or_else(|| failure("component structure is missing"))?
            .extend(extra);
    }
    document.insert("next_id".into(), json!(next));
    document.insert("links".into(), Value::Object(links));
    Ok(())
}

/// Companion paths are portable file names; no input can choose an output directory.
pub(crate) fn companions(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(failure)?;
    let mut files = Vec::new();
    let mut remaining = 256_u64 << 20;
    for i in 0..archive.len() {
        let mut member = archive.by_index(i).map_err(failure)?;
        let member_name = member.name().map_err(failure)?.into_owned();
        let Some(name) = member_name.strip_prefix("companions/") else {
            continue;
        };
        if !name.starts_with("derived-")
            || !name.ends_with(".sc")
            || name.len() != "derived-".len() + 64 + ".sc".len()
            || !name
                .strip_prefix("derived-")
                .and_then(|s| s.strip_suffix(".sc"))
                .is_some_and(|s| s.bytes().all(|b| b.is_ascii_hexdigit()))
            || member.size() > remaining
            || files.len() >= 1000
        {
            return Err(failure("companion names or sizes exceed the design limits"));
        }
        let mut data = Vec::new();
        member
            .by_ref()
            .take(remaining + 1)
            .read_to_end(&mut data)
            .map_err(failure)?;
        if data.len() as u64 > remaining {
            return Err(failure("companion data exceeds the design limit"));
        }
        remaining -= data.len() as u64;
        let digest: String = Sha256::digest(&data)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if name != format!("derived-{digest}.sc") {
            return Err(failure("a companion does not match its content identity"));
        }
        files.push((name.to_owned(), data));
    }
    Ok(files)
}
