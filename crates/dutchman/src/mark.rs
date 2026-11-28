//! The "made by Dutchman" record every `.sc` Dutchman writes carries, and how it is read back.
//!
//! The record is its own entry in the design's ZIP, `dutchman.json`. SolidCraft 0.49.1 keeps extra
//! ZIP entries when it opens and saves a design, and opens one without a notice; it drops unknown
//! keys of `document.json`, and a `generator` key in `container.json` makes it say "Saved by a
//! newer SolidCraft". `container.json` is therefore left exactly as SolidCraft writes it.
//! Reading the mark opens the ZIP's central directory and that one entry: nothing is decoded.
//!
//! TODO(solidcraft#22282): move the record into `container.json` as `generator` once SolidCraft
//! reserves that key and stops warning about it; keep reading `dutchman.json` for older designs.
//!
//! It says which Dutchman, cadmpeg and SolidCraft target made the design, from which source (its
//! name, the SHA-256 of its content, and a SHA-256 of its full path, so a shared design does not
//! carry the path), with which options, and when. `document_crc32` is the checksum of the
//! `document.json` Dutchman wrote: a design SolidCraft has saved since has a different one, so a
//! design edited after conversion is never taken for an untouched output.

use std::io::{Read, Write};
use std::path::Path;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// The ZIP entry that holds the mark.
const MARK: &str = "dutchman.json";
const DOCUMENT: &str = "document.json";
/// The most of the mark that is read.
const MAX_CONTAINER: u64 = 1 << 20;
/// The most of a saved report that is read to find its mark.
const MAX_REPORT: u64 = 64 << 20;

/// What Dutchman records about the conversion that made a design.
#[derive(Clone, Debug)]
pub struct Made {
    /// The source file's name (no folders).
    pub source_name: String,
    /// SHA-256 of the source file's content.
    pub source_sha256: String,
    /// SHA-256 of the source's absolute path.
    pub source_path_sha256: String,
    /// The conversion options that shape the design, as a JSON object.
    pub options: Value,
    /// The SolidCraft release the design targets.
    pub target: String,
}

/// The mark a design carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mark {
    pub source_path_sha256: String,
    pub source_sha256: String,
    /// Whether `document.json` is still the one Dutchman wrote.
    pub untouched: bool,
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// SHA-256 of the full path of `path`, as the mark records a source's place.
pub fn path_digest(path: &Path) -> String {
    let full = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    hex(&Sha256::digest(full.to_string_lossy().as_bytes()))
}

impl Made {
    /// What the conversion of `input` with `options` records; `None` when `input` cannot be read.
    pub fn of(input: &Path, options: Value, target: String) -> Option<Made> {
        let mut file = std::fs::File::open(input).ok()?;
        let mut digest = Sha256::new();
        let mut buffer = vec![0_u8; 1 << 20];
        loop {
            let n = file.read(&mut buffer).ok()?;
            if n == 0 {
                break;
            }
            digest.update(buffer.get(..n)?);
        }
        Some(Made {
            source_name: input.file_name()?.to_string_lossy().into_owned(),
            source_sha256: hex(&digest.finalize()),
            source_path_sha256: path_digest(input),
            options,
            target,
        })
    }
}

