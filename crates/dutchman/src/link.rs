//! The link record written beside a converted design: which source file, and which of its bytes,
//! the output was converted from, and how.
//!
//! `<output stem>.link.json` lets a later run tell that the source changed (`dutchman sync`,
//! the desktop app's "changed since last convert" list) and lets SolidCraft find the source
//! again. It is written by the same code that writes the design, whichever way the conversion
//! was started.

use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::{watch, ConvertOptions, OutputFormat};

/// The record's schema.
pub const SCHEMA: &str = "dutchman.link/1";

/// Where the record of the design `output` is written: its file name with `.link.json` in place
/// of its suffix.
pub fn path_for(output: &Path) -> PathBuf {
    output.with_extension("link.json")
}

/// What a source file was when its conversion began. Taken before converting, so a file saved
/// while the conversion runs is seen as changed afterwards and not recorded as converted.
#[derive(Clone, Debug)]
pub struct Source {
    sha256: Option<String>,
    size: Option<u64>,
    modified: Option<SystemTime>,
    /// When the file was read: a save in the same second as `modified` can leave size and time
    /// as they were, so a stamp is only trusted when it is older than this by a margin.
    read_at: SystemTime,
}

impl Source {
    /// Reads `path`. What cannot be read (or is not one file) is left out of the record.
    pub fn read(path: &Path) -> Self {
        // A file over the input limit is refused by the conversion; hashing it first would only
        // make a refusal wait on reading gigabytes.
        let hashable = std::fs::metadata(path)
            .is_ok_and(|meta| meta.is_file() && meta.len() <= dutchman_core::MAX_INPUT_BYTES);
        Self::with_digest(path, hashable.then(|| watch::hash(path).ok()).flatten())
    }

    /// `path` with a digest already taken of its bytes.
    pub fn with_digest(path: &Path, sha256: impl Into<Option<String>>) -> Self {
        let meta = std::fs::metadata(path).ok().filter(|meta| meta.is_file());
        Self {
            sha256: sha256.into().filter(|_| meta.is_some()),
            size: meta.as_ref().map(std::fs::Metadata::len),
            modified: meta.and_then(|meta| meta.modified().ok()),
            read_at: SystemTime::now(),
        }
    }

    /// A source whose size and time `meta` gave, taken before `sha256` was.
    pub fn with_metadata(sha256: String, meta: &std::fs::Metadata) -> Self {
        Self {
            sha256: Some(sha256),
            size: Some(meta.len()),
            modified: meta.modified().ok(),
            read_at: SystemTime::now(),
        }
    }
}

/// Whether `meta`, the source as it is now, has the size and time `record` names, and those were
/// taken long enough after the file was last saved that a later save could not share them.
/// `true` lets a caller skip reading the whole file.
pub fn stamp_unchanged(record: &Value, meta: &std::fs::Metadata) -> bool {
    let Some(source) = record.get("source") else {
        return false;
    };
    let (Some(size), Some(modified), Some(read_at)) = (
        source.get("size").and_then(Value::as_u64),
        source.get("modified").and_then(Value::as_str),
        source.get("read_at").and_then(Value::as_str),
    ) else {
        return false;
    };
    let Ok(now) = meta.modified() else {
        return false;
    };
    let margin = std::time::Duration::from_secs(2);
    size == meta.len()
        && modified == rfc3339(now)
        && now
            .checked_add(margin)
            .is_some_and(|safe| rfc3339(safe).as_str() <= read_at)
}

/// The source as it is before converting, unless no record will be written.
pub(crate) fn snapshot(input: &Path, options: &ConvertOptions) -> Option<Source> {
    (!options.no_link_record).then(|| Source::read(input))
}

/// Writes the record of a successful conversion beside its output and names it in `report`
/// (`link_record`, the file name); a record that cannot be written is reported as
/// `link_record_error`. A failed conversion, or one that wrote no file, leaves any existing
/// record as it was.
pub(crate) fn write_for_report(
    input: &Path,
    output: &Path,
    options: &ConvertOptions,
    source: Option<&Source>,
    report: &mut Value,
) {
    if options.no_link_record
        || report.get("ok").and_then(Value::as_bool) != Some(true)
        || !output.is_file()
    {
        return;
    }
    let owned;
    let source = match source {
        Some(source) => source,
        None => {
            owned = Source::read(input);
            &owned
        }
    };
    let path = path_for(output);
    let result = record(input, output, options, report, source)
        .and_then(|record| write_file(&path, &record).map(|()| path));
    if let Some(fields) = report.as_object_mut() {
        match result {
            Ok(path) => {
                let name = path.file_name().map(|name| name.to_string_lossy());
                fields.insert("link_record".to_owned(), json!(name));
            }
            Err(why) => {
                fields.insert("link_record_error".to_owned(), json!(why));
            }
        }
    }
}

