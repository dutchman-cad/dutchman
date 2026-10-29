//! The model's feature history, as SolidCraft parametric features.
//!
//! History is all or nothing: CADIR keeps only the final B-rep, so a history that stops part
//! way cannot be finished with an imported body. Either every active feature maps, or the
//! design is written as one import with mapped sketches hidden for reference.
//!
//! The mapping reads the model in CADIR's documented JSON form (`docs/cad-ir.md` in cadmpeg),
//! the stable contract cadmpeg publishes, rather than its Rust accessors. It knows the wire
//! versions in [`IR_VERSIONS`] and refuses others rather than misread them.

pub(crate) mod base;
mod blend;
mod constraints;
mod crossing;
mod extrude;
mod faces;
mod helix;
mod helpers;
mod hole;
mod loft;
mod modify;
pub(crate) mod params;
mod pattern;
mod primitive;
mod profile;
mod revolve;
mod sheet;
mod sketch;
mod sweep;

use std::collections::BTreeMap;

use dutchman_core::{FeatureOutcome, FeatureStatus, Model, Report, Severity};
use serde_json::Value;

/// The CADIR wire versions the mapping reads. Version 7 adds only assembly joint kinds and a
/// sheet-metal thickness side, neither of which the mapping uses.
const IR_VERSIONS: [&str; 5] = ["6", "7", "8", "9", "10"];

/// A replayable history: SolidCraft parameters and features in timeline order.
#[derive(Clone, Debug, Default)]
pub(crate) struct History {
    pub(crate) params: Vec<Value>,
    pub(crate) features: Vec<Value>,
    /// The looks of the bodies the history makes, by body name.
    pub(crate) looks: BTreeMap<String, Value>,
    /// Looks of faces of those bodies that differ from their body's, as SolidCraft face looks
    /// (a point on the face, in the frame the body is built in: the design's, or its
    /// component's inside an assembly, which SolidCraft applies to every occurrence).
    pub(crate) face_looks: Vec<Value>,
    /// The sheet rules its sheet-metal features use, as SolidCraft stores them.
    pub(crate) sheet_rules: Vec<Value>,
    /// The id the next feature after the history takes.
    pub(crate) next_id: u64,
    /// New pattern instance sets must fall back if an attempted replay cannot finish.
    pub(crate) pattern_replay_required: bool,
    /// Source features that became SolidCraft features or needed none.
    pub(crate) mapped: usize,
    /// Active source features.
    pub(crate) total: usize,
    /// Blends whose input edges wait for SolidCraft's replay of the history before them
    /// ([`prove_blends`]); until then each has no body.
    pub(crate) blends: Vec<blend::Pending>,
}

/// What one source feature becomes.
enum Mapped {
    /// SolidCraft needs no feature for it (a principal datum plane).
    Nothing,
    /// This SolidCraft feature.
    Feature(Value),
}

