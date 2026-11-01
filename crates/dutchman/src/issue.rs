//! A GitHub issue about one file that did not convert, filled in from its convert report: the
//! link that opens the "A file didn't convert" form with the Dutchman version, the system and
//! the error's stage, upstream and code, and the file's anonymous report to paste into it.
//! Nothing in it names or shows the file: the error comes from the anonymous report, which has had its paths and
//! names taken out. See `docs/report-anon.md`.

use std::path::Path;

use serde_json::{json, Value};

use crate::anon;
use crate::diagnose::signature::Signature;

/// The issue form for a file that did not convert.
pub const NEW_ISSUE: &str =
    "https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml";

/// The longest link the form is opened with. GitHub answers a long link to a signed-in reader
/// with a 406 well before 8 KB, as the session's cookies count too, and how much they take
/// differs by reader. So the link holds only short fields, and the report is pasted.
pub const MAX_URL: usize = 2_000;

/// The most of one field the link carries. The error's stage, upstream and code are short;
/// this keeps the link's length bounded whatever a report holds.
const MAX_FIELD: usize = 200;

/// What the form's report box opens with.
const PASTE_HERE: &str = "Paste the anonymous report here: Dutchman copied it to your clipboard.";

/// What the form's report box opens with for a narrowed failure.
const PASTE_ZIP: &str = "Paste anonymous.json here. Keep the report zip private; it contains geometry. Use the private-send card for the zip.";

/// The issue to file about the file `report` (a `dutchman.convert/1` report) is of.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    /// The link that opens the form filled in.
    pub url: String,
    /// The file's anonymous report, pretty-printed, for the reporter to paste.
    pub anonymous_report: String,
}

/// The issue about the file `report` is of, converted into `out_dir` in a run `run`
/// describes. The anonymous report's file id uses a new salt, which is not kept: the issue is
/// about one file, which the reporter knows.
pub fn issue(report: &Value, out_dir: &Path, run: &anon::Run) -> Result<Issue, String> {
    let input = report
        .pointer("/input/path")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let batch = json!({"results": [report]});
    let (anonymous, _) = anon::report(&batch, &[input], out_dir, &anon::Salt::random()?, run);
    Ok(fill(report, &anonymous))
}

/// The issue about the file `report` is of, with `anonymous` its anonymous report.
pub fn fill(report: &Value, anonymous: &Value) -> Issue {
    let format = report
        .pointer("/input/format_name")
        .and_then(Value::as_str)
        .unwrap_or("Other");
    let error = anonymous.pointer("/errors/0");
    let field = |key: &str| {
        error
            .and_then(|error| error.get(key))
            .and_then(Value::as_str)
    };
    let stage: Vec<String> = [
        ("stage", "stage"),
        ("upstream", "upstream"),
        ("code", "code"),
    ]
    .into_iter()
    .filter_map(|(key, label)| field(key).map(|value| format!("{label}: {value}")))
    .collect();
    let anonymous_report = serde_json::to_string_pretty(anonymous).unwrap_or_default();
    let fields = [
        ("title", format!("[file] {format} failed to convert")),
        ("format", format.to_owned()),
        (
            "dutchman-version",
            format!("dutchman {}, desktop app", env!("CARGO_PKG_VERSION")),
        ),
        ("os", system()),
        (
            "what-happened",
            "The conversion failed. The anonymous report has the error.".to_owned(),
        ),
        ("stage", stage.join(", ")),
        ("report-anon", PASTE_HERE.to_owned()),
    ];
    let fields = fields.map(|(id, value)| (id, value.chars().take(MAX_FIELD).collect()));
    Issue {
        url: url(&fields),
        anonymous_report,
    }
}

/// The link that opens the form about a failure `dutchman diagnose --narrow` narrowed down, in
/// a small rebuilt file of the format named `format`. It carries the signature and no file.
pub fn narrowed(format: &str, signature: &Signature) -> String {
    let fields = [
        ("title", format!("[file] {format} failed to convert")),
        ("format", format.to_owned()),
        (
            "dutchman-version",
            format!("dutchman {}", env!("CARGO_PKG_VERSION")),
        ),
        ("os", system()),
        (
            "what-happened",
            format!(
                "Narrowed with dutchman diagnose. Failure signature {}. The small rebuilt file \
                 fails the same way.",
                signature.short()
            ),
        ),
        (
            "stage",
            format!(
                "stage: {}, code: {}",
                signature.error.stage.as_deref().unwrap_or("none"),
                signature.error.kind.as_deref().unwrap_or("none")
            ),
        ),
        ("report-anon", PASTE_ZIP.to_owned()),
    ];
    url(&fields.map(|(id, value)| (id, value.chars().take(MAX_FIELD).collect())))
}

