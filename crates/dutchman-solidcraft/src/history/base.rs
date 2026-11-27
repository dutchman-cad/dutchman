//! Base features and stored geometry: bodies a timeline holds without the steps that made
//! them (modelled directly, or imported), as SolidCraft Import features at their place in the
//! timeline, so the features after them keep their history.
//!
//! When the source keeps a base feature's own body (a history body, which cadmpeg gives when a
//! later feature changes it), that body is imported and the later features rebuild on it.
//! Otherwise the bodies are the design's as it ends, so a base body a later feature changes is
//! imported changed already. A later join, cut or intersection re-applies to it without changing
//! the shape: those features are kept but marked as re-applied, not rebuilt, and the base feature as
//! holding the final body. Any other later change (a move, a blend, a deleted face) would apply
//! twice, so such a base feature is refused, naming the feature that changes its body.

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
        // A body stored only earlier in the timeline has no body in the final model for the
        // selection to resolve to; the source still keeps it as this feature's history body.
        if let Some(nest) = produced(feature, ctx) {
            return import_produced(feature, nest, ctx);
        }
        return Err(super::unsupported_selection("body", found));
    }
    let ids: Vec<&str> = bodies
        .pointer("/value/bodies")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if let (Some(nest), [body]) = (history_body(feature, &ids, ctx), ids.as_slice()) {
        return import_history_body(feature, body, nest, ctx);
    }
    place(feature, &ids, ctx)
}

