use dutchman_core::{Check, FeatureStatus, Loss, Mode, PlainView, Report, Severity};
use serde_json::{json, Value};

/// How a loss reads in the default view (see `dutchman_core::PLAIN_NOTES`).
fn view(loss: &Loss) -> PlainView {
    dutchman_core::plain_view(
        loss.severity,
        loss.code.as_deref(),
        &loss.subject,
        &loss.detail,
    )
}

/// The losses the default view lists; the others are in `details.losses` only.
fn shown(report: &Report) -> Vec<&Loss> {
    report
        .losses()
        .iter()
        .filter(|loss| view(loss).shown)
        .collect()
}
/// The loss report SolidCraft shows: a one-line summary, one item per loss, and plain-language
/// conversion details.
pub(crate) fn host(report: &Report) -> Value {
    const MAX_ITEMS: usize = 4096;
    let mut losses = shown(report);
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
                // The plain sentence leads; the code and message below it are the details.
                "title": clip(&crate::notice::plain(&view(loss).summary), 120),
                "code": clip(code, 128),
                "subject": clip(&crate::notice::plain(subject), 256),
                // The host refuses a blank message.
                "message": clip(&crate::notice::plain(if loss.detail.trim().is_empty() { "No further detail" } else { &loss.detail }), 1024),
            })
        })
        .collect();
    let mut host = json!({"summary": clip(&summary(report), 240), "items": items, "details": readable(report)});
    if truncated {
        truncation(&mut host, shown(report).len(), false);
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
        truncation(&mut host, shown(report).len(), true);
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
    if report.missing_links_message().is_some()
        || report.completeness().is_some_and(|c| !c.is_complete())
    {
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
    if let Some(why) = report.missing_links_message() {
        return format!("Incomplete. {why}");
    }
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
        let count = shown(report)
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
    let meaning =
        matches!(check, Check::Unverified(_) | Check::Partial(_)).then_some(UNVERIFIED_MEANING);
    let mut shape_check =
        json!({"result": result, "reason": reason.map(|s| crate::notice::plain(s))});
    if let (Some(meaning), Some(object)) = (meaning, shape_check.as_object_mut()) {
        object.insert("meaning".to_string(), json!(meaning));
    }
    shape_check
}

/// What an unchecked history is, said next to it: not a warning, but what was not checked and
/// how to check it. The plugin runs without SolidCraft's command line, so it never replays a
/// history itself.
const UNVERIFIED_MEANING: &str = "Not checked means this history was written without SolidCraft replaying it and comparing the result with the original shape. It is not a warning that the history is wrong. The plugin cannot run that check; to check it, convert the file with Dutchman Convert with \"Check feature history in SolidCraft\" turned on, and add a STEP export of the same design.";
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
        "losses": report.losses().iter().map(|l| json!({"level": match l.severity { Severity::Info => "Info", Severity::Degraded => "Degraded", Severity::Dropped => "Dropped" }, "part_or_feature": crate::notice::plain(&l.subject), "reason": crate::notice::plain(&l.detail), "summary": crate::notice::plain(&view(l).summary), "shown_by_default": view(l).shown})).collect::<Vec<_>>()
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
    fn missing_inserted_designs_make_the_import_report_incomplete() {
        let mut report = Report::default();
        report.record_missing_inserted_designs(&["Washer".into()]);
        let value = host(&report);
        // The host's one-line summary is at most 240 bytes: it keeps the start of the sentence.
        let summary = value["summary"].as_str().unwrap();
        assert!(summary.len() <= 240, "{summary}");
        assert!(
            summary.starts_with(
                "Incomplete. 1 inserted component comes from other Fusion designs that this .f3d \
                 doesn't contain; convert the whole export folder (or its zip) so the inserted"
            ),
            "{summary}"
        );
        assert_eq!(value["details"]["result"], "Incomplete");
    }

    #[test]
    fn fusion_configuration_and_dimension_notes_lead_with_their_whole_sentence() {
        let mut report = Report::default();
        for (code, message) in [
            (
                "f3d/configuration.active-row-unbound",
                "1 Design configuration root record(s) were retained without an admitted active row of a unique table document.",
            ),
            (
                "f3d/dimension.companion-untyped",
                "2 payload-bearing Design dimension companion(s) were retained without a typed locus frame.",
            ),
        ] {
            report.push_coded(Severity::Degraded, code, code, message);
        }
        let value = host(&report);
        let items = value["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        for item in items {
            let code = item["code"].as_str().unwrap();
            let sentence = dutchman_core::row_for(Some(code), code, "")
                .unwrap()
                .sentence;
            // Not clipped: the title is the whole sentence, and the raw message is the detail.
            assert_eq!(item["title"], sentence, "{code}");
            assert_ne!(item["message"], sentence, "{code}");
        }
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
