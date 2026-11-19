//! Target-aware forecasts from budgeted design decoding, without writing output.
use std::path::Path;
use std::time::Instant;

use dutchman_core::preflight::{
    BlockingFeature, Budget, Prediction, Reason, Scan, TargetPrediction,
};
use dutchman_core::{Check, FeatureStatus, Mode};
use dutchman_solidcraft::SolidCraftWriter;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Prefix,
    Planned,
}

#[derive(Debug, Serialize)]
pub struct Forecast {
    pub tier: Tier,
    pub scan: Scan,
    pub stop_reason: Option<Reason>,
    pub detail: Option<String>,
    pub targets: Vec<TargetPrediction>,
}

/// Forecast the default writer's mapping. Other targets abstain until they expose
/// their own planning path. Final replay and output failures remain conversion checks.
pub fn preflight(path: &Path, budget: Budget) -> Forecast {
    let started = Instant::now();
    let prepared = dutchman_core::preflight::prepare(path, budget, started);
    let mut result = Forecast {
        tier: Tier::Prefix,
        scan: prepared.scan,
        stop_reason: prepared.stop_reason,
        detail: prepared.detail,
        targets: Vec::new(),
    };
    result.targets = crate::json::PREFLIGHT_TARGETS
        .iter()
        .map(|target| result.scan.predict(target))
        .collect();
    let Some(model) = prepared.model else {
        if result.scan.reason == Reason::DecodeFailed {
            for prediction in &mut result.targets {
                if prediction.target == "dxf" {
                    prediction.outcome = Prediction::Unknown;
                    prediction.reason = Reason::TargetPlannerUnavailable;
                }
                prediction.detail = result.detail.clone();
            }
        }
        return result;
    };
    result.tier = Tier::Planned;
    let writer = SolidCraftWriter::new()
        .verification(dutchman_solidcraft::Verification::Off)
        .target_version(Some(Default::default()));
    let mut report = prepared.report;
    let deadline = started.checked_add(budget.max_time);
    let conversion = if started.elapsed() < budget.max_time {
        writer.convertible(&model)
    } else {
        Ok(())
    };
    let outcome = if conversion.is_ok() {
        match deadline {
            Some(deadline) => writer.plan_until(&model, &mut report, deadline),
            None => Some(writer.plan(&model, &mut report)),
        }
    } else {
        None
    };
    if started.elapsed() >= budget.max_time {
        result.stop_reason = Some(Reason::TimeBudget);
        result.scan.reason = Reason::TimeBudget;
        result.targets = crate::json::PREFLIGHT_TARGETS
            .iter()
            .map(|target| {
                let mut prediction = result.scan.predict(target);
                prediction.outcome = Prediction::Unknown;
                prediction.reason = Reason::TimeBudget;
                prediction
            })
            .collect();
        return result;
    }
    for prediction in &mut result.targets {
        prediction.outcome = Prediction::Unknown;
        prediction.reason = Reason::TargetPlannerUnavailable;
        if prediction.target != "sc" {
            continue;
        }
        prediction.reason = Reason::Planned;
        prediction.target_version = Some(writer.resolved_version().to_string());
        if let Err(ref why) = conversion {
            prediction.outcome = Prediction::Refused;
            prediction.detail = Some(why.clone());
            continue;
        }
        if let Some(ref outcome) = outcome {
            prediction.detail = match &outcome.check {
                Check::Partial(why)
                | Check::Unverified(why)
                | Check::Differs(why)
                | Check::Flattened(why)
                | Check::NotApplicable(why) => Some(why.clone()),
                Check::Verified => None,
            };
            prediction.outcome = match outcome.mode {
                Mode::History => Prediction::FullHistory,
                Mode::PartialHistory => Prediction::Partial,
                Mode::Import => Prediction::SolidOnly,
            };
            let active: Vec<_> = outcome
                .features
                .iter()
                .filter(|feature| feature.status != FeatureStatus::LeftOut)
                .collect();
            prediction.total_features = Some(active.len());
            prediction.mapped_features = Some(
                active
                    .iter()
                    .filter(|feature| {
                        matches!(
                            feature.status,
                            FeatureStatus::Mapped | FeatureStatus::NotNeeded
                        )
                    })
                    .count(),
            );
            prediction.first_blocking_feature = active
                .iter()
                .find(|feature| feature.status == FeatureStatus::Blocked)
                .map(|feature| BlockingFeature {
                    kind: feature.kind.clone(),
                    reason: feature
                        .reason
                        .clone()
                        .unwrap_or_else(|| "This feature stops the mapped history.".to_owned()),
                });
            let kinds: std::collections::BTreeSet<_> = outcome
                .features
                .iter()
                .map(|feature| feature.kind.clone())
                .collect();
            prediction.driving_feature_kinds = Some(kinds.into_iter().collect());
        }
    }
    // A mapper's subset is not a complete census of the source's feature kinds.
    if result.scan.feature_count == outcome.as_ref().map(|outcome| outcome.features.len()) {
        result.scan.feature_kinds = result
            .targets
            .iter()
            .find(|target| target.target == "sc")
            .and_then(|target| target.driving_feature_kinds.clone());
    }
    result
}
