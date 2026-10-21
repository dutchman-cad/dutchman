//! The machine-readable reports `--json` prints. Their field names are stable: fields are
//! added, never renamed or removed. See `docs/json.md`.

use std::path::Path;

use dutchman_core::{
    Check, Completeness, Error, FeatureStatus, Format, Mode, Outcome, Report, Severity, Stage,
};
use serde_json::{json, Map, Value};

/// The schema of `dutchman convert --json`.
pub const CONVERT: &str = "dutchman.convert/1";

/// The schema of `dutchman inspect --json`.
pub const INSPECT: &str = "dutchman.inspect/1";

/// The input file as the reports describe it.
pub fn input(path: &Path) -> Value {
    let format = Format::from_path(path);
    json!({
        "path": path.display().to_string(),
        "format": format.map(Format::id),
        "format_name": format.map(Format::name),
    })
}

/// One loss of a report as the text reports print it, `LEVEL: SUBJECT: MESSAGE`, with the
/// message cut to its first sentence unless `verbose`; and whether it was cut.
pub fn loss_line(loss: &Value, verbose: bool) -> (String, bool) {
    let field = |name: &str| loss.get(name).and_then(Value::as_str).unwrap_or("");
    let (message, cut) = if verbose {
        (field("message").to_owned(), false)
    } else {
        first_sentence(field("message"))
    };
    (
        format!("{}: {}: {message}", field("level"), field("subject")),
        cut,
    )
}

/// The first sentence of `text`, marked with "…" when more followed; and whether it was cut.
/// A sentence ends at ". " before a capital letter, so "e.g. so" does not end one.
pub fn first_sentence(text: &str) -> (String, bool) {
    let bytes = text.as_bytes();
    let end = text.match_indices(". ").map(|(at, _)| at).find(|at| {
        bytes
            .get(at + 2)
            .is_some_and(|next| next.is_ascii_uppercase())
    });
    match end {
        Some(at) => (format!("{} …", text.get(..=at).unwrap_or(text)), true),
        None => (text.to_owned(), false),
    }
}

/// What the text reports say once when they cut a note short.
pub const CUT_NOTE: &str = "notes are cut to their first sentence; --verbose prints them in full";

/// A report of a run that stopped with `message`, at the input stage (the file, or how it
/// was asked for).
pub fn failure(schema: &str, input: Value, message: &str) -> Value {
    failure_at(schema, input, message, Stage::Input, None)
}

/// A report of a run that stopped with `error`.
pub fn error(schema: &str, input: Value, error: &Error) -> Value {
    failure_at(
        schema,
        input,
        &error.to_string(),
        error.stage(),
        error.upstream(),
    )
}

/// A report of a run that stopped with `message` at `stage`, in `upstream`'s code when it is
/// not Dutchman's own.
pub fn failure_at(
    schema: &str,
    input: Value,
    message: &str,
    stage: Stage,
    upstream: Option<&str>,
) -> Value {
    json!({
        "schema": schema,
        "ok": false,
        "input": input,
        "error": {"message": message, "stage": stage.id(), "upstream": upstream},
    })
}

/// The fields every successful report shares: what was (or would be) written, and the losses.
pub fn success(schema: &str, input: Value, report: &Report) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("schema".to_owned(), json!(schema));
    out.insert("ok".to_owned(), json!(true));
    out.insert("input".to_owned(), input);
    if let Some(outcome) = report.outcome() {
        out.extend(outcome_fields(outcome));
    }
    if let Some(completeness) = report.completeness() {
        out.insert("completeness".to_owned(), completeness_json(completeness));
    }
    out.insert("losses".to_owned(), losses(report));
    out
}

/// How much of an assembly was placed, as the reports give it.
pub fn completeness_json(completeness: &Completeness) -> Value {
    let left_out: Vec<Value> = completeness
        .left_out
        .iter()
        .map(|group| {
            json!({
                "file": group.file,
                "reason": group.reason,
                "stage": group.stage.id(),
                "count": group.count,
            })
        })
        .collect();
    json!({
        "placed": completeness.placed,
        "placements": completeness.placements(),
        "left_out": left_out,
    })
}

/// The most causes [`completeness_lines`] lists.
const CAUSES_SHOWN: usize = 10;

/// A report's completeness in words, when it has one: `assembly: 18 of 20 placements placed;
/// 2 left out:`, then a line per cause, `  pin.ipt ×2: not found …`.
pub fn completeness_lines(report: &Value) -> Vec<String> {
    let Some(completeness) = report.get("completeness") else {
        return Vec::new();
    };
    let count = |key: &str| completeness.get(key).and_then(Value::as_u64).unwrap_or(0);
    let (placed, placements) = (count("placed"), count("placements"));
    let causes: Vec<&Value> = completeness
        .get("left_out")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    if causes.is_empty() {
        return vec![format!("assembly: all {placements} placements placed")];
    }
    let mut lines = vec![format!(
        "assembly: {placed} of {placements} placements placed; {} left out:",
        placements.saturating_sub(placed)
    )];
    for cause in causes.iter().take(CAUSES_SHOWN) {
        let field = |key: &str| cause.get(key).and_then(Value::as_str);
        let times = cause.get("count").and_then(Value::as_u64).unwrap_or(1);
        let file = field("file").unwrap_or("(no file)");
        let reason = field("reason").unwrap_or("");
        // The reason usually starts with the file's name; say it once.
        let reason = reason
            .strip_prefix(file)
            .and_then(|rest| rest.strip_prefix(": "))
            .unwrap_or(reason);
        lines.push(format!("  {file} ×{times}: {reason}"));
    }
    if causes.len() > CAUSES_SHOWN {
        lines.push(format!(
            "  and {} more causes (--json lists them all)",
            causes.len() - CAUSES_SHOWN
        ));
    }
    lines
}