/// Writes `record` to `path` atomically.
pub fn write_file(path: &Path, record: &Value) -> Result<(), String> {
    let mut text = serde_json::to_string_pretty(record).map_err(|err| err.to_string())?;
    text.push('\n');
    watch::atomic_write(path, text.as_bytes())
        .map_err(|err| format!("cannot write {}: {err}", path.display()))
}

/// The record of `output`, converted from `input` (as it was, `source`) with `options`.
pub fn record(
    input: &Path,
    output: &Path,
    options: &ConvertOptions,
    report: &Value,
    source: &Source,
) -> Result<Value, String> {
    let output_digest = watch::hash(output)
        .map_err(|err| format!("cannot read {} to record it: {err}", output.display()))?;
    let folder = std::path::absolute(output)
        .map_err(|err| err.to_string())?
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    let file_name = output
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or("the output has no file name")?;
    let mut dutchman = json!({"version": env!("CARGO_PKG_VERSION")});
    if let (Some(commit), Some(fields)) = (option_env!("DUTCHMAN_COMMIT"), dutchman.as_object_mut())
    {
        fields.insert("commit".to_owned(), json!(commit));
    }
    Ok(json!({
        "schema": SCHEMA,
        "source": {
            "path": source_path(&folder, input)?,
            "sha256": source.sha256,
            "size": source.size,
            "modified": source.modified.map(rfc3339),
            "read_at": rfc3339(source.read_at),
        },
        "output": {"file": file_name, "sha256": output_digest},
        "dutchman": dutchman,
        "cadmpeg": {"version": crate::anon::CADMPEG_VERSION},
        "options": options_json(options, report, &folder)?,
        "converted_at": rfc3339(SystemTime::now()),
    }))
}

/// The options that change what is written, and the folders part files are searched in (named
/// like the source: relative to `folder`, else absolute), so a later run can convert the same way.
fn options_json(options: &ConvertOptions, report: &Value, folder: &Path) -> Result<Value, String> {
    let choices = options.choices();
    let target = options.solidcraft_version.map_or_else(
        || {
            report
                .pointer("/solidcraft/target")
                .and_then(Value::as_str)
                .map(str::to_owned)
        },
        |version| Some(version.to_string()),
    );
    let project = options
        .project
        .as_deref()
        .map(|path| source_path(folder, path))
        .transpose()?;
    let search = options
        .search
        .iter()
        .map(|path| source_path(folder, path))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({
        "to": match options.to {
            OutputFormat::Solidcraft => "solidcraft",
            OutputFormat::Freecad => "freecad",
            OutputFormat::Fcstd => "fcstd",
            OutputFormat::Dxf => "dxf",
        },
        "preset": options.preset.name(),
        "history": choices.history,
        "sketches": choices.sketches,
        "joints": choices.joints,
        "part_positions": options.effective_part_positions().name(),
        "stand_up": options.stand_up_choice(),
        "solidcraft_version": target,
        "part": options.part.map(|part| json!({
            "number": part.number,
            "children": part.children,
            "stop_at_feature": part.stop_at_feature,
        })),
        "project": project,
        "search": search,
    }))
}

