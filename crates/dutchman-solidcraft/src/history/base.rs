//! Base features and stored geometry: bodies a timeline holds without the steps that made
//! them (modelled directly, or imported), as SolidCraft Import features at their place in the
//! timeline, so the features after them keep their history.
//!
//! Earlier body nests, when supplied, let later changes rebuild from the base step.
//! Otherwise final bodies can only be followed by operations that safely re-apply.

use std::collections::BTreeSet;

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
/// History-body ids in the same order as BODIES.
const HISTORY_BODIES: &str = "_history_bodies";
/// Where it keeps their STEP text, until the text is stored in the container.
const STEP: &str = "_step";

/// Whether a base feature's operation `definition` resolves to no bodies at all: the source
/// proves it holds none, so it builds nothing.
pub(super) fn holds_none(definition: &Value) -> bool {
    let bodies = definition.get("bodies").unwrap_or(&Value::Null);
    bodies.get("kind").and_then(Value::as_str) == Some("resolved")
        && bodies
            .pointer("/value/bodies")
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
}

/// Maps a base feature whose operation is `definition`.
pub(super) fn map(feature: &Value, definition: &Value, ctx: &mut Context) -> Result<Value, String> {
    let bodies = definition.get("bodies").unwrap_or(&Value::Null);
    let found = bodies.get("kind").and_then(Value::as_str);
    if found != Some("resolved") {
        return Err(super::unsupported_selection("body", found));
    }
    let ids: Vec<&str> = bodies
        .pointer("/value/bodies")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let later = later_changes(feature, &ids, ctx);
    if let Some(twice) = later.iter().find(|row| !reapplies(row)) {
        if let Some(history) = history_bodies(feature, &ids, ctx) {
            return import_from(feature, &ids, Some(&history), ctx);
        }
        return Err(format!(
            "its body is stored only as the design ends, and the later feature \"{}\" \
             changes it",
            name(twice).unwrap_or("?")
        ));
    }
    let mapped = import(feature, &ids, ctx)?;
    if !later.is_empty() {
        ctx.final_body = true;
        ctx.reapplied.extend(
            later
                .iter()
                .filter_map(|row| row.get("id").and_then(Value::as_str))
                .map(str::to_owned),
        );
    }
    Ok(mapped)
}

/// Whether a later feature re-applies to a body that already holds its result without
/// changing it: a join, cut or intersection by an extrude, revolve, sweep, loft or hole.
fn reapplies(row: &Value) -> bool {
    let kind = super::kind(row);
    let op = super::operation(row)
        .get("op")
        .and_then(Value::as_str)
        .unwrap_or("");
    kind == "hole"
        || (matches!(kind.as_str(), "extrude" | "revolve" | "sweep" | "loft")
            && matches!(op, "join" | "cut" | "intersect"))
}

/// The active features after `feature` on the timeline that output one of the bodies `ids`,
/// which they then change, in timeline order.
fn later_changes<'a>(feature: &Value, ids: &[&str], ctx: &Context<'a>) -> Vec<&'a Value> {
    let Some(ordinal) = feature.get("ordinal").and_then(Value::as_u64) else {
        return Vec::new();
    };
    let mut rows: Vec<&'a Value> = ctx
        .arena("features")
        .iter()
        .filter(|row| row.get("suppressed").and_then(Value::as_bool) != Some(true))
        .filter(|row| {
            row.get("ordinal")
                .and_then(Value::as_u64)
                .is_some_and(|at| at > ordinal)
        })
        .filter(|row| {
            row.get("outputs")
                .and_then(Value::as_array)
                .is_some_and(|outputs| {
                    outputs
                        .iter()
                        .any(|out| out.as_str().is_some_and(|out| ids.contains(&out)))
                })
        })
        .collect();
    rows.sort_by_key(|row| row.get("ordinal").and_then(Value::as_u64));
    rows
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

/// Match only nests produced by this feature, refusing ambiguous or incomplete pairs.
fn history_bodies(feature: &Value, ids: &[&str], ctx: &Context) -> Option<Vec<String>> {
    let producer = feature.get("id")?.as_str()?;
    let candidates: Vec<_> = ctx
        .source?
        .ir
        .model
        .history_bodies
        .iter()
        .filter(|body| {
            body.produced_by
                .as_ref()
                .is_some_and(|id| id.as_str() == producer)
        })
        .collect();
    let key = |id: &str| id.rsplit_once('#').map(|(_, key)| key.to_owned());
    let mut used = BTreeSet::new();
    ids.iter()
        .map(|id| {
            let matching: Vec<_> = candidates
                .iter()
                .filter(|body| {
                    (ids.len() == 1 && candidates.len() == 1)
                        || key(id).is_some_and(|key| {
                            body.topology
                                .body
                                .id
                                .as_str()
                                .rsplit_once('#')
                                .and_then(|(_, tail)| tail.split_once('-'))
                                .is_some_and(|(_, source)| source == key)
                        })
                })
                .collect();
            if matching.len() != 1 {
                return None;
            }
            let body = matching.first()?;
            used.insert(body.id.as_str())
                .then(|| body.id.as_str().to_owned())
        })
        .collect()
}

