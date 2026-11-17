use crate::{row, Fallback, Outcome, Parameter, Plan, Quantity, Source, Stop, Units, SCHEMA};
use dutchman_core::{FeatureOutcome, FeatureStatus, Format, Model, Report};
use dutchman_solidcraft::MappedHistory;
use serde_json::Value;

impl Plan {
    /// Map a neutral model through the existing guarded history mapper.
    pub fn from_model(model: &Model, report: &mut Report) -> Self {
        let mut mapping_report = Report::default();
        let mapping = dutchman_solidcraft::mapped_history(model, &mut mapping_report);
        let plan = Self::from_mapping(model, mapping);
        report.append(mapping_report);
        plan
    }
    /// Build a plan from already mapped rows and outcomes.
    pub fn from_mapped(model: &Model, mapping: MappedHistory) -> Self {
        Self::from_mapping(model, mapping)
    }

    pub(crate) fn from_mapping(model: &Model, mapping: MappedHistory) -> Self {
        let source_format = model.ir.source.as_ref().map_or("cadir", |s| s.format());
        let format = model
            .source_file
            .as_ref()
            .and_then(|f| Format::from_path(std::path::Path::new(f)))
            .or_else(|| Format::from_codec_id(source_format))
            .or_else(|| {
                Format::ALL
                    .into_iter()
                    .find(|format| format.id() == source_format)
            });
        let mut plan = Self {
            schema: SCHEMA.into(),
            generator: format!("dutchman {}", env!("CARGO_PKG_VERSION")),
            part: model.name.clone(),
            units: Units {
                length: "mm".into(),
                angle: "deg".into(),
            },
            source: Source {
                format: format.map_or("cadir", |f| f.id()).into(),
                program_version: None,
                sha256: None,
            },
            parameters: Vec::new(),
            steps: Vec::new(),
            sketches: Vec::new(),
            stop: None,
            fallback: Fallback {
                step_file: "original.step".into(),
            },
            expected: None,
            outcomes: mapping.outcomes.iter().map(outcome).collect(),
        };
        if model.ir.model.parameters.len() > 100_000
            || mapping.sketches.len() > 100_000
            || mapping.outcomes.len() > 100_000
        {
            plan.stop = Some(Stop {
                step_index: 0,
                feature: "history".into(),
                reason: "replay data exceeds the script limit".into(),
            });
            return plan;
        }
        let mut used = std::collections::BTreeSet::new();
        // Without the mapper's name pairs, source links are safe only when every
        // source name survives unchanged and is unique. Otherwise keep written data.
        let mut source_names = std::collections::BTreeSet::new();
        let unchanged_names = model.ir.model.parameters.iter().all(|p| {
            source_names.insert(p.name.as_str()) && mapping.parameter_values.contains_key(&p.name)
        });
        let mut parameters: Vec<_> = model.ir.model.parameters.iter().collect();
        parameters.sort_by_key(|p| (p.owner.is_some(), p.ordinal));
        for parameter in parameters {
            let source = parameter.name.as_str();
            let name = unchanged_names.then_some(source);
            let Some(name) = name else {
                continue;
            };
            used.insert(name.to_owned());
            let solved = mapping
                .parameter_values
                .get(name)
                .map(String::as_str)
                .unwrap_or("");
            let evaluated = Quantity::from_row(solved, &mapping.parameter_values).value;
            let kind = if ["deg", "rad"]
                .iter()
                .any(|u| solved.trim_end().ends_with(u))
            {
                "angle"
            } else if ["mm", "cm", "m", "in", "ft"]
                .iter()
                .any(|u| solved.trim_end().ends_with(u))
            {
                "length"
            } else {
                "number"
            };
            plan.parameters.push(Parameter {
                name: name.into(),
                expr: parameter.expression.clone(),
                value: evaluated,
                kind: kind.into(),
                source: Some(source.into()),
            });
        }
        // Written values without a proven source identity retain mapped text.
        for (name, text) in &mapping.parameter_values {
            if used.insert(name.clone()) {
                plan.parameters.push(Parameter {
                    name: name.clone(),
                    expr: text.clone(),
                    value: Quantity::from_row(text, &mapping.parameter_values).value,
                    kind: if text.trim_end_matches([')', ' ']).ends_with("deg") {
                        "angle"
                    } else if ["mm", "cm", "m", "in", "ft"]
                        .iter()
                        .any(|unit| text.trim_end_matches([')', ' ']).ends_with(unit))
                    {
                        "length"
                    } else {
                        "number"
                    }
                    .into(),
                    source: None,
                });
            }
        }
        match mapping.features {
            Ok(rows) => {
                let context =
                    row::Context::new(&mapping.parameter_values, &mapping.outcomes, &rows);
                for row in rows.iter().take(100_001) {
                    let result = if plan.steps.len() == 100_000 {
                        Err("more than 100000 mapped features are not supported yet".into())
                    } else {
                        context.step(row)
                    };
                    match result {
                        Ok(step) => plan.steps.push(step),
                        Err(reason) => {
                            plan.stop = Some(Stop {
                                step_index: plan.steps.len() as u32,
                                feature: row
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or("unknown")
                                    .into(),
                                reason,
                            });
                            break;
                        }
                    }
                }
            }
            Err(reason) => {
                if let Some((steps, stop)) = crate::prefix::recover_prefix(model, &mapping.outcomes)
                {
                    plan.steps = steps;
                    plan.stop = Some(stop);
                } else {
                    plan.stop = Some(stop_from(&mapping.outcomes, 0, reason));
                }
            }
        }
        let context = row::Context::new(
            &mapping.parameter_values,
            &mapping.outcomes,
            &mapping.sketches,
        );
        let sketch_outcomes: Vec<_> = mapping
            .outcomes
            .iter()
            .enumerate()
            .filter(|(_, o)| o.kind == "sketch" && o.status == FeatureStatus::Mapped)
            .map(|(index, _)| index)
            .collect();
        for (index, row) in mapping.sketches.iter().enumerate() {
            match context.step(row) {
                Ok(step) => plan.sketches.push(step),
                Err(reason) => {
                    if let Some(outcome) = sketch_outcomes
                        .get(index)
                        .and_then(|i| plan.outcomes.get_mut(*i))
                    {
                        outcome.status = crate::OutcomeStatus::Blocked;
                        outcome.reason = Some(reason);
                    }
                }
            }
        }
        if let Some(reason) = plan.stop.as_ref().map(|stop| stop.reason.clone()) {
            let upto = plan.steps.len();
            crate::prefix::block_after(&mut plan, upto, &reason);
        }
        let names: std::collections::BTreeSet<_> =
            plan.parameters.iter().map(|p| p.name.as_str()).collect();
        if let Some(index) = plan
            .steps
            .iter()
            .position(|step| missing_parameter(step, &names))
        {
            if let Some(step) = plan.steps.get(index) {
                plan.stop = Some(Stop {
                    step_index: index as u32,
                    feature: step.name.clone(),
                    reason: format!("feature {}: a parameter reference was not found", step.name),
                });
            }
            plan.steps.truncate(index);
        }
        for step in &plan.sketches {
            if missing_parameter(step, &names) {
                for outcome in &mut plan.outcomes {
                    if outcome.name.as_deref() == Some(step.name.as_str()) {
                        outcome.status = crate::OutcomeStatus::Blocked;
                        outcome.reason = Some(format!(
                            "feature {}: a parameter reference was not found",
                            step.name
                        ));
                    }
                }
            }
        }
        plan.sketches
            .retain(|step| !missing_parameter(step, &names));
        if let Some(reason) = plan.stop.as_ref().map(|stop| stop.reason.clone()) {
            let upto = plan.steps.len();
            crate::prefix::block_after(&mut plan, upto, &reason);
        }
        plan
    }
}
pub(crate) fn stop_from(outcomes: &[FeatureOutcome], index: usize, reason: String) -> Stop {
    let blocked = outcomes.iter().find(|o| o.status == FeatureStatus::Blocked);
    Stop {
        step_index: index as u32,
        feature: blocked
            .and_then(|o| o.name.clone())
            .unwrap_or_else(|| "history".into()),
        reason: blocked.and_then(|o| o.reason.clone()).unwrap_or(reason),
    }
}
pub(crate) fn outcome(o: &FeatureOutcome) -> Outcome {
    Outcome {
        name: o.name.clone(),
        kind: o.kind.clone(),
        status: match o.status {
            FeatureStatus::Mapped => crate::OutcomeStatus::Mapped,
            FeatureStatus::NotNeeded => crate::OutcomeStatus::NotNeeded,
            FeatureStatus::LeftOut => crate::OutcomeStatus::LeftOut,
            FeatureStatus::Blocked => crate::OutcomeStatus::Blocked,
        },
        reason: o.reason.clone(),
    }
}