fn outcome_fields(outcome: &Outcome) -> Map<String, Value> {
    let (status, reason) = match &outcome.check {
        Check::Verified => ("verified", None),
        Check::Unverified(reason) => ("unverified", Some(reason)),
        Check::Flattened(reason) => ("flattened", Some(reason)),
        Check::NotApplicable(reason) => ("not_applicable", Some(reason)),
    };
    let features: Vec<Value> = outcome
        .features
        .iter()
        .map(|feature| {
            json!({
                "name": feature.name,
                "kind": feature.kind,
                "status": match feature.status {
                    FeatureStatus::Mapped => "mapped",
                    FeatureStatus::NotNeeded => "not_needed",
                    FeatureStatus::LeftOut => "left_out",
                    FeatureStatus::Blocked => "blocked",
                },
                "reason": feature.reason,
            })
        })
        .collect();
    let counted = |f: &&dutchman_core::FeatureOutcome| f.status != FeatureStatus::LeftOut;
    let mapped = outcome
        .features
        .iter()
        .filter(|f| matches!(f.status, FeatureStatus::Mapped | FeatureStatus::NotNeeded))
        .count();
    let total = outcome.features.iter().filter(counted).count();
    let mut out = Map::new();
    out.insert(
        "mode".to_owned(),
        json!(match outcome.mode {
            Mode::History => "history",
            Mode::Import => "import",
        }),
    );
    out.insert(
        "verification".to_owned(),
        json!({"status": status, "reason": reason}),
    );
    out.insert(
        "history".to_owned(),
        json!({"mapped": mapped, "total": total, "features": features}),
    );
    out
}

pub fn losses(report: &Report) -> Value {
    report
        .losses()
        .iter()
        .map(|loss| {
            json!({
                "level": match loss.severity {
                    Severity::Info => "Info",
                    Severity::Degraded => "Degraded",
                    Severity::Dropped => "Dropped",
                },
                "code": loss.code,
                "subject": loss.subject,
                "message": loss.detail,
                "stage": loss.stage.id(),
                "upstream": loss.upstream(),
            })
        })
        .collect()
}

// Test helpers outside `#[test]` functions are not covered by clippy.toml's test allowances.
#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use dutchman_core::FeatureOutcome;

    use super::*;

    #[test]
    fn an_assembly_says_how_much_of_it_was_placed() {
        let completeness = Completeness {
            placed: 18,
            left_out: vec![dutchman_core::LeftOutGroup {
                file: Some("pin.ipt".to_owned()),
                reason: "pin.ipt: not found at its stored path".to_owned(),
                stage: Stage::Input,
                count: 2,
            }],
        };
        let report = json!({"completeness": completeness_json(&completeness)});
        assert_eq!(report["completeness"]["placements"], 20);
        assert_eq!(report["completeness"]["left_out"][0]["stage"], "input");
        assert_eq!(
            completeness_lines(&report),
            [
                "assembly: 18 of 20 placements placed; 2 left out:",
                "  pin.ipt ×2: not found at its stored path",
            ]
        );
        let whole = Completeness {
            placed: 3,
            left_out: Vec::new(),
        };
        assert_eq!(
            completeness_lines(&json!({"completeness": completeness_json(&whole)})),
            ["assembly: all 3 placements placed"]
        );
        assert!(completeness_lines(&json!({})).is_empty());
    }

    fn feature(kind: &str, status: FeatureStatus) -> FeatureOutcome {
        FeatureOutcome {
            name: Some(format!("{kind}1")),
            kind: kind.to_owned(),
            status,
            reason: None,
        }
    }

    #[test]
    fn a_note_is_cut_at_its_first_sentence() {
        let long = "the document carries no declaration, so nothing was verified. The document \
                    is read on the residual path. Agreement is consistency.";
        assert_eq!(
            first_sentence(long),
            (
                "the document carries no declaration, so nothing was verified. …".to_owned(),
                true
            )
        );
        let short = "1 record(s) were not written to STEP: configuration=1";
        assert_eq!(first_sentence(short), (short.to_owned(), false));
        let abbreviated = "a frame, e.g. the origin. Then more";
        assert_eq!(first_sentence(abbreviated).0, "a frame, e.g. the origin. …");
    }

    #[test]
    fn a_written_history_reports_its_features_and_check() {
        let mut report = Report::default();
        report.set_outcome(Outcome {
            mode: Mode::History,
            check: Check::Verified,
            features: vec![
                feature("datum_principal_plane", FeatureStatus::NotNeeded),
                feature("sketch", FeatureStatus::Mapped),
                feature("extrude", FeatureStatus::Mapped),
                feature("fillet", FeatureStatus::LeftOut),
            ],
        });
        let out = Value::Object(success(CONVERT, json!({}), &report));
        assert_eq!(out["mode"], "history");
        assert_eq!(
            out["verification"],
            json!({"status": "verified", "reason": null})
        );
        assert_eq!(out["history"]["mapped"], 3);
        assert_eq!(out["history"]["total"], 3);
        assert_eq!(out["history"]["features"][3]["status"], "left_out");
        assert_eq!(out["losses"], json!([]));
    }
}
