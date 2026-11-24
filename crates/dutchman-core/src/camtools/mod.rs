//! Fusion tool libraries to CamCraft's `.camtools`.
//!
//! Reads a Fusion tool library (`.tools`, a zip of JSON, or a `.json` export) and writes
//! CamCraft's tool library file, with a [`Report`] naming every tool and field CamCraft has no
//! place for. Nothing is dropped silently, and a tool CamCraft cannot represent is listed while
//! the rest of the library is still converted.
//!
//! # Sources
//!
//! The target format is CamCraft's own, read from its repository (Apache-2.0), commit
//! `c5bb09ed13f8b63e691d3a5ed6fb0be6a64ee2aa`: `crates/app/src/tool_library.rs` (the
//! `camcraft-tools` envelope, versions 1 to 4, the strict reader), `crates/app/src/project/file.rs`
//! (`ToolDto`, the tool record) and `crates/tools/src/tool_sanity.rs` (what CamCraft refuses). The
//! Fusion side is the structure of Fusion's exported libraries (tool `type`, `unit`, `geometry`,
//! `post-process`, `holder`, `start-values.presets`). No other tool-library converter was read.
//!
//! # Envelope
//!
//! `{"format": "camcraft-tools", "version": N, "units": "mm", "tools": [{"name", "definition"}]}`.
//! `version` is 1, 2 when a tool is a left-hand tap, 3 when a cutting row has a stepdown or
//! stepover (CamCraft's older readers refuse the newer numbers). Lengths are always millimetres.
//! CamCraft refuses a whole library for one tool it dislikes (an unknown kind, a name twice, a
//! tool that breaks its sanity rules), so this module checks each tool against those rules and
//! lists the tool instead of writing it.
//!
//! # Mapping
//!
//! Fusion `unit` is `millimeters` or `inches`; every length (and every feed, per minute) is
//! multiplied by 25.4 for inches. Any other unit makes that tool unconvertible.
//!
//! | Fusion field | CamCraft field | Notes |
//! |---|---|---|
//! | `description` | tool `name` | A tool with none is named from its type and diameter; a name used twice becomes `Name (2)`. |
//! | `type` | `kind` | See the type table below. |
//! | `geometry.DC` | `diameter` | Required. Turning tools take `INSD` or `IC` when there is no `DC`. |
//! | `geometry.LCF` | `flute_len` | Falls back to `LB`, then `OAL` (noted). Required. |
//! | `geometry.LB` / `OAL` | `shank_len` | The longer of the two, minus the flute length (0 when neither is longer). |
//! | `geometry.SFDM` | `shank_d` | Defaults to the diameter, except undercut cutters, which need a neck. |
//! | `geometry.RE` | `corner_r` (mills), `nose_radius` (bore, turning, form) | Ball nose: half the diameter. |
//! | `geometry.SIG` | `tip_angle` (drill point, countersink) | Drill default 118 degrees (approximation, reported). |
//! | `geometry.TA` | `tip_angle` | Chamfer mill: twice `TA` (the included angle). Dovetail: 90 - `TA`, the angle between flank and bottom face. |
//! | `geometry.TP` | `pitch` | Taps, thread mills. Threading insert width: 0.866 x `TP` when `W` is absent. |
//! | `geometry.thread-profile-angle` | `tip_angle` | Thread mill, threading insert; default 60 degrees. |
//! | `geometry.W` | `width` | Grooving blade, threading insert, form tool. |
//! | `geometry.NOF` | `flutes` | Only 1 to 32. |
//! | `BMC` | `tool_material` | `carbide` and `hss`; other words are noted and left unstated. |
//! | `product-id` | `part_number` | Cut to 80 characters. |
//! | `vendor` | `vendor` | Cut to 80 characters. |
//! | `post-process.number`, `product-link`, `post-process.comment` | `notes` | CamCraft's file stores no controller tool number, so it rides in the notes (`Fusion tool number 5`). |
//! | `holder.segments` | `holder_d`, `holder_len` | One cylinder: the widest segment diameter and the total height. A tool with no holder writes 0 and 0. |
//! | `start-values.presets[]`, turning | `turning`, `rpm`, `feed` | A preset with `v_c` and `f_n` and no `n` is constant surface speed: `turning.vc` (m/min; inch libraries give feet per minute) and `turning.feed_per_rev` (mm); 1000 rpm is the starting spindle speed. |
//! | `start-values.presets[]` | `rpm`, `feed`, `cutting_data` | The first preset for all materials (else the first preset) gives `rpm` (`n`) and `feed` (`v_f`, or `f_n` x `n`, or `f_z` x `NOF` x `n`). Other presets whose material category CamCraft has (`aluminum`, `steel`, `stainless steel`, `cast iron`, `brass`, `plastic`) become one cutting row each, with `stepdown` and `stepover` when the preset uses them. A tool with no usable preset gets placeholder 1000 rpm and 100 mm/min, reported as degraded. |
//!
//! | Fusion `type` | CamCraft `kind` |
//! |---|---|
//! | `flat end mill` | `end_mill` |
//! | `ball end mill` | `ball_nose` |
//! | `bull nose end mill` | `bull_nose` |
//! | `chamfer mill` | `taper` |
//! | `dovetail mill` | `dovetail` |
//! | `lollipop mill` | `lollipop` |
//! | `thread mill` | `thread_mill` |
//! | `drill` | `drill` |
//! | `tap right hand`, `tap left hand` | `tap` (left hand sets `thread_hand`) |
//! | `boring head`, `turning boring` | `bore` |
//! | `probe` | `probe` |
//! | `plasma cutter` | `torch` |
//! | `water jet cutter` | `waterjet` |
//! | `turning general`, `turning grooving`, `turning threading`, `turning form` | `turning`, `groove`, `thread`, `form` |
//! | `face mill` | `end_mill`, not centre cutting (degraded) |
//! | `slot mill` | `keyseat` (degraded) |
//! | `center drill`, `spot drill` | `drill` (degraded) |
//! | `counter sink` | `taper` with the countersink angle (degraded) |
//! | `form mill` | `thread_mill`, when `geometry.profile` draws rows of equal teeth at one pitch (pitch, angle, crest diameter and cutting length read from it); any other profile is listed, not written |
//! | `tapered mill` | `taper`, when its tip is a sharp point; a flat or ball tip is listed, not written |
//! | `radius mill`, `counter bore`, `reamer`, `laser cutter`, `marking`, `turning custom`, any other type | listed, not written |
//!
//! Holder entries (`type` `holder`) are not tools; they are counted in the report.
//!
//! # What the report names
//!
//! * Each tool not written, with the reason (`Dropped`).
//! * Each tool written as something close but not the same, or with placeholder cutting data
//!   (`Degraded`).
//! * Per tool, the Fusion fields that hold data CamCraft has no place for, such as plunge and ramp
//!   feeds, coolant, lead-in and lead-out feeds, tip diameter, shoulder length, offsets that
//!   differ from the tool number (`Info`). A field that is empty, zero, false or `disabled` is not
//!   reported.
//! * Choices that needed no decision, such as a derived shank length or an assembled holder
//!   (`Info`).
//!
//! Not reported, because they carry no tool data: `guid`, `reference_guid`, `expressions`,
//! `version`, `GRADE`, and the `name` and `description` labels of a preset.

