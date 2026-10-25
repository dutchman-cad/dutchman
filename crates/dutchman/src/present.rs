//! Plain presentation data for conversion reports, shared by graphical front ends.
//!
//! Builders accept the stable JSON reports, so serialized reports and freshly converted
//! reports follow the same rules. They never change the report or add text to JSON output.

use crate::{issue, json};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Progress of a file, including files that have no conversion report yet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Waiting,
    Converting,
    Converted,
    Failed,
    NotStarted,
    Skipped,
}

/// The result of a file, independently of any UI's colours or icons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Waiting,
    Converting,
    Converted,
    ConvertedWithLosses,
    Flattened,
    Incomplete,
    Failed,
    NotStarted,
    Skipped,
}

/// A loss's two separately drawn strings, plus its complete text for other renderers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LossLine {
    pub level: String,
    pub text: String,
    pub full_text: String,
}

/// Losses at one level, in their original order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LossGroup {
    pub level: String,
    pub count: usize,
    pub lines: Vec<LossLine>,
}

/// A component's losses in the CLI's shared severity order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentView {
    pub heading: String,
    pub severity_rank: u8,
    pub lines: Vec<LossLine>,
}

/// Everything a front end needs to describe a file's conversion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileView {
    pub name: String,
    pub input: PathBuf,
    pub output: Option<PathBuf>,
    pub status: Status,
    pub status_word: &'static str,
    /// Smaller ranks need attention first. Equal ranks retain input order.
    pub severity_rank: u8,
    pub outcome: String,
    pub capability: Option<String>,
    pub error: Option<String>,
    pub incomplete: bool,
    /// None means there is no missing-parts recovery; Some(empty) means none were found.
    pub missing_parts: Option<Vec<String>>,
    pub missing_summary: Option<String>,
    pub assembly: Option<String>,
    pub assembly_causes: Vec<String>,
    pub loss_summary: String,
    pub losses: Vec<LossGroup>,
    pub components: Vec<ComponentView>,
    pub can_report: bool,
}

pub const REPORT_HELP: &str =
    "If the file opens in the app that made it, this is a bug in Dutchman: \
                     report it on GitHub. The form opens filled in with the error and an \
                     anonymous report that names no file; read it before you submit, and \
                     never attach the design.";
pub const MISSING_HELP: &str =
    "Choose the folder that holds them, or one of them, to convert this file again \
             searching there too.";
pub const REPORT_TOO_LONG: &str =
    "The anonymous report is too long for the link, so it is copied: paste it into \
                 the form's \"Anonymous report for this file\" box.";

/// A pure issue-form action. Anonymization and random salt creation happen at the caller.
#[derive(Clone, Debug, PartialEq)]
pub struct ReportLink {
    pub issue: issue::Issue,
    pub copy_notice: Option<&'static str>,
}

impl ReportLink {
    pub fn new(issue: issue::Issue) -> Self {
        let copy_notice = (!issue.report_in_url).then_some(REPORT_TOO_LONG);
        Self { issue, copy_notice }
    }
}

pub fn report_link(report: &Value, anonymous: &Value) -> ReportLink {
    let issue = issue::fill(report, anonymous);
    ReportLink::new(issue)
}

pub fn loss_line(loss: &Value) -> LossLine {
    let field = |key: &str| loss.get(key).and_then(Value::as_str).unwrap_or("");
    LossLine {
        level: field("level").to_owned(),
        text: format!("{}: {}", field("subject"), field("message")),
        full_text: json::loss_line(loss, true).0,
    }
}

impl FileView {
    /// Present a report using its paths and final result.
    pub fn from_report(report: &Value) -> Self {
        let path = |key| {
            report
                .pointer(key)
                .and_then(Value::as_str)
                .map(PathBuf::from)
        };
        let input = path("/input/path").unwrap_or_default();
        let progress = if report.get("ok").and_then(Value::as_bool) == Some(true) {
            Progress::Converted
        } else {
            Progress::Failed
        };
        Self::new(
            &input,
            path("/output/path").as_deref(),
            progress,
            Some(report),
        )
    }

