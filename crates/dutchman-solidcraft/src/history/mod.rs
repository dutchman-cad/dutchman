//! The model's feature history, as SolidCraft parametric features.
//!
//! Mapping stops body-building history at its first blocker, retaining independent sketches.
//! The writer keeps this editable prefix hidden beside an import of the final B-rep.
//!
//! The mapping reads the model in CADIR's documented JSON form (`docs/cad-ir.md` in cadmpeg),
//! the stable contract cadmpeg publishes, rather than its Rust accessors. It knows the wire
//! versions in [`IR_VERSIONS`] and refuses others rather than misread them.

pub(crate) mod base;
mod blend;
mod cap;
mod chain;
mod constraints;
mod crossing;
mod datum;
mod delete_face;
mod dof;
mod edge_path;
mod extrude;
mod face_profile;
mod faces;
mod helix;
mod helpers;
mod hole;
pub(crate) mod infer;
mod loft;
mod modify;
mod move_body;
mod move_face;
pub(crate) mod params;
mod pattern;
mod primitive;
mod profile;
mod reference;
mod revolve;
mod sheet;
mod shell;
mod sketch;
mod spatial;
mod sweep;
mod thin;
mod thread;
mod wire;

use std::cell::OnceCell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use dutchman_core::{FeatureOutcome, FeatureStatus, Model, Report, Severity};
use serde_json::{json, Value};

/// Accepted document versions. In-memory models use cadmpeg's current v11 serializer;
/// topology homes and enum payloads are read in that serializer's wire layout.
const IR_VERSIONS: [&str; 6] = ["6", "7", "8", "9", "10", "11"];

/// The reason of a feature that is not mapped because an earlier one blocks the history.
pub(crate) const AFTER_BLOCKER: &str =
    "after the first blocking feature; kept in the imported final shape";

/// A replayable history: SolidCraft parameters and features in timeline order.
#[derive(Clone, Debug, Default)]
pub(crate) struct History {
    /// Imported names that previously came from STEP rather than CADIR. They
    /// must not reserve slots in the existing explicit-name allocator.
    pub(crate) default_bodies: BTreeMap<String, String>,
    /// Why the editable timeline stops before the source ends.
    pub(crate) stop: Option<String>,
    /// The names SolidCraft gives the bodies the retained features make, once a replay of
    /// them has shown it; `None` when no replay did. A partial history takes them away again
    /// before the imported final shape, which needs every name.
    pub(crate) replayed_bodies: Option<Vec<String>>,
    /// The name of the first source feature the editable timeline does not hold.
    pub(crate) rest_from: Option<String>,
    /// The source row that emitted each feature, including auxiliary features.
    pub(crate) sources: BTreeMap<u64, usize>,
    /// The centres of the bodies the source stores in the state right after a feature, by the
    /// id of the feature it became. Absent when the source doesn't say, which is not "none".
    pub(crate) stored_bodies: BTreeMap<u64, Vec<crate::verify::StoredBody>>,
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
    /// Newly mapped topological picks cannot replace the original after an inconclusive check.
    pub(crate) topological_replay_required: bool,
    /// Source features that became SolidCraft features or needed none.
    pub(crate) mapped: usize,
    /// Of those, the ones that needed no feature: a component insert, a joint, a default plane.
    pub(crate) not_needed: usize,
    /// Of those, the ones kept that re-apply to a base body already holding their result.
    pub(crate) reapplied: usize,
    /// Active source features.
    pub(crate) total: usize,
    /// Features whose input edges or faces wait for SolidCraft's replay before them
    /// ([`prove_blends`]); until then each has no body.
    pub(crate) blends: Vec<blend::Pending>,
    /// Ids of sketch features whose source sketch is hidden in the source.
    pub(crate) hidden: Vec<u64>,
    /// Names of the bodies the history builds that the source hides (a Fusion body with its
    /// eye off). Imported bodies stay hidden through their STEP's INVISIBILITY; a body the
    /// history rebuilds has no STEP of its own, so the writer lists it in
    /// `visibility.hidden_bodies` instead.
    pub(crate) hidden_bodies: Vec<String>,
    /// Ids of features whose source profile is the whole of a sketch with more than one
    /// profile. Which of them it takes isn't stated, so each is kept only when SolidCraft's
    /// check confirms the replay ([`History::first_unconfirmed`]).
    pub(crate) unconfirmed: BTreeSet<u64>,
    /// Ids of features whose profile region Dutchman chose by measuring every candidate
    /// ([`infer`]). A final shape that differs from the source suspects them as it suspects
    /// unconfirmed profiles, so the timeline keeps no more than it would without them.
    pub(crate) measured: BTreeSet<u64>,
    /// What each source row can do to the body's extent, by row, to tell whether the features
    /// after a stop can still make a retained prefix bigger ([`History::unreachable_prefix`]).
    pub(crate) growth: Vec<Growth>,
}

/// What a source feature can do to the extent of the bodies before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Growth {
    /// It changes no geometry (a sketch, a datum plane, a suppressed feature).
    Inert,
    /// It only removes material: a cut, an intersection.
    Removes,
    /// A fillet or chamfer: it rounds or fills edges, and stays inside the box of the body
    /// it rounds. Its volume change is not bounded here.
    Rounds,
    /// It may build material beyond the earlier bodies, or this is not known.
    Builds,
}

/// Suppresses every feature of `features` that builds or changes a body, leaving sketches,
/// datums and the like live. The features still rebuild when the user unsuppresses them;
/// until then a hidden editable-history copy produces no body of its own, so an export or
/// a measure of the design sees each part once, as the visible original.
pub(crate) fn suppress_bodies(features: &mut [Value]) {
    for feature in features {
        let kind = feature.get("type").and_then(Value::as_str).unwrap_or("");
        let leaves_bodies = kind.starts_with("datum")
            || matches!(
                kind,
                "sketch"
                    | "spatial_sketch"
                    | "tree_node"
                    | "reference_image"
                    | "cosmetic_thread"
                    | "plane"
                    | "axis"
                    | "point"
            )
            || kind.ends_with("_plane")
            || kind.ends_with("_axis");
        if !leaves_bodies {
            if let Some(fields) = feature.as_object_mut() {
                fields.insert("suppressed".to_owned(), Value::Bool(true));
            }
        }
    }
}

/// What `feature` can do to the extent of the bodies before it.
fn growth_of(feature: &Value) -> Growth {
    if feature.get("suppressed").and_then(Value::as_bool) == Some(true)
        || unchanged_geometry(feature)
    {
        return Growth::Inert;
    }
    let kind = kind(feature);
    if kind.starts_with("datum")
        || matches!(
            kind.as_str(),
            "sketch" | "spatial_sketch" | "tree_node" | "reference_image" | "cosmetic_thread"
        )
    {
        return Growth::Inert;
    }
    match kind.as_str() {
        "fillet" | "chamfer" => Growth::Rounds,
        "extrude" | "revolve"
            if matches!(
                operation(feature).get("op").and_then(Value::as_str),
                Some("cut" | "intersect")
            ) =>
        {
            Growth::Removes
        }
        _ => Growth::Builds,
    }
}

/// The bodies the source stores right after `feature`, to compare its replay with. A base
/// feature written with its body as the design ends doesn't hold that state (later features
/// changed the body), so there is nothing at that point to compare it with.
fn compared_state(
    outcome: &FeatureOutcome,
    feature: &Value,
) -> Option<Vec<crate::verify::StoredBody>> {
    if outcome.final_body {
        return None;
    }
    stored_body_centers(feature)
}

/// Whether `feature`'s profile is a whole sketch that has more than one profile. cadmpeg
/// doesn't say which of them such a reference takes: a cut that took them all can remove
/// the body Fusion keeps.
fn whole_of_several(feature: &Value, ctx: &Context) -> bool {
    let Some(profile) = operation(feature).get("profile") else {
        return false;
    };
    let profile = profile::planar(profile);
    profile.get("kind").and_then(Value::as_str) == Some("sketch")
        && profile
            .get("value")
            .and_then(Value::as_str)
            .is_some_and(|sketch| profile::profile_count(ctx, sketch) > 1)
}

/// The bodies the source stores in the state right after `feature`: the centres of their
/// boxes, from `id:x,y,z;id:x,y,z` in its `body_centers`, each exact when its id is in
/// `body_centers_exact`. `None` when it has none, which is unknown, when an entry can't be
/// read, or when `body_centers_incomplete` names any body whose centre rests on incomplete
/// edge evidence, even if that body has a centre entry.
fn stored_body_centers(feature: &Value) -> Option<Vec<crate::verify::StoredBody>> {
    let property = |name: &str| {
        feature
            .pointer(&format!("/source_properties/{name}"))
            .and_then(Value::as_str)
    };
    if property("body_centers_incomplete")
        .is_some_and(|ids| ids.split(';').any(|id| !id.trim().is_empty()))
    {
        return None;
    }
    let exact: std::collections::BTreeSet<&str> = property("body_centers_exact")
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|id| !id.is_empty())
        .collect();
    let centers = property("body_centers")?
        .split(';')
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (id, point) = entry.split_once(':')?;
            let point: Vec<f64> = point
                .split(',')
                .map(|value| value.trim().parse::<f64>().ok().filter(|v| v.is_finite()))
                .collect::<Option<_>>()?;
            Some(crate::verify::StoredBody {
                center: <[f64; 3]>::try_from(point).ok()?,
                exact: exact.contains(id.trim()),
            })
        })
        .collect::<Option<Vec<_>>>()?;
    (!centers.is_empty()).then_some(centers)
}

impl History {
    /// What the source features after this history's last body-building feature can do to its
    /// extent, when none of them can build beyond it: `Some(true)` when they only remove
    /// material, `Some(false)` when some round edges. `None` when a later feature may build,
    /// when nothing builds a body in this history, or when no feature follows.
    pub(crate) fn unreachable_prefix(&self) -> Option<bool> {
        let last = self
            .features
            .iter()
            .filter(|feature| feature.get("type").and_then(Value::as_str) != Some("sketch"))
            .filter_map(|feature| self.sources.get(&feature.get("id")?.as_u64()?))
            .max()
            .copied()?;
        let after = self.growth.get(last.checked_add(1)?..)?;
        if after.contains(&Growth::Builds) || after.iter().all(|g| *g == Growth::Inert) {
            return None;
        }
        Some(!after.contains(&Growth::Rounds))
    }

    /// Where the source stores the bodies at the point this history ends, when its last
    /// feature that isn't a sketch says. An earlier feature's count can't speak for a later one.
    pub(crate) fn stored_bodies_at_end(&self) -> Option<&[crate::verify::StoredBody]> {
        let last = self
            .features
            .iter()
            .rev()
            .find(|feature| feature.get("type").and_then(Value::as_str) != Some("sketch"))?;
        self.stored_bodies
            .get(&last.get("id").and_then(Value::as_u64)?)
            .map(Vec::as_slice)
    }

    /// The source row of the first kept feature whose whole-sketch profile nothing has
    /// confirmed, and why it can't stay unconfirmed.
    pub(crate) fn first_unconfirmed(&self) -> Option<(usize, String)> {
        self.features.iter().find_map(|feature| {
            let id = feature.get("id").and_then(Value::as_u64)?;
            let name = feature.get("name").and_then(Value::as_str).unwrap_or("?");
            let why = if self.unconfirmed.contains(&id) {
                format!(
                    "feature \"{name}\": its profile is the whole of a sketch with several \
                     profiles, and SolidCraft's check could not confirm which it takes"
                )
            } else if self.measured.contains(&id) {
                format!(
                    "feature \"{name}\": its profile region was inferred by measurement, and \
                     the complete replay does not match the source"
                )
            } else {
                return None;
            };
            Some((*self.sources.get(&id)?, why))
        })
    }

    /// Source features that became SolidCraft features, but those that only re-apply to a base
    /// body already holding their result.
    pub(crate) fn rebuilt(&self) -> usize {
        self.mapped
            .saturating_sub(self.not_needed)
            .saturating_sub(self.reapplied)
    }

    /// Source features that need one: all active ones but those that need no feature.
    pub(crate) fn needing(&self) -> usize {
        self.total.saturating_sub(self.not_needed)
    }
}

/// Looks and explicit names for bodies carried by an Import, independent of feature history.
pub(crate) fn imported(
    model: &Model,
    step: &str,
    report: &mut Report,
    name_all: bool,
) -> Result<History, dutchman_core::Error> {
    // A STEP input goes to SolidCraft as its own text, whose names and looks the model's
    // appearance bindings do not describe.
    if model.source_step.is_some() {
        return Ok(History::default());
    }
    imported_restated(model, step, report, name_all)
}

/// Like [`imported`], for a part whose `step` was restated from its geometry (an assembly
/// part, whatever text it was read from): the model's own bindings and colours describe it.
pub(crate) fn imported_restated(
    model: &Model,
    step: &str,
    report: &mut Report,
    name_all: bool,
) -> Result<History, dutchman_core::Error> {
    if !name_all && model.ir.model.appearance_bindings.is_empty() {
        return Ok(History::default());
    }
    let wire = serde_json::to_value(&model.ir)
        .map_err(|err| dutchman_core::Error::Encode(format!("cannot map appearances: {err}")))?;
    let mut ctx = Context::new(wire.get("model").unwrap_or(&Value::Null))
        .with_geometry(wire.get("geometry").unwrap_or(&Value::Null));
    ctx.source = Some(model);
    let ids: Vec<_> = model
        .ir
        .geometry
        .bodies
        .iter()
        .map(|body| body.id.as_str())
        .collect();
    let names = faces::import_names(&mut ctx, &ids, Some(step));
    let face_looks = faces::looks(&mut ctx, report);
    Ok(History {
        features: if names.is_empty() {
            Vec::new()
        } else {
            vec![serde_json::json!({"body_names": names})]
        },
        looks: ctx.looks,
        face_looks,
        default_bodies: ctx.default_bodies,
        ..History::default()
    })
}

/// What one source feature becomes.
enum Mapped {
    /// SolidCraft needs no feature for it (a principal datum plane).
    Nothing,
    /// This SolidCraft feature.
    Feature(Value),
    /// Written with an earlier feature (an attached plate's bend).
    Consumed,
}

