//! Base features and stored geometry: bodies a timeline holds without the steps that made
//! them (modelled directly, or imported), as SolidCraft Import features at their place in the
//! timeline, so the features after them keep their history.
//!
//! The bodies are the design's as it ends, so a base body a later feature changes is imported
//! changed already; the check against the original geometry then finds the replay differs.

use serde_json::{json, Value};

use dutchman_core::{Model, Report, Severity};

use super::{describe, name, Context, History};
use crate::container::Container;
use crate::step;

/// Stored solids have no construction recipe to lose.
pub(crate) const STORED_SOLID: &str =
    "This part holds an imported solid; there is no feature history to bring over";

/// Where a mapped base feature keeps the source bodies it imports, until they are restated.
const BODIES: &str = "_bodies";
/// Where it keeps their STEP text, until the text is stored in the container.
const STEP: &str = "_step";

/// Maps a base feature whose operation is `definition`.
pub(super) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let bodies = definition.get("bodies").unwrap_or(&Value::Null);
    if bodies.get("kind").and_then(Value::as_str) != Some("resolved") {
        return Err("its bodies are not resolved".to_owned());
    }
    let ids: Vec<&str> = bodies
        .pointer("/value/bodies")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    import(feature, &ids, ctx)
}

/// Stored geometry names its retained bodies in the feature's outputs, not its operation.
pub(super) fn stored(
    feature: &Value,
    ctx: &mut Context,
    report: &mut Report,
) -> Result<Value, String> {
    let ids: Vec<&str> = feature
        .get("outputs")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let mapped = import(feature, &ids, ctx)?;
    report.push(Severity::Info, describe(feature), STORED_SOLID);
    Ok(mapped)
}

fn import(feature: &Value, ids: &[&str], ctx: &mut Context) -> Result<Value, String> {
    if ids.is_empty() {
        return Err("it holds no bodies".to_owned());
    }
    let id = ctx.id();
    ctx.builds = true;
    let name = name(feature)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Base Feature{id}"));
    Ok(json!({
        "id": id,
        "name": name,
        "type": "import",
        "suppressed": false,
        "file": format!("{name}.step"),
        "step": "",
        BODIES: ids,
    }))
}

/// Restates the bodies of each base feature in `history` as STEP, from `model`, the design
/// the history was mapped from; why one cannot be, else.
pub(crate) fn restate(history: &mut History, model: &Model) -> Result<(), String> {
    for feature in &mut history.features {
        let Some(fields) = feature.as_object_mut() else {
            continue;
        };
        let Some(bodies) = fields.remove(BODIES) else {
            continue;
        };
        let ids: Vec<&str> = bodies
            .as_array()
            .map(|ids| ids.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let title = fields
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Base Feature")
            .to_owned();
        let part = dutchman_core::with_bodies(model, &ids, title.clone())
            .map_err(|why| format!("base feature \"{title}\": {why}"))?;
        let text = step::restate(&part, &mut Report::default())
            .map_err(|err| format!("base feature \"{title}\": {err}"))?;
        fields.insert(STEP.to_owned(), Value::String(text));
    }
    Ok(())
}

/// Stores the STEP text of each base feature in `features` in `container`, pointing the
/// feature at it.
pub(crate) fn store(
    features: &mut [Value],
    container: &mut Container,
) -> Result<(), dutchman_core::Error> {
    for feature in features {
        let Some(fields) = feature.as_object_mut() else {
            continue;
        };
        fields.remove(BODIES);
        if let Some(Value::String(text)) = fields.remove(STEP) {
            let blob = container.add_step(text.as_bytes())?;
            fields.insert("step_blob".to_owned(), Value::String(blob));
        }
    }
    Ok(())
}