mod map;
mod read;

use std::path::Path;

use serde_json::{json, Value};

pub use read::{is_library, Contents};

use crate::report::{Report, Severity};

/// The CamCraft commit this module's target format was read at.
pub const CAMCRAFT_COMMIT: &str = "c5bb09ed13f8b63e691d3a5ed6fb0be6a64ee2aa";

/// The envelope's `format` tag.
pub const FORMAT: &str = "camcraft-tools";

/// The suffix of the file written.
pub const EXTENSION: &str = "camtools";

/// What converting one library gave.
#[derive(Debug)]
pub struct Converted {
    /// The `.camtools` text; `None` when no tool could be converted (CamCraft refuses an empty
    /// library).
    pub text: Option<String>,
    /// Tool entries in the library (holders excluded).
    pub tools_read: usize,
    /// Holder entries in the library, which are not tools and are not written.
    pub holders: usize,
    /// Tools in the `.camtools`.
    pub tools_written: usize,
    /// Everything not carried across.
    pub report: Report,
}

/// Reads the Fusion tool library at `path` and converts it.
pub fn convert_path(path: &Path) -> Result<Converted, String> {
    Ok(convert(&read::read(path)?))
}

/// Converts the tools of a library already read.
pub fn convert(contents: &Contents) -> Converted {
    let mut report = Report::default();
    let mut tools: Vec<Value> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut version = 1;
    for (index, (_, raw)) in contents.tools.iter().enumerate() {
        let mapped = map::map_tool(raw, index);
        let subject = format!("tool \"{}\"", mapped.name);
        let code = |suffix: &str| format!("camtools/tool.{suffix}");
        match &mapped.result {
            Err(why) => {
                report.push_coded(
                    Severity::Dropped,
                    subject,
                    &code("not-represented"),
                    format!("Not written to the .camtools: {why}."),
                );
                continue;
            }
            Ok(def) => {
                let mut name = mapped.name.clone();
                if names.contains(&name) {
                    let mut n = 2;
                    while names.contains(&format!("{} ({n})", mapped.name)) {
                        n += 1;
                    }
                    name = format!("{} ({n})", mapped.name);
                    report.push_coded(
                        Severity::Info,
                        subject.clone(),
                        &code("renamed"),
                        format!("The name is used by an earlier tool; CamCraft needs unique names, so it is written as \"{name}\"."),
                    );
                }
                version = version.max(def.version());
                names.push(name.clone());
                tools.push(json!({"name": name, "definition": def.to_json()}));
            }
        }
        if !mapped.approximations.is_empty() {
            report.push_coded(
                Severity::Degraded,
                subject.clone(),
                &code("approximated"),
                format!("{}.", mapped.approximations.join("; ")),
            );
        }
        if !mapped.not_carried.is_empty() {
            report.push_coded(
                Severity::Info,
                subject.clone(),
                &code("field-not-carried"),
                format!(
                    "CamCraft has no place for these Fusion fields, so they are not carried: {}.",
                    mapped.not_carried.join(", ")
                ),
            );
        }
        if !mapped.notes.is_empty() {
            report.push_coded(
                Severity::Info,
                subject,
                &code("note"),
                format!("{}.", mapped.notes.join("; ")),
            );
        }
    }
    if contents.holders > 0 {
        report.push_coded(
            Severity::Info,
            "tool holders",
            "camtools/tool.holders-not-converted",
            format!(
                "{} holder {} in the library: holders are not tools, so they are not written (a tool's own holder is written as a cylinder).",
                contents.holders,
                if contents.holders == 1 { "entry" } else { "entries" }
            ),
        );
    }
    let text = (!tools.is_empty())
        .then(|| {
            serde_json::to_string_pretty(&json!({
                "format": FORMAT,
                "version": version,
                "units": "mm",
                "tools": tools,
            }))
            .ok()
        })
        .flatten();
    Converted {
        text,
        tools_read: contents.tools.len(),
        holders: contents.holders,
        tools_written: tools.len(),
        report,
    }
}

#[cfg(test)]
mod tests;