impl History {
    /// Names inferred from a STEP default select looks, but must not override
    /// the import's existing naming behaviour or its automatic body numbering.
    pub(crate) fn preserve_import_names(&mut self) {
        for feature in &mut self.features {
            if feature
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|kind| kind != "import")
            {
                continue;
            }
            let defaults = feature
                .get("body_names")
                .and_then(Value::as_array)
                .is_some_and(|names| {
                    !names.is_empty()
                        && names.iter().all(|name| {
                            name.as_str()
                                .is_some_and(|name| self.default_bodies.contains_key(name))
                        })
                });
            if defaults {
                if let Some(fields) = feature.as_object_mut() {
                    fields.remove("body_names");
                }
            }
        }
    }

    pub(crate) fn import_body_names(&self) -> Option<&Value> {
        self.features.first()?.get("body_names").filter(|names| {
            names.as_array().is_some_and(|names| {
                names.iter().all(|name| {
                    name.as_str()
                        .is_some_and(|name| !self.default_bodies.contains_key(name))
                })
            })
        })
    }

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
                    if key == "path_sketch" && id == 0 {
                        continue;
                    }
                    fields.insert(key.to_owned(), Value::from(id.saturating_add(offset)));
                }
            }
            // Hole locations reference their sketch inside the points selection.
            if let Some(sketch) = fields
                .get_mut("points")
                .and_then(|points| points.get_mut("sketch"))
            {
                if let Some(id) = sketch.as_u64() {
                    *sketch = Value::from(id.saturating_add(offset));
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
            // Feature distances and angles that name a parameter follow its renaming.
            for (parent, key) in [
                ("extent", "distance"),
                ("extent", "distance2"),
                ("", "angle"),
            ] {
                let slot = if parent.is_empty() {
                    fields.get_mut(key)
                } else {
                    fields.get_mut(parent).and_then(|v| v.get_mut(key))
                };
                if let Some(slot) = slot {
                    if let Some(expr) = slot.as_str() {
                        *slot = Value::String(params::rename_in(expr, &renamed));
                    }
                }
            }
            for key in ["constraints", "wire_constraints"] {
                if let Some(constraints) = fields
                    .get_mut("sketch")
                    .and_then(|sketch| sketch.get_mut(key))
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
        }
        for id in &mut self.hidden {
            *id = id.saturating_add(offset);
        }
        self.next_id = self.next_id.saturating_add(offset);
    }

    /// The sketch features that should start hidden: those the source hides, and those no
    /// feature of the history is built on (kept for reference only).
    pub(crate) fn hidden_sketches(&self) -> Vec<Value> {
        let mut used = std::collections::BTreeSet::new();
        for feature in &self.features {
            for key in ["sketch", "path_sketch"] {
                if let Some(id) = feature.get(key).and_then(Value::as_u64) {
                    used.insert(id);
                }
            }
            for key in ["sections", "rails"] {
                for row in feature
                    .get(key)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if let Some(id) = row.get("sketch").and_then(Value::as_u64) {
                        used.insert(id);
                    }
                }
            }
        }
        self.features
            .iter()
            .filter(|feature| feature.get("type").and_then(Value::as_str) == Some("sketch"))
            .filter_map(|feature| feature.get("id").and_then(Value::as_u64))
            .filter(|id| !used.contains(id) || self.hidden.contains(id))
            .map(Value::from)
            .collect()
    }

    /// Keeps CADIR's component disambiguation and STEP's numbered default names
    /// in separate allocation sets, updating features and looks together.
    pub(crate) fn rename_bodies(&mut self, label: &str, taken: &mut BodyNames) {
        let names: std::collections::BTreeSet<String> = self
            .features
            .iter()
            .filter_map(|feature| feature.get("body_names").and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let mut renamed = BTreeMap::new();
        for name in names
            .iter()
            .filter(|name| !self.default_bodies.contains_key(*name))
        {
            let mut new = name.clone();
            let mut n = 2;
            while taken.explicit.contains(&new) {
                new = if n == 2 {
                    format!("{name} ({label})")
                } else {
                    format!("{name} ({label} {n})")
                };
                n += 1;
            }
            taken.explicit.insert(new.clone());
            if new != *name {
                renamed.insert(name.clone(), new);
            }
        }
        // Default names are allocated in import order, including multi-shape
        // products and repeated product names. They never change the explicit
        // allocator above, which did not previously see these imports.
        let mut seen = BTreeSet::new();
        for name in self
            .features
            .iter()
            .filter_map(|feature| feature.get("body_names").and_then(Value::as_array))
            .flatten()
            .filter_map(Value::as_str)
            .filter(|name| seen.insert(*name))
        {
            if let Some(base) = self.default_bodies.get(name) {
                let mut new = base.clone();
                let mut n = 2;
                while taken.all.contains(&new) {
                    new = format!("{base} ({n})");
                    n += 1;
                }
                taken.all.insert(new.clone());
                if new != name {
                    renamed.insert(name.to_owned(), new);
                }
            } else {
                taken
                    .all
                    .insert(renamed.get(name).map_or(name, String::as_str).to_owned());
            }
        }
        if renamed.is_empty() {
            self.preserve_import_names();
            return;
        }
        let rename = |name: &str| {
            renamed
                .get(name)
                .cloned()
                .unwrap_or_else(|| name.to_owned())
        };
        for feature in &mut self.features {
            for key in ["body_names", "bodies", "tools", "targets"] {
                if let Some(names) = feature.get_mut(key).and_then(Value::as_array_mut) {
                    for name in names {
                        if let Some(old) = name.as_str() {
                            *name = Value::String(rename(old));
                        }
                    }
                }
            }
            for key in ["target", "body", "path_body"] {
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
        for name in &mut self.hidden_bodies {
            *name = rename(name);
        }
        for name in self.replayed_bodies.iter_mut().flatten() {
            *name = rename(name);
        }
        for face in &mut self.face_looks {
            if let Some(fields) = face.as_object_mut() {
                if let Some(old) = fields.get("body").and_then(Value::as_str) {
                    let new = rename(old);
                    fields.insert("body".to_owned(), Value::String(new));
                }
            }
        }
        self.default_bodies = std::mem::take(&mut self.default_bodies)
            .into_iter()
            .map(|(name, base)| (rename(&name), base))
            .collect();
        self.preserve_import_names();
    }
}

/// Explicit CADIR names keep their existing component disambiguation; newly
/// matched default imports use the names previously allocated by SolidCraft.
#[derive(Default)]
pub(crate) struct BodyNames {
    explicit: BTreeSet<String>,
    all: BTreeSet<String>,
}

impl BodyNames {
    pub(crate) fn add_defaults(&mut self, names: impl IntoIterator<Item = String>) {
        self.all.extend(names);
    }
}

impl FromIterator<String> for BodyNames {
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> Self {
        let explicit: BTreeSet<_> = iter.into_iter().collect();
        Self {
            all: explicit.clone(),
            explicit,
        }
    }
}

type RowIndex<'a> = OnceCell<HashMap<&'a str, &'a Value>>;
type ChildIndex<'a> = OnceCell<HashMap<&'a str, Vec<&'a Value>>>;

/// The model and what has been mapped so far.
#[derive(Clone)]
struct Context<'a> {
    /// Source for guarded STEP restatement of stored feature bodies.
    source: Option<&'a Model>,
    /// The CADIR `model` object.
    model: &'a Value,
    /// Current topology, separate from the model records on CADIR v11.
    geometry: &'a Value,
    /// Typed v11 outputs paired by identity with model-history projections.
    design: &'a Value,
    /// The arenas `model` holds when it is a subset of the whole; reading another is a bug,
    /// which a debug build panics on.
    reading_only: Option<&'static [&'static str]>,
    /// Borrowed rows, indexed once on the first lookup in each arena. Geometry walks
    /// otherwise scan whole arenas for every edge, vertex and point they visit.
    rows: BTreeMap<&'a str, RowIndex<'a>>,
    children: BTreeMap<&'static str, ChildIndex<'a>>,
    target_version: crate::SolidCraftVersion,
    next_id: u64,
    /// SolidCraft parameter names by CADIR parameter id.
    params: BTreeMap<String, String>,
    /// Mapped sketches by CADIR sketch id.
    sketches: BTreeMap<String, sketch::Mapped>,
    /// Where the dimensions of each mapped sketch put its points, by CADIR sketch id.
    coordinates: BTreeMap<String, Vec<[Option<chain::Coordinate>; 2]>>,
    /// Mapped 3D sketches by CADIR spatial sketch id.
    spatial: BTreeMap<String, spatial::Mapped>,
    /// SolidCraft looks of the bodies the history makes, by body name.
    looks: BTreeMap<String, Value>,
    /// The timeline name each mapped source feature took, by CADIR feature id.
    feature_names: BTreeMap<String, String>,
    /// Source ids of bends written with their attached plate.
    consumed_bends: std::collections::BTreeSet<String>,
    /// All features making a composite pattern's instance set, including its seeds.
    pattern_features: BTreeMap<String, Vec<String>>,
    pattern_replay_required: bool,
    topological_replay_required: bool,
    /// The source bodies the history makes under their own names: (CADIR body id, name).
    named_bodies: Vec<(String, String)>,
    /// Bodies an earlier feature makes that the final model leaves out, under their names:
    /// ((CADIR producing feature id, feature-local body id), name).
    generated_bodies: Vec<((String, String), String)>,
    /// Trial names that must be discarded unless the complete source body check passes.
    confirming_consumed_bodies: bool,
    default_bodies: BTreeMap<String, String>,
    /// Region of each explicitly named imported shape.
    imported_regions: BTreeMap<String, String>,
    /// Bodies proven to produce no solid/surface shape in the STEP restatement.
    unwritten_bodies: BTreeSet<String>,
    /// Resolved frames of earlier nonbuilding datum planes.
    planes: BTreeMap<String, datum::Frame>,
    /// Whether a feature that builds geometry has been mapped.
    builds: bool,
    /// The sheet thicknesses (mm) its sheet-metal features use, each with its rule's name.
    sheet_rules: Vec<sheet::SheetRule>,
    /// Blends, sweep paths and face profiles waiting for SolidCraft's replay to prove them.
    blends: Vec<blend::Pending>,
    /// Features the feature being mapped needs before it (a sketch of a face's boundary).
    extra: Vec<Value>,
    /// Whether exactly one feature of the timeline builds geometry, so the final model's
    /// faces at its end are its own.
    sole_builder: bool,
    /// Set by the feature being mapped: its profile region was not proven against the source.
    profile_unverified: bool,
    /// Set by the feature being mapped: its profile region disagrees with the source.
    profile_mismatch: bool,
    /// Source ids of later features that re-apply to a base body already holding their result.
    reapplied: BTreeSet<String>,
    /// Set by a base feature being mapped with its body as the design ends.
    final_body: bool,
    /// Set by the feature being mapped: a selection the source does not record was inferred.
    inferred: Option<dutchman_core::Inferred>,
    /// Set by the feature being mapped: that selection was chosen by measuring every
    /// candidate, so a final shape that differs suspects it first.
    measured: bool,
}

impl<'a> Context<'a> {
    #[cfg(test)]
    fn testing(model: &'a Value) -> Self {
        Self::new(model).with_geometry(model)
    }

    fn new(model: &'a Value) -> Self {
        Context {
            source: None,
            model,
            geometry: &Value::Null,
            design: &Value::Null,
            reading_only: None,
            children: [("regions", OnceCell::new()), ("faces", OnceCell::new())]
                .into_iter()
                .collect(),
            rows: model
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, rows)| rows.is_array())
                .map(|(name, _)| (name.as_str(), OnceCell::new()))
                .collect(),
            target_version: crate::SolidCraftVersion::default(),
            next_id: 1,
            params: BTreeMap::new(),
            sketches: BTreeMap::new(),
            coordinates: BTreeMap::new(),
            spatial: BTreeMap::new(),
            looks: BTreeMap::new(),
            feature_names: BTreeMap::new(),
            consumed_bends: std::collections::BTreeSet::new(),
            pattern_features: BTreeMap::new(),
            pattern_replay_required: false,
            topological_replay_required: false,
            named_bodies: Vec::new(),
            generated_bodies: Vec::new(),
            confirming_consumed_bodies: false,
            default_bodies: BTreeMap::new(),
            imported_regions: BTreeMap::new(),
            unwritten_bodies: BTreeSet::new(),
            planes: BTreeMap::new(),
            builds: false,
            sheet_rules: Vec::new(),
            blends: Vec::new(),
            extra: Vec::new(),
            sole_builder: false,
            profile_unverified: false,
            profile_mismatch: false,
            reapplied: BTreeSet::new(),
            final_body: false,
            inferred: None,
            measured: false,
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
        numbered_name(&base, taken)
    }

    fn with_geometry(mut self, geometry: &'a Value) -> Self {
        self.geometry = geometry;
        self.rows.extend(
            geometry
                .as_object()
                .into_iter()
                .flatten()
                .filter(|(_, rows)| rows.is_array())
                .map(|(name, _)| (name.as_str(), OnceCell::new())),
        );
        self
    }

    /// Unordered topology children, indexed from their authoritative parent pointers.
    fn children(&self, arena: &str, parent: &str, id: &str) -> &[&'a Value] {
        let Some(index) = self.children.get(arena) else {
            return &[];
        };
        let rows = index.get_or_init(|| {
            let mut rows: HashMap<&str, Vec<&Value>> = HashMap::new();
            for row in self.arena(arena) {
                if let Some(parent) = row.get(parent).and_then(Value::as_str) {
                    rows.entry(parent).or_default().push(row);
                }
            }
            rows
        });
        rows.get(id).map_or(&[], Vec::as_slice)
    }

    /// Whether a body the history makes already has this name.
    fn body_name_taken(&self, name: &str) -> bool {
        self.named_bodies
            .iter()
            .map(|(_, known)| known)
            .chain(self.generated_bodies.iter().map(|(_, known)| known))
            .any(|known| known == name)
    }

    /// A row of a CADIR arena by id.
    fn row(&self, arena: &str, id: &str) -> Option<&'a Value> {
        let rows = self.rows.get(arena)?.get_or_init(|| {
            let mut rows = HashMap::new();
            for row in self.arena(arena) {
                if let Some(id) = row.get("id").and_then(Value::as_str) {
                    // Match the original scan's first row when ids repeat.
                    rows.entry(id).or_insert(row);
                }
            }
            rows
        });
        rows.get(id).copied()
    }

    /// The SolidCraft expression of the one parameter `feature` owns for the source kind
    /// `kind` (`AlongDistance`, `AlongAngle`) whose magnitude is `magnitude` (mm or radians):
    /// the parameter's name, inside `abs()` when the source signs it, since the feature
    /// carries its direction separately. `None` when no parameter, or several, match.
    fn owned_parameter(&self, feature: &Value, kind: &str, magnitude: f64) -> Option<String> {
        let owner = feature.get("id").and_then(Value::as_str)?;
        let value = |row: &Value| row.pointer("/value/value").and_then(Value::as_f64);
        let mut found = self.arena("parameters").iter().filter(|row| {
            row.get("owner").and_then(Value::as_str) == Some(owner)
                && row
                    .pointer("/properties/source_kind")
                    .and_then(Value::as_str)
                    == Some(kind)
                && value(row)
                    .is_some_and(|v| (v.abs() - magnitude).abs() <= 1e-6 * magnitude.max(1.0))
        });
        let row = found.next()?;
        if found.next().is_some() {
            return None;
        }
        let name = self.params.get(row.get("id").and_then(Value::as_str)?)?;
        Some(if value(row)? < 0.0 {
            format!("abs({name})")
        } else {
            name.clone()
        })
    }

    /// The SolidCraft name of the parameter the source feature names under the
    /// `source_properties` key `key`, when that parameter holds `magnitude` (mm or radians)
    /// as its own value. The name is inside `abs()` when the source signs it.
    fn bound_parameter(&self, feature: &Value, key: &str, magnitude: f64) -> Option<String> {
        let id = feature.pointer("/source_properties")?.get(key)?.as_str()?;
        let row = self.row("parameters", id)?;
        let value = row.pointer("/value/value")?.as_f64()?;
        if (value.abs() - magnitude).abs() > 1e-6 * magnitude.max(1.0) {
            return None;
        }
        let name = self.params.get(id)?;
        Some(if value < 0.0 {
            format!("abs({name})")
        } else {
            name.clone()
        })
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// The name of the sheet rule of `thickness` mm, made the first time it is asked for.
    fn sheet_rule(&mut self, thickness: f64) -> String {
        self.sheet_rule_of(thickness, None, None)
    }

    /// The name of the sheet rule of `thickness` mm with the given bend radius and K-factor
    /// (SolidCraft's defaults where `None`), made the first time it is asked for.
    fn sheet_rule_of(
        &mut self,
        thickness: f64,
        radius: Option<f64>,
        k_factor: Option<f64>,
    ) -> String {
        let same = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => (a - b).abs() < 1e-9,
            _ => false,
        };
        if let Some(known) = self.sheet_rules.iter().find(|known| {
            (known.thickness - thickness).abs() < 1e-9
                && same(known.radius, radius)
                && same(known.k_factor, k_factor)
        }) {
            return known.name.clone();
        }
        let mut name = format!("Sheet {thickness} mm");
        if let Some(radius) = radius {
            name.push_str(&format!(" R{radius}"));
        }
        if let Some(k_factor) = k_factor {
            name.push_str(&format!(" K{k_factor}"));
        }
        self.sheet_rules.push(sheet::SheetRule {
            thickness,
            radius,
            k_factor,
            name: name.clone(),
        });
        name
    }

    /// The rows of a CADIR arena.
    fn arena(&self, name: &str) -> &'a [Value] {
        debug_assert!(
            self.reading_only
                .is_none_or(|arenas| arenas.contains(&name)),
            "arena {name:?} is read from a model that holds only {:?}",
            self.reading_only,
        );
        let home = if cadmpeg_ir::document::ArenaName::parse(name)
            .is_some_and(|arena| arena.home() == cadmpeg_ir::document::ArenaHome::Geometry)
        {
            self.geometry
        } else {
            self.model
        };
        home.get(name)
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
    keep_sketches: bool,
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
    map_with_sketches(model, report, target_version, true)
}

