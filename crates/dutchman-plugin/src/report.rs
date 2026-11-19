use dutchman_core::{Check, FeatureStatus, Mode, Report, Severity, Stage};
use serde_json::{json, Value};
fn check(check: &Check) -> Value {
    let (status, message) = match check {
        Check::Verified => ("verified", None),
        Check::Partial(s) => ("partial", Some(s)),
        Check::Differs(s) => ("differs", Some(s)),
        Check::Unverified(s) => ("unverified", Some(s)),
        Check::Flattened(s) => ("flattened", Some(s)),
        Check::NotApplicable(s) => ("not_applicable", Some(s)),
    };
    json!({"status":status,"message":message})
}
pub(crate) fn json(report: &Report) -> Value {
    let losses: Vec<_> = report.losses().iter().map(|loss| json!({
        "stage":loss.stage.id(),
        "severity":match loss.severity { Severity::Info=>"info", Severity::Degraded=>"degraded", Severity::Dropped=>"dropped" },
        "subject":loss.subject, "code":loss.code, "message":loss.detail,
        "upstream":loss.upstream(),
    })).collect();
    let stages: Vec<_> = Stage::ALL.iter().map(|stage| {
        let mut info=0; let mut degraded=0; let mut dropped=0;
        for loss in report.losses().iter().filter(|l| l.stage==*stage) {
            match loss.severity { Severity::Info=>info+=1, Severity::Degraded=>degraded+=1, Severity::Dropped=>dropped+=1 }
        }
        json!({"stage":stage.id(),"total":info+degraded+dropped,"info":info,"degraded":degraded,"dropped":dropped})
    }).collect();
    let outcome = report.outcome().map(|o| json!({
        "mode":match o.mode { Mode::History=>"history", Mode::PartialHistory=>"partial_history", Mode::Import=>"import" },
        "verification":check(&o.check),
        "features":o.features.iter().map(|f| json!({"name":f.name,"kind":f.kind,"ordinal":f.ordinal,"variant":f.variant,
            "status":match f.status { FeatureStatus::Mapped=>"mapped",FeatureStatus::NotNeeded=>"not_needed",FeatureStatus::LeftOut=>"left_out",FeatureStatus::Blocked=>"blocked" },
            "message":f.reason,"profile_unverified":f.profile_unverified,"reapplied":f.reapplied,"final_body":f.final_body})).collect::<Vec<_>>()
    }));
    json!({"schema":"dutchman.plugin.loss/1", "stages":stages,"losses":losses,"outcome":outcome,
        "parts":report.parts().iter().map(|p| json!({"name":p.name,"verification":check(&p.check),"mapped":p.mapped,"total":p.total,"rebuilt":p.rebuilt,"needing":p.needing,"reapplied":p.reapplied})).collect::<Vec<_>>(),
        "completeness":report.completeness().map(|c| json!({"placed":c.placed,"placed_empty":c.placed_empty,"left_out":c.left_out.iter().map(|l| json!({"file":l.file,"message":l.reason,"stage":l.stage.id(),"count":l.count})).collect::<Vec<_>>()})),
        "joints":report.joints()})
}

/// The loss report SolidCraft shows: a one-line summary, one item per loss, and the full
/// `dutchman.plugin.loss/1` report under `details`.
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
                "title": clip(&loss.title(), 120),
                "code": clip(code, 128),
                "subject": clip(subject, 256),
                // The host refuses a blank message.
                "message": clip(
                    [loss.detail.as_str(), loss.code.as_deref().unwrap_or_default(), loss.subject.as_str()]
                        .into_iter()
                        .find(|text| !text.trim().is_empty())
                        .unwrap_or("No further detail"),
                    1024,
                ),
            })
        })
        .collect();
    let mut host =
        json!({"summary": clip(&summary(report), 240), "items": items, "details": json(report)});
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

fn summary(report: &Report) -> String {
    let losses = report.losses().len();
    let losses = match losses {
        0 => String::new(),
        1 => "; 1 loss".to_owned(),
        n => format!("; {n} losses"),
    };
    let Some(outcome) = report.outcome() else {
        return format!("Converted{losses}");
    };
    let needing = outcome
        .features
        .iter()
        .filter(|f| f.status != FeatureStatus::NotNeeded)
        .count();
    let mapped = outcome
        .features
        .iter()
        .filter(|f| f.status == FeatureStatus::Mapped)
        .count();
    match outcome.mode {
        Mode::History => {
            format!("Editable history: {mapped} of {needing} features mapped{losses}")
        }
        Mode::PartialHistory => format!(
            "Partial editable history: {mapped} of {needing} features mapped, original shape visible{losses}"
        ),
        Mode::Import => format!("Geometry without editable history{losses}"),
    }
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