/// Maps a paste that inserts copies of bodies (`definition` is its operation): its outputs,
/// the copies, are stored bodies like a base feature's.
pub(super) fn insert(
    feature: &Value,
    definition: &Value,
    ctx: &mut Context,
) -> Result<Value, String> {
    let found = definition.pointer("/bodies/kind").and_then(Value::as_str);
    if found != Some("resolved") {
        return Err(super::unsupported_selection("body", found));
    }
    let ids: Vec<&str> = feature
        .get("outputs")
        .and_then(Value::as_array)
        .map(|ids| ids.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    place(feature, &ids, ctx)
}

/// Imports the stored bodies `ids` at `feature`'s place in the timeline, unless a later
/// feature changes them in a way that would apply twice.
fn place(feature: &Value, ids: &[&str], ctx: &mut Context) -> Result<Value, String> {
    let later = later_changes(feature, ids, ctx);
    if let Some(twice) = later.iter().find(|row| !reapplies(row)) {
        return Err(format!(
            "its body is stored only as the design ends, and the later feature \"{}\" \
             changes it",
            name(twice).unwrap_or("?")
        ));
    }
    let mapped = import(feature, ids, ctx)?;
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

/// Whether CADIR or a loss attributed to this feature proves a required input absent.
/// Native selections and unresolved carriers alone do not prove an external design.
pub(super) fn missing_input(
    feature: &Value,
    ctx: &Context,
    losses: &[dutchman_core::Loss],
) -> bool {
    let definition = super::operation(feature);
    let references: Vec<&Value> = match super::operation_kind(feature) {
        Some("extrude" | "revolve") => definition.get("profile").into_iter().collect(),
        Some("sweep") => [definition.get("profile"), definition.get("path")]
            .into_iter()
            .flatten()
            .collect(),
        Some("loft") => definition
            .get("sections")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|section| section.get("kind").and_then(Value::as_str) == Some("profile"))
            .filter_map(|section| section.get("value"))
            .collect(),
        _ => return false,
    };
    references
        .into_iter()
        .any(|reference| absent_reference(reference, ctx))
        || (super::operation_kind(feature) == Some("extrude")
            && losses.iter().any(|loss| {
                loss.code
                    .as_deref()
                    .is_some_and(|code| code.ends_with("/feature.extrude-profile-sketch-absent"))
                    && (loss.subject == describe(feature)
                        || loss.detail.contains(&format!("{}:", describe(feature)))
                        // Some decode losses identify no feature by name. With only
                        // one extrude in this model there is no other possible owner.
                        // Count suppressed rows too: their losses remain in the report.
                        // cadmpeg names an extrude whose sketch is in another design only
                        // by its scope record; that must resolve to exactly this feature.
                        || scope_record(&loss.detail).is_some_and(|record| {
                            owns_scope_record(feature, record, ctx)
                        })
                        || (!loss.detail.contains("feature \"")
                            && !loss.subject.starts_with("feature \"")
                            && ctx.arena("features").iter().filter(|row| {
                                super::operation_kind(row) == Some("extrude")
                            }).count() == 1))
            }))
}

/// The scope record number an "Extrude scope record N" loss detail names.
fn scope_record(detail: &str) -> Option<u64> {
    let rest = detail.split("Extrude scope record ").nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The Extrude scope record number a feature is built from. cadmpeg ends a feature's id with
/// `:<scope kind><ordinal>:<scope record>`, e.g. `...BulkStream.dat7:Extrude2:950`.
fn feature_scope_record(row: &Value) -> Option<u64> {
    let mut parts = row.get("id")?.as_str()?.rsplit(':');
    let record = parts.next()?.parse().ok()?;
    parts.next()?.starts_with("Extrude").then_some(record)
}

/// Whether `feature` is the one and only feature of the model built from scope record
/// `record`. A record several features share (two designs) belongs to none of them.
fn owns_scope_record(feature: &Value, record: u64, ctx: &Context) -> bool {
    if feature_scope_record(feature) != Some(record) {
        return false;
    }
    ctx.arena("features")
        .iter()
        .filter(|row| feature_scope_record(row) == Some(record))
        .count()
        == 1
}

fn absent_reference(reference: &Value, ctx: &Context) -> bool {
    let reference = super::profile::planar(reference);
    let kind = reference.get("kind").and_then(Value::as_str);
    let value = reference.get("value").unwrap_or(&Value::Null);
    if kind == Some("unresolved") {
        return value.as_str().is_some_and(|value| {
            value
                .split(|c: char| !c.is_ascii_alphanumeric())
                .any(|word| matches!(word, "absent" | "external"))
        });
    }
    let spatial = kind.is_some_and(|kind| kind.starts_with("spatial_sketch_"));
    let sketch = if kind == Some("sketch") {
        value.as_str()
    } else if kind.is_some_and(|kind| kind.starts_with("sketch_") || spatial) {
        value.get("sketch").and_then(Value::as_str)
    } else {
        None
    };
    if let Some(id) = sketch {
        if ctx
            .row(
                if spatial {
                    "spatial_sketches"
                } else {
                    "sketches"
                },
                id,
            )
            .is_none()
        {
            return true;
        }
        let arena = if spatial {
            "spatial_sketch_entities"
        } else {
            "sketch_entities"
        };
        return ["entities", "curves"].into_iter().any(|field| {
            value
                .get(field)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .any(|id| ctx.row(arena, id).is_none())
        });
    }
    false
}

/// Keeps a failed recipe only when the source supplies its exact result.
/// Never replace a join/cut, or a final body that later active features change.
pub(super) fn fallback(feature: &Value, ctx: &mut Context) -> Result<Value, String> {
    if !matches!(
        super::operation_kind(feature),
        Some("extrude" | "revolve" | "sweep" | "loft")
    ) || super::operation(feature).get("op").and_then(Value::as_str) != Some("new_body")
    {
        return Err("it does not create a new body".to_owned());
    }
    let ids: Vec<&str> = feature
        .get("outputs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    let [body] = ids.as_slice() else {
        return Err("its single output body is not resolved".to_owned());
    };
    // Replacing a previously named body would discard the history that made it.
    if ctx.named_bodies.iter().any(|(id, _)| id.as_str() == *body) {
        return Err("its output already has body history".to_owned());
    }
    if let Some(nest) = history_body(feature, &ids, ctx) {
        return import_history_body(feature, body, nest, ctx);
    }
    // Ambiguous history bodies cannot establish which result belongs here.
    if ctx.source.is_some_and(|model| {
        model.ir.model.history_bodies.iter().any(|nest| {
            nest.produced_by
                .as_ref()
                .is_some_and(|by| Some(by.as_str()) == feature.get("id").and_then(Value::as_str))
        })
    }) || feature.get("ordinal").and_then(Value::as_u64).is_none()
    {
        return Err("its unchanged result body is not available".to_owned());
    }
    // The source stores the body only as the design ends, so a later feature that changes it
    // leaves no state of its own to import, and no face of it to read a profile from.
    let later = later_changes(feature, &ids, ctx);
    if let Some(change) = later.iter().find(|row| !reapplies(row)).or(later.first()) {
        return Err(format!(
            "the body it makes is changed by the later feature \"{}\", and the source stores that body only as the design ends, so neither a stored result nor a profile read from its faces can be proven",
            name(change).unwrap_or("?")
        ));
    }
    import(feature, &ids, ctx)
}

/// The body a base feature of one body made, as it stood when the base feature ended: the
/// history body the source keeps for it, nested with its own topology. cadmpeg keeps one
/// only when a later feature changes the body. None for a base feature of several bodies,
/// or when the source keeps none or more than one.
fn history_body<'a>(
    feature: &Value,
    ids: &[&str],
    ctx: &Context<'a>,
) -> Option<&'a cadmpeg_ir::history_body::HistoryBody> {
    let [_] = ids else {
        return None;
    };
    produced(feature, ctx)
}