/// The form's link with `fields` filled in, each by its form id.
fn url(fields: &[(&str, String)]) -> String {
    let mut out = NEW_ISSUE.to_owned();
    for (id, value) in fields {
        out.push('&');
        out.push_str(id);
        out.push('=');
        out.push_str(&encode(value));
    }
    out
}

/// `text` as a URL query value: unreserved characters as they are, every other byte as `%XX`.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The system this runs on, as the form asks for it: its name and processor.
fn system() -> String {
    let name = match std::env::consts::OS {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    format!("{name} ({})", std::env::consts::ARCH)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn failed() -> Value {
        json!({
            "schema": "dutchman.convert/1",
            "ok": false,
            "input": {"path": "/home/someone/Secret Bracket.f3d", "format": "fusion_archive",
                      "format_name": "Fusion archive (.f3d)"},
            "error": {"message": "cannot read this design: malformed container: parameter 7 \
                       of /home/someone/Secret Bracket.f3d does not match",
                      "stage": "decode", "upstream": "cadmpeg"},
        })
    }

    /// The value of the query field `id` in `url`, decoded.
    fn query(url: &str, id: &str) -> Option<String> {
        let (_, query) = url.split_once('?')?;
        let value = query
            .split('&')
            .find_map(|pair| pair.strip_prefix(&format!("{id}=")))?;
        let bytes = value.as_bytes();
        let mut out = Vec::new();
        let mut at = 0;
        while at < bytes.len() {
            if bytes[at] == b'%' {
                let hex = std::str::from_utf8(&bytes[at + 1..at + 3]).unwrap();
                out.push(u8::from_str_radix(hex, 16).unwrap());
                at += 3;
            } else {
                out.push(bytes[at]);
                at += 1;
            }
        }
        Some(String::from_utf8(out).unwrap())
    }

    #[test]
    fn a_failed_file_fills_in_the_form_without_naming_it() {
        let issue = issue(
            &failed(),
            Path::new("/home/someone/out"),
            &anon::Run::default(),
        )
        .unwrap();
        assert!(issue.url.starts_with(NEW_ISSUE));
        assert_eq!(
            query(&issue.url, "title").unwrap(),
            "[file] Fusion archive (.f3d) failed to convert"
        );
        assert_eq!(
            query(&issue.url, "format").unwrap(),
            "Fusion archive (.f3d)"
        );
        assert!(query(&issue.url, "dutchman-version")
            .unwrap()
            .starts_with("dutchman "));
        assert!(!query(&issue.url, "os").unwrap().is_empty());
        assert_eq!(
            query(&issue.url, "stage").unwrap(),
            "stage: decode, upstream: cadmpeg"
        );
        assert!(query(&issue.url, "report-anon")
            .unwrap()
            .starts_with("Paste the anonymous report here"));
        let anonymous: Value = serde_json::from_str(&issue.anonymous_report).unwrap();
        assert_eq!(anonymous["schema"], anon::SCHEMA);
        assert_eq!(anonymous["errors"][0]["stage"], "decode");
        for text in [&issue.url, &issue.anonymous_report] {
            assert!(!text.contains("Secret"), "{text}");
            assert!(!text.contains("someone"), "{text}");
        }
    }

    #[test]
    fn the_longest_report_stays_short_in_the_link_and_whole_on_the_clipboard() {
        // Every byte of these encodes to three in the link.
        let code = "é".repeat(MAX_URL);
        let long = json!({
            "errors": [{"message": "y".repeat(MAX_URL * 4), "stage": code,
                        "upstream": code, "code": code}],
            "losses": [{"message": "x".repeat(MAX_URL * 4)}]
        });
        let mut report = failed();
        report["input"]["format_name"] = json!("Fusion archive (.f3d)");
        let issue = fill(&report, &long);
        assert!(issue.url.len() <= MAX_URL, "{}", issue.url.len());
        assert!(issue.url.is_ascii() && !issue.url.contains(['\'', '(', ')', ' ', ';']));
        assert!(issue.anonymous_report.contains("xxxx"));
        assert!(issue.anonymous_report.contains("yyyy"));
    }

    #[test]
    fn a_narrowed_failure_fills_in_the_form_with_its_signature_and_no_more() {
        let signature = Signature::new(
            crate::diagnose::signature::ErrorCode {
                outcome: "failed".into(),
                stage: Some("write".into()),
                kind: Some("restate".into()),
                codes: Vec::new(),
            },
            None,
        );
        let url = narrowed("Fusion archive (.f3d)", &signature);
        assert!(url.starts_with(NEW_ISSUE));
        assert!(url.len() <= MAX_URL, "{}", url.len());
        assert!(query(&url, "what-happened")
            .unwrap()
            .contains(signature.short()));
        assert_eq!(query(&url, "stage").unwrap(), "stage: write, code: restate");
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(encode("A-z_0.9~"), "A-z_0.9~");
    }
}