impl History {
    /// Moves the history into component `component` of a design whose features so far end
    /// before id `offset + 1`: every id and sketch reference shifts by `offset`, and each
    /// parameter whose name is in `taken` is renamed (prefixed with `prefix`) everywhere it is
    /// used. The names it ends up with are added to `taken`.
    pub(crate) fn relocate(
        &mut self,
        offset: u64,
        component: u64,
        prefix: &str,
        taken: &mut std::collections::BTreeSet<String>,
    ) {
        let mut renamed = BTreeMap::new();
        for param in &self.params {
            let Some(name) = param.get("name").and_then(Value::as_str) else {
                continue;
            };
            let mut new = name.to_owned();
            let mut n = 1;
            while taken.contains(&new) {
                new = if n == 1 {
                    format!("{prefix}_{name}")
                } else {
                    format!("{prefix}_{name}_{n}")
                };
                n += 1;
            }
            taken.insert(new.clone());
            if new != name {
                renamed.insert(name.to_owned(), new);
            }
        }
        for param in &mut self.params {
            if let Some(fields) = param.as_object_mut() {
                if let Some(name) = fields.get("name").and_then(Value::as_str) {
                    let name = renamed
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| name.to_owned());
                    fields.insert("name".to_owned(), Value::String(name));
                }
                if let Some(expr) = fields.get("expr").and_then(Value::as_str) {
                    let expr = params::rename_in(expr, &renamed);
                    fields.insert("expr".to_owned(), Value::String(expr));
                }
            }
        }
        for feature in &mut self.features {
            let Some(fields) = feature.as_object_mut() else {
                continue;
            };
            for key in ["id", "sketch", "path_sketch"] {
                if let Some(id) = fields.get(key).and_then(Value::as_u64) {
                    fields.insert(key.to_owned(), Value::from(id.saturating_add(offset)));
                }
            }
            // A loft names the sketch of each section, and a sweep that of each rail.
            for key in ["sections", "rails"] {
                let Some(Value::Array(rows)) = fields.get_mut(key) else {
                    continue;
                };
                for row in rows {
                    if let Some(sketch) = row.get("sketch").and_then(Value::as_u64) {
                        if let Some(row) = row.as_object_mut() {
                            row.insert(
                                "sketch".to_owned(),
                                Value::from(sketch.saturating_add(offset)),
                            );
                        }
                    }
                }
            }
            fields.insert("component".to_owned(), Value::from(component));
            if let Some(constraints) = fields
                .get_mut("sketch")
                .and_then(|sketch| sketch.get_mut("constraints"))
                .and_then(Value::as_array_mut)
            {
                for constraint in constraints {
                    let param = constraint.get("param").and_then(Value::as_str);
                    if let Some(new) = param.and_then(|p| renamed.get(p)).cloned() {
                        if let Some(fields) = constraint.as_object_mut() {
                            fields.insert("param".to_owned(), Value::String(new));
                        }
                    }
                }
            }
        }
        self.next_id = self.next_id.saturating_add(offset);
    }

    /// Renames each body this history names that `taken` already holds to `<name> (<label>)`,
    /// in its features, looks and face looks, so bodies of different parts of one design
    /// keep their own looks; then adds the names it uses to `taken`.
    pub(crate) fn rename_bodies(
        &mut self,
        label: &str,
        taken: &mut std::collections::BTreeSet<String>,
    ) {
        let names: std::collections::BTreeSet<String> = self
            .features
            .iter()
            .filter_map(|feature| feature.get("body_names").and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let mut renamed = BTreeMap::new();
        for name in names {
            let mut new = name.clone();
            let mut n = 2;
            while taken.contains(&new) {
                new = if n == 2 {
                    format!("{name} ({label})")
                } else {
                    format!("{name} ({label} {n})")
                };
                n += 1;
            }
            taken.insert(new.clone());
            if new != name {
                renamed.insert(name, new);
            }
        }
        if renamed.is_empty() {
            return;
        }
        let rename = |name: &str| {
            renamed
                .get(name)
                .cloned()
                .unwrap_or_else(|| name.to_owned())
        };
        for feature in &mut self.features {
            for key in ["body_names", "bodies", "tools"] {
                if let Some(names) = feature.get_mut(key).and_then(Value::as_array_mut) {
                    for name in names {
                        if let Some(old) = name.as_str() {
                            *name = Value::String(rename(old));
                        }
                    }
                }
            }
            for key in ["target", "body"] {
                if let Some(value) = feature.get_mut(key) {
                    if let Some(old) = value.as_str() {
                        *value = Value::String(rename(old));
                    }
                }
            }
        }
        self.looks = std::mem::take(&mut self.looks)
            .into_iter()
            .map(|(name, look)| (rename(&name), look))
            .collect();
        for face in &mut self.face_looks {
            if let Some(fields) = face.as_object_mut() {
                if let Some(old) = fields.get("body").and_then(Value::as_str) {
                    let new = rename(old);
                    fields.insert("body".to_owned(), Value::String(new));
                }
            }
        }
    }
}