fn produced<'a>(
    feature: &Value,
    ctx: &Context<'a>,
) -> Option<&'a cadmpeg_ir::history_body::HistoryBody> {
    let id = feature.get("id").and_then(Value::as_str)?;
    let mut nests = ctx.source?.ir.model.history_bodies.iter().filter(|nest| {
        nest.produced_by
            .as_ref()
            .is_some_and(|by| by.as_str() == id)
    });
    match (nests.next(), nests.next()) {
        (Some(nest), None) => Some(nest),
        _ => None,
    }
}

/// Imports a base feature's own body, `nest`, in place of the design's final body `body`.
/// Later features then rebuild on it as they did in the source, so none is re-applied, and
/// the replay right after it is compared with the bodies the source stores at that point.
/// The body keeps the name later features select it by.
fn import_history_body(
    feature: &Value,
    body: &str,
    nest: &cadmpeg_ir::history_body::HistoryBody,
    ctx: &mut Context,
) -> Result<Value, String> {
    let id = ctx.id();
    ctx.builds = true;
    let name = name(feature)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Base Feature{id}"));
    let model = ctx.source.ok_or("its source geometry is not available")?;
    let part = nested_model(model, nest, &name);
    let part = dutchman_core::with_bodies(&part, &[nest.topology.body.id.as_str()], name.clone())
        .map_err(|why| format!("base feature \"{name}\": its own body {why}"))?;
    let text = step::restate(&part, &mut Report::default())
        .map_err(|err| format!("base feature \"{name}\": {err}"))?;
    let names = super::faces::import_names(ctx, &[body], Some(&text));
    if let [name] = names.as_slice() {
        ctx.named_bodies
            .push((nest.topology.body.id.as_str().to_owned(), name.clone()));
    }
    let mut mapped = json!({
        "id": id,
        "name": name,
        "type": "import",
        "suppressed": false,
        "file": format!("{name}.step"),
        "step": "",
        STEP: text,
    });
    if let (false, Some(fields)) = (names.is_empty(), mapped.as_object_mut()) {
        fields.insert("body_names".to_owned(), json!(names));
    }
    Ok(mapped)
}

/// A model holding only the body of the history body `nest`, with `model`'s source and
/// tolerances. The nest's references resolve among its own records.
fn nested_model(model: &Model, nest: &cadmpeg_ir::history_body::HistoryBody, name: &str) -> Model {
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.source.clone_from(&model.ir.source);
    ir.tolerances.clone_from(&model.ir.tolerances);
    ir.geometry = nest.topology.to_topology();
    Model {
        ir,
        fidelity: None,
        name: name.to_owned(),
        source_file: None,
        assembly: None,
        source_step: None,
        merge_bodies: false,
    }
}