pub(crate) fn map_with_sketches(
    model: &Model,
    report: &mut Report,
    target_version: crate::SolidCraftVersion,
    keep_sketches: bool,
) -> Mapping {
    map_with_profile_check(
        model,
        report,
        target_version,
        keep_sketches,
        &mut infer::Unmeasured,
        None,
    )
}

/// Mapping with a host check for otherwise unresolved sketch-selection candidates.
/// Planning has no check and retains the original profile refusal. `progress` hears of
/// each timeline feature as mapping reaches it.
pub(crate) fn map_with_profile_check(
    model: &Model,
    report: &mut Report,
    target_version: crate::SolidCraftVersion,
    keep_sketches: bool,
    check: &mut infer::Check<'_>,
    progress: Option<&dutchman_core::Progress<'_>>,
) -> Mapping {
    map_with_source_profile_check(
        model,
        report,
        target_version,
        keep_sketches,
        check,
        progress,
        &[],
    )
}

/// Decode losses are read-only evidence, separate from the history losses being written.
pub(crate) fn map_with_source_profile_check(
    model: &Model,
    report: &mut Report,
    target_version: crate::SolidCraftVersion,
    keep_sketches: bool,
    check: &mut infer::Check<'_>,
    progress: Option<&dutchman_core::Progress<'_>>,
    source_losses: &[dutchman_core::Loss],
) -> Mapping {
    map_checked_until(
        model,
        report,
        target_version,
        keep_sketches,
        check,
        progress,
        source_losses,
        None,
    )
}

pub(crate) fn map_until(
    model: &Model,
    report: &mut Report,
    target_version: crate::SolidCraftVersion,
    keep_sketches: bool,
    source_losses: &[dutchman_core::Loss],
    deadline: Option<std::time::Instant>,
) -> Mapping {
    map_checked_until(
        model,
        report,
        target_version,
        keep_sketches,
        &mut infer::Unmeasured,
        None,
        source_losses,
        deadline,
    )
}

#[allow(clippy::too_many_arguments)]
fn map_checked_until(
    model: &Model,
    report: &mut Report,
    target_version: crate::SolidCraftVersion,
    keep_sketches: bool,
    check: &mut infer::Check<'_>,
    progress: Option<&dutchman_core::Progress<'_>>,
    source_losses: &[dutchman_core::Loss],
    deadline: Option<std::time::Instant>,
) -> Mapping {
    let mut features_out = Vec::new();
    let mut references = History::default();
    let mut notes = Report::default();
    let mut result = map_into(
        model,
        &mut notes,
        &mut features_out,
        &mut references,
        target_version,
        keep_sketches,
        false,
        check,
        progress,
        source_losses,
        deadline,
    );
    // A missing consumed identity may be tried only in an isolated complete replay.
    // Failed, unavailable or partial checks leave the original mapping and its blocker.
    if crate::VersionGate::ScopedBodies.supported(target_version)
        && result.as_ref().is_ok_and(|history| {
            history.stop.as_deref().is_some_and(|why| {
                why.contains("it selects a body that an earlier feature has not named")
            })
        })
    {
        let mut trial_notes = Report::default();
        let mut trial_features = Vec::new();
        let mut trial_references = History::default();
        let trial = map_into(
            model,
            &mut trial_notes,
            &mut trial_features,
            &mut trial_references,
            target_version,
            keep_sketches,
            true,
            check,
            progress,
            source_losses,
            deadline,
        );
        if trial.as_ref().is_ok_and(|history| {
            history.stop.is_none()
                && matches!(
                    check.fit(history, None, true, false),
                    infer::Fit::Matches(_)
                )
        }) {
            result = trial;
            notes = trial_notes;
            features_out = trial_features;
            references = trial_references;
            notes.push(
                Severity::Info,
                "history",
                "The source body check confirmed the consumed tool bodies",
            );
        }
    }
    report.append(notes);
    let mut mapping = Mapping {
        result,
        features: features_out,
        references,
        keep_sketches,
    };
    discard_reference_sketches(&mut mapping);
    mapping
}

/// Keep only sketches that the retained body-building history references, transitively.
fn discard_reference_sketches(mapping: &mut Mapping) {
    if mapping.keep_sketches {
        return;
    }
    mapping.references.features.clear();
    let Ok(history) = &mut mapping.result else {
        return;
    };
    let sketches: BTreeMap<u64, &Value> = history
        .features
        .iter()
        .filter(|f| f.get("type").and_then(Value::as_str) == Some("sketch"))
        .filter_map(|f| Some((f.get("id")?.as_u64()?, f)))
        .collect();
    let mut pending: Vec<&Value> = history
        .features
        .iter()
        .filter(|f| f.get("type").and_then(Value::as_str) != Some("sketch"))
        .collect();
    let mut needed = std::collections::BTreeSet::new();
    while let Some(value) = pending.pop() {
        match value {
            Value::Object(fields) => {
                for (key, value) in fields {
                    if matches!(key.as_str(), "sketch" | "path_sketch") {
                        if let Some(id) = value.as_u64() {
                            if needed.insert(id) {
                                if let Some(sketch) = sketches.get(&id) {
                                    pending.push(sketch);
                                }
                            }
                        }
                    }
                    if value.is_object() || value.is_array() {
                        pending.push(value);
                    }
                }
            }
            Value::Array(values) => pending.extend(values),
            _ => {}
        }
    }
    let mut removed = Vec::new();
    let mut retained = Vec::new();
    for (index, feature) in std::mem::take(&mut history.features)
        .into_iter()
        .enumerate()
    {
        let id = feature.get("id").and_then(Value::as_u64);
        if feature.get("type").and_then(Value::as_str) == Some("sketch")
            && id.is_none_or(|id| !needed.contains(&id))
        {
            removed.push(index);
            if let Some(outcome) = id
                .and_then(|id| history.sources.get(&id))
                .and_then(|&row| mapping.features.get_mut(row))
            {
                outcome.status = FeatureStatus::LeftOut;
                outcome.reason = Some("reference sketch left out by choice".to_owned());
            }
        } else {
            retained.push(feature);
        }
    }
    history.features = retained;
    for blend in &mut history.blends {
        blend.feature = blend
            .feature
            .saturating_sub(removed.partition_point(|&index| index < blend.feature));
    }
    history.mapped = mapping
        .features
        .iter()
        .filter(|f| matches!(f.status, FeatureStatus::Mapped | FeatureStatus::NotNeeded))
        .count();
    history.not_needed = mapping
        .features
        .iter()
        .filter(|f| f.status == FeatureStatus::NotNeeded)
        .count();
    history.reapplied = mapping
        .features
        .iter()
        .filter(|f| f.status == FeatureStatus::Mapped && f.reapplied)
        .count();
    history.total = mapping
        .features
        .iter()
        .filter(|f| f.status != FeatureStatus::LeftOut)
        .count();
}

/// The reason cadmpeg gave when the design's timeline was in the file but its feature history
/// did not decode, in plain words; `None` when the decoder reported no such failure, so the
/// design really has no timeline.
fn undecoded_history(source_losses: &[dutchman_core::Loss]) -> Option<String> {
    let loss = source_losses.iter().find(|loss| {
        loss.code.as_deref().is_some_and(|code| {
            [
                "/design.intent-undecoded",
                "/feature.timeline-undecoded",
                "/design.primary-record-undecoded",
            ]
            .iter()
            .any(|suffix| code.ends_with(suffix))
        })
    })?;
    // The decoder's text ends with what failed, after the last ": ".
    let cause = loss
        .detail
        .rsplit_once(": ")
        .map_or(loss.detail.as_str(), |(_, cause)| cause)
        .trim()
        .trim_end_matches('.');
    let cause = cause.strip_prefix("F3D ").unwrap_or(cause);
    Some(format!(
        "the design's feature timeline is in the file, but cadmpeg could not decode it ({cause})"
    ))
}

/// Whether a mapped feature cuts from or intersects the bodies before it.
fn cuts_a_body(feature: &Value) -> bool {
    matches!(
        feature.get("operation").and_then(Value::as_str),
        Some("cut" | "intersect")
    ) && feature.get("type").and_then(Value::as_str) != Some("sketch")
}

/// The names of the bodies the history builds whose source body is hidden, once each.
fn hidden_bodies(ctx: &Context) -> Vec<String> {
    let mut names = Vec::new();
    for (id, name) in &ctx.named_bodies {
        let hidden = ctx
            .row("bodies", id)
            .and_then(|row| row.get("visible"))
            .and_then(Value::as_bool)
            == Some(false);
        if hidden && !names.contains(name) {
            names.push(name.clone());
        }
    }
    names
}