/// The model and what has been mapped so far.
struct Context<'a> {
    /// The CADIR `model` object.
    model: &'a Value,
    target_version: crate::SolidCraftVersion,
    next_id: u64,
    /// SolidCraft parameter names by CADIR parameter id.
    params: BTreeMap<String, String>,
    /// Mapped sketches by CADIR sketch id.
    sketches: BTreeMap<String, sketch::Mapped>,
    /// SolidCraft looks of the bodies the history makes, by body name.
    looks: BTreeMap<String, Value>,
    /// The timeline name each mapped source feature took, by CADIR feature id.
    feature_names: BTreeMap<String, String>,
    /// All features making a composite pattern's instance set, including its seeds.
    pattern_features: BTreeMap<String, Vec<String>>,
    pattern_replay_required: bool,
    /// The source bodies the history makes under their own names: (CADIR body id, name).
    named_bodies: Vec<(String, String)>,
    /// Whether a feature that builds geometry has been mapped.
    builds: bool,
    /// The sheet thicknesses (mm) its sheet-metal features use, each with its rule's name.
    sheet_rules: Vec<(f64, String)>,
    /// Blends mapped so far, waiting for their input edges to be proven.
    blends: Vec<blend::Pending>,
}

impl<'a> Context<'a> {
    fn new(model: &'a Value) -> Self {
        Context {
            model,
            target_version: crate::SolidCraftVersion::default(),
            next_id: 1,
            params: BTreeMap::new(),
            sketches: BTreeMap::new(),
            looks: BTreeMap::new(),
            feature_names: BTreeMap::new(),
            pattern_features: BTreeMap::new(),
            pattern_replay_required: false,
            named_bodies: Vec::new(),
            builds: false,
            sheet_rules: Vec::new(),
            blends: Vec::new(),
        }
    }

    /// The timeline name `feature` keeps: its own, numbered when an earlier feature has it,
    /// since SolidCraft refers to features by name.
    fn unique_name<'b>(
        &self,
        feature: &Value,
        preceding: impl Iterator<Item = &'b Value> + Clone,
    ) -> String {
        let base = feature
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Feature")
            .to_owned();
        let taken = |name: &str| {
            self.feature_names.values().any(|known| known == name)
                || preceding
                    .clone()
                    .any(|feature| feature.get("name").and_then(Value::as_str) == Some(name))
        };
        let mut name = base.clone();
        let mut n = 2;
        while taken(&name) {
            name = format!("{base} ({n})");
            n += 1;
        }
        name
    }

    /// A row of a CADIR arena by id.
    fn row(&self, arena: &str, id: &str) -> Option<&'a Value> {
        self.arena(arena)
            .iter()
            .find(|row| row.get("id").and_then(Value::as_str) == Some(id))
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// The name of the sheet rule of `thickness` mm, made the first time it is asked for.
    fn sheet_rule(&mut self, thickness: f64) -> String {
        if let Some((_, name)) = self
            .sheet_rules
            .iter()
            .find(|(known, _)| (known - thickness).abs() < 1e-9)
        {
            return name.clone();
        }
        let name = format!("Sheet {thickness} mm");
        self.sheet_rules.push((thickness, name.clone()));
        name
    }

    /// The rows of a CADIR arena.
    fn arena(&self, name: &str) -> &'a [Value] {
        self.model
            .get(name)
            .and_then(Value::as_array)
            .map_or(&[], Vec::as_slice)
    }
}

/// The history as far as it maps, and what became of each feature.
pub(crate) struct Mapping {
    /// The replayable history, or why there is none.
    pub(crate) result: Result<History, String>,
    /// Each source feature's status, in timeline order. Every feature is looked at, also after
    /// one that does not map, so a caller can show everything that stands in the way.
    pub(crate) features: Vec<FeatureOutcome>,
    /// Mapped, resolved sketches retained even when the history cannot replay.
    pub(crate) references: History,
}

/// Helper decisions remain useful even when other features flatten the history.
pub(crate) fn helper_notes(report: &Report) -> Report {
    let mut kept = Report::default();
    for loss in report.losses().iter().filter(|loss| {
        loss.severity == Severity::Info && loss.detail.starts_with("helper not converted:")
    }) {
        kept.push_at(
            loss.stage,
            loss.severity,
            loss.subject.clone(),
            loss.detail.clone(),
        );
    }
    kept
}

/// Maps the model's history, or says why it cannot be replayed. Losses of a history that is
/// written go to `report`.
pub(crate) fn map(
    model: &Model,
    report: &mut Report,
    target_version: crate::SolidCraftVersion,
) -> Mapping {
    let mut features_out = Vec::new();
    let mut references = History::default();
    let result = map_into(
        model,
        report,
        &mut features_out,
        &mut references,
        target_version,
    );
    Mapping {
        result,
        features: features_out,
        references,
    }
}

