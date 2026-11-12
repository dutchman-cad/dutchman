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