    /// Present a file during a run. Paths are supplied separately for queued files.
    pub fn new(
        input: &Path,
        output: Option<&Path>,
        progress: Progress,
        report: Option<&Value>,
    ) -> Self {
        let incomplete = report.is_some_and(incomplete);
        let missing_parts = report.and_then(missing_parts);
        let missing_summary = missing_parts.as_deref().map(missing_summary);
        let raw: Vec<&Value> = report
            .and_then(|r| r.get("losses"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .collect();
        let flattened = report
            .and_then(|r| r.pointer("/verification/status"))
            .and_then(Value::as_str)
            == Some("flattened");
        let status = match progress {
            Progress::Waiting => Status::Waiting,
            Progress::Converting => Status::Converting,
            Progress::Failed => Status::Failed,
            Progress::NotStarted => Status::NotStarted,
            Progress::Skipped => Status::Skipped,
            Progress::Converted if incomplete => Status::Incomplete,
            Progress::Converted if flattened => Status::Flattened,
            Progress::Converted if !raw.is_empty() => Status::ConvertedWithLosses,
            Progress::Converted => Status::Converted,
        };
        let (status_word, severity_rank) = match status {
            Status::Failed => ("Failed", 0),
            Status::Incomplete => ("Incomplete", 0),
            Status::Flattened | Status::ConvertedWithLosses => ("Converted", 1),
            Status::Converted => ("Converted", 2),
            Status::Waiting => ("Waiting", 3),
            Status::Converting => ("Converting", 3),
            Status::NotStarted => ("Not started", 3),
            Status::Skipped => ("Skipped", 3),
        };
        let losses = ["Info", "Degraded", "Dropped"]
            .into_iter()
            .filter_map(|level| {
                let lines: Vec<_> = raw
                    .iter()
                    .filter(|l| l.get("level").and_then(Value::as_str) == Some(level))
                    .map(|l| loss_line(l))
                    .collect();
                (!lines.is_empty()).then(|| LossGroup {
                    level: level.to_owned(),
                    count: lines.len(),
                    lines,
                })
            })
            .collect();
        let components = report
            .map(json::components)
            .unwrap_or_default()
            .into_iter()
            .map(|c| ComponentView {
                heading: format!(
                    "{}: {}",
                    c.file.as_deref().unwrap_or("The design"),
                    c.summary()
                ),
                severity_rank: if c.dropped > 0 {
                    0
                } else if c.degraded > 0 || c.flattened.is_some() {
                    1
                } else {
                    2
                },
                lines: c.losses.into_iter().map(loss_line).collect(),
            })
            .collect();
        let error = report
            .and_then(|r| r.pointer("/error/message"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let assembly_lines = report.map(json::completeness_lines).unwrap_or_default();
        Self {
            name: input
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| input.display().to_string()),
            input: input.to_path_buf(),
            output: output.map(Path::to_path_buf),
            status,
            status_word,
            severity_rank,
            outcome: error.clone().unwrap_or_else(|| {
                if progress == Progress::Converted {
                    report.map(outcome).unwrap_or_default()
                } else {
                    String::new()
                }
            }),
            capability: report.and_then(json::status_line),
            can_report: error.is_some() && missing_parts.is_none(),
            error,
            incomplete,
            missing_parts,
            missing_summary,
            assembly: report
                .and_then(json::assembly_line)
                .filter(|_| !assembly_lines.is_empty()),
            assembly_causes: assembly_lines
                .into_iter()
                .skip(1)
                .map(|s| s.trim().to_owned())
                .collect(),
            loss_summary: loss_counts(&raw),
            losses,
            components,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub converted: usize,
    pub incomplete: usize,
    pub failed: usize,
    pub not_started: usize,
    pub skipped: usize,
}

/// Files sorted by severity; source indices allow front ends to preserve input order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunView {
    pub files: Vec<(usize, FileView)>,
    pub counts: Counts,
    pub invitation: Option<String>,
    pub summary: Option<String>,
    pub first_to_show: usize,
}

impl RunView {
    /// Present files in a live run, with one invitation derived from all available reports.
    pub fn from_files<'a>(
        files: Vec<FileView>,
        reports: impl IntoIterator<Item = &'a Value>,
        running: bool,
    ) -> Self {
        Self::new(files, json::fixtures_pitch(reports), running)
    }

    pub fn new(files: Vec<FileView>, invitation: Option<String>, running: bool) -> Self {
        let mut counts = Counts::default();
        for file in &files {
            match file.status {
                Status::Converted
                | Status::ConvertedWithLosses
                | Status::Flattened
                | Status::Incomplete => counts.converted += 1,
                Status::Failed => counts.failed += 1,
                Status::NotStarted => counts.not_started += 1,
                Status::Skipped => counts.skipped += 1,
                _ => {}
            }
            counts.incomplete += usize::from(file.status == Status::Incomplete);
        }
        let first_to_show = files
            .iter()
            .position(|f| matches!(f.status, Status::Failed | Status::Incomplete))
            .unwrap_or(0);
        let summary = (!files.is_empty()).then(|| {
            if running {
                return format!(
                    "{} of {} done",
                    counts.converted + counts.failed,
                    files.len()
                );
            }
            let mut text = if counts.incomplete > 0 {
                format!(
                    "{} converted ({} incomplete), {} failed",
                    counts.converted, counts.incomplete, counts.failed
                )
            } else {
                format!("{} converted, {} failed", counts.converted, counts.failed)
            };
            if counts.not_started > 0 {
                text.push_str(&format!(", {} not started", counts.not_started));
            }
            text
        });
        let mut files: Vec<_> = files.into_iter().enumerate().collect();
        files.sort_by_key(|(_, file)| file.severity_rank);
        Self {
            files,
            counts,
            invitation: if running { None } else { invitation },
            summary,
            first_to_show,
        }
    }

    /// Present either a convert report or a convert-all report. JSON remains untouched.
    pub fn from_report(report: &Value) -> Self {
        let reports: Vec<&Value> = match report.get("results").and_then(Value::as_array) {
            Some(results) => results.iter().collect(),
            None => vec![report],
        };
        let mut view = Self::new(
            reports.iter().map(|r| FileView::from_report(r)).collect(),
            json::fixtures_pitch(reports.iter().copied()),
            false,
        );
        view.counts.skipped = report
            .get("skipped")
            .and_then(Value::as_u64)
            .and_then(|n| usize::try_from(n).ok())
            .unwrap_or(0);
        view
    }
}

/// Whether `report` is of an assembly that converted with placements left out.
pub fn incomplete(report: &Value) -> bool {
    report
        .pointer("/completeness/left_out")
        .and_then(Value::as_array)
        .is_some_and(|causes| !causes.is_empty())
}

/// What marks a part file an assembly places as not found, in its loss.
const NOT_FOUND: &str = ": not found at its stored path";

/// The part files an assembly's conversion could not find, by name, when it missed any; an
/// empty list when it failed for want of all of them, without naming them.
pub fn missing_parts(report: &Value) -> Option<Vec<String>> {
    let mut names: Vec<String> = report
        .get("losses")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|loss| loss.get("message").and_then(Value::as_str))
        .filter_map(|message| message.find(NOT_FOUND).map(|at| message.get(..at)))
        .flatten()
        .map(str::to_owned)
        .collect();
    names.sort();
    names.dedup();
    let none_found = report
        .pointer("/error/message")
        .and_then(Value::as_str)
        .is_some_and(|message| message.contains("parts could be found or read"));
    (!names.is_empty() || none_found).then_some(names)
}

/// A missing-parts recovery summary, with at most five names.
pub fn missing_summary(missing: &[String]) -> String {
    const SHOWN: usize = 5;
    let text = match missing.len() {
        0 => "Its part files were not found.".to_owned(),
        n => {
            let mut text = format!(
                "{n} part file{} not found: {}",
                if n == 1 { " was" } else { "s were" },
                missing
                    .iter()
                    .take(SHOWN)
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            if n > SHOWN {
                text.push_str(&format!(" and {} more", n - SHOWN));
            }
            text
        }
    };
    text
}

/// What was written, in a sentence.
pub fn outcome(report: &Value) -> String {
    let field = |pointer: &str| report.pointer(pointer).and_then(Value::as_str);
    let history = |mapped: u64, total: u64| format!("{mapped} of {total} features mapped");
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
                (Some(m), Some(t)) => format!("Editable history, {}; {check}.", history(m, t)),
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

pub fn loss_counts(losses: &[&Value]) -> String {
    let count = |level: &str| {
        losses
            .iter()
            .filter(|l| l.get("level").and_then(Value::as_str) == Some(level))
            .count()
    };
    let parts: Vec<String> = ["Info", "Degraded", "Dropped"]
        .into_iter()
        .map(|level| (level, count(level)))
        .filter(|(_, n)| *n > 0)
        .map(|(level, n)| format!("{n} {level}"))
        .collect();
    format!("Losses: {}", parts.join(", "))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn statuses_and_outcomes_preserve_desktop_words() {
        let cases = [
            (
                json!({"ok": true}),
                Status::Converted,
                "Geometry without history.",
            ),
            (
                json!({"ok": true, "losses": [{"level": "Info"}]}),
                Status::ConvertedWithLosses,
                "Geometry without history.",
            ),
            (
                json!({"ok": true, "verification": {"status": "flattened", "reason": "unsupported feature"}}),
                Status::Flattened,
                "Geometry without history (flattened: unsupported feature).",
            ),
            (
                json!({"ok": true, "mode": "history", "history": {"mapped": 2, "total": 3}, "verification": {"status": "verified"}}),
                Status::Converted,
                "Editable history, 2 of 3 features mapped; verified in SolidCraft.",
            ),
            (
                json!({"ok": true, "mode": "history", "output": {"format": "freecad-document/1"}, "verification": {"status": "verified"}}),
                Status::Converted,
                "Editable history; verified in FreeCAD.",
            ),
            (
                json!({"ok": true, "mode": "history", "verification": {"status": "differs", "reason": "volume"}}),
                Status::Converted,
                "Editable history; rebuild differs: volume.",
            ),
            (
                json!({"ok": true, "mode": "history", "verification": {"reason": "no CLI"}}),
                Status::Converted,
                "Editable history; unverified: no CLI.",
            ),
            (
                json!({"ok": true, "mode": "history"}),
                Status::Converted,
                "Editable history; unverified.",
            ),
            (
                json!({"ok": true, "completeness": {"left_out": [{"count": 1}]}}),
                Status::Incomplete,
                "Geometry without history.",
            ),
            (
                json!({"ok": false, "error": {"message": "no geometry"}}),
                Status::Failed,
                "no geometry",
            ),
        ];
        for (report, status, outcome) in cases {
            let view = FileView::from_report(&report);
            assert_eq!(view.status, status);
            assert_eq!(view.outcome, outcome);
        }
        for (progress, status, word) in [
            (Progress::Waiting, Status::Waiting, "Waiting"),
            (Progress::Converting, Status::Converting, "Converting"),
            (Progress::NotStarted, Status::NotStarted, "Not started"),
            (Progress::Skipped, Status::Skipped, "Skipped"),
        ] {
            let file = FileView::new(Path::new("part.f3d"), None, progress, None);
            assert_eq!((file.status, file.status_word), (status, word));
        }
    }

    #[test]
    fn run_order_and_reveal_keep_equal_severity_in_input_order() {
        let report = json!({"results": [
            {"ok": true},
            {"ok": true, "completeness": {"left_out": [{"count": 1}]}},
            {"ok": false}, {"ok": false},
            {"ok": true, "losses": [{"level": "Info"}]}
        ], "skipped": 4});
        let run = RunView::from_report(&report);
        assert_eq!(
            run.files.iter().map(|(i, _)| *i).collect::<Vec<_>>(),
            [1, 2, 3, 4, 0]
        );
        assert_eq!(run.first_to_show, 1);
        assert_eq!(
            run.counts,
            Counts {
                converted: 3,
                incomplete: 1,
                failed: 2,
                skipped: 4,
                not_started: 0
            }
        );
        assert_eq!(
            run.summary.as_deref(),
            Some("3 converted (1 incomplete), 2 failed")
        );
        let files = vec![FileView::new(
            Path::new("a"),
            None,
            Progress::NotStarted,
            None,
        )];
        assert_eq!(
            RunView::new(files.clone(), None, false).summary.as_deref(),
            Some("0 converted, 0 failed, 1 not started")
        );
        assert_eq!(
            RunView::new(files, Some("invite".into()), true)
                .summary
                .as_deref(),
            Some("0 of 1 done")
        );
        assert!(RunView::new(vec![], None, false).summary.is_none());
        assert_eq!(RunView::new(vec![], None, false).first_to_show, 0);
    }

    #[test]
    fn losses_use_shared_component_order_and_full_messages() {
        let report = json!({"ok": true, "losses": [
            {"level": "Info", "subject": "a.ipt: sketch", "message": "First. Second."},
            {"level": "Dropped", "subject": "z.ipt: face", "message": "gone"},
            {"level": "Degraded", "subject": "history", "message": "flattened: feature"},
            {"level": "Info", "subject": "a.ipt: parameter", "message": "kept"}
        ]});
        let view = FileView::from_report(&report);
        assert_eq!(view.loss_summary, "Losses: 2 Info, 1 Degraded, 1 Dropped");
        assert_eq!(
            view.losses
                .iter()
                .map(|g| (g.level.as_str(), g.count))
                .collect::<Vec<_>>(),
            [("Info", 2), ("Degraded", 1), ("Dropped", 1)]
        );
        assert_eq!(
            view.losses[0].lines[0].text,
            "a.ipt: sketch: First. Second."
        );
        assert_eq!(
            view.losses[0].lines[0].full_text,
            "Info: a.ipt: sketch: First. Second."
        );
        assert_eq!(
            view.components
                .iter()
                .map(|c| c.heading.as_str())
                .collect::<Vec<_>>(),
            [
                "The design: history flattened (feature); 1 Degraded",
                "z.ipt: 1 Dropped",
                "a.ipt: 2 Info"
            ]
        );
    }

    #[test]
    fn missing_parts_are_unique_sorted_and_bounded_in_the_summary() {
        let report = json!({"ok": true, "losses": [
            {"message": "pin.ipt: not found at its stored path"},
            {"message": "base.ipt: not found at its stored path"},
            {"message": "pin.ipt: not found at its stored path"}
        ]});
        let view = FileView::from_report(&report);
        assert_eq!(view.missing_parts.unwrap(), ["base.ipt", "pin.ipt"]);
        assert_eq!(
            view.missing_summary.as_deref(),
            Some("2 part files were not found: base.ipt, pin.ipt")
        );
        let failed = FileView::from_report(
            &json!({"ok": false, "error": {"message": "none of the assembly's parts could be found or read"}}),
        );
        assert_eq!(failed.missing_parts, Some(vec![]));
        assert!(!failed.can_report);
        assert_eq!(
            failed.missing_summary.as_deref(),
            Some("Its part files were not found.")
        );
        assert_eq!(
            missing_summary(&(0..7).map(|n| n.to_string()).collect::<Vec<_>>()),
            "7 part files were not found: 0, 1, 2, 3, 4 and 2 more"
        );
        assert!(!incomplete(&json!({"completeness": {"left_out": []}})));
    }

    #[test]
    fn issue_links_reuse_the_form_builder_and_explain_the_paste_fallback() {
        let report = json!({"ok": false, "input": {"format_name": "Fusion"}, "error": {"message": "private name"}});
        let anonymous = json!({"errors": [{"message": "no geometry", "stage": "decode", "upstream": "cadmpeg"}]});
        let link = report_link(&report, &anonymous);
        assert_eq!(link.issue, issue::fill(&report, &anonymous));
        for field in [
            "dutchman-version=",
            "os=",
            "what-happened=",
            "stage=stage%3A%20decode%2C%20upstream%3A%20cadmpeg",
            "report-anon=",
        ] {
            assert!(link.issue.url.contains(field));
        }
        assert!(!link.issue.url.contains("private"));
        assert_eq!(link.copy_notice, None);
        let long = json!({"errors": [], "losses": [{"message": "x".repeat(issue::MAX_URL)}]});
        let link = report_link(&report, &long);
        assert_eq!(link.copy_notice, Some(REPORT_TOO_LONG));
        assert!(!link.issue.report_in_url);
        assert!(link.issue.url.len() <= issue::MAX_URL);
        assert!(!link.issue.url.contains("report-anon="));
    }

    #[test]
    fn invitations_are_once_per_finished_run_and_never_added_to_reports() {
        let report = json!({"schema": "dutchman.convert-all/1", "results": [
            {"ok": false, "input": {"program": "Inventor"}},
            {"ok": false, "input": {"program": "Inventor"}},
            {"ok": true, "input": {"program": "Fusion"}, "verification": {"status": "flattened"}}
        ]});
        let before = report.to_string();
        let view = RunView::from_report(&report);
        let invitation = view.invitation.unwrap();
        assert_eq!(invitation.matches("Want to help").count(), 1);
        assert!(invitation.contains("Fusion, Inventor"));
        assert_eq!(report.to_string(), before);
        assert!(!before.contains("Want to help"));
        assert!(RunView::new(vec![], Some(invitation), true)
            .invitation
            .is_none());
        assert!(RunView::from_report(&json!({"ok": true}))
            .invitation
            .is_none());
        assert!(
            RunView::from_report(&json!({"ok": false, "input": {"format": "iges"}}))
                .invitation
                .is_none()
        );
    }
}
