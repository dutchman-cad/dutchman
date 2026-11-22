use dutchman_core::{Check, FeatureStatus, Mode, Report, Severity};
use serde_json::{json, Value};
/// The loss report SolidCraft shows: a one-line summary, one item per loss, and plain-language
/// conversion details.
pub(crate) fn host(report: &Report) -> Value {
    const MAX_ITEMS: usize = 4096;
    let mut losses: Vec<_> = report.losses().iter().collect();
    // Stable sorting retains source order within each severity.
    losses.sort_by_key(|loss| match loss.severity {
        Severity::Dropped => 0,
        Severity::Degraded => 1,
        Severity::Info => 2,
    });
    let truncated = losses.len() > MAX_ITEMS;
    let items: Vec<_> = losses
        .into_iter()
        .take(if truncated { MAX_ITEMS - 1 } else { MAX_ITEMS })
        .map(|loss| {
            let code = loss.code.as_deref().unwrap_or_default();
            // A cadmpeg loss's subject is its code: say it once, under the code.
            let subject = if loss.subject == code { "" } else { loss.subject.as_str() };
            json!({
                "severity": match loss.severity { Severity::Info=>"info", Severity::Degraded=>"degraded", Severity::Dropped=>"dropped" },
                "title": clip(&crate::notice::plain(&loss.detail), 120),
                "code": clip(code, 128),
                "subject": clip(&crate::notice::plain(subject), 256),
                // The host refuses a blank message.
                "message": clip(&crate::notice::plain(if loss.detail.trim().is_empty() { "No further detail" } else { &loss.detail }), 1024),
            })
        })
        .collect();
    let mut host = json!({"summary": clip(&summary(report), 240), "items": items, "details": readable(report)});
    if truncated {
        truncation(&mut host, report.losses().len(), false);
    }
    host
}

/// Reserve a row for the omission notice; it participates in both host limits.
fn truncation(host: &mut Value, total: usize, details_omitted: bool) {
    if let Some(items) = host.get_mut("items").and_then(Value::as_array_mut) {
        if items.last().is_some_and(|item| {
            item.get("code").and_then(Value::as_str) == Some("dutchman.report.truncated")
        }) {
            items.pop();
        }
        if items.len() == 4096 {
            items.pop();
        }
        let omitted = total.saturating_sub(items.len());
        let detail = if details_omitted {
            "Full loss details are omitted."
        } else {
            "Full loss details remain under Details."
        };
        items.push(json!({
            "severity":"info", "title":"Conversion report shortened",
            "code":"dutchman.report.truncated", "subject":"report",
            "message":format!("{omitted} loss rows omitted from items; dropped and degraded losses are listed first. {detail} Convert fewer parts for a smaller report.")
        }));
        if let Some(details) = host.get_mut("details").and_then(Value::as_object_mut) {
            details.insert(
                "truncation".into(),
                json!({"omitted_items":omitted,"details_losses_omitted":details_omitted}),
            );
        }
    }
}

pub(crate) fn encode(report: &Report) -> Result<String, dutchman_core::Error> {
    let encode = |value: &Value| {
        serde_json::to_string(value).map_err(|e| dutchman_core::Error::Encode(e.to_string()))
    };
    let mut host = host(report);
    let text = encode(&host)?;
    if text.len() <= crate::MAX_REPORT_BYTES {
        return Ok(text);
    }
    if let Some(details) = host.get_mut("details").and_then(Value::as_object_mut) {
        details.remove("losses");
    }
    loop {
        truncation(&mut host, report.losses().len(), true);
        let text = encode(&host)?;
        if text.len() <= crate::MAX_REPORT_BYTES {
            return Ok(text);
        }
        let items = host
            .get_mut("items")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| dutchman_core::Error::Encode("Missing report items".into()))?;
        if items.len() <= 1 {
            return Err(dutchman_core::Error::Encode(
                "Conversion report exceeds 1 MiB; convert fewer parts at once".into(),
            ));
        }
        items.pop(); // The notice is regenerated after reducing loss rows.
        items.truncate(items.len() / 2);
    }
}

