//! A GitHub issue about one file that did not convert, filled in from its convert report: the
//! link that opens the "A file didn't convert" form with the Dutchman version, the system, the
//! error, its stage and upstream, and the file's anonymous report. Nothing in it names or
//! shows the file: the error comes from the anonymous report, which has had its paths and
//! names taken out. See `docs/report-anon.md`.

use std::path::Path;

use serde_json::{json, Value};

use crate::anon;

/// The issue form for a file that did not convert.
pub const NEW_ISSUE: &str =
    "https://github.com/dutchman-cad/dutchman/issues/new?template=file-did-not-convert.yml";

/// The longest link the form is opened with. GitHub refuses a long link to a signed-in
/// reader well before 8 KB, as the session's cookies count too, and how much they take
/// differs by reader; past this, the anonymous report is left for the reporter to paste.
pub const MAX_URL: usize = 3_000;

/// The most of the error message the link carries; the whole message is in the anonymous
/// report.
const MAX_MESSAGE: usize = 800;

/// The issue to file about the file `report` (a `dutchman.convert/1` report) is of.
#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    /// The link that opens the form filled in.
    pub url: String,
    /// The file's anonymous report, pretty-printed.
    pub anonymous_report: String,
    /// Whether the anonymous report is in the link; when it is not, it is for the reporter
    /// to paste.
    pub report_in_url: bool,
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
    let happened = match field("message") {
        Some(message) if message.chars().count() > MAX_MESSAGE => {
            let start: String = message.chars().take(MAX_MESSAGE).collect();
            format!(
                "The conversion failed with this message (shortened; the anonymous report \
                 has all of it):\n\n{start}…"
            )
        }
        Some(message) => format!("The conversion failed with this message:\n\n{message}"),
        None => "The conversion failed.".to_owned(),
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
    let mut fields = vec![
        ("title", format!("[file] {format} failed to convert")),
        ("format", format.to_owned()),
        (
            "dutchman-version",
            format!("dutchman {}, desktop app", env!("CARGO_PKG_VERSION")),
        ),
        ("os", system()),
        ("what-happened", happened),
        ("stage", stage.join(", ")),
    ];
    let without = url(&fields);
    // Compact in the link, where every newline and indent costs encoded bytes; the copy to
    // paste stays readable.
    fields.push((
        "report-anon",
        serde_json::to_string(anonymous).unwrap_or_default(),
    ));
    let with = url(&fields);
    let report_in_url = with.len() <= MAX_URL;
    Issue {
        url: if report_in_url { with } else { without },
        anonymous_report,
        report_in_url,
    }
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
        assert!(issue.report_in_url);
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
        let happened = query(&issue.url, "what-happened").unwrap();
        assert!(happened.contains("malformed container"), "{happened}");
        let pasted = query(&issue.url, "report-anon").unwrap();
        let anonymous: Value = serde_json::from_str(&pasted).unwrap();
        assert_eq!(
            anonymous,
            serde_json::from_str::<Value>(&issue.anonymous_report).unwrap()
        );
        assert_eq!(anonymous["schema"], anon::SCHEMA);
        assert_eq!(anonymous["errors"][0]["stage"], "decode");
        for text in [&issue.url, &issue.anonymous_report, &happened] {
            assert!(!text.contains("Secret"), "{text}");
            assert!(!text.contains("someone"), "{text}");
        }
    }

    #[test]
    fn a_report_too_long_for_the_link_is_left_to_paste() {
        let long = json!({"errors": [{"message": "it failed", "stage": "decode"}],
                          "losses": [{"message": "x".repeat(MAX_URL)}]});
        let issue = fill(&failed(), &long);
        assert!(!issue.report_in_url);
        assert!(issue.url.len() <= MAX_URL);
        assert!(query(&issue.url, "report-anon").is_none());
        assert!(issue.anonymous_report.contains("xxxx"));
    }

    #[test]
    fn a_check_failure_with_two_counts_stays_in_the_link_and_survives_encoding() {
        let message = "cannot read this design: cadmpeg's model of it fails its checks (24 \
                       errors): pcurve mapped through the face surface misses the edge's \
                       vertex positions by 0.912455; read again as geometry alone, without \
                       its history, sketches and parameters, it still fails them (83 \
                       errors): pcurve mapped through the face surface misses the edge's \
                       vertex positions by 0.912455";
        let mut report = failed();
        report["error"]["message"] = json!(message);
        let issue = issue(
            &report,
            Path::new("/home/someone/out"),
            &anon::Run::default(),
        )
        .unwrap();
        assert!(issue.url.len() <= MAX_URL, "{}", issue.url.len());
        assert!(issue.url.is_ascii() && !issue.url.contains(['\'', '(', ')', ' ', ';']));
        assert!(query(&issue.url, "what-happened")
            .unwrap()
            .contains(message));
    }

    #[test]
    fn a_long_error_message_is_shortened_in_the_link() {
        let message = "y".repeat(MAX_URL * 2);
        let long = json!({"errors": [{"message": message, "stage": "decode"}]});
        let issue = fill(&failed(), &long);
        assert!(issue.url.len() <= MAX_URL, "{}", issue.url.len());
        let happened = query(&issue.url, "what-happened").unwrap();
        assert!(happened.contains("shortened"), "{happened}");
        assert!(issue.anonymous_report.contains(&message));
    }

    #[test]
    fn query_values_are_encoded() {
        assert_eq!(encode("a b&c=d/é"), "a%20b%26c%3Dd%2F%C3%A9");
        assert_eq!(encode("A-z_0.9~"), "A-z_0.9~");
    }
}
