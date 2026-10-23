//! Preserve editable references without replacing the final embedded geometry.
use std::collections::BTreeSet;

use dutchman_core::{Error, FeatureOutcome, FeatureStatus, Model, Report, Severity};
use serde_json::Value;

use crate::generate;

pub(crate) struct Prefix {
    rows: Vec<Value>,
    sketches: Vec<Value>,
    mapped: usize,
    total: usize,
}

pub(crate) fn recover(
    model: &Model,
    rows: Option<Vec<Value>>,
    sketches: Vec<Value>,
    outcomes: &mut [FeatureOutcome],
) -> Option<Prefix> {
    if outcomes.len() > 100_000 || sketches.len() > 100_000 {
        return None;
    }
    let boundary = outcomes
        .iter()
        .position(|feature| feature.status == FeatureStatus::Blocked)
        .unwrap_or(outcomes.len());
    let rows = if let Some(rows) = rows {
        rows
    } else {
        // Keep the original arenas and topology. Only evaluation of the suffix is disabled;
        // references, ownership and feature ids still resolve through the normal mapper.
        let mut prefix = model.clone();
        prefix
            .ir
            .model
            .features
            .sort_by_key(|feature| feature.ordinal);
        for feature in prefix.ir.model.features.iter_mut().skip(boundary) {
            feature.suppressed = Some(true);
        }
        let mapping = dutchman_solidcraft::mapped_history(&prefix, &mut Report::default());
        mapping.features.unwrap_or(mapping.sketches)
    };
    let mut accepted = Vec::new();
    let mut names = BTreeSet::new();
    let mut refusal = None;
    for row in rows {
        if accepted.len() == 100_000 {
            refusal = Some("more than 100000 mapped features are not supported yet".to_owned());
            break;
        }
        let name = row.get("name").and_then(Value::as_str).unwrap_or("");
        let result = if names.insert(name.to_owned()) {
            generate::history(std::slice::from_ref(&row))
        } else {
            Err(format!(
                "feature {name:?}: duplicate feature names are not supported yet"
            ))
        };
        match result {
            Ok(_) => accepted.push(row),
            Err(reason) => {
                refusal = Some(reason);
                break;
            }
        }
    }
    // Count source features, not generated helper rows. Not-needed and suppressed rows
    // consume no FreeCAD feature and do not stop the prefix.
    let mut remaining = accepted.iter().peekable();
    let mut mapped = 0;
    let mut sketch_checks = sketches
        .iter()
        .map(|row| generate::history(std::slice::from_ref(row)).err());
    for (index, feature) in outcomes.iter_mut().enumerate() {
        if feature.status != FeatureStatus::Mapped {
            continue;
        }
        if feature.kind == "sketch" {
            if let Some(Some(reason)) = sketch_checks.next() {
                feature.status = FeatureStatus::Blocked;
                feature.reason = Some(reason);
                continue;
            }
        }
        // A loft can insert helper sketch rows before its own row. Those do not count
        // as source features, and must not make a refused loft look rebuilt.
        if feature.kind != "sketch" {
            while remaining
                .peek()
                .is_some_and(|row| row.get("type").and_then(Value::as_str) == Some("sketch"))
            {
                let _ = remaining.next();
            }
        }
        let retained = index < boundary
            && remaining.peek().is_some_and(|row| {
                row.get("type").and_then(Value::as_str) == Some(feature.kind.as_str())
            });
        if retained {
            let _ = remaining.next();
            mapped += 1;
            feature.reason = Some("rebuilt as reference in partial history".to_owned());
        } else if feature.kind == "sketch" {
            feature.reason = Some("kept as a hidden reference sketch".to_owned());
        } else {
            feature.status = FeatureStatus::Blocked;
            feature.reason = Some(refusal.clone().unwrap_or_else(|| {
                "an earlier feature cannot be rebuilt; kept the final STEP geometry".to_owned()
            }));
        }
    }
    let sketches: Vec<_> = sketches
        .into_iter()
        .filter(|row| generate::history(std::slice::from_ref(row)).is_ok())
        .collect();
    if accepted.is_empty() && sketches.is_empty() {
        return None;
    }
    Some(Prefix {
        rows: accepted,
        sketches,
        mapped,
        total: outcomes
            .iter()
            .filter(|feature| {
                !matches!(
                    feature.status,
                    FeatureStatus::LeftOut | FeatureStatus::NotNeeded
                )
            })
            .count(),
    })
}

impl Prefix {
    pub(crate) fn write(
        self,
        script: &mut String,
        name: &str,
        report: &mut Report,
    ) -> Result<(), Error> {
        let note = format!(
            "rebuilt {} of {} features; kept as reference",
            self.mapped, self.total
        );
        report.push(Severity::Degraded, name, &note);
        script.push_str(PRELUDE);
        script.push_str("selected = partial_history(");
        script.push_str(&generate::data(&Value::Array(self.rows))?);
        script.push_str(", ");
        script.push_str(&generate::data(&Value::Array(self.sketches))?);
        script.push_str(", ");
        script.push_str(&generate::quoted(name)?);
        script.push_str(", original)\n");
        Ok(())
    }
}

const PRELUDE: &str = r#"
def partial_history(rows, sketches, label, original):
    group = doc.addObject('App::DocumentObjectGroup', 'PartialHistory')
    group.Label = 'Rebuilt history (partial)'
    attach(group)
    body = doc.addObject('PartDesign::Body', 'Body')
    body.Label = label
    group.addObject(body)
    objects, names = {}, {}
    tip = None
    completed = 0
    for row in rows:
        try:
            tip = rebuild(row, body, objects, names, tip)
            completed += 1
        except Exception as error:
            App.Console.PrintWarning('Partial rebuild stopped: ' + str(error) + '\n')
            break
    for row in sketches:
        if row['id'] not in objects:
            try:
                rebuild(row, body, objects, names, tip)
            except Exception as error:
                App.Console.PrintWarning('Reference sketch failed: ' + str(error) + '\n')
    if tip:
        body.Tip = tip
    doc.recompute()
    for obj in body.Group:
        visible(obj, False)
    visible(body, False)
    visible(group, False)
    visible(original, True)
    # Compare only the last successfully rebuilt tip with the final STEP shape. Reference
    # sketches and intermediate features never participate in the final body's measures.
    if tip and completed == len(rows):
        try:
            compare_original(tip.Shape, original.Shape)
            visible(group, True)
            visible(body, True)
            visible(tip, True)
            visible(original, False)
            return tip
        except Exception:
            pass
    return original

"#;
