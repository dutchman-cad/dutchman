//! A bounded, in-memory bundle. Member names are never extracted to disk.

use std::collections::BTreeMap;
use std::io::{Cursor, Read};

use serde_json::Value;

use crate::Error;

pub(super) const SCHEMA: &str = "dutchman.onshape-bundle/1";
const MEMBERS: usize = 4096;
const JSON_BYTES: u64 = 32 << 20;
const EXPANDED_BYTES: u64 = 256 << 20;

pub(super) struct Bundle {
    members: BTreeMap<String, Vec<u8>>,
    json: BTreeMap<String, Value>,
}

fn failure(detail: impl std::fmt::Display) -> Error {
    Error::Decode(format!(
        "Onshape bundle: {detail}; fetch a new bundle and try again"
    ))
}

impl Bundle {
    pub fn open(
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        bytes: &[u8],
    ) -> Result<Self, Error> {
        if !bytes.starts_with(b"PK\x03\x04") {
            return Err(failure("the contents do not have ZIP bundle magic"));
        }
        directory(bytes).ok_or_else(|| {
            failure("the ZIP directory exceeds the bundle limits or cannot be read")
        })?;
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(failure)?;
        if zip.len() > MEMBERS {
            return Err(failure("the member count exceeds the bundle limit"));
        }
        let mut members = BTreeMap::new();
        let mut remaining = EXPANDED_BYTES;
        for i in 0..zip.len() {
            let mut member = zip.by_index(i).map_err(failure)?;
            let name = member.name().map_err(failure)?.into_owned();
            if name.contains('\\')
                || name.split('/').any(|part| {
                    part.is_empty() || part == "." || part == ".." || part.contains(':')
                })
                || member.is_dir()
                || !matches!(
                    member.compression(),
                    zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated
                )
            {
                return Err(failure(
                    "a member name or compression method is not supported yet",
                ));
            }
            let limit = if name.ends_with(".json") {
                JSON_BYTES.min(remaining)
            } else {
                remaining
            };
            if member.size() > limit {
                return Err(failure("the expanded data exceeds the bundle limit"));
            }
            ctx.retain_bytes(member.size(), "Onshape expanded member")
                .map_err(|error| Error::Decode(error.to_string()))?;
            let mut data = Vec::new();
            data.try_reserve(usize::try_from(member.size()).map_err(failure)?)
                .map_err(failure)?;
            member
                .by_ref()
                .take(limit + 1)
                .read_to_end(&mut data)
                .map_err(failure)?;
            if data.len() as u64 > limit {
                return Err(failure("the expanded data exceeds the bundle limit"));
            }
            remaining = remaining.saturating_sub(data.len() as u64);
            if members.insert(name, data).is_some() {
                return Err(failure("a member name occurs more than once"));
            }
        }
        let parse = |path: &str| -> Result<Value, Error> {
            let data = members
                .get(path)
                .ok_or_else(|| failure(format!("{path} is missing")))?;
            let text = std::str::from_utf8(data).map_err(failure)?;
            ctx.parse_json_value(text, "Onshape JSON member")
                .map_err(|error| Error::Decode(error.to_string()))
        };
        let manifest = parse("manifest.json")?;
        let prefix = manifest
            .pointer("/elements/0/id")
            .and_then(Value::as_str)
            .map(|id| format!("partstudios/{id}/"));
        let mut json = BTreeMap::from([("manifest.json".to_owned(), manifest)]);
        for name in members.keys() {
            if prefix
                .as_deref()
                .is_some_and(|prefix| schema_response(name, prefix))
            {
                json.insert(name.clone(), parse(name)?);
            }
        }
        Ok(Self { members, json })
    }

    pub fn bytes(&self, path: &str) -> Result<&[u8], Error> {
        self.members
            .get(path)
            .map(Vec::as_slice)
            .ok_or_else(|| failure(format!("{path} is missing")))
    }

    pub fn contains(&self, path: &str) -> bool {
        self.members.contains_key(path)
    }