/// The options `record` was converted with, to convert its source again the same way. Folders it
/// names relatively are taken from `folder`, the record's own folder.
pub fn options_from_record(record: &Value, folder: &Path) -> Result<ConvertOptions, String> {
    use clap::ValueEnum;
    let recorded = record.get("options").ok_or("the record has no options")?;
    let text = |key: &str| recorded.get(key).and_then(Value::as_str);
    let flag = |key: &str| recorded.get(key).and_then(Value::as_bool);
    let mut options = ConvertOptions {
        to: match text("to") {
            Some("solidcraft") | None => OutputFormat::Solidcraft,
            Some("freecad") => OutputFormat::Freecad,
            Some("fcstd") => OutputFormat::Fcstd,
            Some("dxf") => return Err("DXF pages cannot be kept up to date".to_owned()),
            Some(other) => return Err(format!("unknown output format {other:?} in the record")),
        },
        ..ConvertOptions::default()
    };
    if let Some(name) = text("preset") {
        options.preset = crate::choices::Preset::from_str(name, true)
            .map_err(|_| format!("unknown preset {name:?} in the record"))?;
    }
    // A switch is only needed where it narrows the preset.
    let preset = crate::choices::Choices::of(options.preset);
    options.import_only = preset.history && flag("history") == Some(false);
    options.no_sketches = preset.sketches && flag("sketches") == Some(false);
    options.no_joints = preset.joints && flag("joints") == Some(false);
    if let Some(name) = text("part_positions") {
        options.part_positions = name.parse()?;
    }
    match flag("stand_up") {
        Some(true) => options.stand_up = true,
        Some(false) => options.no_stand_up = true,
        None => {}
    }
    if let Some(version) = text("solidcraft_version") {
        options.solidcraft_version =
            Some(version.parse().map_err(|err| {
                format!("bad SolidCraft version {version:?} in the record: {err}")
            })?);
    }
    let folder_path = |name: &str| {
        let path = Path::new(name);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            folder.join(path)
        }
    };
    options.project = text("project").map(folder_path);
    options.search = recorded
        .get("search")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(folder_path)
        .collect();
    options.part = recorded
        .get("part")
        .filter(|part| part.is_object())
        .map(|part| {
            Ok::<_, String>(dutchman_core::PartSelection {
                number: part
                    .get("number")
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or("the record's part has no number")?,
                children: part.get("children").and_then(Value::as_bool) == Some(true),
                stop_at_feature: part
                    .get("stop_at_feature")
                    .and_then(Value::as_u64)
                    .and_then(|n| usize::try_from(n).ok()),
            })
        })
        .transpose()?;
    Ok(options)
}

/// `source` as the record names it: relative to `folder` (the output's folder) with `/`
/// separators when the two share more than the root, otherwise absolute.
fn source_path(folder: &Path, source: &Path) -> Result<String, String> {
    let source = std::path::absolute(source).map_err(|err| err.to_string())?;
    Ok(relative(folder, &source).unwrap_or_else(|| source.to_string_lossy().into_owned()))
}

fn relative(folder: &Path, target: &Path) -> Option<String> {
    let from: Vec<Component<'_>> = normal(folder);
    let to: Vec<Component<'_>> = normal(target);
    let shared = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    // Only a root (or a drive) in common: a relative path would name the whole machine.
    let root = from
        .iter()
        .take_while(|c| matches!(c, Component::Prefix(_) | Component::RootDir))
        .count();
    if shared <= root {
        return None;
    }
    let mut parts: Vec<String> = vec!["..".to_owned(); from.len().saturating_sub(shared)];
    parts.extend(
        to.iter()
            .skip(shared)
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    Some(parts.join("/"))
}

/// The components of an absolute path, `.` and `..` already applied.
fn normal(path: &Path) -> Vec<Component<'_>> {
    let mut parts: Vec<Component<'_>> = Vec::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(parts.last(), Some(Component::Normal(_))) {
                    parts.pop();
                }
            }
            other => parts.push(other),
        }
    }
    parts
}

/// `time` as an RFC 3339 UTC timestamp, to the second.
pub(crate) fn rfc3339(time: SystemTime) -> String {
    let seconds = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(since) => i64::try_from(since.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    };
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn a_file_over_the_input_limit_is_not_hashed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("huge.step");
        std::fs::File::create(&path)
            .unwrap()
            .set_len(dutchman_core::MAX_INPUT_BYTES + 1)
            .unwrap();
        let source = Source::read(&path);
        assert_eq!(source.sha256, None);
        assert_eq!(source.size, Some(dutchman_core::MAX_INPUT_BYTES + 1));
    }

    #[test]
    fn timestamps_are_rfc_3339_utc() {
        let at = |seconds| SystemTime::UNIX_EPOCH + Duration::from_secs(seconds);
        assert_eq!(rfc3339(at(0)), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(at(951_782_400)), "2000-02-29T00:00:00Z");
        assert_eq!(rfc3339(at(1_700_000_000)), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn paths_are_relative_when_they_share_more_than_the_root() {
        let rel = |a: &str, b: &str| relative(Path::new(a), Path::new(b));
        assert_eq!(rel("/work/out", "/work/in/a.f3d").unwrap(), "../in/a.f3d");
        assert_eq!(rel("/work/out", "/work/out/a.f3d").unwrap(), "a.f3d");
        assert_eq!(rel("/work/out", "/other/a.f3d"), None);
        assert_eq!(rel("/work/out", "/work/out/x/../a.f3d").unwrap(), "a.f3d");
    }

    #[test]
    fn the_record_sits_beside_the_output() {
        assert_eq!(
            path_for(Path::new("/o/part.sc")),
            Path::new("/o/part.link.json")
        );
    }
}
