//! Finalize links only after retention and id allocation have been decided.

use std::collections::BTreeSet;

use dutchman_core::{FeatureOutcome, FeatureStatus, OutputMatch};
use serde_json::Value;

pub(crate) fn not_written(rows: &mut [FeatureOutcome]) {
    for row in rows {
        row.output.ids.clear();
        row.output.method = OutputMatch::NotWritten;
    }
}

pub(crate) fn retain(rows: &mut [FeatureOutcome], features: &[Value], offset: u64) {
    let retained: BTreeSet<u64> = features
        .iter()
        .filter_map(|feature| feature.get("id").and_then(Value::as_u64))
        .collect();
    for row in rows {
        row.output.ids = row
            .output
            .ids
            .iter()
            .filter(|id| row.status == FeatureStatus::Mapped && retained.contains(*id))
            .filter_map(|id| id.checked_add(offset))
            .collect();
        row.output.method = if row.output.ids.is_empty() {
            OutputMatch::NoOutput
        } else {
            OutputMatch::Emitted
        };
    }
}

pub(crate) fn history(rows: &mut [FeatureOutcome], history: &crate::history::History, offset: u64) {
    for row in rows.iter_mut() {
        row.output.ids.clear();
    }
    for feature in &history.features {
        if let Some(id) = feature.get("id").and_then(Value::as_u64) {
            if let Some(row) = history
                .sources
                .get(&id)
                .and_then(|index| rows.get_mut(*index))
            {
                row.output.ids.push(id);
            }
        }
    }
    retain(rows, &history.features, offset);
}
