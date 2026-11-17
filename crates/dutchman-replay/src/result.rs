use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Identifier of the runtime result JSON schema.
pub const SCHEMA: &str = "dutchman.script-result/1";
/// Validated result data supplied by an emitter runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ScriptResult {
    /// Versioned schema identifier.
    pub schema: String,
    /// Runtime operation kind; this schema records rebuilds.
    pub kind: ResultKind,
    /// Target identifier.
    pub target: String,
    /// Version reported by the target program.
    pub program_version: String,
    /// Runtime script version.
    pub script_version: String,
    /// SHA-256 of the serialized plan including its trailing newline.
    pub plan_sha256: String,
    /// Reported comparison verdict.
    pub verdict: Verdict,
    /// Reported comparison check.
    pub check: ResultCheck,
    /// Number of lowered steps.
    pub steps_total: u32,
    /// Number of completed steps.
    pub steps_done: u32,
    /// Optional first failed operation.
    pub failed: Option<Failed>,
    /// Optional rebuilt measurements.
    pub measured: Option<Measured>,
    /// Optional original geometry measurements.
    pub original: Option<Measured>,
    /// Finite nonnegative elapsed seconds.
    pub seconds: f64,
}
/// The first failed runtime operation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Failed {
    /// Step record number.
    pub line: u32,
    /// Operation identifier.
    pub op: String,
    /// Feature kind being rebuilt.
    pub step_kind: String,
    /// Plain failure reason.
    pub message: String,
}
/// Measured solid counts, sizes and bounds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Measured {
    /// Number of solid bodies.
    pub bodies: u32,
    /// Finite nonnegative volume in cubic millimetres.
    pub volume_mm3: f64,
    /// Finite nonnegative area in square millimetres.
    pub area_mm2: f64,
    /// Minimum and maximum part-coordinate corners.
    pub bbox_mm: [[f64; 3]; 2],
}
fn error(field: &str) -> String {
    format!("That result file isn't one Dutchman can read: {field}.")
}
impl ScriptResult {
    /// Read and validate bounded result JSON without exposing parser diagnostics.
    pub fn read(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > 65_536 {
            return Err(error("size"));
        }
        let value: Value = serde_json::from_slice(bytes).map_err(|_| error("JSON"))?;
        let obj = value.as_object().ok_or_else(|| error("object"))?;
        let mut pending = vec![(&value, 0usize)];
        while let Some((value, depth)) = pending.pop() {
            if depth > 64 {
                return Err(error("nesting"));
            }
            match value {
                Value::Array(a) => pending.extend(a.iter().map(|v| (v, depth + 1))),
                Value::Object(o) => {
                    if o.keys().any(|key| key.chars().any(forbidden)) {
                        return Err(error("text"));
                    }
                    pending.extend(o.values().map(|v| (v, depth + 1)));
                }
                Value::String(text) if text.chars().any(forbidden) => return Err(error("text")),
                _ => {}
            }
        }
        // Option fields are required too: a missing measurement must be an explicit null.
        for field in [
            "schema",
            "kind",
            "target",
            "program_version",
            "script_version",
            "plan_sha256",
            "verdict",
            "check",
            "steps_total",
            "steps_done",
            "failed",
            "measured",
            "original",
            "seconds",
        ] {
            if !obj.contains_key(field) {
                return Err(error(field));
            }
        }
        // Check each known field separately so diagnostics name the field, not JSON internals.
        for field in [
            "schema",
            "kind",
            "target",
            "program_version",
            "script_version",
            "plan_sha256",
            "verdict",
            "check",
        ] {
            let text = obj
                .get(field)
                .and_then(Value::as_str)
                .ok_or_else(|| error(field))?;
            if text.chars().count() > 2_000 {
                return Err(error(field));
            }
        }
        for field in ["steps_total", "steps_done"] {
            if !obj
                .get(field)
                .and_then(Value::as_u64)
                .is_some_and(|n| u32::try_from(n).is_ok())
            {
                return Err(error(field));
            }
        }
        for field in ["measured", "original"] {
            if let Some(v) = obj.get(field).filter(|v| !v.is_null()) {
                if !v.is_object() {
                    return Err(error(field));
                }
                let m = serde_json::from_value::<Measured>(v.clone()).map_err(|_| error(field))?;
                if !m.volume_mm3.is_finite()
                    || m.volume_mm3 < 0.0
                    || !m.area_mm2.is_finite()
                    || m.area_mm2 < 0.0
                    || !m.bbox_mm.iter().flatten().all(|n| n.is_finite())
                {
                    return Err(error(field));
                }
            }
        }
        if let Some(v) = obj.get("failed").filter(|v| !v.is_null()) {
            if !v.is_object() {
                return Err(error("failed"));
            }
            serde_json::from_value::<Failed>(v.clone()).map_err(|_| error("failed"))?;
        }
        if obj.get("seconds").and_then(Value::as_f64).is_none() {
            return Err(error("seconds"));
        }
        for (field, valid) in [
            (
                "kind",
                serde_json::from_value::<ResultKind>(
                    obj.get("kind").cloned().unwrap_or(Value::Null),
                )
                .is_ok(),
            ),
            (
                "verdict",
                serde_json::from_value::<Verdict>(
                    obj.get("verdict").cloned().unwrap_or(Value::Null),
                )
                .is_ok(),
            ),
            (
                "check",
                serde_json::from_value::<ResultCheck>(
                    obj.get("check").cloned().unwrap_or(Value::Null),
                )
                .is_ok(),
            ),
        ] {
            if !valid {
                return Err(error(field));
            }
        }
        let result: Self = serde_json::from_slice(bytes).map_err(|_| error("fields"))?;
        if let Some(failed) = &result.failed {
            for (field, text) in [
                ("failed.op", &failed.op),
                ("failed.step_kind", &failed.step_kind),
                ("failed.message", &failed.message),
            ] {
                if text.chars().count() > 2_000 {
                    return Err(error(field));
                }
            }
        }
        if result.schema != SCHEMA {
            return Err(error("schema"));
        }
        if result.plan_sha256.len() != 64
            || !result
                .plan_sha256
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        {
            return Err(error("plan_sha256"));
        }
        if result.steps_done > result.steps_total {
            return Err(error("steps_done"));
        }
        if result.seconds < 0.0 || !result.seconds.is_finite() {
            return Err(error("seconds"));
        }
        Ok(result)
    }
}

fn forbidden(c: char) -> bool {
    c.is_control()
        || matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}
/// Result operation implemented by this schema.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResultKind {
    /// Rebuild result.
    Rebuild,
}
/// Reported comparison verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Verdict {
    /// Comparison passed.
    Pass,
    /// Comparison failed.
    Fail,
}
/// Reported check; this crate transports it without performing a comparison.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ResultCheck {
    /// All geometry comparisons passed.
    Passed,
    /// Body count differs.
    Bodies,
    /// Volume differs.
    Volume,
    /// Area differs.
    Area,
    /// Bounds differ.
    Bbox,
    /// No comparison ran.
    NotRun,
}