fn status(report: &Report) -> &'static str {
    if report.completeness().is_some_and(|c| !c.is_complete()) {
        return "Incomplete";
    }
    match report.outcome() {
        Some(o) if matches!(o.check, Check::Flattened(_)) => "Converted, no history",
        Some(o) => match o.mode {
            Mode::History => "Converted",
            Mode::PartialHistory => "Partly converted",
            Mode::Import => "Converted, no history",
        },
        None => "Converted",
    }
}
fn summary(report: &Report) -> String {
    if let Some(message) = report
        .completeness()
        .and_then(dutchman_core::Completeness::unplaced_message)
    {
        return format!("Incomplete. {message}");
    }
    let counts: Vec<_> = [
        (Severity::Info, "Info"),
        (Severity::Degraded, "Degraded"),
        (Severity::Dropped, "Dropped"),
    ]
    .into_iter()
    .filter_map(|(severity, label)| {
        let count = report
            .losses()
            .iter()
            .filter(|l| l.severity == severity)
            .count();
        (count > 0).then(|| format!("{count} {label}"))
    })
    .collect();
    if counts.is_empty() {
        status(report).into()
    } else {
        format!("{}; Losses: {}", status(report), counts.join(", "))
    }
}
fn verification(check: &Check) -> Value {
    let (result, reason) = match check {
        Check::Verified => ("Checked against the original shape", None),
        Check::Partial(s) => ("Partly checked", Some(s)),
        Check::Differs(s) => ("The rebuilt shape differs", Some(s)),
        Check::Unverified(s) => ("Not checked", Some(s)),
        Check::Flattened(s) => ("Original shape kept without editable history", Some(s)),
        Check::NotApplicable(s) => ("No editable history to check", Some(s)),
    };
    json!({"result": result, "reason": reason.map(|s| crate::notice::plain(s))})
}
/// The host renders every details field, so keep machine enums out of this surface.
pub(crate) fn readable(report: &Report) -> Value {
    json!({
        "schema": "dutchman.plugin.report/2",
        "result": status(report),
        "help": crate::notice::help(),
        "shape_check": report.outcome().map(|o| verification(&o.check)),
        "editable_part": report.outcome().filter(|o| o.mode == Mode::PartialHistory).and_then(|o| dutchman_core::RestImported::of(&o.features)).map(|rest| crate::notice::plain(&rest.sentence())),
        "features": report.outcome().map(|o| o.features.iter().map(|f| json!({
            "name": f.name.as_deref().map(crate::notice::plain),
            "feature": crate::notice::plain(&f.kind.replace('_', " ")),
            "result": match f.status { FeatureStatus::Mapped => "Converted", FeatureStatus::NotNeeded => "Not needed", FeatureStatus::LeftOut => "Left out", FeatureStatus::Blocked => "Not supported yet" },
            "reason": f.reason.as_deref().map(crate::notice::plain)
        })).collect::<Vec<_>>()),
        "assembly": report.completeness().map(|c| json!({"placed_parts": c.placed, "empty_parts": c.placed_empty, "unplaced_parts": c.unplaced.iter().map(|(name, count)| json!({"part": crate::notice::plain(name), "placements": count})).collect::<Vec<_>>(), "placement_summary": c.unplaced_message(), "missing_parts": c.left_out.iter().map(|p| json!({"part": p.file.as_deref().map(crate::notice::plain), "reason": crate::notice::plain(&p.reason), "placements": p.count})).collect::<Vec<_>>()})),
        "parts": report.parts().iter().map(|p| json!({"name": crate::notice::plain(&p.name), "shape_check": verification(&p.check), "converted_features": p.mapped, "source_features": p.total})).collect::<Vec<_>>(),
        "losses": report.losses().iter().map(|l| json!({"level": match l.severity { Severity::Info => "Info", Severity::Degraded => "Degraded", Severity::Dropped => "Dropped" }, "part_or_feature": crate::notice::plain(&l.subject), "reason": crate::notice::plain(&l.detail)})).collect::<Vec<_>>()
    })
}

/// At most `max` bytes, cut at a character boundary.
fn clip(text: &str, max: usize) -> String {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.get(..end).unwrap_or_default().to_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn unproven_positions_make_the_import_report_incomplete() {
        let mut report = Report::default();
        report.set_completeness(dutchman_core::Completeness {
            unplaced: [("Panel".into(), 1)].into(),
            ..Default::default()
        });
        let value = host(&report);
        assert_eq!(
            value["summary"],
            "Incomplete. 1 part couldn't be placed (its position in the archive couldn't be read)."
        );
        assert_eq!(value["details"]["result"], "Incomplete");
        assert_eq!(
            value["details"]["assembly"]["unplaced_parts"][0]["part"],
            "Panel"
        );
    }

    #[test]
    fn late_severe_losses_survive_item_and_byte_compaction() {
        let mut report = Report::default();
        for _ in 0..4096 {
            report.push(Severity::Info, "decode", "i".repeat(1024));
        }
        report.push(Severity::Degraded, "appearance", "Late appearance changed");
        report.push(Severity::Dropped, "shape", "Late shape dropped");
        assert!(serde_json::to_vec(&host(&report)).unwrap().len() > crate::MAX_REPORT_BYTES);
        let text = encode(&report).unwrap();
        assert!(text.len() <= crate::MAX_REPORT_BYTES);
        let value: Value = serde_json::from_str(&text).unwrap();
        let items = value["items"].as_array().unwrap();
        assert!(items.len() <= 4096);
        assert_eq!(items[0]["message"], "Late shape dropped");
        assert_eq!(items[1]["message"], "Late appearance changed");
        assert_eq!(items.last().unwrap()["code"], "dutchman.report.truncated");
        assert_eq!(
            value["details"]["truncation"]["omitted_items"],
            4098 - (items.len() - 1)
        );
        assert_eq!(
            value["details"]["truncation"]["details_losses_omitted"],
            true
        );
        assert!(value["details"].get("losses").is_none());
    }
}