fn map_into(
    model: &Model,
    report: &mut Report,
    out: &mut Vec<FeatureOutcome>,
    references: &mut History,
    target_version: crate::SolidCraftVersion,
) -> Result<History, String> {
    let document =
        serde_json::to_value(&model.ir).map_err(|err| format!("cannot read the model: {err}"))?;
    check_version(&document)?;
    let wire = document.get("model").cloned().unwrap_or(Value::Null);
    let (params, param_names) = params::map(model, report);
    references.params = params.clone();
    let mut ctx = Context::new(&wire);
    ctx.target_version = target_version;
    ctx.params = param_names;
    let mut features: Vec<&Value> = ctx.arena("features").iter().collect();
    if features.is_empty() {
        return Err("the design carries no feature history".to_owned());
    }
    features.sort_by_key(|feature| feature.get("ordinal").and_then(Value::as_u64));

    let mut history = History::default();
    let mut blocked: Option<String> = None;
    for feature in features {
        let outcome = |status, reason: Option<String>| FeatureOutcome {
            name: name(feature).map(str::to_owned),
            kind: kind(feature),
            status,
            reason,
        };
        if feature.get("suppressed").and_then(Value::as_bool) == Some(true) {
            report.push(
                Severity::Info,
                describe(feature),
                "suppressed in the source; left out",
            );
            out.push(outcome(
                FeatureStatus::LeftOut,
                Some("suppressed in the source".to_owned()),
            ));
            continue;
        }
        history.total += 1;
        let blends = ctx.blends.len();
        match map_feature(feature, &mut ctx, report, &mut history.features) {
            Ok(Mapped::Nothing) => {
                history.mapped += 1;
                out.push(outcome(FeatureStatus::NotNeeded, None));
            }
            Ok(Mapped::Feature(mut mapped)) => {
                history.mapped += 1;
                if mapped.get("type").and_then(Value::as_str) == Some("sketch") {
                    references.features.push(mapped.clone());
                }
                let name = ctx.unique_name(&mapped, history.features.iter());
                if let Some(fields) = mapped.as_object_mut() {
                    fields.insert("name".to_owned(), Value::String(name.clone()));
                }
                if let Some(id) = feature.get("id").and_then(Value::as_str) {
                    ctx.feature_names.insert(id.to_owned(), name);
                }
                history.features.push(mapped);
                out.push(outcome(FeatureStatus::Mapped, None));
                if let Some(pending) = ctx.blends.get_mut(blends) {
                    pending.feature = history.features.len().saturating_sub(1);
                    pending.outcome = out.len().saturating_sub(1);
                    pending.subject = describe(feature);
                }
            }
            Err(reason) => {
                blocked.get_or_insert_with(|| format!("{}: {reason}", describe(feature)));
                out.push(outcome(FeatureStatus::Blocked, Some(reason)));
            }
        }
    }
    params::unsigned_dimensions(references, &ctx);
    references.next_id = ctx.next_id;
    if let Some(reason) = blocked {
        return Err(reason);
    }
    if !ctx.builds {
        return Err("no feature in the history builds geometry".to_owned());
    }
    history.params = params;
    params::unsigned_dimensions(&mut history, &ctx);
    history.looks = std::mem::take(&mut ctx.looks);
    let (face_looks, unplaced) = faces::looks(&ctx);
    history.face_looks = face_looks;
    if unplaced > 0 {
        report.push(
            Severity::Info,
            "appearance",
            format!("{unplaced} coloured curved faces keep their body's colour"),
        );
    }
    history.sheet_rules = ctx
        .sheet_rules
        .iter()
        .map(|(thickness, name)| sheet::rule(name, *thickness))
        .collect();
    history.next_id = ctx.next_id;
    history.pattern_replay_required = ctx.pattern_replay_required;
    history.blends = std::mem::take(&mut ctx.blends);
    Ok(history)
}