/// `design` (a `.sc` container) with the mark added as the entry `dutchman.json`. Every other
/// entry, `container.json` included, is copied as stored. `Err` when `design` is not a container.
pub fn stamp(design: &[u8], made: &Made) -> Result<Vec<u8>, String> {
    let mut source =
        zip::ZipArchive::new(std::io::Cursor::new(design)).map_err(|e| e.to_string())?;
    let document_crc = source.by_name(DOCUMENT).map_err(|e| e.to_string())?.crc32();
    let record = json!({
        "generator": "Dutchman",
        "version": env!("CARGO_PKG_VERSION"),
        "cadmpeg": crate::anon::CADMPEG_VERSION,
        "solidcraft_target": made.target,
        "source": {
            "name": made.source_name,
            "sha256": made.source_sha256,
            "path_sha256": made.source_path_sha256,
        },
        "options": made.options,
        "document_crc32": format!("{document_crc:08x}"),
    });
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::with_capacity(
        design.len() + 1024,
    )));
    for index in 0..source.len() {
        let entry = source.by_index_raw(index).map_err(|e| e.to_string())?;
        if entry.name().is_ok_and(|name| name == MARK) {
            continue;
        }
        out.raw_copy_file(entry).map_err(|e| e.to_string())?;
    }
    out.start_file(
        MARK,
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored),
    )
    .map_err(|e| e.to_string())?;
    out.write_all(&serde_json::to_vec(&record).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

/// The mark on the design at `path`, or `None` when it has none or cannot be read as a design.
pub fn read(path: &Path) -> Option<Mark> {
    let mut archive =
        zip::ZipArchive::new(std::io::BufReader::new(std::fs::File::open(path).ok()?)).ok()?;
    let document_crc = archive.by_name(DOCUMENT).ok()?.crc32();
    let mut text = Vec::new();
    archive
        .by_name(MARK)
        .ok()?
        .take(MAX_CONTAINER)
        .read_to_end(&mut text)
        .ok()?;
    let record: Value = serde_json::from_slice(&text).ok()?;
    if record.get("generator")?.as_str()? != "Dutchman" {
        return None;
    }
    let source = record.get("source")?;
    Some(Mark {
        source_path_sha256: source.get("path_sha256")?.as_str()?.to_owned(),
        source_sha256: source.get("sha256")?.as_str()?.to_owned(),
        untouched: record.get("document_crc32")?.as_str()? == format!("{document_crc:08x}"),
    })
}

/// Whether the file at `existing` is a design Dutchman made from `input` and nothing has
/// changed since: the one a new conversion of `input` may replace.
pub fn made_from(existing: &Path, input: &Path) -> bool {
    read(existing)
        .is_some_and(|mark| mark.untouched && mark.source_path_sha256 == path_digest(input))
}

/// The line that says a design was numbered: `Toob v2.sc already existed and wasn't made from
/// this file, so saved as Toob v2 (2).sc`.
pub fn numbered_note(requested: &Path, saved: &Path) -> String {
    let name = |path: &Path| {
        path.file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    };
    format!(
        "{} already existed and wasn't made from this file, so saved as {}",
        name(requested),
        name(saved)
    )
}

/// The mark a saved report carries: who made it and from which source (by path digest).
pub fn report_mark(input: &Path) -> Value {
    json!({
        "name": "Dutchman",
        "version": env!("CARGO_PKG_VERSION"),
        "source": {"path_sha256": path_digest(input)},
    })
}

/// Whether the report at `path` is one Dutchman saved for `input`.
fn report_made_from(path: &Path, input: &Path) -> bool {
    let Ok(file) = std::fs::File::open(path) else {
        return false;
    };
    let mut text = Vec::new();
    if file.take(MAX_REPORT).read_to_end(&mut text).is_err() {
        return false;
    }
    serde_json::from_slice::<Value>(&text).is_ok_and(|report| {
        report.pointer("/generator/name").and_then(Value::as_str) == Some("Dutchman")
            && report
                .pointer("/generator/source/path_sha256")
                .and_then(Value::as_str)
                == Some(path_digest(input).as_str())
    })
}

/// What is already in the output slot of `output` (the design and the reports beside it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot {
    /// Nothing is there.
    Free,
    /// Only what Dutchman made from `input`, and nothing has changed since: a new conversion
    /// of `input` takes the slot.
    Ours,
    /// Something Dutchman did not make from `input` is there: it is never touched.
    Taken,
}

/// The state of the slot of `output` for a conversion of `input`.
pub fn slot(output: &Path, input: &Path) -> Slot {
    let design = output.try_exists().unwrap_or(true);
    let mut any = design;
    let mut ours = !design || made_from(output, input);
    for suffix in ["report.json", "failed.json", "replay.json"] {
        let side = output.with_extension(suffix);
        if side.try_exists().unwrap_or(true) {
            any = true;
            // A replay script has no mark of its own: it belongs to a design that is ours.
            ours &= if suffix == "replay.json" {
                design && ours
            } else {
                report_made_from(&side, input)
            };
        }
    }
    match (any, ours) {
        (false, _) => Slot::Free,
        (true, true) => Slot::Ours,
        (true, false) => Slot::Taken,
    }
}