fn import_produced(
    feature: &Value,
    nest: &cadmpeg_ir::history_body::HistoryBody,
    ctx: &mut Context,
) -> Result<Value, String> {
    let id = ctx.id();
    let name = name(feature)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Base Feature{id}"));
    let text = nest_step("base feature", &name, nest, ctx)?;
    let base = nest
        .topology
        .body
        .name
        .as_deref()
        .filter(|body| !body.trim().is_empty())
        .unwrap_or("Body")
        .to_owned();
    let mut body = base.clone();
    let mut n = 2;
    while ctx.named_bodies.iter().any(|(_, known)| *known == body) {
        body = format!("{base} ({n})");
        n += 1;
    }
    ctx.named_bodies
        .push((nest.topology.body.id.as_str().to_owned(), body.clone()));
    ctx.builds = true;
    Ok(json!({
        "id": id,
        "name": name,
        "type": "import",
        "suppressed": false,
        "file": format!("{name}.step"),
        "step": "",
        STEP: text,
        "body_names": [body],
    }))
}

fn require_history_version(ctx: &Context) -> Result<(), String> {
    if !crate::VersionGate::HistoryBody.supported(ctx.target_version) {
        return Err(format!(
            "history bodies need SolidCraft 0.40 or newer (target {})",
            ctx.target_version.minor()
        ));
    }
    Ok(())
}

/// The STEP text of the body of a history body `nest`, as `label` feature `name` imports it.
fn nest_step(
    label: &str,
    name: &str,
    nest: &cadmpeg_ir::history_body::HistoryBody,
    ctx: &Context,
) -> Result<String, String> {
    require_history_version(ctx)?;
    let model = ctx.source.ok_or("its source geometry is not available")?;
    let part = nested_model(model, nest, name);
    let nest_body = nest.topology.body.id.as_str();
    let part = dutchman_core::with_bodies(&part, &[nest_body], name.to_owned())
        .map_err(|why| format!("{label} \"{name}\": {why}"))?;
    let text = step::restate(&part, &mut Report::default())
        .map_err(|err| format!("{label} \"{name}\": {err}"))?;
    if super::faces::shape_count(&text) != 1 {
        return Err(format!(
            "{label} \"{name}\": its history body does not restate as exactly one solid or sheet"
        ));
    }
    Ok(text)
}