/// The design's own sketches, each mapped as a SolidCraft sketch, for a design whose sketches
/// no feature history carries (an assembly's): every planar sketch that maps, with the
/// parameters their dimensions use, and how many sketches did not map.
pub(crate) fn sketches(model: &Model, report: &mut Report) -> (History, usize) {
    let mut kept = History::default();
    let count = model.ir.model.sketches.len();
    if count == 0 {
        return (kept, 0);
    }
    let document = match serde_json::to_value(&model.ir) {
        Ok(document) if check_version(&document).is_ok() => document,
        _ => return (kept, count),
    };
    let wire = document.get("model").cloned().unwrap_or(Value::Null);
    let mut param_notes = Report::default();
    let (params, param_names) = params::map(model, &mut param_notes);
    let mut ctx = Context::new(&wire);
    ctx.params = param_names;
    let mut left_out = 0;
    for sketch in ctx.arena("sketches") {
        let feature = serde_json::json!({ "name": sketch.get("name") });
        let definition = serde_json::json!({
            "sketch": { "space": "planar", "sketch": sketch.get("id") },
        });
        // A sketch that does not map is counted, and leaves no notes of its own behind.
        let mut notes = Report::default();
        match sketch::map(&feature, &definition, &mut ctx, &mut notes) {
            Ok(mut mapped) => {
                report.append(notes);
                let name = ctx.unique_name(&mapped, kept.features.iter());
                if let Some(fields) = mapped.as_object_mut() {
                    fields.insert("name".to_owned(), Value::String(name));
                }
                kept.features.push(mapped);
            }
            Err(_) => left_out += 1,
        }
    }
    kept.params = params;
    params::unsigned_dimensions(&mut kept, &ctx);
    let used = kept
        .features
        .iter()
        .filter_map(|feature| feature.pointer("/sketch/constraints"))
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|constraint| constraint.get("param").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    kept.params = params::needed(kept.params, used);
    // Only the notes about parameters the sketches keep.
    let names: Vec<&str> = kept
        .params
        .iter()
        .filter_map(|param| param.get("name").and_then(Value::as_str))
        .collect();
    for loss in param_notes.losses() {
        let about = |name: &&str| {
            loss.subject == format!("parameter {name}")
                || loss.detail.starts_with(&format!("renamed to {name}:"))
        };
        if names.iter().any(about) {
            report.push_at(loss.stage, loss.severity, &loss.subject, &loss.detail);
        }
    }
    kept.next_id = ctx.next_id;
    (kept, left_out)
}

/// What SolidCraft rebuilt before each blend, as [`crate::verify::replay_edges`] reports it.
pub(crate) type Replayed = Result<Vec<Result<crate::verify::ReplayedBodies, String>>, String>;

/// Proves the input edges of every blend of a history that maps, on the bodies `replay`
/// gives for the history before each blend (`replay(history, after)` with the ids of the
/// features each blend follows), and names its body. A blend that cannot be proven blocks
/// the history with the reason; without a replay (`replay` returning `Err`) none can be.
pub(crate) fn prove_blends(
    mapping: &mut Mapping,
    replay: impl FnOnce(&History, &[u64]) -> Replayed,
) {
    let Ok(history) = &mut mapping.result else {
        return;
    };
    if history.blends.is_empty() {
        return;
    }
    let after: Vec<Option<u64>> = history
        .blends
        .iter()
        .map(|pending| {
            pending
                .feature
                .checked_sub(1)
                .and_then(|before| history.features.get(before))
                .and_then(|feature| feature.get("id"))
                .and_then(Value::as_u64)
        })
        .collect();
    let steps: Vec<u64> = after.iter().flatten().copied().collect();
    let replayed = if steps.is_empty() {
        Ok(Vec::new())
    } else {
        replay(history, &steps)
    };
    let blends = std::mem::take(&mut history.blends);
    let mut step = 0usize;
    let mut first = None;
    for (pending, after) in blends.iter().zip(&after) {
        let proven = match (after, &replayed) {
            (None, _) => Err("no feature before it builds a body to blend".to_owned()),
            (Some(_), Err(why)) => Err(format!(
                "its input edges are found only on SolidCraft's replay of the history before \
                 it, and there is none: {why}"
            )),
            (Some(_), Ok(bodies)) => {
                let at = bodies.get(step);
                step = step.saturating_add(1);
                match at {
                    Some(Ok(bodies)) => blend::prove(pending, bodies),
                    Some(Err(why)) => Err(format!(
                        "SolidCraft cannot replay the history before it: {why}"
                    )),
                    None => {
                        Err("SolidCraft's replay of the history before it is missing".to_owned())
                    }
                }
            }
        };
        match proven {
            Ok(body) => {
                if let Some(fields) = history
                    .features
                    .get_mut(pending.feature)
                    .and_then(Value::as_object_mut)
                {
                    fields.insert("body".to_owned(), Value::String(body));
                }
            }
            Err(reason) => {
                if let Some(outcome) = mapping.features.get_mut(pending.outcome) {
                    outcome.status = FeatureStatus::Blocked;
                    outcome.reason = Some(reason.clone());
                }
                first.get_or_insert_with(|| format!("{}: {reason}", pending.subject));
            }
        }
    }
    if let Some(reason) = first {
        mapping.result = Err(reason);
    }
}

