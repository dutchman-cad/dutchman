//! A batch's `dutchman.convert-all/1` report, made fit for the browser: server paths taken out,
//! download links added, and each file's result put in words as the desktop app puts it.

use std::path::Path;

use serde_json::{json, Value};

/// Takes the job's folders out of every string in `report`, so paths read as the uploaded
/// names (`project/part.ipt`) and messages do not show where the server keeps them, and adds
/// to each converted file's `output` a `download` link, relative to the page at `/`.
pub fn for_browser(report: &mut Value, job: &str, in_dir: &Path, out_dir: &Path) {
    let prefixes: Vec<String> = [in_dir, out_dir]
        .iter()
        .flat_map(|dir| {
            let shown = dir.display().to_string();
            [format!("{shown}/"), format!("{shown}\\"), shown]
        })
        .collect();
    scrub(report, &prefixes);
    if let Some(Value::Array(results)) = report.get_mut("results") {
        for result in results {
            result.as_object_mut().map(|r| r.remove("names"));
            if let Some(output) = result.get_mut("output").and_then(Value::as_object_mut) {
                if let Some(path) = output.get("path").and_then(Value::as_str) {
                    let path = path.replace('\\', "/");
                    output.insert(
                        "download".to_owned(),
                        json!(format!("jobs/{job}/files/{}", encode_path(&path))),
                    );
                    output.insert("path".to_owned(), json!(path));
                }
            }
            if let Some(input) = result.get_mut("input").and_then(Value::as_object_mut) {
                if let Some(path) = input.get("path").and_then(Value::as_str) {
                    let path = path.replace('\\', "/");
                    input.insert("path".to_owned(), json!(path));
                }
            }
        }
    }
}