fn import(feature: &Value, ids: &[&str], ctx: &mut Context) -> Result<Value, String> {
    import_from(feature, ids, None, ctx)
}

fn import_from(
    feature: &Value,
    ids: &[&str],
    history: Option<&[String]>,
    ctx: &mut Context,
) -> Result<Value, String> {
    if ids.is_empty() {
        return Err("it holds no bodies".to_owned());
    }
    let id = ctx.id();
    ctx.builds = true;
    let name = name(feature)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Base Feature{id}"));
    // Validate the emitted sequence before committing body names to later selections
    // or colours. A colour correspondence gap must not discard usable history.
    let model = ctx.source.ok_or("its source geometry is not available")?;
    let part = match history {
        Some(history) => dutchman_core::with_history_bodies(
            model,
            &ids.iter()
                .copied()
                .zip(history.iter().map(String::as_str))
                .collect::<Vec<_>>(),
            name.clone(),
        ),
        None => dutchman_core::with_bodies(model, ids, name.clone()),
    }
    .map_err(|why| format!("base feature \"{name}\": {why}"))?;
    let text = step::restate(&part, &mut Report::default())
        .map_err(|err| format!("base feature \"{name}\": {err}"))?;
    let names = if history.is_some() {
        // Name the actual emitted regions. Final faces have no proven correspondence
        // with these earlier faces, so do not register final-region appearance targets.
        let wire = serde_json::to_value(&part.ir.model).map_err(|err| err.to_string())?;
        let mut earlier = Context::new(&wire);
        earlier.source = Some(&part);
        earlier.named_bodies.clone_from(&ctx.named_bodies);
        let nest_ids: Vec<_> = part
            .ir
            .model
            .bodies
            .iter()
            .map(|body| body.id.as_str())
            .collect();
        let names = super::faces::import_names(&mut earlier, &nest_ids, Some(&text));
        for (nest, name) in earlier.named_bodies.iter().skip(ctx.named_bodies.len()) {
            if let Some(index) = nest_ids.iter().position(|id| *id == nest) {
                if let Some(final_id) = ids.get(index) {
                    ctx.named_bodies
                        .push(((*final_id).to_owned(), name.clone()));
                }
            }
        }
        ctx.default_bodies.extend(earlier.default_bodies);
        names
    } else {
        super::faces::import_names(ctx, ids, Some(&text))
    };
    let mut mapped = json!({
        "id": id,
        "name": name,
        "type": "import",
        "suppressed": false,
        "file": format!("{name}.step"),
        "step": "",
        BODIES: ids,
        STEP: text,
    });
    if let (Some(history), Some(fields)) = (history, mapped.as_object_mut()) {
        fields.insert(HISTORY_BODIES.to_owned(), json!(history));
    }
    if let (false, Some(fields)) = (names.is_empty(), mapped.as_object_mut()) {
        fields.insert("body_names".to_owned(), json!(names));
    }
    Ok(mapped)
}