fn missing_parameter(step: &crate::Step, names: &std::collections::BTreeSet<&str>) -> bool {
    let missing = |quantity: &Quantity| {
        quantity
            .param
            .as_deref()
            .is_some_and(|p| !names.contains(p))
    };
    let f = &step.fields;
    let quantities = [
        &f.angle,
        &f.diameter,
        &f.radius,
        &f.distance,
        &f.thickness,
        &f.pitch,
        &f.turns,
        &f.section_size,
        &f.start_angle,
        &f.major,
        &f.minor,
    ];
    if quantities.into_iter().flatten().any(missing) {
        return true;
    }
    if let Some(e) = &f.extent {
        if [&e.distance, &e.distance2, &e.depth]
            .into_iter()
            .flatten()
            .any(missing)
        {
            return true;
        }
    }
    if let Some(e) = &f.form {
        if [
            &e.tip_angle,
            &e.cb_diameter,
            &e.cb_depth,
            &e.cs_diameter,
            &e.cs_angle,
        ]
        .into_iter()
        .flatten()
        .any(missing)
        {
            return true;
        }
    }
    if let Some(e) = &f.layout {
        if [
            &e.count1,
            &e.spacing1,
            &e.count2,
            &e.spacing2,
            &e.count,
            &e.angle,
        ]
        .into_iter()
        .flatten()
        .any(missing)
        {
            return true;
        }
    }
    let mut plane = f.plane.as_ref();
    while let Some(crate::Plane::Offset { base, distance }) = plane {
        if missing(distance) {
            return true;
        }
        plane = Some(base);
    }
    f.constraints
        .iter()
        .flatten()
        .filter_map(|c| c.value.as_ref())
        .any(missing)
}