/// Imports the body a split feature works on, when it exists only earlier in the timeline:
/// the history body the source keeps as consumed by `feature`, as it stood before the split.
/// The import goes in just before the split, and `target`, the body the split selects,
/// is named by it. None when the source keeps no such body; several are refused.
pub(super) fn import_consumed(
    feature: &Value,
    target: &str,
    ctx: &mut Context,
) -> Result<Option<String>, String> {
    let id = feature.get("id").and_then(Value::as_str);
    let mut nests = ctx
        .source
        .into_iter()
        .flat_map(|model| &model.ir.model.history_bodies)
        .filter(|nest| id.is_some() && nest.consumed_by.as_ref().map(|by| by.as_str()) == id);
    let nest = match (nests.next(), nests.next()) {
        (Some(nest), None) => nest,
        (None, _) => return Ok(None),
        _ => {
            return Err(
                "splitting several consumed history bodies at once is not supported yet".to_owned(),
            )
        }
    };
    require_history_version(ctx)?;
    // A body an earlier feature (a Base Feature) made and this split consumes was imported when
    // that feature mapped: the split works on that body, which is not imported a second time.
    if nest.produced_by.is_some() {
        let made = ctx
            .named_bodies
            .iter()
            .find(|(known, _)| known == nest.topology.body.id.as_str())
            .map(|(_, body)| body.clone());
        if let Some(body) = made {
            if !target.is_empty() {
                ctx.named_bodies.push((target.to_owned(), body.clone()));
            }
            return Ok(Some(body));
        }
    }
    let feature_id = ctx.id();
    let label = format!("{} import", name(feature).unwrap_or("Split"));
    let text = nest_step("split", &label, nest, ctx)?;
    let base = nest
        .topology
        .body
        .name
        .as_deref()
        .filter(|body| !body.trim().is_empty())
        .unwrap_or("Body")
        .to_owned();
    let mut body = base.clone();
    let mut n = 2;
    while ctx.named_bodies.iter().any(|(_, known)| *known == body) {
        body = format!("{base} ({n})");
        n += 1;
    }
    ctx.builds = true;
    ctx.extra.push(json!({
        "id": feature_id,
        "name": label,
        "type": "import",
        "suppressed": false,
        "file": format!("{label}.step"),
        "step": "",
        STEP: text,
        "body_names": [body],
    }));
    // An empty `target` is a native selection: nothing later names the body by that id.
    if !target.is_empty() {
        ctx.named_bodies.push((target.to_owned(), body.clone()));
    }
    Ok(Some(body))
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

fn import(feature: &Value, ids: &[&str], ctx: &mut Context) -> Result<Value, String> {
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
    let part = dutchman_core::with_bodies(model, ids, name.clone())
        .map_err(|why| format!("base feature \"{name}\": {why}"))?;
    let text = step::restate(&part, &mut Report::default())
        .map_err(|err| format!("base feature \"{name}\": {err}"))?;
    let names = super::faces::import_names(ctx, ids, Some(&text));
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
        let part = dutchman_core::with_bodies(model, &ids, title.clone())
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
            .geometry
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
    fn an_unnamed_absence_loss_requires_a_single_possible_feature() {
        let feature = json!({"name": "Block", "definition": {"kind": "operation", "value": {
            "kind": "extrude", "value": {"profile": {"kind": "planar", "value": {
                "kind": "native", "value": "synthetic:profile#1"}}}}}});
        let mut report = Report::default();
        report.push_coded(
            Severity::Degraded,
            "source",
            "f3d/feature.extrude-profile-sketch-absent",
            "Profile sketch absent",
        );
        let model = json!({"features": [feature]});
        assert!(missing_input(
            &feature,
            &Context::testing(&model),
            report.losses()
        ));
        for suppressed in [false, true] {
            let mut other = feature.clone();
            other["name"] = json!("Other");
            other["suppressed"] = json!(suppressed);
            let model = json!({"features": [feature, other]});
            assert!(!missing_input(
                &feature,
                &Context::testing(&model),
                report.losses()
            ));
        }
    }

    #[test]
    fn a_scope_record_loss_belongs_to_the_one_feature_built_from_it() {
        let extrude = |name: &str, record: u32| {
            json!({"name": name, "id": format!("f3d:model:feature#52:f3d%3ADesign/BulkStream.dat7:Extrude2:{record}"),
                "definition": {"kind": "operation", "value": {
                    "kind": "extrude", "value": {"profile": {"kind": "planar", "value": {
                        "kind": "native", "value": "synthetic:profile#1"}}}}}})
        };
        let (first, second) = (extrude("Extrude 1", 900), extrude("Extrude 2", 950));
        let model = json!({"features": [first, second]});
        let ctx = Context::testing(&model);
        let losses = |record: u32| {
            let mut report = Report::default();
            report.push_coded(
                Severity::Degraded,
                "source",
                "f3d/feature.extrude-profile-sketch-absent",
                format!(
                    "xref Panel: 1 Sketch-profile operand(s) of Extrude scope record {record} \
                     name a Sketch that the Design stream does not register; the profile stays native."
                ),
            );
            report
        };
        let named = losses(950);
        assert!(missing_input(&second, &ctx, named.losses()));
        assert!(!missing_input(&first, &ctx, named.losses()));
        let none = losses(7);
        assert!(!missing_input(&first, &ctx, none.losses()));
        assert!(!missing_input(&second, &ctx, none.losses()));
        // A record two features share cannot name one of them.
        let model = json!({"features": [second, extrude("Copy", 950)]});
        let ctx = Context::testing(&model);
        assert!(!missing_input(&second, &ctx, named.losses()));
    }

    #[test]
    fn only_absent_inputs_qualify_for_a_stored_result() {
        let model = json!({"sketches": [{"id": "present"}],
            "sketch_entities": [{"id": "curve", "sketch": "present"}]});
        let ctx = Context::testing(&model);
        let report = Report::default();
        for (reference, absent) in [
            (json!({"kind": "sketch", "value": "missing"}), true),
            (json!({"kind": "sketch", "value": "present"}), false),
            (
                json!({"kind": "unresolved", "value": "synthetic:external:profile#1"}),
                true,
            ),
            (
                json!({"kind": "unresolved", "value": "synthetic:profile#1"}),
                false,
            ),
            (
                json!({"kind": "native", "value": "synthetic:profile#1"}),
                false,
            ),
            (
                json!({"kind": "sketch_selection", "value": {"sketch": "present", "selections": ["opaque"]}}),
                false,
            ),
            (
                json!({"kind": "sketch_curves", "value": {"sketch": "present", "curves": ["curve"]}}),
                false,
            ),
            (
                json!({"kind": "sketch_curves", "value": {"sketch": "present", "curves": ["missing"]}}),
                true,
            ),
        ] {
            for kind in ["extrude", "revolve", "sweep", "loft"] {
                let definition = if kind == "loft" {
                    json!({"sections": [{"kind": "profile", "value": reference}]})
                } else {
                    json!({"profile": reference})
                };
                let feature = json!({"definition": {"kind": "operation", "value": {
                    "kind": kind, "value": definition}}});
                assert_eq!(
                    missing_input(&feature, &ctx, report.losses()),
                    absent,
                    "{feature}"
                );
            }
            let feature = json!({"definition": {"kind": "operation", "value": {
                "kind": "sweep", "value": {"path": reference}}}});
            assert_eq!(
                missing_input(&feature, &ctx, report.losses()),
                absent,
                "{feature}"
            );
        }
    }

    #[test]
    fn a_base_feature_whose_body_a_later_move_changes_is_refused() {
        let definition = json!({"bodies": {"kind": "resolved", "value": {"bodies": ["body#1"]}}});
        let model = json!({"features": [
            base(1),
            {"id": "e", "name": "Extrude 1", "kind": "extrude", "ordinal": 2, "definition": {"kind": "operation", "value": {"kind": "extrude", "value": {"op": "join"}}}, "outputs": ["body#1"], "dependencies": [], "source_properties": {}, "source_content": []},
            {"id": "m", "name": "Move 1", "kind": "move_body", "ordinal": 3, "definition": {"kind": "operation", "value": {"kind": "move_body", "value": {}}}, "outputs": ["body#1"], "dependencies": [], "source_properties": {}, "source_content": []},
        ]});
        let mut ctx = Context::testing(&model);
        let err = map(&base(1), &definition, &mut ctx).unwrap_err();
        assert_eq!(
            err,
            "its body is stored only as the design ends, and the later feature \"Move 1\" changes it"
        );
        assert!(ctx.reapplied.is_empty());
    }

    #[test]
    fn later_joins_and_cuts_re_apply_and_others_do_not() {
        let row = |kind: &str, op: &str| json!({"kind": kind, "definition": {"kind":"operation", "value": {"kind": kind, "value": {"op": op}}}});
        assert!(reapplies(&row("extrude", "join")));
        assert!(reapplies(&row("revolve", "cut")));
        assert!(reapplies(&row("loft", "intersect")));
        assert!(reapplies(
            &json!({"kind": "hole", "definition": {"kind":"operation", "value": {"kind": "hole", "value": {}}}})
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
        let ctx = Context::testing(&model);
        assert!(later_changes(&base(1), &["body#1"], &ctx).is_empty());
    }
}