/// Where a conversion of `input` writes the design planned at `output`, and whether that
/// replaces a design Dutchman made from `input`: `output` itself when its slot is free or ours,
/// else the next free numbered name (a numbered design made from `input` is taken again too, so
/// converting twice does not pile up copies). With `replace` set, `output` is always used.
pub fn place(input: &Path, output: &Path, replace: bool) -> (std::path::PathBuf, bool) {
    if replace {
        return (output.to_path_buf(), true);
    }
    let taken = |path: &Path| slot(path, input) == Slot::Taken;
    let path = dutchman_core::safe_output::next_free(output, &taken);
    let replaces = slot(&path, input) == Slot::Ours;
    (path, replaces)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const CONTAINER: &str = "container.json";

    fn design(document: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file(DOCUMENT, stored).unwrap();
        zip.write_all(document.as_bytes()).unwrap();
        zip.start_file(CONTAINER, stored).unwrap();
        zip.write_all(br#"{"format":"solidcraft-container/1"}"#)
            .unwrap();
        zip.start_file("blobs/a.step", stored).unwrap();
        zip.write_all(b"STEP").unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn made(source: &Path) -> Made {
        Made::of(source, json!({"preset": "standard"}), "0.49".into()).unwrap()
    }

    #[test]
    fn a_stamped_design_names_its_source_and_keeps_its_entries() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("Toob v2.f3z");
        std::fs::write(&source, b"source bytes").unwrap();
        let stamped = stamp(&design("{}"), &made(&source)).unwrap();
        let path = dir.path().join("Toob v2.sc");
        std::fs::write(&path, &stamped).unwrap();
        let mark = read(&path).unwrap();
        assert!(mark.untouched);
        assert_eq!(mark.source_path_sha256, path_digest(&source));
        assert_eq!(mark.source_sha256, hex(&Sha256::digest(b"source bytes")));
        assert!(made_from(&path, &source));
        assert!(!made_from(&path, &dir.path().join("other.f3z")));
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(stamped)).unwrap();
        let mut blob = String::new();
        archive
            .by_name("blobs/a.step")
            .unwrap()
            .read_to_string(&mut blob)
            .unwrap();
        assert_eq!(blob, "STEP");
        // container.json is exactly as it was: SolidCraft warns about a key it does not know.
        let mut text = String::new();
        archive
            .by_name(CONTAINER)
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, r#"{"format":"solidcraft-container/1"}"#);
        let mut record = String::new();
        archive
            .by_name(MARK)
            .unwrap()
            .read_to_string(&mut record)
            .unwrap();
        let container: Value = serde_json::from_str(&record).unwrap();
        assert_eq!(container["generator"], "Dutchman");
        assert_eq!(container["source"]["name"], "Toob v2.f3z");
    }

    #[test]
    fn an_old_or_foreign_or_edited_design_is_not_ours_to_replace() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("a.f3d");
        std::fs::write(&source, b"x").unwrap();
        // No mark (an output of an older Dutchman, or a design made by hand).
        let plain = dir.path().join("plain.sc");
        std::fs::write(&plain, design("{}")).unwrap();
        assert_eq!(read(&plain), None);
        assert!(!made_from(&plain, &source));
        // Not a design at all.
        let text = dir.path().join("text.sc");
        std::fs::write(&text, b"my notes").unwrap();
        assert!(!made_from(&text, &source));
        assert!(!made_from(&dir.path().join("missing.sc"), &source));
        // The document changed after Dutchman wrote it: SolidCraft saved it, or the user did.
        let stamped = stamp(&design("{}"), &made(&source)).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(stamped.clone())).unwrap();
        let mut container = String::new();
        archive
            .by_name(MARK)
            .unwrap()
            .read_to_string(&mut container)
            .unwrap();
        let mut edited = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        edited.start_file(DOCUMENT, stored).unwrap();
        edited.write_all(br#"{"edited":true}"#).unwrap();
        edited.start_file(MARK, stored).unwrap();
        edited.write_all(container.as_bytes()).unwrap();
        let path = dir.path().join("edited.sc");
        std::fs::write(&path, edited.finish().unwrap().into_inner()).unwrap();
        assert!(!read(&path).unwrap().untouched);
        assert!(!made_from(&path, &source));
    }
}
