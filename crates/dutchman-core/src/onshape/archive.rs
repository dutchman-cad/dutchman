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
    failures: BTreeMap<String, String>,
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
        let mut failures = BTreeMap::new();
        let mut remaining = EXPANDED_BYTES;
        for i in 0..zip.len() {
            let mut member = match zip.by_index(i) {
                Ok(member) => member,
                Err(error) => {
                    failures.insert(format!("member {i}"), error.to_string());
                    continue;
                }
            };
            let name = match member.name() {
                Ok(name) => name.into_owned(),
                Err(error) => {
                    failures.insert(format!("member {i}"), error.to_string());
                    continue;
                }
            };
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
                failures.insert(
                    name,
                    "the member name or compression method is not supported yet".into(),
                );
                continue;
            }
            let limit = if name.ends_with(".json") {
                JSON_BYTES.min(remaining)
            } else {
                remaining
            };
            if member.size() > remaining {
                return Err(failure("the expanded data exceeds the bundle limit"));
            }
            if member.size() > limit {
                failures.insert(name, "the expanded data exceeds the JSON size limit".into());
                continue;
            }
            ctx.retain_bytes(member.size(), "Onshape expanded member")
                .map_err(crate::decode::onshape_failure)?;
            let mut data = Vec::new();
            data.try_reserve(usize::try_from(member.size()).map_err(failure)?)
                .map_err(failure)?;
            if let Err(error) = member.by_ref().take(limit + 1).read_to_end(&mut data) {
                failures.insert(name, format!("the member cannot be read ({error})"));
                continue;
            }
            if data.len() as u64 > limit {
                if data.len() as u64 > remaining {
                    return Err(failure("the expanded data exceeds the bundle limit"));
                }
                failures.insert(name, "the expanded data exceeds the JSON size limit".into());
                continue;
            }
            remaining = remaining.saturating_sub(data.len() as u64);
            if failures.contains_key(&name) || members.contains_key(&name) {
                members.remove(&name);
                failures.insert(name, "the member name occurs more than once".into());
            } else {
                members.insert(name, data);
            }
        }
        for name in failures.keys() {
            members.remove(name);
        }
        if let Some(reason) = failures.get("manifest.json") {
            return Err(failure(format!("manifest.json: {reason}")));
        }
        let parse = |path: &str| -> Result<Value, Error> {
            let data = members
                .get(path)
                .ok_or_else(|| failure(format!("{path} is missing")))?;
            let text = std::str::from_utf8(data).map_err(failure)?;
            ctx.parse_json_value(text, "Onshape JSON member")
                .map_err(crate::decode::onshape_failure)
        };
        let manifest = parse("manifest.json")?;
        let mut json = BTreeMap::from([("manifest.json".to_owned(), manifest)]);
        // Admit captured manifests first, then only the responses owned by each
        // manifest's selected studio. Unrelated attachments remain opaque.
        for name in members.keys().filter(|name| reference_manifest(name)) {
            match parse(name) {
                Ok(value) => {
                    json.insert(name.clone(), value);
                }
                Err(error) => {
                    if let Some(resource) = ctx.resource_refusal() {
                        return Err(Error::Decode(format!(
                            "{error} during {}",
                            resource.operation
                        )));
                    }
                    failures.insert(name.clone(), error.to_string());
                }
            }
        }
        let prefixes: Vec<_> = json
            .iter()
            .filter_map(|(path, manifest)| {
                let folder = path.strip_suffix("manifest.json")?;
                let element = manifest
                    .get("root_element")
                    .or_else(|| manifest.pointer("/elements/0/id"))?
                    .as_str()?;
                if element.is_empty()
                    || !element
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                {
                    return None;
                }
                Some(format!("{folder}partstudios/{element}/"))
            })
            .collect();
        for name in members.keys() {
            if prefixes.iter().any(|prefix| schema_response(name, prefix)) {
                // Syntax is member-local. Resource refusals retain the caller's budget.
                let data = members
                    .get(name)
                    .ok_or_else(|| failure("the member is missing"))?;
                let parsed = match std::str::from_utf8(data) {
                    Ok(text) => match ctx.parse_json_value(text, "Onshape JSON member") {
                        Ok(value) => Ok(value),
                        Err(error) => {
                            if let Some(resource) = ctx.resource_refusal() {
                                return Err(Error::Decode(format!(
                                    "{error} during {}",
                                    resource.operation
                                )));
                            }
                            Err(error.to_string())
                        }
                    },
                    Err(error) => Err(error.to_string()),
                };
                match parsed {
                    Ok(value) => {
                        json.insert(name.clone(), value);
                    }
                    Err(reason) => {
                        failures.insert(name.clone(), reason);
                    }
                }
            }
        }
        Ok(Self {
            members,
            json,
            failures,
        })
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
    pub(super) fn validate_microversions(&mut self, expected: &str) {
        // References have independent pins. The nearest enclosing manifest owns
        // each member; never validate a referenced response against the host pin.
        let pins: BTreeMap<_, _> = self
            .json
            .iter()
            .filter_map(|(path, value)| {
                let folder = path.strip_suffix("manifest.json")?;
                let mid = value.get("document")?.get("microversion")?.as_str()?;
                Some((folder.to_owned(), mid.to_owned()))
            })
            .collect();
        self.json.retain(|path, value| {
            let expected = if let Some((folder, _)) = path.rsplit_once("partstudios/") {
                if folder.is_empty() {
                    expected
                } else if let Some(mid) = pins.get(folder) {
                    mid.as_str()
                } else {
                    self.failures.insert(
                        path.clone(),
                        "the referenced studio microversion is missing; fetch a new bundle".into(),
                    );
                    return false;
                }
            } else {
                expected
            };
            for key in ["sourceMicroversion", "microversionId"] {
                if let Err(reason) = super::microversion(value, key, expected) {
                    self.failures.insert(path.clone(), reason);
                    return false;
                }
            }
            true
        });
    }
    pub(super) fn admit_records(&mut self) {
        for (path, response) in &mut self.json {
            if path.ends_with("bodydetails/index.json") {
                let entries = response
                    .as_array()
                    .or_else(|| response.get("indices").and_then(Value::as_array));
                if entries.is_none_or(|rows| {
                    let mut seen = std::collections::BTreeSet::new();
                    rows.len() > super::ROWS
                        || rows.iter().any(|row| {
                            let ordinal = row
                                .as_u64()
                                .and_then(|value| usize::try_from(value).ok())
                                .or_else(|| {
                                    row.as_str()?.strip_suffix(".json")?.parse::<usize>().ok()
                                });
                            ordinal.is_none_or(|ordinal| !seen.insert(ordinal))
                        })
                }) {
                    self.failures.insert(
                        path.clone(),
                        "the rollback index list is missing or not supported yet".into(),
                    );
                }
            }
            if path.contains("/bodydetails/") && !path.ends_with("/index.json") {
                if let Some(bodies) = response.get_mut("bodies").and_then(Value::as_array_mut) {
                    for (current, body) in bodies.iter().enumerate() {
                        let supported = super::text(body, "id").is_ok()
                            && ["faces", "edges", "vertices"]
                                .into_iter()
                                .all(|key| super::optional_rows(body, key).is_ok());
                        if !supported {
                            self.failures.insert(
                                format!("{path} body {current}"),
                                "the body topology tables are missing or exceed the row limit"
                                    .into(),
                            );
                        }
                        // Keep the unadmitted body as unknown evidence. Dropping it
                        // could falsely prove absence or turn a multi-body scope into one.
                    }
                } else {
                    self.failures.insert(
                        path.clone(),
                        "the body topology list is missing or not supported yet".into(),
                    );
                }
            }
            if path.ends_with("/sketches.json") {
                if let Some(sketches) = response.get_mut("sketches").and_then(Value::as_array_mut) {
                    let mut ordinal = 0usize;
                    let mut seen = std::collections::BTreeSet::new();
                    sketches.retain(|sketch| {
                        let current = ordinal;
                        ordinal = ordinal.saturating_add(1);
                        let native = super::text(sketch, "featureId");
                        let supported = native.is_ok();
                        if native.is_ok_and(|native| !seen.insert(native.to_owned())) {
                            self.failures.insert(
                                format!("{path} sketch {current}"),
                                "the sketch information repeats a feature id".into(),
                            );
                        }
                        if !supported {
                            self.failures.insert(
                                format!("{path} sketch {current}"),
                                "the sketch information has no feature id".into(),
                            );
                        }
                        supported
                    });
                } else {
                    self.failures.insert(
                        path.clone(),
                        "the sketch information list is missing or not supported yet".into(),
                    );
                }
            }
        }
    }
    pub(super) fn report_losses(&self, report: &mut crate::Report) {
        for (path, reason) in &self.failures {
            report.push_coded(
                crate::Severity::Degraded,
                path,
                "onshape/member.not-supported",
                format!("{reason}; skipped this record; fetch a new bundle to recover it"),
            );
        }
    }
    pub(super) fn admit_topology(
        &mut self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    ) -> Result<(), Error> {
        let mut count = 0usize;
        let mut refused = Vec::new();
        for (path, response) in &self.json {
            let before = count;
            let local = (|| -> Result<(), Error> {
                if path.ends_with("/sketches.json") {
                    if let Some(sketches) = response.get("sketches").and_then(Value::as_array) {
                        if sketches.len() > super::ROWS {
                            return Err(failure("sketch information exceeds the row limit"));
                        }
                        count = count
                            .checked_add(sketches.len())
                            .ok_or_else(|| failure("sketch information count overflow"))?;
                    }
                }
                if !path.contains("/bodydetails/") {
                    return Ok(());
                }
                let Some(bodies) = response.get("bodies").and_then(Value::as_array) else {
                    return Ok(());
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
                Ok(())
            })();
            if let Err(error) = local {
                count = before;
                refused.push((path.clone(), error.to_string()));
            }
        }
        for (path, reason) in refused {
            self.json.remove(&path);
            self.failures.insert(path, reason);
        }
        ctx.charge_entities(
            cadmpeg_core::decode::u64_from_index(count),
            "Onshape topology admission",
        )
        .map_err(crate::decode::onshape_failure)
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
// Captures are nested under complete immutable document/version/element paths.
// Keep the same eight-studio ceiling as reference traversal, including the root.
fn reference_manifest(name: &str) -> bool {
    let mut remaining = name;
    for _ in 0..7 {
        let Some(path) = remaining.strip_prefix("references/") else {
            return false;
        };
        let mut parts = path.splitn(4, '/');
        for _ in 0..3 {
            if !parts
                .next()
                .is_some_and(|id| id.len() == 24 && id.bytes().all(|b| b.is_ascii_hexdigit()))
            {
                return false;
            }
        }
        let Some(tail) = parts.next() else {
            return false;
        };
        if tail == "manifest.json" {
            return true;
        }
        remaining = tail;
    }
    false
}

fn schema_response(name: &str, prefix: &str) -> bool {
    let Some(member) = name.strip_prefix(prefix) else {
        return false;
    };
    if matches!(
        member,
        "features.json"
            | "sketches.json"
            | "configuration.json"
            | "variables.json"
            | "bodydetails/index.json"
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

#[cfg(test)]
mod tests {
    use super::reference_manifest;

    #[test]
    fn captured_manifest_admission_bounds_paths_and_nesting() {
        let segment = format!(
            "references/{}/{}/{}/",
            "1".repeat(24),
            "2".repeat(24),
            "3".repeat(24)
        );
        for depth in 1..=8 {
            assert_eq!(
                reference_manifest(&format!("{}manifest.json", segment.repeat(depth))),
                depth <= 7
            );
        }
        for path in [
            "manifest.json",
            "attachments/manifest.json",
            "references/short/short/short/manifest.json",
            "references/../manifest.json",
        ] {
            assert!(!reference_manifest(path));
        }
        assert!(!reference_manifest(&format!(
            "{segment}attachments/manifest.json"
        )));
    }
}