#[allow(clippy::too_many_arguments)]
fn map_into(
    model: &Model,
    report: &mut Report,
    out: &mut Vec<FeatureOutcome>,
    references: &mut History,
    target_version: crate::SolidCraftVersion,
    keep_sketches: bool,
    confirming_consumed_bodies: bool,
    check: &mut infer::Check<'_>,
    progress: Option<&dutchman_core::Progress<'_>>,
    source_losses: &[dutchman_core::Loss],
    deadline: Option<std::time::Instant>,
) -> Result<History, String> {
    if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
        return Err("planning time budget reached".to_owned());
    }
    if model.ir.model.features.is_empty() && !model.ir.design.features.is_empty() {
        return Err(
            "CADIR v11 design-only history is not mapped: the design layer's history was not projected into model history (the design-history entries of the report say why)".to_owned(),
        );
    }
    let mut document =
        serde_json::to_value(&model.ir).map_err(|err| format!("cannot read the model: {err}"))?;
    check_version(&document)?;
    if let Some(model) = document.get_mut("model") {
        profile::resolve_sketch_features(model, target_version);
    }
    let wire = document.get("model").unwrap_or(&Value::Null);
    let (params, param_names) = params::map(model, report);
    references.params = params.clone();
    let mut ctx =
        Context::new(wire).with_geometry(document.get("geometry").unwrap_or(&Value::Null));
    ctx.source = Some(model);
    ctx.design = document.get("design").unwrap_or(&Value::Null);
    ctx.target_version = target_version;
    ctx.confirming_consumed_bodies = confirming_consumed_bodies;
    ctx.params = param_names;
    let modern_spatial = target_version >= crate::SolidCraftVersion::V0_44_0;
    let is_sketch = |feature: &Value| {
        kind(feature) == "sketch"
            || (modern_spatial && operation_kind(feature) == Some("spatial_sketch"))
    };
    let mut features: Vec<&Value> = ctx.arena("features").iter().collect();
    let no_timeline = features.is_empty();
    features.sort_by_key(|feature| feature.get("ordinal").and_then(Value::as_u64));
    // Suppressed features count too: a caller can suppress a suffix of the timeline (the
    // FreeCAD partial prefix does) while the final model still holds what that suffix built.
    ctx.sole_builder = features
        .iter()
        .filter(|feature| {
            let kind = kind(feature);
            !(is_sketch(feature)
                || kind.starts_with("datum")
                || matches!(kind.as_str(), "tree_node" | "reference_image")
                || is_assembly_feature(feature)
                || helpers::is_helper(feature))
        })
        .count()
        == 1;

    let mut history = History::default();
    let mut blocked: Option<String> = None;
    let last_builder = features.iter().rposition(|feature| {
        let kind = kind(feature);
        feature.get("suppressed").and_then(Value::as_bool) != Some(true)
            && !unchanged_geometry(feature)
            && !(is_sketch(feature)
                || kind.starts_with("datum")
                || matches!(kind.as_str(), "tree_node" | "reference_image")
                || is_assembly_feature(feature)
                || helpers::is_helper(feature))
    });
    // Sketches the source keeps without a timeline row of their own (cadmpeg gives some only in
    // its sketch arena). One a feature uses is mapped just before that feature; the others are
    // added after the timeline, below.
    let bound: std::collections::BTreeSet<String> = ctx
        .arena("features")
        .iter()
        .filter_map(|feature| {
            operation(feature)
                .pointer("/sketch/sketch")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    let timeline = progress.map(|p| p.features(features.len() as u64));
    let growth: Vec<Growth> = features.iter().map(|feature| growth_of(feature)).collect();
    for (position, feature) in features.into_iter().enumerate() {
        if let Some(timeline) = &timeline {
            timeline.at(position as u64 + 1, name(feature));
        }
        if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            return Err("planning time budget reached".to_owned());
        }
        let outcome = |status, reason: Option<String>| FeatureOutcome {
            output: Default::default(),
            name: name(feature).map(str::to_owned),
            kind: kind(feature),
            status,
            reason,
            ordinal: feature.get("ordinal").and_then(Value::as_u64),
            variant: variant(feature),
            profile_unverified: false,
            reapplied: false,
            final_body: false,
            inferred: None,
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
        if unchanged_geometry(feature) {
            report.push(
                Severity::Info,
                describe(feature),
                "changes no geometry in the source; left out",
            );
            out.push(outcome(
                FeatureStatus::LeftOut,
                Some("changes no geometry in the source".to_owned()),
            ));
            continue;
        }
        if !keep_sketches && blocked.is_some() && is_sketch(feature) {
            out.push(outcome(
                FeatureStatus::LeftOut,
                Some("reference sketch left out by choice".to_owned()),
            ));
            continue;
        }
        history.total += 1;
        let structural = matches!(
            operation_kind(feature),
            Some("datum_principal_plane" | "tree_node" | "reference_image")
        ) || is_assembly_feature(feature)
            || helpers::is_helper(feature);
        if blocked.is_some() && (!is_sketch(feature) || !keep_sketches) && !structural {
            out.push(outcome(
                FeatureStatus::Blocked,
                Some(AFTER_BLOCKER.to_owned()),
            ));
            continue;
        }
        map_unbound_sketches(feature, &bound, &mut ctx, report, &mut history, references);
        let builds = ctx.builds;
        let named_bodies = ctx.named_bodies.len();
        let before = history.features.len();
        let blends = ctx.blends.len();
        let inferred = infer::map(
            feature,
            &mut ctx,
            report,
            &mut history,
            &params,
            last_builder == Some(position),
            check,
        );
        let mapped = inferred
            .unwrap_or_else(|| map_feature(feature, &mut ctx, report, &mut history.features));
        // A cut or intersection with no body before it: SolidCraft has nothing to cut and
        // refuses the feature when it replays.
        let mapped = mapped.and_then(|mapped| match &mapped {
            Mapped::Feature(built) if !builds && cuts_a_body(built) => Err(
                "it cuts or intersects a body, and no earlier feature in the history builds one"
                    .to_owned(),
            ),
            _ => Ok(mapped),
        });
        let mut imported_body = false;
        let mapped = mapped.or_else(|reason| {
            // Measured inference above has already had its chance. Only a source input
            // proven absent can replace a refused recipe with its retained result.
            if !base::missing_input(feature, &ctx, source_losses) {
                return Err(reason);
            }
            ctx.builds = builds;
            ctx.named_bodies.truncate(named_bodies);
            ctx.extra.clear();
            ctx.blends.truncate(blends);
            history.features.truncate(before);
            let imported = base::fallback(feature, &mut ctx).map_err(|why| {
                format!(
                    "{reason}; the source states no sketch for its profile (it is absent from this design), and {why}"
                )
            })?;
            ctx.profile_unverified = false;
            ctx.profile_mismatch = false;
            ctx.inferred = None;
            ctx.measured = false;
            report.push_coded(
                Severity::Degraded,
                describe(feature),
                "history/body-fallback-imported",
                format!(
                    "Stored result imported because its required profile or path is absent from this design ({reason}); later features continue on this body"
                ),
            );
            imported_body = true;
            Ok(Mapped::Feature(imported))
        });
        match mapped {
            Ok(Mapped::Nothing) => {
                history.mapped += 1;
                history.not_needed += 1;
                out.push(outcome(FeatureStatus::NotNeeded, None));
            }
            Ok(Mapped::Consumed) => {
                history.mapped += 1;
                history.not_needed += 1;
                out.push(outcome(
                    FeatureStatus::NotNeeded,
                    Some("mapped with its plate".to_owned()),
                ));
            }
            Ok(Mapped::Feature(mut mapped)) => {
                if let Some(fields) = mapped.as_object_mut() {
                    if fields.contains_key(extrude::BASE_FACE_RESULT) {
                        fields.insert(
                            extrude::BASE_FACE_RESULT.to_owned(),
                            Value::Bool(last_builder == Some(position)),
                        );
                    }
                }
                history.mapped += 1;
                let mut extras = Vec::new();
                for mut extra in std::mem::take(&mut ctx.extra) {
                    let name = ctx.unique_name(&extra, history.features.iter());
                    if let Some(fields) = extra.as_object_mut() {
                        fields.insert("name".to_owned(), Value::String(name));
                    }
                    history.features.push(extra);
                    extras.push(history.features.len().saturating_sub(1));
                }
                if mapped.get("type").and_then(Value::as_str) == Some("sketch") {
                    references.features.push(mapped.clone());
                    let source =
                        if modern_spatial && operation_kind(feature) == Some("spatial_sketch") {
                            operation(feature)
                                .get("sketch")
                                .and_then(Value::as_str)
                                .and_then(|id| ctx.row("spatial_sketches", id))
                        } else {
                            operation(feature)
                                .pointer("/sketch/sketch")
                                .and_then(Value::as_str)
                                .and_then(|id| ctx.row("sketches", id))
                        };
                    if let (Some(source), Some(id)) =
                        (source, mapped.get("id").and_then(Value::as_u64))
                    {
                        if source.get("visible").and_then(Value::as_bool) == Some(false) {
                            history.hidden.push(id);
                        }
                    }
                }
                let name = ctx.unique_name(&mapped, history.features.iter());
                if let Some(fields) = mapped.as_object_mut() {
                    fields.insert("name".to_owned(), Value::String(name.clone()));
                }
                if let Some(id) = feature.get("id").and_then(Value::as_str) {
                    ctx.feature_names.insert(id.to_owned(), name);
                }
                history.features.push(mapped);
                let mut kept = outcome(FeatureStatus::Mapped, None);
                kept.output.ids = history
                    .features
                    .iter()
                    .skip(before)
                    .filter_map(|feature| feature.get("id").and_then(Value::as_u64))
                    .collect();
                kept.profile_unverified = std::mem::take(&mut ctx.profile_unverified);
                kept.final_body = std::mem::take(&mut ctx.final_body);
                kept.inferred = ctx.inferred.take();
                kept.reapplied = feature
                    .get("id")
                    .and_then(Value::as_str)
                    .is_some_and(|id| ctx.reapplied.contains(id));
                out.push(kept);
                let main = history.features.len().saturating_sub(1);
                for pending in ctx.blends.iter_mut().skip(blends) {
                    pending.feature = pending
                        .extra
                        .and_then(|index| extras.get(index).copied())
                        .unwrap_or(main);
                    pending.outcome = out.len().saturating_sub(1);
                    pending.subject = describe(feature);
                }
            }
            Err(reason) => {
                ctx.builds = builds;
                ctx.profile_unverified = false;
                ctx.final_body = false;
                ctx.inferred = None;
                ctx.measured = false;
                if std::mem::take(&mut ctx.profile_mismatch) {
                    report.push_coded(
                        Severity::Degraded,
                        describe(feature),
                        "history/profile-region-mismatch",
                        &reason,
                    );
                }
                ctx.extra.clear();
                history.features.truncate(before);
                ctx.blends.truncate(blends);
                blocked.get_or_insert_with(|| format!("{}: {reason}", describe(feature)));
                out.push(outcome(FeatureStatus::Blocked, Some(reason)));
            }
        }
        let measured = std::mem::take(&mut ctx.measured);
        for emitted in history.features.iter().skip(before) {
            if let Some(id) = emitted.get("id").and_then(Value::as_u64) {
                if measured {
                    history.measured.insert(id);
                }
                history.sources.insert(id, out.len().saturating_sub(1));
                if let Some(centers) = out
                    .last()
                    .and_then(|outcome| compared_state(outcome, feature))
                {
                    history.stored_bodies.insert(id, centers);
                }
                if !imported_body && whole_of_several(feature, &ctx) {
                    history.unconfirmed.insert(id);
                }
            }
        }
    }
    history.growth = growth;
    history.reapplied = out
        .iter()
        .filter(|f| f.status == FeatureStatus::Mapped && f.reapplied)
        .count();
    // Some decoders supply sketches without timeline rows. Their resolved frames are
    // independent of replayed topology; unresolved attachments are refused by sketch::map.
    for sketch in ctx.arena("sketches") {
        if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            return Err("planning time budget reached".to_owned());
        }
        let Some(id) = sketch.get("id").and_then(Value::as_str) else {
            continue;
        };
        if !keep_sketches || bound.contains(id) || ctx.sketches.contains_key(id) {
            continue;
        }
        let feature = serde_json::json!({"name": sketch.get("name")});
        let definition = serde_json::json!({"sketch": {"kind": "planar", "sketch": id}});
        match sketch::map(&feature, &definition, &mut ctx, report) {
            Ok(mut mapped) => {
                let name = ctx.unique_name(&mapped, history.features.iter());
                if let Some(fields) = mapped.as_object_mut() {
                    fields.insert("name".to_owned(), Value::String(name));
                }
                references.features.push(mapped.clone());
                if sketch.get("visible").and_then(Value::as_bool) == Some(false) {
                    if let Some(id) = mapped.get("id").and_then(Value::as_u64) {
                        history.hidden.push(id);
                    }
                }
                history.features.push(mapped);
            }
            Err(why) => report.push(
                Severity::Degraded,
                describe(&feature),
                format!("sketch left out: {why}"),
            ),
        }
    }
    params::unsigned_dimensions(references, &ctx);
    references.next_id = ctx.next_id;
    // A complete sketch-only design has no source bodies to rebuild. Other
    // histories still need a body builder, including any stopped sketch prefix.
    let complete_sketches = keep_sketches
        && crate::sketches_only(model)
        && blocked.is_none()
        && !history.features.is_empty();
    if !ctx.builds && !complete_sketches {
        return Err(blocked.unwrap_or_else(|| {
            if no_timeline {
                undecoded_history(source_losses)
                    .unwrap_or_else(|| "the design carries no feature history".to_owned())
            } else {
                "no feature in the history builds geometry".to_owned()
            }
        }));
    }
    history.stop = blocked;
    history.params = params;
    params::unsigned_dimensions(&mut history, &ctx);
    let mut appearance_report = Report::default();
    history.face_looks = faces::looks(&mut ctx, &mut appearance_report);
    // A partial prefix becomes a hidden editable copy. Its unnamed final
    // bodies are mapped by the visible original's Import instead.
    if history.stop.is_none() {
        report.append(appearance_report);
    }
    history.hidden_bodies = hidden_bodies(&ctx);
    history.looks = std::mem::take(&mut ctx.looks);
    history.default_bodies = std::mem::take(&mut ctx.default_bodies);
    history.sheet_rules = ctx
        .sheet_rules
        .iter()
        .map(sheet::SheetRule::value)
        .collect();
    history.next_id = ctx.next_id;
    history.pattern_replay_required = ctx.pattern_replay_required;
    history.topological_replay_required = ctx.topological_replay_required;
    history.blends = std::mem::take(&mut ctx.blends);
    Ok(history)
}

/// The design's own sketches, each mapped as a SolidCraft sketch, for a design whose sketches
/// no feature history carries (an assembly's): every planar sketch that maps, with the
/// parameters their dimensions use, and how many sketches did not map.
pub(crate) fn sketches(model: &Model, report: &mut Report) -> (History, usize) {
    sketches_where(model, &OnceCell::new(), report, &|_| true)
}

/// The arenas the reference sketches read, in the wire form of the model. Built the first
/// time it is asked for, so a conversion that maps several groups of sketches builds it once.
pub(crate) type SketchWire = OnceCell<Option<Value>>;