fn scrub(value: &mut Value, prefixes: &[String]) {
    match value {
        Value::String(text) => {
            for prefix in prefixes {
                if text.contains(prefix.as_str()) {
                    *text = text.replace(prefix.as_str(), "");
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| scrub(item, prefixes)),
        Value::Object(fields) => fields.values_mut().for_each(|item| scrub(item, prefixes)),
        _ => {}
    }
}

/// `path` with each segment percent-encoded for a URL, keeping the `/` between them.
pub fn encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'/') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// One file's result, as the results page shows it.
#[derive(Debug, PartialEq, Eq)]
pub struct Card {
    /// The uploaded file, as uploaded (`project/part.ipt`).
    pub name: String,
    pub ok: bool,
    /// What was written, in a sentence; or, for a failure, why it failed.
    pub outcome: String,
    /// For a failure: the stage it stopped in.
    pub stage: Option<String>,
    /// Features that did not map: name and reason.
    pub blocked: Vec<(String, String)>,
    /// Losses, most severe first: level, subject and message.
    pub losses: Vec<(String, String, String)>,
    /// How many losses of each level, as "2 Dropped, 1 Info".
    pub loss_counts: String,
    /// The written design: its name and download link.
    pub download: Option<(String, String)>,
}

/// The cards of a browser-ready report (see [`for_browser`]), in the report's order.
pub fn cards(report: &Value) -> Vec<Card> {
    report
        .get("results")
        .and_then(Value::as_array)
        .map(|results| results.iter().map(card).collect())
        .unwrap_or_default()
}

fn card(result: &Value) -> Card {
    let text = |pointer: &str| {
        result
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let ok = result.get("ok") == Some(&json!(true));
    let mut losses: Vec<(String, String, String)> = result
        .get("losses")
        .and_then(Value::as_array)
        .map(|losses| {
            losses
                .iter()
                .map(|loss| {
                    let field = |key: &str| {
                        loss.get(key)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    (field("level"), field("subject"), field("message"))
                })
                .collect()
        })
        .unwrap_or_default();
    losses.sort_by_key(|(level, _, _)| severity(level));
    let loss_counts = ["Dropped", "Degraded", "Info"]
        .into_iter()
        .map(|level| (level, losses.iter().filter(|l| l.0 == level).count()))
        .filter(|(_, n)| *n > 0)
        .map(|(level, n)| format!("{n} {level}"))
        .collect::<Vec<_>>()
        .join(", ");
    let blocked = result
        .pointer("/history/features")
        .and_then(Value::as_array)
        .map(|features| {
            features
                .iter()
                .filter(|f| f.get("status").and_then(Value::as_str) == Some("blocked"))
                .map(|f| {
                    let field = |key: &str| {
                        f.get(key)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    (field("name"), field("reason"))
                })
                .collect()
        })
        .unwrap_or_default();
    let download = match (text("/output/path"), text("/output/download")) {
        (Some(path), Some(link)) if ok => Some((path, link)),
        _ => None,
    };
    Card {
        name: text("/input/path").unwrap_or_else(|| "file".to_owned()),
        ok,
        outcome: if ok {
            outcome(result)
        } else {
            text("/error/message").unwrap_or_else(|| "It could not be converted.".to_owned())
        },
        stage: if ok { None } else { text("/error/stage") },
        blocked,
        losses,
        loss_counts,
        download,
    }
}

fn severity(level: &str) -> u8 {
    match level {
        "Dropped" => 0,
        "Degraded" => 1,
        _ => 2,
    }
}

/// What was written, in a sentence, as the desktop app says it.
pub fn outcome(report: &Value) -> String {
    let field = |pointer: &str| report.pointer(pointer).and_then(Value::as_str);
    let counts = (
        report.pointer("/history/mapped").and_then(Value::as_u64),
        report.pointer("/history/total").and_then(Value::as_u64),
    );
    match field("/mode") {
        Some("history") => {
            let check = match (field("/verification/status"), field("/verification/reason")) {
                (Some("verified"), _) => {
                    if field("/output/format").is_some_and(|format| format.starts_with("freecad-"))
                    {
                        "verified in FreeCAD".to_owned()
                    } else {
                        "verified in SolidCraft".to_owned()
                    }
                }
                (Some("differs"), Some(reason)) => format!("rebuild differs: {reason}"),
                (_, Some(reason)) => format!("unverified: {reason}"),
                _ => "unverified".to_owned(),
            };
            match counts {
                (Some(m), Some(t)) => {
                    format!("Editable history, {m} of {t} features mapped; {check}.")
                }
                _ => format!("Editable history; {check}."),
            }
        }
        _ => match (field("/verification/status"), field("/verification/reason")) {
            (Some("flattened"), Some(reason)) => {
                format!("Geometry without history (flattened: {reason}).")
            }
            _ => "Geometry without history.".to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn server_paths_are_taken_out_and_links_added() {
        let in_dir = PathBuf::from("/data/jobs/abc/in");
        let out_dir = PathBuf::from("/data/jobs/abc/out");
        let mut report = json!({"results": [
            {"ok": true, "input": {"path": "/data/jobs/abc/in/proj/a b.ipt"},
             "output": {"path": "/data/jobs/abc/out/proj/a b.solidcraft"},
             "names": ["/data/jobs/abc/in/proj"]},
            {"ok": false, "input": {"path": "/data/jobs/abc/in/x.sldasm"},
             "error": {"message": "cannot read /data/jobs/abc/in/x.sldasm"}}
        ]});
        for_browser(&mut report, "abc", &in_dir, &out_dir);
        assert_eq!(report["results"][0]["input"]["path"], "proj/a b.ipt");
        assert_eq!(
            report["results"][0]["output"]["path"],
            "proj/a b.solidcraft"
        );
        assert_eq!(
            report["results"][0]["output"]["download"],
            "jobs/abc/files/proj/a%20b.solidcraft"
        );
        assert!(report["results"][0].get("names").is_none());
        assert_eq!(
            report["results"][1]["error"]["message"],
            "cannot read x.sldasm"
        );
    }

    #[test]
    fn cards_put_results_in_words() {
        let report = json!({"results": [
            {"ok": true, "input": {"path": "a.f3d"},
             "output": {"path": "a.solidcraft", "download": "jobs/j/files/a.solidcraft"},
             "mode": "import",
             "verification": {"status": "flattened", "reason": "feature \"Fillet\": not mapped yet"},
             "history": {"mapped": 1, "total": 2, "features": [
                {"name": "Sketch", "kind": "sketch", "status": "mapped", "reason": null},
                {"name": "Fillet", "kind": "fillet", "status": "blocked", "reason": "not mapped yet"}]},
             "losses": [
                {"level": "Info", "subject": "s", "message": "fine"},
                {"level": "Dropped", "subject": "t", "message": "gone"}]},
            {"ok": false, "input": {"path": "b.sldasm"},
             "error": {"message": "not supported yet", "stage": "decode"}}
        ]});
        let cards = cards(&report);
        assert_eq!(
            cards[0].outcome,
            "Geometry without history (flattened: feature \"Fillet\": not mapped yet)."
        );
        assert_eq!(
            cards[0].blocked,
            vec![("Fillet".to_owned(), "not mapped yet".to_owned())]
        );
        assert_eq!(cards[0].losses[0].0, "Dropped");
        assert_eq!(cards[0].loss_counts, "1 Dropped, 1 Info");
        assert_eq!(
            cards[0].download,
            Some((
                "a.solidcraft".to_owned(),
                "jobs/j/files/a.solidcraft".to_owned()
            ))
        );
        assert!(!cards[1].ok);
        assert_eq!(cards[1].outcome, "not supported yet");
        assert_eq!(cards[1].stage.as_deref(), Some("decode"));
        assert_eq!(cards[1].download, None);
    }

    #[test]
    fn a_verified_history_says_so() {
        let report = json!({"mode": "history", "history": {"mapped": 3, "total": 3},
            "verification": {"status": "verified"}, "output": {"format": "solidcraft/23"}});
        assert_eq!(
            outcome(&report),
            "Editable history, 3 of 3 features mapped; verified in SolidCraft."
        );
    }
}
