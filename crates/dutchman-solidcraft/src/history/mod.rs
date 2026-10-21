//! The model's feature history, as SolidCraft parametric features.
//!
//! History is all or nothing: CADIR keeps only the final B-rep, so a history that stops part
//! way cannot be finished with an imported body. Either every active feature maps, or the
//! design is written as one import.
//!
//! The mapping reads the model in CADIR's documented JSON form (`docs/cad-ir.md` in cadmpeg),
//! the stable contract cadmpeg publishes, rather than its Rust accessors. It knows the wire
//! versions in [`IR_VERSIONS`] and refuses others rather than misread them.

mod constraints;
mod extrude;
mod faces;
mod loft;
mod params;
mod pattern;
mod profile;
mod revolve;
mod sketch;

use std::collections::BTreeMap;

use dutchman_core::{FeatureOutcome, FeatureStatus, Model, Report, Severity};
use serde_json::Value;

/// The CADIR wire versions the mapping reads. Version 7 adds only assembly joint kinds and a
/// sheet-metal thickness side, neither of which the mapping uses.
const IR_VERSIONS: [&str; 2] = ["6", "7"];

/// A replayable history: SolidCraft parameters and features in timeline order.
#[derive(Debug, Default)]
pub(crate) struct History {
    pub(crate) params: Vec<Value>,
    pub(crate) features: Vec<Value>,
    /// The looks of the bodies the history makes, by body name.
    pub(crate) looks: BTreeMap<String, Value>,
    /// Looks of faces of those bodies that differ from their body's, as SolidCraft face looks
    /// (a point on the face, in the frame the body is built in: the design's, or its
    /// component's inside an assembly, which SolidCraft applies to every occurrence).
    pub(crate) face_looks: Vec<Value>,
    /// The id the next feature after the history takes.
    pub(crate) next_id: u64,
    /// Source features that became SolidCraft features or needed none.
    pub(crate) mapped: usize,
    /// Active source features.
    pub(crate) total: usize,
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
            for key in ["id", "sketch"] {
                if let Some(id) = fields.get(key).and_then(Value::as_u64) {
                    fields.insert(key.to_owned(), Value::from(id.saturating_add(offset)));
                }
            }
            // A loft names the sketch of each section.
            for section in fields
                .get_mut("sections")
                .and_then(Value::as_array_mut)
                .into_iter()
                .flatten()
            {
                if let Some(sketch) = section.get("sketch").and_then(Value::as_u64) {
                    if let Some(section) = section.as_object_mut() {
                        section.insert(
                            "sketch".to_owned(),
                            Value::from(sketch.saturating_add(offset)),
                        );
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
            if let Some(names) = feature.get_mut("body_names").and_then(Value::as_array_mut) {
                for name in names {
                    if let Some(old) = name.as_str() {
                        *name = Value::String(rename(old));
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
    next_id: u64,
    /// SolidCraft parameter names by CADIR parameter id.
    params: BTreeMap<String, String>,
    /// Mapped sketches by CADIR sketch id.
    sketches: BTreeMap<String, sketch::Mapped>,
    /// SolidCraft looks of the bodies the history makes, by body name.
    looks: BTreeMap<String, Value>,
    /// The timeline name each mapped source feature took, by CADIR feature id.
    feature_names: BTreeMap<String, String>,
    /// The source bodies the history makes under their own names: (CADIR body id, name).
    named_bodies: Vec<(String, String)>,
    /// Whether a feature that builds geometry has been mapped.
    builds: bool,
}

impl<'a> Context<'a> {
    fn new(model: &'a Value) -> Self {
        Context {
            model,
            next_id: 1,
            params: BTreeMap::new(),
            sketches: BTreeMap::new(),
            looks: BTreeMap::new(),
            feature_names: BTreeMap::new(),
            named_bodies: Vec::new(),
            builds: false,
        }
    }

    /// The timeline name `feature` keeps: its own, numbered when an earlier feature has it,
    /// since SolidCraft refers to features by name.
    fn unique_name(&self, feature: &Value) -> String {
        let base = feature
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Feature")
            .to_owned();
        let taken = |name: &str| self.feature_names.values().any(|known| known == name);
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
}

/// Maps the model's history, or says why it cannot be replayed. Losses of a history that is
/// written go to `report`.
pub(crate) fn map(model: &Model, report: &mut Report) -> Mapping {
    let mut features_out = Vec::new();
    let result = map_into(model, report, &mut features_out);
    Mapping {
        result,
        features: features_out,
    }
}

fn map_into(
    model: &Model,
    report: &mut Report,
    out: &mut Vec<FeatureOutcome>,
) -> Result<History, String> {
    let document =
        serde_json::to_value(&model.ir).map_err(|err| format!("cannot read the model: {err}"))?;
    check_version(&document)?;
    let wire = document.get("model").cloned().unwrap_or(Value::Null);
    let (params, param_names) = params::map(&model.ir.model.parameters, report);
    let mut ctx = Context::new(&wire);
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
        match map_feature(feature, &mut ctx, report) {
            Ok(Mapped::Nothing) => {
                history.mapped += 1;
                out.push(outcome(FeatureStatus::NotNeeded, None));
            }
            Ok(Mapped::Feature(mut mapped)) => {
                history.mapped += 1;
                let name = ctx.unique_name(&mapped);
                if let Some(fields) = mapped.as_object_mut() {
                    fields.insert("name".to_owned(), Value::String(name.clone()));
                }
                if let Some(id) = feature.get("id").and_then(Value::as_str) {
                    ctx.feature_names.insert(id.to_owned(), name);
                }
                history.features.push(mapped);
                out.push(outcome(FeatureStatus::Mapped, None));
            }
            Err(reason) => {
                blocked.get_or_insert_with(|| format!("{}: {reason}", describe(feature)));
                out.push(outcome(FeatureStatus::Blocked, Some(reason)));
            }
        }
    }
    if let Some(reason) = blocked {
        return Err(reason);
    }
    if !ctx.builds {
        return Err("no feature in the history builds geometry".to_owned());
    }
    history.params = params;
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
    history.next_id = ctx.next_id;
    Ok(history)
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
fn map_feature(feature: &Value, ctx: &mut Context, report: &mut Report) -> Result<Mapped, String> {
    let definition = operation(feature);
    match definition.get("definition").and_then(Value::as_str) {
        Some("datum_principal_plane") => Ok(Mapped::Nothing),
        Some("sketch") => sketch::map(feature, definition, ctx, report).map(Mapped::Feature),
        Some("extrude") => extrude::map(feature, definition, ctx).map(Mapped::Feature),
        Some("revolve") => revolve::map(feature, definition, ctx).map(Mapped::Feature),
        Some("loft") => loft::map(feature, definition, ctx).map(Mapped::Feature),
        Some("pattern") => pattern::map(feature, definition, ctx).map(Mapped::Feature),
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
    fn known_cadir_versions_are_read_and_others_refused() {
        assert!(check_version(&json!({"ir_version": "6"})).is_ok());
        assert!(check_version(&json!({"ir_version": "7"})).is_ok());
        assert_eq!(
            check_version(&json!({"ir_version": "8"})).unwrap_err(),
            "unsupported CADIR version \"8\" (Dutchman reads 6, 7)"
        );
        assert!(check_version(&json!({})).is_err());
    }

    #[test]
    fn a_model_without_history_is_flattened() {
        let err = map(&cube(), &mut Report::default()).result.unwrap_err();
        assert_eq!(err, "the design carries no feature history");
    }

    #[test]
    fn datum_planes_alone_build_no_geometry() {
        let model = with(cube(), json!([datum(1, 0), datum(2, 1)]), json!([]));
        let err = map(&model, &mut Report::default()).result.unwrap_err();
        assert_eq!(err, "no feature in the history builds geometry");
    }

    #[test]
    fn the_first_unmapped_feature_names_the_reason() {
        let model = with(
            cube(),
            json!([datum(1, 0), opaque(2, 2, "Later"), opaque(3, 1, "Earlier")]),
            json!([]),
        );
        let err = map(&model, &mut Report::default()).result.unwrap_err();
        assert_eq!(err, "feature \"Earlier\": native is not mapped yet");
    }

    #[test]
    fn suppressed_features_are_left_out_and_reported() {
        let mut hidden = opaque(2, 1, "Hidden");
        hidden["suppressed"] = json!(true);
        let model = with(cube(), json!([datum(1, 0), hidden]), json!([]));
        let mut report = Report::default();
        let err = map(&model, &mut report).result.unwrap_err();
        assert_eq!(err, "no feature in the history builds geometry");
        let loss = report.losses().first().unwrap();
        assert_eq!(loss.subject, "feature \"Hidden\"");
    }
}