/// [`sketches`] for the sketches whose ids `wanted` accepts; the count left out is of those.
/// `wire` is shared between calls on the same model.
pub(crate) fn sketches_where(
    model: &Model,
    wire: &SketchWire,
    report: &mut Report,
    wanted: &dyn Fn(&str) -> bool,
) -> (History, usize) {
    let mut kept = History::default();
    let count = model
        .ir
        .model
        .sketches
        .iter()
        .filter(|sketch| wanted(sketch.id.as_str()))
        .count();
    if count == 0 {
        return (kept, 0);
    }
    if model.ir.check_scope().is_err() || !IR_VERSIONS.contains(&model.ir.ir_version()) {
        return (kept, count);
    }
    // Reference sketches read only the neutral arenas in `wire::SKETCH_ARENAS`. Keep those
    // expanded once, without expanding native records or the rest of the model.
    let Some(wire) = wire
        .get_or_init(|| wire::only_arenas(&model.ir.model, wire::SKETCH_ARENAS).ok())
        .as_ref()
    else {
        return (kept, count);
    };
    let mut param_notes = Report::default();
    let (params, param_names) = params::map(model, &mut param_notes);
    let mut ctx = Context::new(wire);
    ctx.reading_only = Some(wire::SKETCH_ARENAS);
    ctx.params = param_names;
    let mut left_out = 0;
    for sketch in ctx.arena("sketches") {
        if !sketch.get("id").and_then(Value::as_str).is_some_and(wanted) {
            continue;
        }
        let feature = serde_json::json!({ "name": sketch.get("name") });
        let definition = serde_json::json!({
            "sketch": { "kind": "planar", "sketch": sketch.get("id") },
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
pub(crate) type Replayed = Result<Vec<Result<crate::verify::Replay, String>>, String>;

/// The existing one-source/many-features naming convention, shared by mapping and proving.
fn numbered_name(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let mut name = base.to_owned();
    let mut n = 2;
    while taken(&name) {
        name = format!("{base} ({n})");
        n += 1;
    }
    name
}

/// Replace a blend with its independent per-body operations. Keep its original ID last:
/// rollback markers and the source's complete post-feature comparison then include every
/// operation. Added IDs share the source outcome, so replay failures shorten it as a unit.
fn split_blend(
    history: &mut History,
    at: usize,
    source: usize,
    selections: Vec<blend::Selection>,
) -> Result<(), String> {
    let template = history
        .features
        .get(at)
        .cloned()
        .ok_or("the mapped blend is missing")?;
    let base = template
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Blend");
    let last = selections.len().saturating_sub(1);
    let mut features: Vec<Value> = Vec::new();
    for (index, selection) in selections.into_iter().enumerate() {
        let mut feature = template.clone();
        let name = numbered_name(base, |name| {
            history
                .features
                .iter()
                .enumerate()
                .any(|(i, f)| i != at && f.get("name").and_then(Value::as_str) == Some(name))
                || features
                    .iter()
                    .any(|f| f.get("name").and_then(Value::as_str) == Some(name))
        });
        if let Some(fields) = feature.as_object_mut() {
            fields.insert("body".to_owned(), json!(selection.body));
            fields.insert("edges".to_owned(), json!(selection.points));
            fields.insert("name".to_owned(), json!(name));
            if index != last {
                let id = history.next_id;
                history.next_id = history.next_id.saturating_add(1);
                fields.insert("id".to_owned(), json!(id));
                history.sources.insert(id, source);
            }
        }
        features.push(feature);
    }
    if last > 0 {
        let names: Vec<Value> = features
            .iter()
            .filter_map(|f| f.get("name").cloned())
            .collect();
        for target in history.features.iter_mut().skip(at.saturating_add(1)) {
            if !matches!(
                target.get("type").and_then(Value::as_str),
                Some("pattern" | "mirror")
            ) {
                continue;
            }
            let Some(seeds) = target.get_mut("features").and_then(Value::as_array_mut) else {
                continue;
            };
            let matches = seeds
                .iter()
                .filter(|seed| seed.as_str() == Some(base))
                .count();
            if matches == 0 {
                continue;
            }
            if seeds.len().saturating_add(matches.saturating_mul(last)) > pattern::MAX_SEEDS {
                return Err("patterns with more than 10000 seeds are not supported yet".to_owned());
            }
            *seeds = seeds
                .iter()
                .flat_map(|seed| {
                    if seed.as_str() == Some(base) {
                        names.clone()
                    } else {
                        vec![seed.clone()]
                    }
                })
                .collect();
        }
    }
    history.features.splice(at..at.saturating_add(1), features);
    Ok(())
}

/// Settles which face each of an unequal chamfer's per-body features (`at` and the `added` ones
/// after it) takes first, on SolidCraft's own cut: as written when the replay after it has
/// edges on its body where `cuts` put them, else with `flip`. Bodies are disjoint, so each
/// feature is settled on its own. `Ok(true)` when any had to be flipped.
fn orient_unequal(
    history: &mut History,
    at: usize,
    added: usize,
    cuts: &[blend::Cut],
    replay: &mut impl FnMut(&History, &[u64], bool) -> Replayed,
) -> Result<bool, String> {
    let mut flipped = false;
    for index in at..=at.saturating_add(added) {
        let feature = history
            .features
            .get(index)
            .ok_or("the mapped chamfer is missing")?;
        let id = feature
            .get("id")
            .and_then(Value::as_u64)
            .ok_or("the mapped chamfer is missing")?;
        let body = feature
            .get("body")
            .and_then(Value::as_str)
            .ok_or("the mapped chamfer names no body")?
            .to_owned();
        let mut failed = None;
        let mut settled = false;
        for flip in [false, true] {
            if flip {
                if let Some(fields) = history
                    .features
                    .get_mut(index)
                    .and_then(Value::as_object_mut)
                {
                    fields.insert("flip".to_owned(), Value::Bool(true));
                }
            }
            match replay(history, &[id], false)?.first() {
                Some(Ok(after)) if blend::cut_as(cuts, &body, &after.bodies) => {
                    flipped |= flip;
                    settled = true;
                    break;
                }
                Some(Err(why)) => failed = Some(why.clone()),
                _ => {}
            }
        }
        if !settled {
            return Err(match failed {
                Some(why) => format!("SolidCraft cannot cut it either way round: {why}"),
                None => {
                    "SolidCraft cuts it on other faces than the source either way round".to_owned()
                }
            });
        }
    }
    Ok(flipped)
}

/// What proving a pending feature settled.
enum Proven {
    /// The blend is proven and emitted, with `added` additional per-body operations; `flipped`
    /// when an unequal chamfer had to take SolidCraft's other face first.
    Bodies { added: usize, flipped: bool },
    /// A sweep path bound to the unique earlier body carrying all its edges.
    Path,
    /// A face profile's sketch rewritten on the other side of its plane, if it had to be.
    Sketch(Option<Value>),
    /// A hole axis signed to enter the proven face.
    Hole(sketch::V3),
}

/// Proves the input edges of every blend of a history that maps, on the bodies `replay`
/// gives for the history before each blend (`replay(history, after)` with the ids of the
/// features each blend follows), and names its body. A blend that cannot be proven blocks
/// the history with the reason; without a replay (`replay` returning `Err`) none can be.
pub(crate) fn prove_blends(
    mapping: &mut Mapping,
    mut replay: impl FnMut(&History, &[u64], bool) -> Replayed,
    mut measure: impl FnMut(&History, std::time::Duration) -> infer::Fit,
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
    let mut replayed = if steps.is_empty() {
        Ok(Vec::new())
    } else {
        let faces = history
            .blends
            .iter()
            .any(|pending| pending.face.is_some() || pending.needs_faces());
        replay(history, &steps, faces)
    };
    let blends = std::mem::take(&mut history.blends);
    let mut step = 0usize;
    let mut first = None;
    let mut inserted = 0usize;
    for (index, (pending, marker)) in blends.iter().zip(&after).enumerate() {
        let proven = match (marker, &replayed) {
            (None, _) => Err(if pending.path {
                "no feature before it builds a body carrying its path".to_owned()
            } else {
                "no feature before it builds a body to blend".to_owned()
            }),
            (Some(_), Err(why)) => Err(format!(
                "its input edges are found only on SolidCraft's replay of the history before \
                 it, and there is none: {why}"
            )),
            (Some(_), Ok(bodies)) => {
                let at = bodies.get(step);
                step = step.saturating_add(1);
                match at {
                    Some(Ok(before)) => match &pending.face {
                        None => blend::prove(pending, &before.bodies, &before.faces).and_then(
                            |selections| {
                                if pending.path {
                                    let [selection] = selections.as_slice() else {
                                        return Err("its input edge path crosses several rebuilt bodies, which is not supported yet".to_owned());
                                    };
                                    let fields = history.features
                                        .get_mut(pending.feature.saturating_add(inserted))
                                        .and_then(Value::as_object_mut)
                                        .ok_or("the mapped sweep is missing")?;
                                    fields.insert("path_body".to_owned(), json!(selection.body));
                                    fields.insert("path_edges".to_owned(), json!(selection.points));
                                    return Ok(Proven::Path);
                                }
                                let cuts = pending
                                    .unequal
                                    .as_ref()
                                    .map(|unequal| {
                                        blend::cuts(
                                            unequal,
                                            &pending.edges,
                                            &before.bodies,
                                            &before.faces,
                                        )
                                    })
                                    .transpose()?;
                                let added = selections.len().saturating_sub(1);
                                let at = pending.feature.saturating_add(inserted);
                                split_blend(history, at, pending.outcome, selections)?;
                                let flipped = match cuts {
                                    Some(cuts) => {
                                        orient_unequal(history, at, added, &cuts, &mut replay)?
                                    }
                                    None => false,
                                };
                                Ok(Proven::Bodies { added, flipped })
                            },
                        ),
                        Some(face) => face_profile::prove(face, before).and_then(|mirrored| {
                            if let Some(direction) = face.entry_direction(mirrored)? {
                                Ok(Proven::Hole(direction))
                            } else if mirrored {
                                face.sketch(true)
                                    .map(|(sketch, _)| Proven::Sketch(Some(sketch)))
                            } else {
                                Ok(Proven::Sketch(None))
                            }
                        }),
                    },
                    Some(Err(why)) => Err(format!(
                        "SolidCraft cannot replay the history before it: {why}"
                    )),
                    None => {
                        Err("SolidCraft's replay of the history before it is missing".to_owned())
                    }
                }
            }
        };
        // A last blend whose edges the proof cannot pin down: measure the nearby rebuilt edges.
        let proven = match proven {
            Err(reason) if pending.face.is_none() && reason.starts_with(blend::UNPROVEN) => {
                let at = pending.feature.saturating_add(inserted);
                let bodies = replayed
                    .as_ref()
                    .ok()
                    .and_then(|steps| steps.get(step.saturating_sub(1)))
                    .and_then(|replay| replay.as_ref().ok())
                    .map(|replay| &replay.bodies);
                match bodies.and_then(|bodies| {
                    infer::edges(history, at, pending.outcome, pending, bodies, &mut measure)
                }) {
                    Some(Ok((selections, inferred))) => {
                        let added = selections.len().saturating_sub(1);
                        split_blend(history, at, pending.outcome, selections).map(|()| {
                            for id in history
                                .features
                                .iter()
                                .skip(at)
                                .take(added.saturating_add(1))
                                .filter_map(|feature| feature.get("id").and_then(Value::as_u64))
                            {
                                history.measured.insert(id);
                            }
                            if let Some(outcome) = mapping.features.get_mut(pending.outcome) {
                                outcome.inferred = Some(inferred);
                            }
                            Proven::Bodies {
                                added,
                                flipped: false,
                            }
                        })
                    }
                    Some(Err(why)) => Err(format!("{reason}; {why}")),
                    None => Err(reason),
                }
            }
            proven => proven,
        };
        let admitted = proven.is_ok();
        match proven {
            Ok(proven @ (Proven::Bodies { .. } | Proven::Path)) => {
                let (added, changed) = match proven {
                    Proven::Bodies { added, flipped } => (added, flipped),
                    _ => (0, true),
                };
                inserted = inserted.saturating_add(added);
                if (added > 0 || changed) && index.saturating_add(1) < blends.len() {
                    // Later blends must see all per-body operations, including earlier
                    // radius groups, as written. Their original last IDs remain rollback markers.
                    let remaining: Vec<u64> = after
                        .iter()
                        .skip(index.saturating_add(1))
                        .flatten()
                        .copied()
                        .collect();
                    let faces = blends
                        .iter()
                        .skip(index.saturating_add(1))
                        .any(|p| p.face.is_some() || p.needs_faces());
                    replayed = replay(history, &remaining, faces);
                    step = 0;
                }
            }
            // The face's sketch on the other side, which runs along the face's outward normal.
            Ok(Proven::Sketch(mirrored)) => {
                let id = pending.face.as_ref().map(|face| face.sketch);
                let target = history
                    .features
                    .iter_mut()
                    .find(|feature| feature.get("id").and_then(Value::as_u64) == id);
                if let (Some(mirrored), Some(target)) = (mirrored, target) {
                    for key in ["plane", "sketch"] {
                        if let (Some(value), Some(fields)) =
                            (mirrored.get(key), target.as_object_mut())
                        {
                            fields.insert(key.to_owned(), value.clone());
                        }
                    }
                    if index.saturating_add(1) < blends.len() {
                        // The extrude now runs the other way, so later proofs need a replay
                        // of the history as it will be written, not the one before the flip.
                        let remaining: Vec<u64> = after
                            .iter()
                            .skip(index.saturating_add(1))
                            .flatten()
                            .copied()
                            .collect();
                        let faces = blends
                            .iter()
                            .skip(index.saturating_add(1))
                            .any(|pending| pending.face.is_some() || pending.needs_faces());
                        replayed = replay(history, &remaining, faces);
                        step = 0;
                    }
                }
            }
            Ok(Proven::Hole(direction)) => {
                let target = history
                    .features
                    .get_mut(pending.feature.saturating_add(inserted));
                let value = serde_json::json!(direction);
                if let Some(fields) = target.and_then(Value::as_object_mut) {
                    let changed = fields.get("direction") != Some(&value);
                    fields.insert("direction".to_owned(), value);
                    if changed && index.saturating_add(1) < blends.len() {
                        let remaining: Vec<u64> = after
                            .iter()
                            .skip(index.saturating_add(1))
                            .flatten()
                            .copied()
                            .collect();
                        let faces = blends
                            .iter()
                            .skip(index.saturating_add(1))
                            .any(|p| p.face.is_some() || p.needs_faces());
                        replayed = replay(history, &remaining, faces);
                        step = 0;
                    }
                }
            }
            Err(reason) => {
                if let Some(outcome) = mapping.features.get_mut(pending.outcome) {
                    outcome.status = FeatureStatus::Blocked;
                    outcome.reason = Some(reason.clone());
                }
                first.get_or_insert_with(|| {
                    (pending.outcome, format!("{}: {reason}", pending.subject))
                });
            }
        }
        let at = pending.feature.saturating_add(inserted);
        let result_proof = history
            .features
            .get_mut(at)
            .and_then(Value::as_object_mut)
            .and_then(|fields| fields.remove(extrude::BASE_FACE_RESULT))
            .and_then(|value| value.as_bool());
        if admitted && result_proof.is_some() {
            // Matching the selected face on the imported base proves its boundary and
            // outward side, but not that the base and the feature share the source's
            // result frame. Never guess a translation or choose the opposite end face.
            let final_state = result_proof == Some(true) && history.stop.is_none();
            let fit = if final_state {
                measure(history, infer::MEASURE_BUDGET)
            } else {
                infer::Fit::Unknown
            };
            let refusal = match fit {
                infer::Fit::Matches(measure)
                    if measure.deviation.is_some_and(|deviation| deviation.within()) => None,
                infer::Fit::Matches(measure) if measure.deviation.is_some() => {
                    measure.deviation.map(|deviation| format!(
                        "a face extrude on an imported base is not supported yet without a matching source result check: surfaces differ by {:.6} mm",
                        deviation.max
                    ))
                }
                infer::Fit::Differs(why) => Some(format!(
                    "a face extrude on an imported base is not supported yet without a matching source result check: {why}"
                )),
                infer::Fit::Unknown | infer::Fit::Matches(_) => Some(
                    "a face extrude on an imported base is not supported yet without a matching source result check".to_owned()
                ),
            };
            if let Some(reason) = refusal {
                if let Some(outcome) = mapping.features.get_mut(pending.outcome) {
                    outcome.status = FeatureStatus::Blocked;
                    outcome.reason = Some(reason.clone());
                }
                first.get_or_insert_with(|| {
                    (pending.outcome, format!("{}: {reason}", pending.subject))
                });
            }
        }
    }
    if let Some((source, reason)) = first {
        shorten(mapping, source, reason);
    }
}

/// Maps, just before `feature`, each sketch it uses that the source keeps without a timeline
/// row (`bound` holds those that have one) and that is not mapped yet, so the feature finds it.
/// A sketch that does not map is left for the feature to refuse with its own reason.
fn map_unbound_sketches(
    feature: &Value,
    bound: &std::collections::BTreeSet<String>,
    ctx: &mut Context,
    report: &mut Report,
    history: &mut History,
    references: &mut History,
) {
    let mut used = Vec::new();
    collect_strings(operation(feature), &mut used);
    for id in used {
        if bound.contains(&id) || ctx.sketches.contains_key(&id) {
            continue;
        }
        let Some(sketch) = ctx.row("sketches", &id) else {
            continue;
        };
        let source = serde_json::json!({"name": sketch.get("name")});
        let definition = serde_json::json!({"sketch": {"kind": "planar", "sketch": id}});
        let Ok(mut mapped) = sketch::map(&source, &definition, ctx, report) else {
            continue;
        };
        let name = ctx.unique_name(&mapped, history.features.iter());
        if let Some(fields) = mapped.as_object_mut() {
            fields.insert("name".to_owned(), Value::String(name));
        }
        if sketch.get("visible").and_then(Value::as_bool) == Some(false) {
            if let Some(id) = mapped.get("id").and_then(Value::as_u64) {
                history.hidden.push(id);
            }
        }
        references.features.push(mapped.clone());
        history.features.push(mapped);
    }
}

/// Every string in `value`, depth first.
fn collect_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(text) => out.push(text.clone()),
        Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        Value::Object(fields) => fields.values().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}

/// Keeps the prefix before a failed source feature, plus independently placed sketches.
pub(crate) fn shorten(mapping: &mut Mapping, source: usize, reason: String) {
    let first_blocker = mapping
        .features
        .iter()
        .position(|f| f.status == FeatureStatus::Blocked);
    let Ok(history) = &mut mapping.result else {
        return;
    };
    history.features.retain(|feature| {
        feature.get("type").and_then(Value::as_str) == Some("sketch")
            || feature
                .get("id")
                .and_then(Value::as_u64)
                .and_then(|id| history.sources.get(&id))
                .is_some_and(|row| *row < source)
    });
    // Only the 0.44 writer emits wire_constraints. Keep its independent/repaired spatial
    // sketches as reference rows, just like planar sketches, when body history stops.
    let spatial: BTreeSet<usize> = history
        .features
        .iter()
        .filter(|feature| feature.pointer("/sketch/wire_constraints").is_some())
        .filter_map(|feature| {
            feature
                .get("id")
                .and_then(Value::as_u64)
                .and_then(|id| history.sources.get(&id))
                .copied()
        })
        .collect();
    for (index, feature) in mapping.features.iter_mut().enumerate().skip(source) {
        let sketch = feature.kind == "sketch"
            || (feature.kind == "spatial_sketch" && spatial.contains(&index));
        if feature.status == FeatureStatus::Mapped && !sketch {
            feature.status = FeatureStatus::Blocked;
            feature.reason = Some(reason.clone());
        }
    }
    history.mapped = mapping
        .features
        .iter()
        .filter(|feature| {
            matches!(
                feature.status,
                FeatureStatus::Mapped | FeatureStatus::NotNeeded
            )
        })
        .count();
    history.not_needed = mapping
        .features
        .iter()
        .filter(|feature| feature.status == FeatureStatus::NotNeeded)
        .count();
    history.reapplied = mapping
        .features
        .iter()
        .filter(|feature| feature.status == FeatureStatus::Mapped && feature.reapplied)
        .count();
    if first_blocker.is_none_or(|first| source <= first) {
        history.stop = Some(reason.clone());
    }
    history.blends.clear();
    if history
        .features
        .iter()
        .all(|f| f.get("type").and_then(Value::as_str) == Some("sketch"))
    {
        mapping.result = Err(reason);
    }
    discard_reference_sketches(mapping);
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

/// Only body-building features can be omitted for an unchanged stored transition.
fn unchanged_geometry(feature: &Value) -> bool {
    if feature
        .pointer("/source_properties/transition_geometry")
        .and_then(Value::as_str)
        != Some("unchanged")
    {
        return false;
    }
    let definition = operation(feature);
    let kind = kind(feature);
    !(matches!(
        kind.as_str(),
        "sketch"
            | "spatial_sketch"
            | "tree_node"
            | "insert_component"
            | "component_transfer"
            | "assembly_joint"
            | "reference_image"
            | "cosmetic_thread"
    ) || kind.starts_with("datum")
        || helpers::is_helper(feature)
        || (kind == "base_feature" && base::holds_none(definition)))
}

/// One source feature as a SolidCraft feature, or why it cannot be.
fn map_feature(
    feature: &Value,
    ctx: &mut Context,
    report: &mut Report,
    preceding: &mut Vec<Value>,
) -> Result<Mapped, String> {
    let definition = operation(feature);
    if matches!(
        operation_kind(feature),
        Some("datum_principal_plane" | "datum_plane" | "datum_offset_plane")
    ) {
        return datum::map(feature, ctx).map(|()| Mapped::Nothing);
    }
    if is_assembly_feature(feature) {
        return Ok(Mapped::Nothing);
    }
    if helpers::is_helper(feature) {
        return helpers::map(feature, ctx, report);
    }
    match operation_kind(feature) {
        // A folder, a PartDesign Body or another node that orders the tree and builds nothing.
        Some("tree_node") => Ok(Mapped::Nothing),
        // A raster image placed for reference (a Fusion Canvas): it builds no geometry.
        Some("reference_image") => Ok(Mapped::Nothing),
        // A corner the source proves leaves its body as it was (topology included) needs no
        // SolidCraft feature: a corner of one flange, say, with nothing to relieve.
        Some("sheet_metal_corner")
            if definition.get("effect").and_then(Value::as_str) == Some("unchanged") =>
        {
            Ok(Mapped::Nothing)
        }
        Some("sketch") => sketch::map(feature, definition, ctx, report).map(Mapped::Feature),
        Some("spatial_sketch") => {
            spatial::map(feature, definition, ctx, report).map(Mapped::Feature)
        }
        Some("extrude") => extrude::map(feature, definition, ctx, preceding).map(Mapped::Feature),
        Some("revolve") => revolve::map(feature, definition, ctx).map(Mapped::Feature),
        Some("loft") => loft::map(feature, definition, ctx, preceding).map(Mapped::Feature),
        Some("sweep") => sweep::map(feature, definition, ctx).map(Mapped::Feature),
        Some("sphere") => primitive::sphere(feature, definition, ctx).map(Mapped::Feature),
        Some("torus") => primitive::torus(feature, definition, ctx).map(Mapped::Feature),
        Some("coil") => primitive::coil(feature, definition, ctx).map(Mapped::Feature),
        Some("helical_sweep") => helix::map(feature, definition, ctx, report).map(Mapped::Feature),
        Some("primitive") => primitive::placed(feature, definition, ctx).map(Mapped::Feature),
        Some("block") => primitive::block(feature, definition, ctx).map(Mapped::Feature),
        Some("shell") => shell::map(feature, definition, ctx).map(Mapped::Feature),
        // Draft still needs the geometry of its own input faces.
        Some("draft") => Err("draft waits on input face geometry".to_owned()),
        Some("combine") => modify::combine(feature, definition, ctx).map(Mapped::Feature),
        Some("split_body") => modify::split(feature, definition, ctx).map(Mapped::Feature),
        Some("delete_body") => modify::remove(feature, definition, ctx).map(Mapped::Feature),
        Some("mirror_shape") => Err(
            "replacing a source shape with its mirror is not supported yet; SolidCraft mirrors keep the original body".to_owned(),
        ),
        Some("unresolved")
            if matches!(
                definition.get("family").and_then(Value::as_str),
                Some("thread" | "detailed_thread")
            ) =>
        {
            // The decoder found the thread but admitted none of its record layouts, so the
            // design carries no designation, nominal size, pitch, faces or modeled flag:
            // nothing says whether it is a cosmetic annotation or a modeled groove.
            Err("threads whose construction is unresolved are not supported yet: the source's thread record was not decoded, so its designation, size, faces and whether it is cosmetic or modeled are unknown".to_owned())
        }
        Some("cosmetic_thread") => thread::map(feature, definition, ctx, report).map(Mapped::Feature),
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
        Some("move_face") => move_face::map(feature, definition, ctx).map(Mapped::Feature),
        Some("move_body") => move_body::map(feature, definition, ctx, preceding).map(Mapped::Feature),
        Some("delete_face") => delete_face::map(feature, definition, ctx).map(Mapped::Feature),
        Some("hole") => hole::map(feature, definition, ctx, report).map(Mapped::Feature),
        Some("sheet_metal_base_flange") => {
            sheet::base_flange(feature, definition, ctx).map(Mapped::Feature)
        }
        Some("sheet_metal_contour_flange") => {
            sheet::contour_flange(feature, definition, ctx).map(Mapped::Feature)
        }
        Some("sheet_metal_edge_flange") => {
            sheet::edge_flange(feature, definition, ctx, preceding).map(Mapped::Feature)
        }
        Some("sheet_metal_hem") => Err(sheet::hem_refusal(definition)),
        Some("sheet_metal_attached_plate") => {
            sheet::attached_plate(feature, definition, ctx, preceding).map(Mapped::Feature)
        }
        Some("sheet_metal_bend")
            if feature
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| ctx.consumed_bends.contains(id)) =>
        {
            Ok(Mapped::Consumed)
        }
        Some("sheet_metal_bend") => Err(sheet::bend_refusal()),
        Some("stored_geometry") if feature.pointer("/source_properties/design_reference").is_some() => {
            reference::map(feature, feature.pointer("/source_properties/design_reference").unwrap_or(&Value::Null), ctx, report).map(Mapped::Feature)
        }
        Some("stored_geometry") => base::stored(feature, ctx, report).map(Mapped::Feature),
        Some("base_feature") if base::holds_none(definition) => Ok(Mapped::Nothing),
        Some("base_feature") => base::map(feature, definition, ctx).map(Mapped::Feature),
        Some("insert_bodies") => base::insert(feature, definition, ctx).map(Mapped::Feature),
        _ => Err(format!("{} is not mapped yet", kind(feature))),
    }
}

/// Assembly timeline rows build no part geometry; occurrences and joints carry them. A
/// component transfer copies an occurrence or moves it to another owner: the occurrence
/// tree places the result, whether or not cadmpeg resolves which occurrence it was.
fn is_assembly_feature(feature: &Value) -> bool {
    match operation_kind(feature) {
        Some("insert_component" | "component_transfer" | "assembly_joint") => true,
        // cadmpeg retains unresolved As-built joints as native operations with a stable
        // source tag. Display names may be edited and must never identify an operation.
        Some("native") => matches!(
            feature.get("source_tag").and_then(Value::as_str),
            Some("As-built" | "As-built Joint")
        ),
        _ => false,
    }
}

/// The tagged operation enum, looking through the definition wrapper.
fn operation_enum(feature: &Value) -> &Value {
    let definition = feature.get("definition").unwrap_or(&Value::Null);
    match definition.get("kind").and_then(Value::as_str) {
        Some("post_process") => definition
            .pointer("/value/operation")
            .unwrap_or(&Value::Null),
        Some("operation") => definition.get("value").unwrap_or(&Value::Null),
        _ => &Value::Null,
    }
}

/// The payload of the feature's operation on the CADIR v11 wire.
fn operation(feature: &Value) -> &Value {
    operation_enum(feature).get("value").unwrap_or(&Value::Null)
}

fn operation_kind(feature: &Value) -> Option<&str> {
    operation_enum(feature).get("kind").and_then(Value::as_str)
}

/// The CADIR operation name of a feature (`extrude`, `fillet`, …).
fn kind(feature: &Value) -> String {
    if operation_kind(feature) == Some("stored_geometry")
        && feature
            .pointer("/source_properties/design_reference")
            .is_some()
    {
        return "design_reference".into();
    }

    operation_kind(feature).unwrap_or("unknown").to_owned()
}

/// The message for an input selection of a form that is not mapped, naming that form:
/// `noun` is `face`, `edge` or `body`, `kind` the CADIR selection kind that was found.
pub(crate) fn unsupported_selection(noun: &str, kind: Option<&str>) -> String {
    match kind {
        Some("historical") => {
            format!("a {noun} from an earlier history state is not supported yet")
        }
        Some("historical_set") => {
            format!("{noun}s from an earlier history state are not supported yet")
        }
        Some("historical_partial") => format!(
            "a {noun} selection from an earlier history state with some {noun}s unresolved is \
             not supported yet"
        ),
        Some("native" | "native_set") => format!(
            "a native {noun} reference is not supported yet: cadmpeg does not resolve it to a \
             model {noun}"
        ),
        Some("unresolved") => format!(
            "an unresolved {noun} reference is not supported yet: cadmpeg did not resolve it \
             to a model {noun}"
        ),
        Some("generated") => {
            format!("a {noun} generated by an earlier feature is not supported yet")
        }
        Some("local") => {
            format!("a {noun} local to an earlier feature is not supported yet")
        }
        Some("faces" | "edges" | "bodies") => {
            format!("a {noun} selection in this form is not supported yet")
        }
        Some(other) => format!("a {noun} selection of kind {other} is not supported yet"),
        None => format!("a {noun} selection with no kind is not supported yet"),
    }
}

/// A short description of a feature's form, from its extent, start, hole form or group
/// count. It holds no names and no sizes.
pub(crate) fn variant(feature: &Value) -> Option<String> {
    let op = operation(feature);
    let kind = kind(feature);
    let text = |pointer: &str| op.pointer(pointer).and_then(Value::as_str);
    let end = |termination: Option<&str>| match termination {
        Some("blind") => "blind",
        Some("through_all") => "through all",
        Some("to_face") => "to a face",
        Some("to_plane") => "to a plane",
        Some("to_vertex") => "to a vertex",
        Some("to_first") => "to the first face",
        Some("to_last") => "to the last face",
        Some("to_shape") => "to a shape",
        _ => "other end",
    };
    let mut parts = Vec::new();
    match kind.as_str() {
        "extrude" => {
            parts.push(match text("/extent/kind") {
                Some("symmetric") => "symmetric".to_owned(),
                Some("two_sided") => format!(
                    "two sides, {} and {}",
                    end(text("/extent/first/termination/kind")),
                    end(text("/extent/second/termination/kind"))
                ),
                _ => end(text("/extent/side/termination/kind")).to_owned(),
            });
            match text("/start/kind") {
                Some("from_face") => parts.push("start from a face".to_owned()),
                Some("from_plane") => parts.push("start from a plane".to_owned()),
                Some("offset_profile_plane") => parts.push("offset start".to_owned()),
                _ => {}
            }
        }
        "hole" => {
            let form = op
                .pointer("/shape/construction/value/kind/kind")
                .and_then(Value::as_str)
                .unwrap_or("other");
            parts.push(form.replace('_', " "));
            parts.push(end(text("/extent/kind")).to_owned());
        }
        "fillet" | "chamfer" => {
            let groups = op
                .get("groups")
                .and_then(Value::as_array)
                .map_or(0, Vec::len);
            if groups > 0 {
                parts.push(format!(
                    "{groups} {} group{}",
                    if kind == "fillet" {
                        "radius"
                    } else {
                        "distance"
                    },
                    if groups == 1 { "" } else { "s" }
                ));
            }
        }
        _ => return None,
    }
    Some(format!("{kind}, {}", parts.join(", ")))
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

    #[test]
    fn a_stopped_prefix_is_unreachable_only_when_nothing_after_it_builds() {
        let mut history = History {
            features: vec![
                json!({"id": 1, "type": "sketch"}),
                json!({"id": 2, "type": "extrude"}),
            ],
            sources: [(1, 0), (2, 1)].into(),
            ..History::default()
        };
        history.growth = vec![
            Growth::Inert,
            Growth::Builds,
            Growth::Rounds,
            Growth::Rounds,
        ];
        assert_eq!(history.unreachable_prefix(), Some(false));
        history.growth = vec![
            Growth::Inert,
            Growth::Builds,
            Growth::Removes,
            Growth::Inert,
        ];
        assert_eq!(history.unreachable_prefix(), Some(true));
        history.growth = vec![
            Growth::Inert,
            Growth::Builds,
            Growth::Rounds,
            Growth::Builds,
        ];
        assert_eq!(history.unreachable_prefix(), None);
        // Nothing follows the last retained feature: the whole history has its own check.
        history.growth = vec![Growth::Inert, Growth::Builds, Growth::Inert];
        assert_eq!(history.unreachable_prefix(), None);
    }

    #[test]
    fn source_features_are_classed_by_what_they_can_do_to_the_bodies_before_them() {
        let with_op = |kind: &str, op: &str| json!({"definition": {"kind": "operation", "value": {"kind": kind, "value": {"op": op}}}});
        assert_eq!(growth_of(&with_op("fillet", "")), Growth::Rounds);
        assert_eq!(growth_of(&with_op("extrude", "cut")), Growth::Removes);
        assert_eq!(growth_of(&with_op("extrude", "join")), Growth::Builds);
        assert_eq!(growth_of(&with_op("revolve", "intersect")), Growth::Removes);
        assert_eq!(growth_of(&with_op("sweep", "cut")), Growth::Builds);
        assert_eq!(growth_of(&with_op("sketch", "")), Growth::Inert);
        let mut suppressed = with_op("extrude", "join");
        suppressed["suppressed"] = json!(true);
        assert_eq!(growth_of(&suppressed), Growth::Inert);
    }

    #[test]
    fn split_blends_keep_source_state_and_expand_later_feature_seeds() {
        let mut history = History {
            features: vec![
                json!({"id": 1, "name": "Block", "type": "extrude"}),
                json!({"id": 2, "name": "Treatment", "type": "fillet", "radius": "1 mm"}),
                json!({"id": 3, "name": "Treatment (2)", "type": "pattern", "features": ["Block", "Treatment"]}),
            ],
            next_id: 4,
            sources: BTreeMap::from([(1, 0), (2, 1), (3, 2)]),
            stored_bodies: BTreeMap::from([(2, Vec::new())]),
            ..History::default()
        };
        let original = history.clone();
        split_blend(
            &mut history,
            1,
            1,
            vec![
                blend::Selection {
                    body: "First".to_owned(),
                    points: vec![[0.0; 3]],
                },
                blend::Selection {
                    body: "Second".to_owned(),
                    points: vec![[10.0; 3]],
                },
            ],
        )
        .unwrap();
        assert_eq!(
            history.features[3]["features"],
            json!(["Block", "Treatment", "Treatment (3)"])
        );
        assert_eq!(history.features[1]["id"], 4);
        assert_eq!(history.features[1]["name"], "Treatment");
        assert_eq!(history.features[2]["id"], 2);
        assert_eq!(history.features[2]["name"], "Treatment (3)");
        assert_eq!(history.features[1]["radius"], history.features[2]["radius"]);
        assert_eq!(history.sources.get(&4), Some(&1));
        assert_eq!(history.sources.get(&2), Some(&1));
        let mut rows: Vec<FeatureOutcome> = (0..3)
            .map(|_| FeatureOutcome {
                output: Default::default(),
                name: None,
                kind: "fillet".to_owned(),
                status: FeatureStatus::Mapped,
                reason: None,
                ordinal: None,
                variant: None,
                profile_unverified: false,
                reapplied: false,
                final_body: false,
                inferred: None,
            })
            .collect();
        crate::output_links::history(&mut rows, &history, 10);
        assert_eq!(rows[1].output.ids, [14, 12]);
        assert_eq!(rows[1].output.method, dutchman_core::OutputMatch::Emitted);

        assert_eq!(
            history.stored_bodies.keys().copied().collect::<Vec<_>>(),
            [2]
        );
        assert_eq!(history.next_id, 5);
        let mut over_limit = original;
        over_limit.features[2]["features"] = json!(vec!["Treatment"; pattern::MAX_SEEDS]);
        let selections = ["First", "Second"]
            .into_iter()
            .map(|body| blend::Selection {
                body: body.to_owned(),
                points: vec![[0.0; 3]],
            })
            .collect();
        assert!(split_blend(&mut over_limit, 1, 1, selections)
            .unwrap_err()
            .contains("10000 seeds"));
    }

    /// The reference sketches map the same from the arenas they read as from the whole
    /// model, and read no other arena (a debug build panics on one).
    #[test]
    fn sketches_read_the_same_from_the_arenas_they_need() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../dutchman-core/tests/fixtures");
        let mut mapped = 0;
        for name in [
            "design_sketch_constraints.f3d",
            "design_copy_paste.f3d",
            "generated_unit_cube.f3d",
            "container_metadata_only.f3d",
        ] {
            let Ok(model) = dutchman_core::read(
                dutchman_core::Format::FusionArchive,
                &root.join(name),
                &mut Report::default(),
            ) else {
                continue;
            };
            let whole = SketchWire::new();
            whole
                .set(Some(serde_json::to_value(&model.ir.model).unwrap()))
                .unwrap();
            let subset = SketchWire::new();
            let (mut a_notes, mut b_notes) = (Report::default(), Report::default());
            let a = sketches_where(&model, &whole, &mut a_notes, &|_| true);
            let b = sketches_where(&model, &subset, &mut b_notes, &|_| true);
            let keys = |wire: &SketchWire| -> Vec<String> {
                wire.get()
                    .and_then(Option::as_ref)
                    .and_then(Value::as_object)
                    .map(|rows| rows.keys().cloned().collect())
                    .unwrap_or_default()
            };
            if let Some(held) = subset.get().and_then(Option::as_ref) {
                assert!(keys(&subset)
                    .iter()
                    .all(|k| wire::SKETCH_ARENAS.contains(&k.as_str())));
                let full = whole.get().and_then(Option::as_ref).unwrap();
                for arena in wire::SKETCH_ARENAS {
                    assert_eq!(held.get(arena), full.get(arena), "{name}: {arena}");
                }
            }
            assert_eq!(a.1, b.1, "{name}");
            assert_eq!(a.0.features, b.0.features, "{name}");
            assert_eq!(a.0.params, b.0.params, "{name}");
            assert_eq!(a.0.next_id, b.0.next_id, "{name}");
            assert_eq!(format!("{a_notes:?}"), format!("{b_notes:?}"), "{name}");
            mapped += a.0.features.len();
        }
        assert!(mapped > 0, "no fixture sketch mapped");
    }

    #[test]
    fn unsupported_selections_name_their_form() {
        assert_eq!(
            unsupported_selection("face", Some("historical")),
            "a face from an earlier history state is not supported yet"
        );
        assert!(unsupported_selection("edge", Some("native")).contains("a native edge reference"));
        assert!(unsupported_selection("face", Some("unresolved"))
            .starts_with("an unresolved face reference"));
        assert!(unsupported_selection("face", Some("generated"))
            .starts_with("a face generated by an earlier feature"));
        assert!(
            unsupported_selection("body", Some("local")).contains("local to an earlier feature")
        );
        assert!(unsupported_selection("edge", Some("weird")).contains("of kind weird"));
        assert!(unsupported_selection("edge", None).contains("with no kind"));
    }

    #[test]
    fn variants_describe_the_form_without_sizes() {
        let extrude = json!({"definition": {"kind":"operation", "value": {"kind": "extrude", "value": {"start": {"kind": "from_face"}, "extent": {"kind": "one_sided", "side": {"termination": {"kind": "to_plane"}}}}}}});
        assert_eq!(
            variant(&extrude).as_deref(),
            Some("extrude, to a plane, start from a face")
        );
        let hole = json!({"definition": {"kind":"operation", "value": {"kind": "hole", "value": {"shape": {"construction": {"kind": "form", "value": {"kind": {"kind": "counterbore"}}}, "diameter": 5.0}, "extent": {"kind": "through_all"}}}}});
        assert_eq!(
            variant(&hole).as_deref(),
            Some("hole, counterbore, through all")
        );
        let fillet = json!({"definition": {"kind":"operation", "value": {"kind": "fillet", "value": {"groups": [{"radius": 1.0}, {"radius": 2.0}]}}}});
        assert_eq!(variant(&fillet).as_deref(), Some("fillet, 2 radius groups"));
        assert_eq!(
            variant(
                &json!({"definition": {"kind":"operation", "value": {"kind": "native", "value": {"parameters": {}}}}})
            ),
            None
        );
    }

    #[test]
    fn row_lookups_reuse_one_borrowed_index_per_arena() {
        let faces: Vec<_> = (0..16_384)
            .map(|i| json!({"id": format!("synthetic:face:{i}"), "ordinal": i}))
            .collect();
        let model = json!({"faces": faces, "points": [{"id": "point"}]});
        let ctx = Context::testing(&model);
        assert!(ctx.rows.values().all(|index| index.get().is_none()));
        for row in ctx.arena("faces").iter().rev() {
            let id = row["id"].as_str().unwrap();
            assert!(std::ptr::eq(ctx.row("faces", id).unwrap(), row));
        }
        let index = ctx.rows["faces"].get().unwrap();
        assert_eq!(index.len(), 16_384);
        // Repeated misses also reuse the index; unrelated arenas stay unindexed.
        for _ in 0..16_384 {
            assert!(ctx.row("faces", "missing").is_none());
        }
        assert!(std::ptr::eq(ctx.rows["faces"].get().unwrap(), index));
        assert!(ctx.rows["points"].get().is_none());
        assert!(ctx.row("unknown", "point").is_none());
    }

    #[test]
    fn row_index_keeps_first_duplicate_and_arena_scope() {
        let model = json!({
            "faces": [null, {"id": 7}, {"id": "same", "value": 1},
                      {"id": "same", "value": 2}],
            "points": [{"id": "same", "value": 3}],
            "metadata": {"id": "same"}
        });
        let ctx = Context::testing(&model);
        assert_eq!(ctx.row("faces", "same").unwrap()["value"], 1);
        assert_eq!(ctx.row("points", "same").unwrap()["value"], 3);
        assert!(ctx.row("metadata", "same").is_none());
        assert!(ctx.row("faces", "7").is_none());
        assert!(Context::testing(&Value::Null)
            .row("faces", "same")
            .is_none());
    }

    fn datum(key: u32, ordinal: u64) -> Value {
        feature(
            key,
            ordinal,
            "XY Plane",
            json!({"kind": "datum_principal_plane", "value": {"plane": "top"}}),
        )
    }

    fn opaque(key: u32, ordinal: u64, name: &str) -> Value {
        feature(
            key,
            ordinal,
            name,
            json!({"kind": "native", "value": {"kind": "Mystery", "parameters": {}}}),
        )
    }

    #[test]
    fn spatial_dimension_parameters_follow_assembly_renaming() {
        let mut history = History {
            params: vec![json!({"name":"rise", "expr":"30 mm", "unit":"mm"})],
            features: vec![
                json!({"id":1, "type":"sketch", "sketch": {"wire_constraints": [{"id":"k1", "type":"distance", "a":{"wire":"w1","index":0}, "b":{"wire":"w1","index":1}, "param":"rise", "value":30.0}]}}),
            ],
            ..History::default()
        };
        let mut taken = ["rise".to_owned()].into_iter().collect();
        history.relocate(10, 2, "Part", &mut taken);
        assert_eq!(history.params[0]["name"], "Part_rise");
        assert_eq!(
            history.features[0]["sketch"]["wire_constraints"][0]["param"],
            "Part_rise"
        );
        assert_eq!(history.features[0]["id"], 11);
    }

    #[test]
    fn hole_location_sketch_follows_relocated_history() {
        let mut history = History {
            features: vec![
                json!({"id":3,"type":"sketch"}),
                json!({"id":4,"type":"hole","points":{"sketch":3,"ids":["p1"]}}),
            ],
            ..History::default()
        };
        history.relocate(10, 2, "Part", &mut Default::default());
        assert_eq!(history.features[0]["id"], 13);
        assert_eq!(history.features[1]["points"]["sketch"], 13);
        assert_eq!(history.features[1]["points"]["ids"], json!(["p1"]));
    }

    #[test]
    fn body_renaming_updates_modify_operands() {
        let mut history = History {
            features: vec![
                json!({"body_names": ["Part"]}),
                json!({"target": "Part", "tools": ["Part"]}),
                json!({"bodies": ["Part"]}),
                json!({"body": "Part"}),
                json!({"targets": ["Part", "Other"]}),
            ],
            ..History::default()
        };
        let mut taken = ["Part".to_owned()].into_iter().collect();
        history.rename_bodies("Second", &mut taken);
        assert_eq!(history.features[0]["body_names"], json!(["Part (Second)"]));
        assert_eq!(history.features[1]["target"], "Part (Second)");
        assert_eq!(history.features[1]["tools"], json!(["Part (Second)"]));
        assert_eq!(history.features[2]["bodies"], json!(["Part (Second)"]));
        assert_eq!(history.features[3]["body"], "Part (Second)");
        assert_eq!(
            history.features[4]["targets"],
            json!(["Part (Second)", "Other"])
        );
    }

    #[test]
    fn default_import_names_follow_shape_order_without_reserving_cadir_names() {
        let mut taken: BodyNames = ["Block".to_owned()].into_iter().collect();
        let mut history = History {
            features: vec![json!({"body_names": ["Block", "Block (2)"]})],
            default_bodies: BTreeMap::from([
                ("Block".to_owned(), "Block".to_owned()),
                ("Block (2)".to_owned(), "Block".to_owned()),
            ]),
            looks: BTreeMap::from([("Block".to_owned(), json!({"color": [255, 0, 0]}))]),
            face_looks: vec![json!({"body": "Block (2)", "color": [0, 0, 255]})],
            ..History::default()
        };
        history.rename_bodies("Component", &mut taken);
        assert!(history.features[0].get("body_names").is_none());
        assert_eq!(
            history.default_bodies.keys().cloned().collect::<Vec<_>>(),
            ["Block (2)", "Block (3)"]
        );
        assert_eq!(history.looks["Block (2)"]["color"], json!([255, 0, 0]));
        assert_eq!(history.face_looks[0]["body"], "Block (3)");
        assert_eq!(taken.explicit, BTreeSet::from(["Block".to_owned()]));
        let mut next = History {
            features: vec![json!({"body_names": ["Block (2)"]})],
            ..History::default()
        };
        next.rename_bodies("Later", &mut taken);
        assert_eq!(next.features[0]["body_names"], json!(["Block (2)"]));
    }

    #[test]
    fn known_cadir_versions_are_read_and_others_refused() {
        assert!(check_version(&json!({"ir_version": "6"})).is_ok());
        for version in ["7", "8", "9", "10", "11"] {
            assert!(check_version(&json!({"ir_version": version})).is_ok());
        }
        assert_eq!(
            check_version(&json!({"ir_version": "12"})).unwrap_err(),
            "unsupported CADIR version \"12\" (Dutchman reads 6, 7, 8, 9, 10, 11)"
        );
        assert!(check_version(&json!({})).is_err());
    }

    #[test]
    fn sketches_nothing_is_built_on_or_the_source_hides_start_hidden() {
        let history = History {
            features: vec![
                json!({"id": 1, "type": "sketch"}),
                json!({"id": 2, "type": "sketch"}),
                json!({"id": 3, "type": "sketch"}),
                json!({"id": 4, "type": "sketch"}),
                json!({"id": 5, "type": "extrude", "sketch": 1}),
                json!({"id": 6, "type": "loft", "sections": [{"sketch": 3}]}),
            ],
            hidden: vec![3],
            ..History::default()
        };
        // 1 is extruded and shown; 2 is built on by nothing; 3 is lofted but hidden in the
        // source; 4 is built on by nothing.
        assert_eq!(history.hidden_sketches(), [json!(2), json!(3), json!(4)]);
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

    fn no_history_reason(losses: &[(&str, &str)]) -> String {
        let mut notes = Report::default();
        for (code, detail) in losses {
            notes.push_coded(Severity::Degraded, *code, code, *detail);
        }
        map_until(
            &cube(),
            &mut Report::default(),
            crate::SolidCraftVersion::default(),
            false,
            notes.losses(),
            None,
        )
        .result
        .unwrap_err()
    }

    #[test]
    fn a_timeline_cadmpeg_could_not_decode_is_named_instead_of_no_history() {
        let reason = no_history_reason(&[
            ("f3d/geometry.nurbs-surface-carrier", "8 spline surfaces were decoded."),
            (
                "f3d/design.intent-undecoded",
                "Design intent was dropped because its decoding failed, so the dimension-companion and configuration reports were not produced: F3D sheet-metal settings frame or typed link is invalid",
            ),
        ]);
        assert_eq!(
            reason,
            "the design's feature timeline is in the file, but cadmpeg could not decode it (sheet-metal settings frame or typed link is invalid)"
        );
        let reason = no_history_reason(&[(
            "f3d/feature.timeline-undecoded",
            "the timeline record is damaged",
        )]);
        assert!(
            reason.contains("cadmpeg could not decode it (the timeline record is damaged)"),
            "{reason}"
        );
    }

    #[test]
    fn a_design_with_no_timeline_and_no_decoder_failure_keeps_the_plain_reason() {
        let reason = no_history_reason(&[(
            "f3d/geometry.nurbs-surface-carrier",
            "8 spline surfaces were decoded.",
        )]);
        assert_eq!(reason, "the design carries no feature history");
        assert_eq!(
            no_history_reason(&[]),
            "the design carries no feature history"
        );
    }

    #[test]
    fn transition_geometry_preserves_non_body_features() {
        for kind in [
            "sketch",
            "spatial_sketch",
            "datum_principal_plane",
            "datum_plane",
            "datum_offset_plane",
            "datum_axis",
            "datum_point",
            "datum_coordinate_system",
            "binder",
            "tree_node",
            "insert_component",
            "component_transfer",
            "assembly_joint",
            "reference_image",
            "cosmetic_thread",
        ] {
            let feature = json!({"source_properties": {"transition_geometry": "unchanged"}, "definition": {"kind":"operation", "value": {"kind": kind, "value": {}}}});
            assert!(!unchanged_geometry(&feature), "{kind}");
        }
        let empty_base = json!({"source_properties": {"transition_geometry": "unchanged"}, "definition": {"kind":"operation", "value": {"kind": "base_feature", "value": {"bodies": {"kind": "resolved", "value": {"bodies": []}}}}}});
        assert!(!unchanged_geometry(&empty_base));
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

    /// A v11 feature row whose operation is `kind` with payload `value`.
    fn op_row(
        id: &str,
        ordinal: u64,
        name: &str,
        kind: &str,
        value: Value,
        outputs: Value,
    ) -> Value {
        json!({"id": id, "ordinal": ordinal, "name": name, "suppressed": false,
            "definition": {"kind": "operation", "value": {"kind": kind, "value": value}},
            "dependencies": [], "source_properties": {}, "source_content": [], "outputs": outputs})
    }

    #[test]
    fn a_body_the_final_model_omits_is_named_mirrored_and_removed() {
        let model = json!({"features": [], "feature_result_topologies": [
            {"id": "r", "output_of": "f:extrude", "members": [{"kind": "body", "id": "1"}]}]});
        let mut ctx = Context::testing(&model);
        let extrude = json!({"id": "f:extrude", "name": "Extrude 1", "outputs": []});
        assert_eq!(
            extrude::body_names(&extrude, &mut ctx, "new_body"),
            Some(vec!["Body 1".to_owned()])
        );
        let generated = json!({"kind": "generated", "value": {
            "bodies": [{"feature": "f:extrude", "local_id": "1"}], "native": "n"}});
        let mut features = Vec::new();
        let mirror = op_row(
            "f:mirror",
            3,
            "Mirror 1",
            "pattern",
            json!({"seeds": [{"kind": "bodies", "value": generated}],
                "pattern": {"kind": "mirror", "plane_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
                    "plane_normal": {"x": 1.0, "y": 0.0, "z": 0.0}}}),
            json!(["copy"]),
        );
        let Ok(Mapped::Feature(mapped)) =
            map_feature(&mirror, &mut ctx, &mut Report::default(), &mut features)
        else {
            panic!("the mirror maps");
        };
        assert_eq!(mapped["bodies"], json!(["Body 1"]));
        assert_eq!(mapped["combine"], json!(false));
        let remove = op_row(
            "f:remove",
            4,
            "Remove 1",
            "delete_body",
            json!({"bodies": generated, "mode": "delete_selected"}),
            json!([]),
        );
        let Ok(Mapped::Feature(mapped)) =
            map_feature(&remove, &mut ctx, &mut Report::default(), &mut features)
        else {
            panic!("the removal maps");
        };
        assert_eq!(mapped["type"], "remove");
        assert_eq!(mapped["bodies"], json!(["Body 1"]));
        assert_eq!(
            map_feature(&remove, &mut ctx, &mut Report::default(), &mut features).err(),
            Some("its generated body has no earlier mapped producer".to_owned())
        );
        let keep = op_row(
            "f:keep",
            5,
            "Keep 1",
            "delete_body",
            json!({"bodies": generated, "mode": "keep_selected"}),
            json!([]),
        );
        assert_eq!(
            map_feature(&keep, &mut ctx, &mut Report::default(), &mut features).err(),
            Some("removing every body except the selected ones is not supported yet".to_owned())
        );
        ctx.target_version = "0.30.0".parse().unwrap();
        assert_eq!(
            map_feature(&remove, &mut ctx, &mut Report::default(), &mut features).err(),
            Some(
                "removing a body is written for SolidCraft 0.40 or newer (target 0.30)".to_owned()
            )
        );
    }

    #[test]
    fn a_generated_body_name_is_not_given_to_a_later_body() {
        let model = json!({"features": [], "bodies": [{"id": "b", "name": null}],
            "feature_result_topologies": [
                {"id": "r", "output_of": "f:first", "members": [{"kind": "body", "id": "1"}]}]});
        let mut ctx = Context::testing(&model);
        let first = json!({"id": "f:first", "outputs": []});
        let later = json!({"id": "f:later", "outputs": ["b"]});
        assert_eq!(
            extrude::body_names(&first, &mut ctx, "new_body"),
            Some(vec!["Body 1".to_owned()])
        );
        assert_eq!(
            extrude::body_names(&later, &mut ctx, "new_body"),
            Some(vec!["Body 2".to_owned()])
        );
    }

    #[test]
    fn a_mirror_of_the_one_body_in_its_input_state_joins_when_it_outputs_that_body() {
        let state = |id: &str, of: &str, bodies: Value| {
            json!({"id": id, "input_of": of, "bodies": bodies, "faces": [], "edges": [],
                "vertices": [], "edge_geometry": []})
        };
        let model = json!({"features": [], "bodies": [{"id": "c", "name": null}],
            "feature_input_topologies": [
            state("one", "f:mirror", json!(["one:1"])),
            state("two", "f:two", json!(["two:1", "two:2"])),
            state("other", "f:else", json!(["other:1"]))]});
        let mut ctx = Context::testing(&model);
        ctx.named_bodies.push(("b".into(), "Body 1".into()));
        let seed = |state: &str, body: &str| {
            json!([{"kind": "bodies", "value": {"kind": "historical",
                "value": {"state": state, "bodies": [body], "native": "n"}}}])
        };
        let plane = json!({"kind": "mirror", "plane_origin": {"x": 0.0, "y": 0.0, "z": 0.0},
            "plane_normal": {"x": 0.0, "y": -1.0, "z": 0.0}});
        let mirror = |id: &str, seeds: Value, outputs: Value| {
            op_row(
                id,
                2,
                "Mirror 1",
                "pattern",
                json!({"seeds": seeds, "pattern": plane.clone()}),
                outputs,
            )
        };
        let mut features = Vec::new();
        let mut run = |row: &Value, ctx: &mut Context| match map_feature(
            row,
            ctx,
            &mut Report::default(),
            &mut features,
        ) {
            Ok(Mapped::Feature(mapped)) => Ok(mapped),
            Ok(_) => Err("built nothing".to_owned()),
            Err(reason) => Err(reason),
        };
        let joined = run(
            &mirror("f:mirror", seed("one", "one:1"), json!(["b"])),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(joined["bodies"], json!(["Body 1"]));
        assert_eq!(joined["combine"], json!(true));
        let copied = run(
            &mirror("f:mirror", seed("one", "one:1"), json!(["c"])),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(copied["combine"], json!(false));
        assert_eq!(copied["body_names"], json!(["Body 2"]));
        let removed = modify::remove(
            &json!({"id": "f:remove"}),
            &json!({"mode": "delete_selected", "bodies": {"kind": "bodies", "value": ["c"]}}),
            &mut ctx,
        )
        .unwrap();
        assert_eq!(removed["bodies"], copied["body_names"]);
        assert_eq!(
            run(
                &mirror("f:two", seed("two", "two:1"), json!(["b"])),
                &mut ctx
            )
            .unwrap_err(),
            "a body from an earlier history state is not supported yet when that state holds \
             several bodies"
        );
        assert_eq!(
            run(
                &mirror("f:mirror", seed("other", "other:1"), json!(["b"])),
                &mut ctx
            )
            .unwrap_err(),
            "its input body state belongs to another feature"
        );
        ctx.named_bodies.push(("d".into(), "Body 2".into()));
        assert_eq!(
            run(
                &mirror("f:mirror", seed("one", "one:1"), json!(["b"])),
                &mut ctx
            )
            .unwrap_err(),
            "its one input body is not the one body an earlier feature named"
        );
    }

    #[test]
    fn a_reference_image_builds_nothing() {
        let model = json!({"features": []});
        let mut ctx = Context::testing(&model);
        let canvas = json!({"id": "f3d:model:feature#7", "ordinal": 7, "name": "Canvas 1", "definition": {"kind": "operation", "value": {"kind": "reference_image", "value": {"asset": "f3d:model:asset#1"}}}, "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []});
        let mut features = Vec::new();
        let mapped = map_feature(&canvas, &mut ctx, &mut Report::default(), &mut features);
        assert!(matches!(mapped, Ok(Mapped::Nothing)));
        assert!(features.is_empty());
    }

    #[test]
    fn tree_nodes_build_nothing_and_block_nothing() {
        let body = feature(
            2,
            1,
            "Body",
            json!({"kind": "tree_node", "value": {"role": "history", "children": {"children": []}}}),
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
            json!({"kind": "insert_component", "value": {"occurrence": "f3d:model:occurrence#1"}}),
        );
        let joint = feature(
            3,
            2,
            "Joint 1",
            json!({"kind": "assembly_joint", "value": {"joint": "f3d:model:joint#1"}}),
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
    fn component_copies_and_moves_in_the_timeline_are_not_needed() {
        let copy = feature(
            2,
            1,
            "CopyPaste 1",
            json!({"kind": "component_transfer", "value": {"mode": "copy", "occurrence": "f3d:model:occurrence#1"}}),
        );
        // An unresolved source still builds no part geometry.
        let moved = feature(
            3,
            2,
            "CutPaste 1",
            json!({"kind": "component_transfer", "value": {"mode": "reparent"}}),
        );
        let model = with(cube(), json!([datum(1, 0), copy, moved]), json!([]));
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
            json!({"kind": "datum_plane", "value": {"frame": frame}}),
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

    /// A stored-geometry feature imports exactly the bodies it outputs, solids and sheets.
    #[allow(clippy::unwrap_used, clippy::indexing_slicing)]
    fn stored_outputs(outputs: Value) -> Result<Value, String> {
        let mut source = cube();
        source.ir = cadmpeg_ir::examples::unit_cube().unwrap();
        source.fidelity = None;
        let mut wire = serde_json::to_value(&source.ir).unwrap();
        let template = wire["geometry"].clone();
        for (prefix, label) in [("skin", "Skin"), ("other", "Other")] {
            let text = serde_json::to_string(&template)
                .unwrap()
                .replace("synthetic:cube:", &format!("synthetic:{prefix}:"));
            let mut copy: Value = serde_json::from_str(&text).unwrap();
            copy["bodies"][0]["name"] = json!(label);
            copy["bodies"][0]["kind"] = json!("sheet");
            for arena in [
                "points", "vertices", "curves", "edges", "coedges", "loops", "surfaces", "faces",
                "shells", "regions", "bodies",
            ] {
                wire["geometry"][arena]
                    .as_array_mut()
                    .unwrap()
                    .extend(copy[arena].as_array().unwrap().iter().cloned());
            }
        }
        wire["geometry"]["bodies"][0]["name"] = json!("Solid");
        source.ir = cadmpeg_ir::CadIr::from_json(&wire.to_string()).unwrap();
        let model = &wire["model"];
        let mut ctx = Context::new(model).with_geometry(&wire["geometry"]);
        ctx.source = Some(&source);
        let outputs: Value = serde_json::from_str(
            &outputs
                .to_string()
                .replace("b#1", "synthetic:cube:body#0")
                .replace("b#2", "synthetic:skin:body#0")
                .replace("b#3", "synthetic:other:body#0"),
        )
        .unwrap();
        let stored = json!({"id": "f#1", "ordinal": 0, "name": "Composite1", "suppressed": false, "outputs": outputs, "definition": {"kind": "operation", "value": {"kind": "stored_geometry", "value": {}}}, "dependencies": [], "source_properties": {}, "source_content": []});
        match map_feature(&stored, &mut ctx, &mut Report::default(), &mut Vec::new())? {
            Mapped::Feature(mapped) => Ok(mapped),
            Mapped::Nothing | Mapped::Consumed => Err("mapped to nothing".to_owned()),
        }
    }

    #[test]
    fn stored_geometry_imports_its_solid_and_sheet_outputs_only() {
        let mapped = stored_outputs(json!(["b#1", "b#2"])).unwrap();
        assert_eq!(mapped["type"], "import");
        assert_eq!(mapped["name"], "Composite1");
        assert_eq!(
            mapped["_bodies"],
            json!(["synthetic:cube:body#0", "synthetic:skin:body#0"])
        );
        assert_eq!(mapped["body_names"], json!(["Solid", "Skin"]));
        let sheets = stored_outputs(json!(["b#2", "b#3"])).unwrap();
        assert_eq!(
            sheets["_bodies"],
            json!(["synthetic:skin:body#0", "synthetic:other:body#0"])
        );
        assert_eq!(sheets["body_names"], json!(["Skin", "Other"]));
    }

    #[test]
    fn stored_geometry_without_outputs_is_reported_not_guessed() {
        assert_eq!(stored_outputs(json!([])).unwrap_err(), "it holds no bodies");
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

    #[test]
    fn a_whole_sketch_of_several_profiles_waits_for_confirmation() {
        let model = json!({"sketches": [
            {"id": "two", "profiles": [[{"entity": "a", "reversed": false}], [{"entity": "b", "reversed": false}]]},
            {"id": "one", "profiles": [[{"entity": "a", "reversed": false}]]},
        ]});
        let ctx = Context::testing(&model);
        let whole = |sketch: &str| {
            feature(
                1,
                0,
                "Extrude",
                json!({"kind": "extrude", "value": {
                    "profile": {"kind": "planar", "value": {"kind": "sketch", "value": sketch}}
                }}),
            )
        };
        assert!(whole_of_several(&whole("two"), &ctx));
        assert!(!whole_of_several(&whole("one"), &ctx));
        let picked = feature(
            1,
            0,
            "Extrude",
            json!({"kind": "extrude", "value": {
                "profile": {"kind": "planar", "value": {"kind": "sketch_profiles",
                    "value": {"sketch": "two", "profiles": [0]}}}
            }}),
        );
        assert!(!whole_of_several(&picked, &ctx));

        let mut history = History {
            features: vec![
                json!({"id": 1, "name": "Sketch 1", "type": "sketch"}),
                json!({"id": 2, "name": "Extrude 1", "type": "extrude"}),
                json!({"id": 3, "name": "Extrude 4", "type": "extrude"}),
            ],
            sources: BTreeMap::from([(1, 0), (2, 1), (3, 2)]),
            ..History::default()
        };
        assert_eq!(history.first_unconfirmed(), None);
        history.unconfirmed.insert(3);
        let (source, why) = history.first_unconfirmed().unwrap();
        assert_eq!(source, 2);
        assert!(
            why.starts_with("feature \"Extrude 4\": its profile is the whole of a sketch"),
            "{why}"
        );
    }

    #[test]
    fn body_centers_with_incomplete_bodies_are_unknown() {
        let feature = json!({"source_properties": {
            "body_centers": "1:0,0,10",
            "body_centers_exact": "1",
            "body_centers_incomplete": "2;3",
        }});
        assert!(stored_body_centers(&feature).is_none());
    }

    #[test]
    fn body_centers_with_incomplete_evidence_are_unknown_even_with_all_centers() {
        let feature = json!({"source_properties": {
            "body_centers": "1:0,0,10;2:5,0,20",
            "body_centers_exact": "1,2",
            "body_centers_incomplete": "2",
        }});
        assert!(stored_body_centers(&feature).is_none());
    }

    #[test]
    fn body_centers_with_empty_incomplete_list_are_usable() {
        let feature = json!({"source_properties": {
            "body_centers": "1:0,0,10;2:5,0,20",
            "body_centers_exact": "1",
            "body_centers_incomplete": "",
        }});
        assert_eq!(
            stored_body_centers(&feature),
            Some(vec![
                crate::verify::StoredBody {
                    center: [0.0, 0.0, 10.0],
                    exact: true,
                },
                crate::verify::StoredBody {
                    center: [5.0, 0.0, 20.0],
                    exact: false,
                },
            ])
        );
    }

    #[test]
    fn body_centers_without_incomplete_property_are_usable() {
        let feature = json!({"source_properties": {
            "body_centers": "1:0,0,10;2:5,0,20",
            "body_centers_exact": "1",
        }});
        assert_eq!(
            stored_body_centers(&feature),
            Some(vec![
                crate::verify::StoredBody {
                    center: [0.0, 0.0, 10.0],
                    exact: true,
                },
                crate::verify::StoredBody {
                    center: [5.0, 0.0, 20.0],
                    exact: false,
                },
            ])
        );
    }

    #[test]
    fn a_base_feature_holding_its_final_body_has_no_stored_state_to_compare() {
        let feature = json!({"name": "Base Feature 1",
            "source_properties": {"body_centers": "1:0,0,10"}});
        let outcome = |final_body: bool| FeatureOutcome {
            output: Default::default(),
            name: Some("Base Feature 1".to_owned()),
            kind: "base_feature".to_owned(),
            status: FeatureStatus::Mapped,
            reason: None,
            ordinal: Some(0),
            variant: None,
            profile_unverified: false,
            reapplied: false,
            final_body,
            inferred: None,
        };
        assert_eq!(
            compared_state(&outcome(false), &feature).map(|c| c.len()),
            Some(1)
        );
        assert!(compared_state(&outcome(true), &feature).is_none());
    }

    /// Two per-body features of one unequal chamfer whose bodies need opposite flips: each is
    /// settled on its own body's cut.
    #[test]
    fn unequal_chamfers_settle_their_flip_per_body() {
        let mut history = History {
            features: vec![
                json!({"id": 5, "type": "chamfer", "body": "A"}),
                json!({"id": 6, "type": "chamfer", "body": "B"}),
            ],
            ..History::default()
        };
        let along = [1.0, 0.0, 0.0];
        let cuts = [
            blend::Cut::testing(
                "A",
                along,
                [[0.0, 1.0, 10.0], [0.0, 0.0, 7.0]],
                ([0.0, 0.0, 10.0], [10.0, 0.0, 10.0]),
            ),
            blend::Cut::testing(
                "B",
                along,
                [[20.0, 0.0, 9.0], [20.0, 3.0, 10.0]],
                ([20.0, 0.0, 10.0], [30.0, 0.0, 10.0]),
            ),
        ];
        let edge = |x0: f64, [_, y, z]: [f64; 3]| crate::verify::ReplayedEdge {
            start: [x0, y, z],
            end: [x0 + 10.0, y, z],
            mid: [x0 + 5.0, y, z],
            length: 10.0,
            name: String::new(),
            index: None,
            circle: None,
        };
        // A is cut as the source has it unflipped, B only flipped.
        let mut replays = 0;
        let mut replay = |history: &History, _: &[u64], _: bool| -> Replayed {
            replays += 1;
            let flipped = |id: u64| {
                history
                    .features
                    .iter()
                    .any(|f| f["id"] == id && f["flip"] == true)
            };
            let cut = |name: &str, x0: f64, points: [[f64; 3]; 2], right: bool| {
                let edges = if right {
                    points.iter().map(|&p| edge(x0, p)).collect()
                } else {
                    Vec::new()
                };
                (name.to_owned(), edges)
            };
            let bodies = vec![
                cut("A", 0.0, [[0.0, 1.0, 10.0], [0.0, 0.0, 7.0]], !flipped(5)),
                cut("B", 20.0, [[20.0, 0.0, 9.0], [20.0, 3.0, 10.0]], flipped(6)),
            ];
            Ok(vec![Ok(crate::verify::Replay {
                bodies,
                faces: Vec::new(),
            })])
        };
        assert_eq!(
            orient_unequal(&mut history, 0, 1, &cuts, &mut replay),
            Ok(true)
        );
        assert_eq!(replays, 3);
        assert!(history.features[0].get("flip").is_none());
        assert_eq!(history.features[1]["flip"], true);
    }
}
