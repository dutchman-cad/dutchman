//! Reading a Fusion tool library: a `.tools` file (a zip holding JSON) or a `.json` export.
//!
//! The reader is tolerant about shape (`{"data": [...]}`, a bare array, or one tool) and
//! strict about nothing it does not need: each tool stays a `serde_json::Value`, and the
//! mapping reads only the fields it carries. Fields it does not read are named in the report.

use std::io::Read;
use std::path::Path;

use serde_json::Value;

/// The largest library, or archive member, read: a real library is kilobytes to a few MiB.
const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// The most members a `.tools` archive may hold.
const MAX_MEMBERS: usize = 1000;

/// What a library file holds: its tools and the entries that are not tools (holders).
#[derive(Debug, Default)]
pub struct Contents {
    /// Every tool entry, in file order, each with the file it came from.
    pub tools: Vec<(String, Value)>,
    /// How many entries were holders, which CamCraft keeps in its own holder library.
    pub holders: usize,
}

/// Whether `path` is a Fusion tool library this module reads: any `.tools` file, and a `.json`
/// file whose tools look like Fusion's (a `data` array of objects with a `type` and a `unit`
/// or `geometry`).
pub fn is_library(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    if ext.eq_ignore_ascii_case("tools") {
        return path.is_file();
    }
    if !ext.eq_ignore_ascii_case("json") {
        return false;
    }
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file.take(MAX_BYTES).read_to_end(&mut bytes).is_err() {
        return false;
    }
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .is_some_and(|root| looks_like_fusion(&root))
}

fn looks_like_fusion(root: &Value) -> bool {
    let entries = match root {
        Value::Object(o) => match o.get("data") {
            Some(Value::Array(a)) => a.as_slice(),
            _ => return false,
        },
        Value::Array(a) => a.as_slice(),
        _ => return false,
    };
    !entries.is_empty()
        && entries.iter().all(|e| {
            e.as_object().is_some_and(|o| {
                o.get("type").is_some_and(Value::is_string)
                    && (o.contains_key("geometry") || o.contains_key("unit"))
            })
        })
}

/// Reads the library at `path`.
pub fn read(path: &Path) -> Result<Contents, String> {
    let mut file =
        std::fs::File::open(path).map_err(|e| format!("cannot open the library: {e}"))?;
    let mut magic = [0u8; 4];
    let n = file
        .read(&mut magic)
        .map_err(|e| format!("cannot read the library: {e}"))?;
    let zipped = magic.get(..n.min(2)) == Some(b"PK".as_slice());
    drop(file);
    let mut contents = Contents::default();
    if zipped {
        read_archive(path, &mut contents)?;
    } else {
        let mut text = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(MAX_BYTES + 1).read_to_end(&mut text))
            .map_err(|e| format!("cannot read the library: {e}"))?;
        if text.len() as u64 > MAX_BYTES {
            return Err("the library is over 64 MiB; it is not a tool library".into());
        }
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        read_json(&text, &name, &mut contents)?;
    }
    if contents.tools.is_empty() && contents.holders == 0 {
        return Err("the library holds no tools".into());
    }
    Ok(contents)
}

fn read_archive(path: &Path, into: &mut Contents) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot open the library: {e}"))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|e| format!("the .tools file is not a readable archive: {e}"))?;
    if zip.len() > MAX_MEMBERS {
        return Err(format!(
            "the .tools archive holds {} members; a library holds a few",
            zip.len()
        ));
    }
    let mut names: Vec<(usize, String)> = Vec::new();
    for index in 0..zip.len() {
        let member = zip
            .by_index(index)
            .map_err(|e| format!("the .tools archive is damaged: {e}"))?;
        let name = member
            .name()
            .map_or_else(|_| String::new(), |n| n.into_owned());
        if member.is_file() && name.to_ascii_lowercase().ends_with(".json") {
            names.push((index, name));
        }
    }
    names.sort_by(|a, b| a.1.cmp(&b.1));
    if names.is_empty() {
        return Err("the .tools archive holds no JSON library".into());
    }
    for (index, name) in names {
        let member = zip
            .by_index(index)
            .map_err(|e| format!("the .tools archive is damaged: {e}"))?;
        let mut text = Vec::new();
        member
            .take(MAX_BYTES + 1)
            .read_to_end(&mut text)
            .map_err(|e| format!("cannot read {name} in the archive: {e}"))?;
        if text.len() as u64 > MAX_BYTES {
            return Err(format!(
                "{name} in the archive is over 64 MiB; it is not a tool library"
            ));
        }
        read_json(&text, &name, into)?;
    }
    Ok(())
}

/// Adds the tools of one JSON document.
pub(super) fn read_json(text: &[u8], source: &str, into: &mut Contents) -> Result<(), String> {
    let root: Value =
        serde_json::from_slice(text).map_err(|e| format!("{source} is not valid JSON: {e}"))?;
    let entries: Vec<Value> = match root {
        Value::Object(mut o) => match o.remove("data") {
            Some(Value::Array(a)) => a,
            Some(_) => return Err(format!("{source}: \"data\" is not a list of tools")),
            None if o.contains_key("type") => vec![Value::Object(o)],
            None => {
                return Err(format!(
                    "{source} is not a Fusion tool library (no \"data\" list)"
                ))
            }
        },
        Value::Array(a) => a,
        _ => return Err(format!("{source} is not a Fusion tool library")),
    };
    for entry in entries {
        if !entry.is_object() {
            return Err(format!("{source}: a library entry is not an object"));
        }
        if entry.get("type").and_then(Value::as_str) == Some("holder") {
            into.holders += 1;
        } else {
            into.tools.push((source.to_owned(), entry));
        }
    }
    Ok(())
}
