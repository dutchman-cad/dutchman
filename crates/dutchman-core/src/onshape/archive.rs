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
}

fn failure(detail: impl std::fmt::Display) -> Error {
    Error::Decode(format!(
        "Onshape bundle: {detail}; fetch a new bundle and try again"
    ))
}

impl Bundle {
    pub fn open(bytes: &[u8]) -> Result<Self, Error> {
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
            let mut data = Vec::new();
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
        Ok(Self { members })
    }

    pub fn bytes(&self, path: &str) -> Result<&[u8], Error> {
        self.members
            .get(path)
            .map(Vec::as_slice)
            .ok_or_else(|| failure(format!("{path} is missing")))
    }

    pub fn json(&self, path: &str) -> Result<Value, Error> {
        serde_json::from_slice(self.bytes(path)?)
            .map_err(|_| failure(format!("{path} could not be read as JSON")))
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