/// Restates the bodies of each base feature in `history` as STEP, from `model`, the design
/// the history was mapped from; why one cannot be, else.
pub(crate) fn restate(history: &mut History, model: &Model) -> Result<(), String> {
    for feature in &mut history.features {
        let Some(fields) = feature.as_object_mut() else {
            continue;
        };
        if fields.get(STEP).and_then(Value::as_str).is_some() {
            continue;
        }
        let Some(bodies) = fields.get(BODIES).cloned() else {
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
        let part = if let Some(history) = fields.get(HISTORY_BODIES).and_then(Value::as_array) {
            let pairs: Vec<_> = ids
                .iter()
                .copied()
                .zip(history.iter().filter_map(Value::as_str))
                .collect();
            if pairs.len() != ids.len() {
                return Err(format!(
                    "base feature \"{title}\": its history bodies are not available"
                ));
            }
            dutchman_core::with_history_bodies(model, &pairs, title.clone())
        } else {
            dutchman_core::with_bodies(model, &ids, title.clone())
        }
        .map_err(|why| format!("base feature \"{title}\": {why}"))?;
        let text = step::restate(&part, &mut Report::default())
            .map_err(|err| format!("base feature \"{title}\": {err}"))?;
        if fields
            .get("body_names")
            .and_then(Value::as_array)
            .is_some_and(|names| names.len() != super::faces::shape_count(&text))
        {
            return Err(format!("base feature \"{title}\": its written shapes cannot be matched to its appearance targets"));
        }
        fields.insert(STEP.to_owned(), Value::String(text));
    }
    Ok(())
}

/// Whether `history` holds nothing but Imports: nothing in it can be edited.
pub(crate) fn only_stored(history: &History) -> bool {
    history
        .features
        .iter()
        .all(|feature| feature.get("type").and_then(Value::as_str) == Some("import"))
}

/// Whether `history` already is `model`'s final geometry: only base features holding every
/// body of the model, and sketches. The original would repeat those Imports.
pub(crate) fn whole(history: &History, model: &Model) -> bool {
    let mut held = BTreeSet::new();
    for feature in &history.features {
        match feature.get("type").and_then(Value::as_str) {
            Some("sketch") => {}
            Some("import") => {
                if feature.get(HISTORY_BODIES).is_some() {
                    return false;
                }
                let Some(bodies) = feature.get(BODIES).and_then(Value::as_array) else {
                    return false;
                };
                held.extend(bodies.iter().filter_map(Value::as_str));
            }
            _ => return false,
        }
    }
    !held.is_empty()
        && model
            .ir
            .model
            .bodies
            .iter()
            .all(|body| held.contains(body.id.as_str()))
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
        fields.remove(HISTORY_BODIES);
        if let Some(Value::String(text)) = fields.remove(STEP) {
            let blob = container.add_step(text.as_bytes())?;
            fields.insert("step_blob".to_owned(), Value::String(blob));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(ordinal: u64) -> Value {
        json!({"id": "b", "name": "Base Feature 1", "ordinal": ordinal, "outputs": ["body#1"]})
    }

    #[test]
    fn a_base_feature_whose_body_a_later_move_changes_is_refused() {
        let definition = json!({"bodies": {"kind": "resolved", "value": {"bodies": ["body#1"]}}});
        let model = json!({"features": [
            base(1),
            {"id": "e", "name": "Extrude 1", "kind": "extrude", "ordinal": 2,
                "definition": {"definition": "extrude", "op": "join"}, "outputs": ["body#1"]},
            {"id": "m", "name": "Move 1", "kind": "move_body", "ordinal": 3,
                "definition": {"definition": "move_body"}, "outputs": ["body#1"]},
        ]});
        let mut ctx = Context::new(&model);
        let err = map(&base(1), &definition, &mut ctx).unwrap_err();
        assert_eq!(
            err,
            "its body is stored only as the design ends, and the later feature \"Move 1\" changes it"
        );
        assert!(ctx.reapplied.is_empty());
    }

    fn snapshot_source() -> Model {
        let mut ir = cadmpeg_ir::examples::unit_cube_with_history_body().unwrap();
        ir.model.history_bodies[0].produced_by =
            Some(serde_json::from_value(json!("test:base:feature#1")).unwrap());
        // Distinguish the nest from final geometry in the actual STEP output.
        let mut wire = serde_json::to_value(&ir).unwrap();
        for point in wire["model"]["points"].as_array_mut().unwrap() {
            point["position"]["x"] = json!(point["position"]["x"].as_f64().unwrap() + 100.0);
        }
        ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        Model {
            ir,
            fidelity: None,
            name: "Box".to_owned(),
            source_file: None,
            assembly: None,
            source_step: None,
        }
    }

    #[test]
    fn base_snapshot_rebuilds_before_later_changes_and_survives_restatement() {
        let source = snapshot_source();
        let body = source.ir.model.bodies[0].id.as_str();
        let feature = json!({"id": "test:base:feature#1", "name": "Base Feature 1", "ordinal": 0});
        let definition = json!({"bodies": {"kind": "resolved", "value": {"bodies": [body]}}});
        let wire = json!({"features": [feature,
            {"id": "join", "kind": "extrude", "ordinal": 1, "outputs": [body],
                "definition": {"definition": "extrude", "op": "join"}},
            {"id": "move", "name": "Move 1", "kind": "move_body", "ordinal": 2,
                "outputs": [body], "definition": {"definition": "move_body"}}]});
        let mut ctx = Context::new(&wire);
        ctx.source = Some(&source);
        let mapped = map(&feature, &definition, &mut ctx).unwrap();
        assert_eq!(mapped["type"], "import");
        assert_eq!(
            mapped["body_names"],
            json!([source.ir.model.bodies[0].name.as_deref().unwrap()])
        );
        assert!(!ctx.final_body);
        assert!(ctx.reapplied.is_empty(), "joins must rebuild too");
        let expected = mapped[STEP].clone();
        let final_part =
            dutchman_core::with_bodies(&source, &[body], "Base Feature 1".to_owned()).unwrap();
        let final_step = step::restate(&final_part, &mut Report::default()).unwrap();
        assert_ne!(expected.as_str().unwrap(), final_step);
        let mut history = History {
            features: vec![mapped],
            ..History::default()
        };
        assert!(!whole(&history, &source));
        history.features[0].as_object_mut().unwrap().remove(STEP);
        restate(&mut history, &source).unwrap();
        assert_eq!(history.features[0][STEP], expected);
        let mut container = Container::new(b"synthetic final STEP").unwrap();
        store(&mut history.features, &mut container).unwrap();
        assert!(history.features[0].get(HISTORY_BODIES).is_none());
        assert!(history.features[0].get("step_blob").is_some());
    }

    #[test]
    fn base_snapshots_match_keys_instead_of_arena_order() {
        let mut source = snapshot_source();
        let mut second = source.ir.model.history_bodies[0].clone();
        source.ir.model.history_bodies[0].topology.body.id =
            serde_json::from_value(json!("f3d:history-body:body#h0001-12")).unwrap();
        second.id = serde_json::from_value(json!("f3d:history-body:nest#h0001-21")).unwrap();
        second.topology.body.id =
            serde_json::from_value(json!("f3d:history-body:body#h0001-21")).unwrap();
        let first_id = source.ir.model.history_bodies[0].id.as_str().to_owned();
        let second_id = second.id.as_str().to_owned();
        source.ir.model.history_bodies.push(second);
        let wire = json!({});
        let mut ctx = Context::new(&wire);
        ctx.source = Some(&source);
        let feature = json!({"id": "test:base:feature#1"});
        assert_eq!(
            history_bodies(&feature, &["f3d:model:body#21", "f3d:model:body#12"], &ctx),
            Some(vec![second_id, first_id])
        );
        assert!(
            history_bodies(&feature, &["f3d:model:body#12", "f3d:model:body#99"], &ctx).is_none()
        );
        assert!(
            history_bodies(&feature, &["f3d:model:body#12", "f3d:model:body#12"], &ctx).is_none()
        );
    }

    #[test]
    fn base_snapshot_requires_every_body_and_the_right_producer() {
        let mut source = snapshot_source();
        // Give the first body a matching source key; only the second nest is absent.
        let nest = &mut source.ir.model.history_bodies[0].topology;
        let old_id = nest.body.id.as_str().to_owned();
        let text = serde_json::to_string(nest)
            .unwrap()
            .replace(&old_id, "f3d:history-body:body#h0001-0");
        *nest = serde_json::from_str(&text).unwrap();
        let body = source.ir.model.bodies[0].id.as_str();
        let feature = json!({"id": "test:base:feature#1", "ordinal": 0});
        let wire = json!({"features": [feature, {"name": "Move 1", "kind": "move_body",
            "ordinal": 1, "outputs": [body]}]});
        let mut ctx = Context::new(&wire);
        ctx.source = Some(&source);
        let definition =
            json!({"bodies": {"kind": "resolved", "value": {"bodies": [body, "body#missing"]}}});
        assert_eq!(map(&feature, &definition, &mut ctx).unwrap_err(),
            "its body is stored only as the design ends, and the later feature \"Move 1\" changes it");
        assert!(history_bodies(&json!({"id": "other"}), &[body], &ctx).is_none());
    }

    #[test]
    fn later_joins_and_cuts_re_apply_and_others_do_not() {
        let row = |kind: &str, op: &str| json!({"kind": kind, "definition": {"definition": kind, "op": op}});
        assert!(reapplies(&row("extrude", "join")));
        assert!(reapplies(&row("revolve", "cut")));
        assert!(reapplies(&row("loft", "intersect")));
        assert!(reapplies(
            &json!({"kind": "hole", "definition": {"definition": "hole"}})
        ));
        assert!(!reapplies(&row("extrude", "new_body")));
        assert!(!reapplies(&row("fillet", "")));
        assert!(!reapplies(&row("move_body", "")));
        assert!(!reapplies(&row("delete_face", "")));
        // A suppressed later feature, or one making another body, is not a later change.
        let model = json!({"features": [
            base(1),
            {"id": "s", "name": "Move 2", "ordinal": 3, "outputs": ["body#1"], "suppressed": true},
            {"id": "o", "name": "Extrude 1", "ordinal": 4, "outputs": ["body#2"]},
        ]});
        let ctx = Context::new(&model);
        assert!(later_changes(&base(1), &["body#1"], &ctx).is_empty());
    }
}
