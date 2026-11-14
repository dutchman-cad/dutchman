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
    let items: Vec<_> = report
        .losses()
        .iter()
        .take(MAX_ITEMS)
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
    json!({"summary": clip(&summary(report), 240), "items": items, "details": json(report)})
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
