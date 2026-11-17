//! Retain the longest editable timeline before a mapping or emitter refusal.
use crate::{build::stop_from, row, Emitter, Plan, Step, Stop};
use dutchman_core::{FeatureOutcome, FeatureStatus, Model, Report};

/// Remap the source prefix without deleting source arenas or topology.
pub fn recover_prefix(model: &Model, outcomes: &[FeatureOutcome]) -> Option<(Vec<Step>, Stop)> {
    if outcomes.len() > 100_000 || model.ir.model.features.len() > 100_000 {
        return None;
    }
    let boundary = outcomes
        .iter()
        .position(|o| o.status == FeatureStatus::Blocked)
        .unwrap_or(outcomes.len());
    let mut prefix = model.clone();
    prefix.ir.model.features.sort_by_key(|f| f.ordinal);
    for feature in prefix.ir.model.features.iter_mut().skip(boundary) {
        feature.suppressed = Some(true);
    }
    let mapping = dutchman_solidcraft::mapped_history(&prefix, &mut Report::default());
    let rows = mapping.features.unwrap_or(mapping.sketches);
    if rows.len() > 100_000 {
        return None;
    }
    let mut steps = Vec::new();
    let mut stop = stop_from(
        outcomes,
        0,
        "an earlier feature cannot be rebuilt; kept the final STEP geometry".into(),
    );
    let context = row::Context::new(&mapping.parameter_values, outcomes, &rows);
    for r in &rows {
        match context.step(r) {
            Ok(step) => steps.push(step),
            Err(reason) => {
                stop.feature = r
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("history")
                    .into();
                stop.reason = reason;
                break;
            }
        }
    }
    stop.step_index = steps.len() as u32;
    (!steps.is_empty()).then_some((steps, stop))
}

/// Retain the longest prefix supported by the emitter and check reference sketches.
pub fn supported_prefix(emitter: &impl Emitter, plan: &mut Plan) -> usize {
    let mut upto = plan.steps.len();
    for (index, step) in plan.steps.iter().enumerate() {
        let support = check_support(emitter, step, plan);
        if let Err(reason) = support {
            upto = index;
            if plan
                .stop
                .as_ref()
                .is_none_or(|stop| stop.step_index as usize > index)
            {
                plan.stop = Some(Stop {
                    step_index: index as u32,
                    feature: step.name.clone(),
                    reason,
                });
            }
            break;
        }
    }
    if let Some(reason) = plan.stop.as_ref().map(|stop| stop.reason.clone()) {
        block_after(plan, upto, &reason);
    }
    let sketch_outcomes: Vec<_> = plan
        .outcomes
        .iter()
        .enumerate()
        .filter(|(_, o)| o.kind == "sketch" && o.status == "mapped")
        .map(|(index, _)| index)
        .collect();
    let rejected: Vec<_> = plan
        .sketches
        .iter()
        .enumerate()
        .filter_map(|(index, step)| {
            check_support(emitter, step, plan)
                .err()
                .map(|reason| (step.id, sketch_outcomes.get(index).copied(), reason))
        })
        .collect();
    for (_, index, reason) in &rejected {
        if let Some(outcome) = index.and_then(|i| plan.outcomes.get_mut(i)) {
            outcome.status = crate::OutcomeStatus::Blocked;
            outcome.reason = Some(reason.clone());
        }
    }
    plan.sketches
        .retain(|s| !rejected.iter().any(|(id, _, _)| *id == s.id));
    upto
}

pub(crate) fn block_after(plan: &mut Plan, upto: usize, reason: &str) {
    // A separately refused reference sketch does not cut a valid building prefix.
    let boundary = plan
        .outcomes
        .iter()
        .position(|o| {
            o.status == "blocked"
                && (o.kind != "sketch"
                    || plan
                        .stop
                        .as_ref()
                        .is_some_and(|stop| o.name.as_deref() == Some(stop.feature.as_str())))
        })
        .unwrap_or(plan.outcomes.len());
    let mut remaining = plan.steps.iter().take(upto).peekable();
    for (index, outcome) in plan.outcomes.iter_mut().enumerate() {
        if outcome.status != "mapped" {
            continue;
        }
        if outcome.kind != "sketch" {
            while remaining.peek().is_some_and(|s| s.kind == "sketch") {
                let _ = remaining.next();
            }
        }
        let retained = index < boundary && remaining.peek().is_some_and(|s| s.kind == outcome.kind);
        if retained {
            let _ = remaining.next();
            outcome.reason = Some("rebuilt as reference in partial history".into());
        } else if outcome.kind == "sketch" {
            outcome.reason = Some("kept as a hidden reference sketch".into());
        } else {
            outcome.status = crate::OutcomeStatus::Blocked;
            outcome.reason = Some(reason.into());
        }
    }
}

/// Check the emitter and every untyped option before a step can be lowered.
pub fn check_support(emitter: &impl Emitter, step: &Step, plan: &Plan) -> Result<(), String> {
    emitter.supports(step, plan).map_err(|reason| {
        if reason.starts_with("feature ") {
            reason
        } else {
            format!("feature {}: {reason}", step.name)
        }
    })?;
    use serde_json::{Map, Value};
    fn keys(map: &Map<String, Value>, prefix: &str, paths: &mut Vec<String>) {
        for (key, value) in map {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            if let Some(object) = value.as_object().filter(|o| !o.is_empty()) {
                keys(object, &path, paths);
            } else {
                paths.push(path);
            }
        }
    }
    let mut paths = Vec::new();
    keys(&step.extra, "", &mut paths);
    let fields = &step.fields;
    macro_rules! record {
        ($value:expr, $path:expr) => {
            if let Some(record) = $value {
                keys(&record.extra, $path, &mut paths);
            }
        };
    }
    record!(&fields.extent, "extent");
    record!(&fields.form, "form");
    record!(&fields.layout, "layout");
    if let Some(crate::Points::Sketch(points)) = &fields.points {
        for p in points {
            keys(&p.extra, "points", &mut paths);
        }
    }
    for c in fields.curves.iter().flatten() {
        keys(&c.extra, "curves", &mut paths);
    }
    for c in fields.constraints.iter().flatten() {
        keys(&c.extra, "constraints", &mut paths);
    }
    for c in fields.sections.iter().flatten() {
        keys(&c.extra, "sections", &mut paths);
    }
    for path in paths {
        if !emitter.handled_options(&step.kind).contains(&path.as_str()) {
            return Err(format!(
                "feature {}: option {path} is not supported yet",
                step.name
            ));
        }
    }
    Ok(())
}