/// Refuses a CADIR wire version the mapping does not know.
fn check_version(document: &Value) -> Result<(), String> {
    let version = document
        .get("ir_version")
        .map(|v| v.as_str().map_or_else(|| v.to_string(), str::to_owned))
        .unwrap_or_default();
    if IR_VERSIONS.contains(&version.as_str()) {
        Ok(())
    } else {
        Err(format!(
            "unsupported CADIR version {version:?} (Dutchman reads {})",
            IR_VERSIONS.join(", ")
        ))
    }
}

/// One source feature as a SolidCraft feature, or why it cannot be.
fn map_feature(
    feature: &Value,
    ctx: &mut Context,
    report: &mut Report,
    preceding: &mut Vec<Value>,
) -> Result<Mapped, String> {
    if helpers::is_helper(feature) {
        return helpers::map(feature, ctx, report);
    }
    let definition = operation(feature);
    match definition.get("definition").and_then(Value::as_str) {
        // A folder, a PartDesign Body or another node that orders the tree and builds nothing.
        Some("datum_principal_plane" | "tree_node") => Ok(Mapped::Nothing),
        // A component inserted, or a joint made, in the timeline: the assembly carries it.
        Some("insert_component" | "assembly_joint") => Ok(Mapped::Nothing),
        Some("sketch") => sketch::map(feature, definition, ctx, report).map(Mapped::Feature),
        Some("extrude") => extrude::map(feature, definition, ctx).map(Mapped::Feature),
        Some("revolve") => revolve::map(feature, definition, ctx).map(Mapped::Feature),
        Some("loft") => loft::map(feature, definition, ctx, preceding).map(Mapped::Feature),
        Some("sweep") => sweep::map(feature, definition, ctx).map(Mapped::Feature),
        Some("sphere") => primitive::sphere(feature, definition, ctx).map(Mapped::Feature),
        Some("torus") => primitive::torus(feature, definition, ctx).map(Mapped::Feature),
        Some("coil") => primitive::coil(feature, definition, ctx).map(Mapped::Feature),
        Some("helical_sweep") => helix::map(feature, definition, ctx, report).map(Mapped::Feature),
        Some(kind @ ("block" | "primitive")) => Err(primitive::unplaced(kind)),
        // Their faces are picked on the final body, where the feature itself has already
        // removed or tilted them; mapping waits until CADIR carries the input faces.
        Some("shell") => Err("shell waits on input face geometry".to_owned()),
        Some("draft") => Err("draft waits on input face geometry".to_owned()),
        Some("combine") => modify::combine(feature, definition, ctx).map(Mapped::Feature),
        Some("mirror_shape") => Err(
            "replacing a source shape with its mirror is not supported yet; SolidCraft mirrors keep the original body".to_owned(),
        ),
        Some("unresolved")
            if matches!(
                definition.get("family").and_then(Value::as_str),
                Some("thread" | "detailed_thread")
            ) =>
        {
            Err("threads whose construction is unresolved are not supported yet".to_owned())
        }
        Some("cosmetic_thread") => Err(
            "cosmetic threads without a resolved standard and pitch are not supported yet".to_owned(),
        ),
        Some("pattern")
            if definition.pointer("/pattern/kind").and_then(Value::as_str) == Some("mirror")
                && definition
                    .get("seeds")
                    .and_then(Value::as_array)
                    .is_some_and(|seeds| {
                        seeds.iter().any(|seed| {
                            seed.get("kind").and_then(Value::as_str) == Some("bodies")
                        })
                    }) =>
        {
            modify::mirror(feature, definition, ctx).map(Mapped::Feature)
        }
        Some("pattern") => pattern::map(feature, definition, ctx, preceding).map(Mapped::Feature),
        Some("fillet" | "chamfer") => {
            blend::map(feature, definition, ctx, report).map(Mapped::Feature)
        }
        Some("hole") => hole::map(feature, definition, ctx, report).map(Mapped::Feature),
        Some("sheet_metal_base_flange") => {
            sheet::base_flange(feature, definition, ctx).map(Mapped::Feature)
        }
        Some("sheet_metal_edge_flange") => {
            sheet::edge_flange(feature, definition, ctx, preceding).map(Mapped::Feature)
        }
        Some("sheet_metal_hem") => Err(sheet::hem_refusal(definition)),
        Some("stored_geometry") => base::stored(feature, ctx, report).map(Mapped::Feature),
        Some("base_feature") => base::map(feature, definition, ctx).map(Mapped::Feature),
        // A placed datum plane: what is sketched on it carries the same frame of its own.
        Some("datum_plane") if definition.get("frame").is_some_and(Value::is_object) => {
            Ok(Mapped::Nothing)
        }
        _ => Err(format!("{} is not mapped yet", kind(feature))),
    }
}

