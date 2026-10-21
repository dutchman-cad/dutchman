//! The machine-readable reports `--json` prints. Their field names are stable: fields are
//! added, never renamed or removed. See `docs/json.md`.

use std::path::Path;

use dutchman_core::{Check, FeatureStatus, Format, Mode, Outcome, Report, Severity};
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

/// A report of a run that stopped with `message`.
pub fn failure(schema: &str, input: Value, message: &str) -> Value {
    json!({
        "schema": schema,
        "ok": false,
        "input": input,
        "error": {"message": message},
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
    out.insert("losses".to_owned(), losses(report));
    out
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
