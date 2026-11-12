//! A final container placement, after source-coordinate mapping and checks.
use dutchman_core::Error;
use serde_json::{json, Value};
use std::io::{Cursor, Read, Write};
fn error(e: impl std::fmt::Display) -> Error {
    Error::Encode(format!("Cannot stand up the converted design: {e}"))
}
pub(crate) fn apply(bytes: &[u8]) -> Result<Vec<u8>, Error> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(error)?;
    let mut output = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(error)?;
        let name = file.name().map_err(error)?.into_owned();
        let mut data = Vec::new();
        file.read_to_end(&mut data).map_err(error)?;
        if name == "document.json" {
            let mut doc: Value = serde_json::from_slice(&data).map_err(error)?;
            wrap(&mut doc)?;
            data = serde_json::to_vec(&doc).map_err(error)?;
        }
        output
            .start_file(
                &name,
                zip::write::SimpleFileOptions::default().compression_method(file.compression()),
            )
            .map_err(error)?;
        output.write_all(&data).map_err(error)?;
    }
    Ok(output.finish().map_err(error)?.into_inner())
}
fn wrap(doc: &mut Value) -> Result<(), Error> {
    let root = doc
        .as_object_mut()
        .ok_or_else(|| error("Missing document"))?;
    let component = root
        .get("next_id")
        .and_then(Value::as_u64)
        .ok_or_else(|| error("Missing next id"))?;
    let occurrence = component
        .checked_add(1)
        .ok_or_else(|| error("Too many ids"))?;
    let next = occurrence
        .checked_add(1)
        .ok_or_else(|| error("Too many ids"))?;
    for c in root
        .get_mut("components")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if c.get("parent").and_then(Value::as_u64) == Some(0) {
            if let Some(fields) = c.as_object_mut() {
                fields.insert("parent".into(), json!(component));
            }
        }
    }
    for o in root
        .get_mut("occurrences")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if o.get("parent").and_then(Value::as_u64) == Some(0) {
            if let Some(fields) = o.as_object_mut() {
                fields.insert("parent".into(), json!(occurrence));
            }
        }
    }
    for f in root
        .get_mut("features")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        if f.get("component").and_then(Value::as_u64).unwrap_or(0) == 0 {
            if let Some(fields) = f.as_object_mut() {
                fields.insert("component".into(), json!(component));
            }
        }
    }
    // Occurrence visibility is addressed by paths from the root occurrence.
    if let Some(paths) = root
        .get_mut("visibility")
        .and_then(|v| v.get_mut("hidden_occurrences"))
        .and_then(Value::as_array_mut)
    {
        for path in paths {
            if let Some(path) = path.as_array_mut() {
                path.insert(0, json!(occurrence));
            }
        }
    }
    // Joint snaps are local to their endpoint parts, but endpoint occurrence paths
    // start at the design root and must include the new wrapper.
    if let Some(joints) = root
        .get_mut("assembly")
        .and_then(|v| v.get_mut("joints"))
        .and_then(Value::as_array_mut)
    {
        for joint in joints {
            for end in ["a", "b"] {
                if let Some(path) = joint
                    .get_mut(end)
                    .and_then(|v| v.get_mut("occurrence"))
                    .and_then(Value::as_array_mut)
                {
                    path.insert(0, json!(occurrence));
                }
            }
        }
    }
    root.entry("components")
        .or_insert_with(|| json!([]))
        .as_array_mut()
        .ok_or_else(|| error("Missing components"))?
        .push(json!({"id":component,"name":"Stand up","parent":0}));
    // SolidCraft uses column-major matrices: rotate +90 degrees about X (+Y -> +Z).
    root.entry("occurrences").or_insert_with(|| json!([])).as_array_mut().ok_or_else(|| error("Missing occurrences"))?
        .push(json!({"id":occurrence,"name":"Stand up:1","component":component,"parent":0,"grounded":true,
            "transform":[[1,0,0,0],[0,0,1,0],[0,-1,0,0],[0,0,0,1]]}));
    root.insert("next_id".into(), json!(next));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn placement_preserves_local_fillet_and_hidden_history_paths() {
        let fillet = json!({"id":7,"kind":"fillet","component":2,"edges":[{"point":[1,2,3]}],"radius":"2 mm"});
        let mut doc = json!({"next_id":8,"components":[{"id":2,"parent":0}],
            "occurrences":[{"id":3,"parent":0,"component":2}],"features":[fillet.clone()],
            "visibility":{"hidden_occurrences":[[3]],"hidden_sketches":[1]},"params":{"r":"2 mm"},
            "assembly":{"joints":[{"a":{"occurrence":[3],"snap":{"origin":[1,2,3]}},"b":{"occurrence":[]}}]}});
        wrap(&mut doc).unwrap();
        assert_eq!(doc["features"][0], fillet);
        assert_eq!(doc["visibility"]["hidden_occurrences"], json!([[9, 3]]));
        assert_eq!(doc["occurrences"][0]["parent"], 9);
        assert_eq!(doc["components"][0]["parent"], 8);
        assert_eq!(doc["params"]["r"], "2 mm");
        assert_eq!(
            doc["assembly"]["joints"][0]["a"]["occurrence"],
            json!([9, 3])
        );
        assert_eq!(doc["assembly"]["joints"][0]["b"]["occurrence"], json!([9]));
        assert_eq!(
            doc["assembly"]["joints"][0]["a"]["snap"]["origin"],
            json!([1, 2, 3])
        );
    }
}