/// The feature's operation, looking through a post-processing wrapper.
fn operation(feature: &Value) -> &Value {
    let definition = feature.get("definition").unwrap_or(&Value::Null);
    match definition.get("definition").and_then(Value::as_str) {
        Some("post_process") => definition.get("operation").unwrap_or(&Value::Null),
        _ => definition,
    }
}

/// The CADIR operation name of a feature (`extrude`, `fillet`, …).
fn kind(feature: &Value) -> String {
    operation(feature)
        .get("definition")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned()
}

/// A feature's source name, else its kind, for the report and the timeline.
fn name(feature: &Value) -> Option<&str> {
    feature
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
}

/// A feature as the report names it.
fn describe(feature: &Value) -> String {
    match name(feature) {
        Some(name) => format!("feature \"{name}\""),
        None => format!("{} feature", kind(feature)),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::testing::{cube, feature, with};

    fn datum(key: u32, ordinal: u64) -> Value {
        feature(
            key,
            ordinal,
            "XY Plane",
            json!({"definition": "datum_principal_plane", "plane": "top"}),
        )
    }

    fn opaque(key: u32, ordinal: u64, name: &str) -> Value {
        feature(
            key,
            ordinal,
            name,
            json!({"definition": "native", "kind": "Mystery", "parameters": {}}),
        )
    }

    #[test]
    fn body_renaming_updates_modify_operands() {
        let mut history = History {
            features: vec![
                json!({"body_names": ["Part"]}),
                json!({"target": "Part", "tools": ["Part"]}),
                json!({"bodies": ["Part"]}),
                json!({"body": "Part"}),
            ],
            ..History::default()
        };
        let mut taken = std::collections::BTreeSet::from(["Part".to_owned()]);
        history.rename_bodies("Second", &mut taken);
        assert_eq!(history.features[0]["body_names"], json!(["Part (Second)"]));
        assert_eq!(history.features[1]["target"], "Part (Second)");
        assert_eq!(history.features[1]["tools"], json!(["Part (Second)"]));
        assert_eq!(history.features[2]["bodies"], json!(["Part (Second)"]));
        assert_eq!(history.features[3]["body"], "Part (Second)");
    }

    #[test]
    fn known_cadir_versions_are_read_and_others_refused() {
        assert!(check_version(&json!({"ir_version": "6"})).is_ok());
        for version in ["7", "8", "9", "10"] {
            assert!(check_version(&json!({"ir_version": version})).is_ok());
        }
        assert_eq!(
            check_version(&json!({"ir_version": "11"})).unwrap_err(),
            "unsupported CADIR version \"11\" (Dutchman reads 6, 7, 8, 9, 10)"
        );
        assert!(check_version(&json!({})).is_err());
    }

    #[test]
    fn a_model_without_history_is_flattened() {
        let err = map(
            &cube(),
            &mut Report::default(),
            crate::SolidCraftVersion::default(),
        )
        .result
        .unwrap_err();
        assert_eq!(err, "the design carries no feature history");
    }

    #[test]
    fn datum_planes_alone_build_no_geometry() {
        let model = with(cube(), json!([datum(1, 0), datum(2, 1)]), json!([]));
        let err = map(
            &model,
            &mut Report::default(),
            crate::SolidCraftVersion::default(),
        )
        .result
        .unwrap_err();
        assert_eq!(err, "no feature in the history builds geometry");
    }

    #[test]
    fn tree_nodes_build_nothing_and_block_nothing() {
        let body = feature(
            2,
            1,
            "Body",
            json!({"definition": "tree_node", "role": "history"}),
        );
        let model = with(cube(), json!([datum(1, 0), body]), json!([]));
        let mapping = map(
            &model,
            &mut Report::default(),
            crate::SolidCraftVersion::default(),
        );
        // Nothing else builds geometry here; the node itself does not stop the history.
        assert_eq!(
            mapping.result.unwrap_err(),
            "no feature in the history builds geometry"
        );
        let statuses: Vec<FeatureStatus> = mapping.features.iter().map(|f| f.status).collect();
        assert_eq!(
            statuses,
            [FeatureStatus::NotNeeded, FeatureStatus::NotNeeded]
        );
    }

    #[test]
    fn components_inserted_and_joints_made_in_the_timeline_are_not_needed() {
        let insert = feature(
            2,
            1,
            "Component Insert 1",
            json!({"definition": "insert_component", "occurrence": "f3d:model:occurrence#1"}),
        );
        let joint = feature(
            3,
            2,
            "Joint 1",
            json!({"definition": "assembly_joint", "joint": "f3d:model:joint#1"}),
        );
        let model = with(cube(), json!([datum(1, 0), insert, joint]), json!([]));
        let mapping = map(
            &model,
            &mut Report::default(),
            crate::SolidCraftVersion::default(),
        );
        assert_eq!(
            mapping.result.unwrap_err(),
            "no feature in the history builds geometry"
        );
        let statuses: Vec<FeatureStatus> = mapping.features.iter().map(|f| f.status).collect();
        assert_eq!(statuses, [FeatureStatus::NotNeeded; 3]);
    }

    #[test]
    fn a_placed_datum_plane_is_not_needed() {
        let frame = json!({"origin": {"x": 0.0, "y": 0.0, "z": 15.0},
            "normal": {"x": 0.0, "y": 0.0, "z": 1.0}, "u_axis": {"x": 1.0, "y": 0.0, "z": 0.0}});
        let placed = feature(
            2,
            1,
            "DatumPlane",
            json!({"definition": "datum_plane", "frame": frame}),
        );
        let model = with(cube(), json!([datum(1, 0), placed]), json!([]));
        let mapping = map(
            &model,
            &mut Report::default(),
            crate::SolidCraftVersion::default(),
        );
        assert_eq!(
            mapping.result.unwrap_err(),
            "no feature in the history builds geometry"
        );
        let statuses: Vec<FeatureStatus> = mapping.features.iter().map(|f| f.status).collect();
        assert_eq!(
            statuses,
            [FeatureStatus::NotNeeded, FeatureStatus::NotNeeded]
        );
    }

    #[test]
    fn the_first_unmapped_feature_names_the_reason() {
        let model = with(
            cube(),
            json!([datum(1, 0), opaque(2, 2, "Later"), opaque(3, 1, "Earlier")]),
            json!([]),
        );
        let err = map(
            &model,
            &mut Report::default(),
            crate::SolidCraftVersion::default(),
        )
        .result
        .unwrap_err();
        assert_eq!(err, "feature \"Earlier\": native is not mapped yet");
    }

    #[test]
    fn suppressed_features_are_left_out_and_reported() {
        let mut hidden = opaque(2, 1, "Hidden");
        hidden["suppressed"] = json!(true);
        let model = with(cube(), json!([datum(1, 0), hidden]), json!([]));
        let mut report = Report::default();
        let err = map(&model, &mut report, crate::SolidCraftVersion::default())
            .result
            .unwrap_err();
        assert_eq!(err, "no feature in the history builds geometry");
        let loss = report.losses().first().unwrap();
        assert_eq!(loss.subject, "feature \"Hidden\"");
    }
}