    pub(super) fn json(&self, path: &str) -> Result<&Value, Error> {
        self.json
            .get(path)
            .ok_or_else(|| failure(format!("{path} is missing")))
    }
    pub(super) fn validate_microversions(&self, expected: &str) -> Result<(), Error> {
        for value in self.json.values() {
            for key in ["sourceMicroversion", "microversionId"] {
                super::microversion(value, key, expected).map_err(Error::Decode)?;
            }
        }
        Ok(())
    }
    pub(super) fn admit_topology(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<(), Error> {
        let mut count = 0usize;
        for (path, response) in &self.json {
            if !path.contains("/bodydetails/") {
                continue;
            }
            let Some(bodies) = response.get("bodies").and_then(Value::as_array) else {
                continue;
            };
            let mut add = |length: usize| -> Result<(), Error> {
                if length > super::ROWS {
                    return Err(failure("topology exceeds the row limit"));
                }
                count = count
                    .checked_add(length)
                    .ok_or_else(|| failure("topology count overflow"))?;
                Ok(())
            };
            add(bodies.len())?;
            for body in bodies {
                for key in ["faces", "edges", "vertices"] {
                    if let Some(rows) = body.get(key).and_then(Value::as_array) {
                        add(rows.len())?;
                    }
                }
                if let Some(faces) = body.get("faces").and_then(Value::as_array) {
                    for face in faces {
                        if let Some(loops) = face.get("loops").and_then(Value::as_array) {
                            add(loops.len())?;
                            for boundary in loops {
                                if let Some(coedges) =
                                    boundary.get("coedges").and_then(Value::as_array)
                                {
                                    add(coedges.len())?;
                                }
                            }
                        }
                    }
                }
            }
        }
        ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(count),
            "Onshape topology admission",
        )
        .map_err(|error| Error::Decode(error.to_string()))
    }
}

// Bound the directory before zip allocates it. The container policy matches the
// shared upload guard, without making the format-neutral core depend on HTTP code.
fn directory(bytes: &[u8]) -> Option<()> {
    let start = bytes.len().saturating_sub(65_633);
    let tail = bytes.get(start..)?;
    let offset = tail.windows(4).rposition(|w| w == b"PK\x05\x06")?;
    let end = tail.get(offset..)?;
    let word = |at: usize| {
        Some(u16::from_le_bytes(
            end.get(at..at.checked_add(2)?)?.try_into().ok()?,
        ))
    };
    let dword = |at: usize| {
        Some(u32::from_le_bytes(
            end.get(at..at.checked_add(4)?)?.try_into().ok()?,
        ))
    };
    if end.len() != 22usize.checked_add(usize::from(word(20)?))? || word(4)? != 0 || word(6)? != 0 {
        return None;
    }
    let locator = offset.checked_sub(20).and_then(|at| tail.get(at..offset));
    let (members, size, position) =
        if let Some(locator) = locator.filter(|l| l.starts_with(b"PK\x06\x07")) {
            let disk = u32::from_le_bytes(locator.get(4..8)?.try_into().ok()?);
            let at = u64::from_le_bytes(locator.get(8..16)?.try_into().ok()?);
            let disks = u32::from_le_bytes(locator.get(16..20)?.try_into().ok()?);
            if disk != 0 || disks != 1 {
                return None;
            }
            let at = usize::try_from(at).ok()?;
            let record = bytes.get(at..at.checked_add(56)?)?;
            if !record.starts_with(b"PK\x06\x06") || record.get(16..24)? != [0; 8] {
                return None;
            }
            let value = |at: usize| {
                Some(u64::from_le_bytes(
                    record.get(at..at.checked_add(8)?)?.try_into().ok()?,
                ))
            };
            let size = value(4)?;
            if !(44..=16 << 20).contains(&size)
                || at
                    .checked_add(12)?
                    .checked_add(usize::try_from(size).ok()?)?
                    > bytes.len()
                || value(24)? != value(32)?
            {
                return None;
            }
            (value(32)?, value(40)?, value(48)?)
        } else {
            if word(8)? != word(10)? {
                return None;
            }
            (
                u64::from(word(10)?),
                u64::from(dword(12)?),
                u64::from(dword(16)?),
            )
        };
    if members > MEMBERS as u64
        || size > 16 << 20
        || position.checked_add(size)? > bytes.len() as u64
    {
        return None;
    }
    Some(())
}

// Only declared responses of the selected studio are JSON. Other attachments stay bytes.
fn schema_response(name: &str, prefix: &str) -> bool {
    let Some(member) = name.strip_prefix(prefix) else {
        return false;
    };
    if matches!(
        member,
        "features.json" | "configuration.json" | "variables.json" | "bodydetails/index.json"
    ) {
        return true;
    }
    member
        .strip_prefix("bodydetails/")
        .and_then(|name| name.rsplit_once('.'))
        .is_some_and(|(ordinal, extension)| {
            extension == "json"
                && !ordinal.is_empty()
                && ordinal.bytes().all(|b| b.is_ascii_digit())
        })
}
