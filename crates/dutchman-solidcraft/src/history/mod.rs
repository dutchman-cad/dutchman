//! The model's feature history, as SolidCraft parametric features.
//!
//! History is all or nothing: CADIR keeps only the final B-rep, so a history that stops part
//! way cannot be finished with an imported body. Either every active feature maps, or the
//! design is written as one import.
//!
//! The mapping reads the model in CADIR's documented JSON form (`docs/cad-ir.md` in cadmpeg),
//! the stable contract cadmpeg publishes, rather than its Rust accessors. It knows the wire
//! versions in [`IR_VERSIONS`] and refuses others rather than misread them.

mod params;
mod sketch;

use dutchman_core::{Model, Report, Severity};
use serde_json::Value;

/// The CADIR wire versions the mapping reads. Version 7 adds only assembly joint kinds and a
/// sheet-metal thickness side, neither of which the mapping uses.
const IR_VERSIONS: [&str; 2] = ["6", "7"];

/// A replayable history: SolidCraft parameters and features in timeline order.
#[derive(Debug, Default)]
pub(crate) struct History {
    pub(crate) params: Vec<Value>,
    pub(crate) features: Vec<Value>,
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

/// The model and what has been mapped so far.
struct Context<'a> {
    /// The CADIR `model` object.
    model: &'a Value,
    next_id: u64,
    /// Whether a feature that builds geometry has been mapped.
    builds: bool,
}

impl<'a> Context<'a> {
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

/// Maps the model's history, or says why it cannot be replayed. Losses of a history that is
/// written go to `report`.
pub(crate) fn map(model: &Model, report: &mut Report) -> Result<History, String> {
    let document =
        serde_json::to_value(&model.ir).map_err(|err| format!("cannot read the model: {err}"))?;
    check_version(&document)?;
    let wire = document.get("model").cloned().unwrap_or(Value::Null);
    let mut ctx = Context {
        model: &wire,
        next_id: 1,
        builds: false,
    };
    let mut features: Vec<&Value> = ctx.arena("features").iter().collect();
    if features.is_empty() {
        return Err("the file carries no feature history".to_owned());
    }
    features.sort_by_key(|feature| feature.get("ordinal").and_then(Value::as_u64));

    let mut history = History::default();
    for feature in features {
        if feature.get("suppressed").and_then(Value::as_bool) == Some(true) {
            report.push(
                Severity::Info,
                describe(feature),
                "suppressed in the source; left out",
            );
            continue;
        }
        history.total += 1;
        match map_feature(feature, &mut ctx, report) {
            Ok(Mapped::Nothing) => history.mapped += 1,
            Ok(Mapped::Feature(mapped)) => {
                history.mapped += 1;
                history.features.push(mapped);
            }
            Err(reason) => return Err(format!("{}: {reason}", describe(feature))),
        }
    }
    if !ctx.builds {
        return Err("no feature in the history builds geometry".to_owned());
    }
    history.params = params::map(&model.ir.model.parameters, report);
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
        let err = map(&cube(), &mut Report::default()).unwrap_err();
        assert_eq!(err, "the file carries no feature history");
    }

    #[test]
    fn datum_planes_alone_build_no_geometry() {
        let model = with(cube(), json!([datum(1, 0), datum(2, 1)]), json!([]));
        let err = map(&model, &mut Report::default()).unwrap_err();
        assert_eq!(err, "no feature in the history builds geometry");
    }

    #[test]
    fn the_first_unmapped_feature_names_the_reason() {
        let model = with(
            cube(),
            json!([datum(1, 0), opaque(2, 2, "Later"), opaque(3, 1, "Earlier")]),
            json!([]),
        );
        let err = map(&model, &mut Report::default()).unwrap_err();
        assert_eq!(err, "feature \"Earlier\": native is not mapped yet");
    }

    #[test]
    fn suppressed_features_are_left_out_and_reported() {
        let mut hidden = opaque(2, 1, "Hidden");
        hidden["suppressed"] = json!(true);
        let model = with(cube(), json!([datum(1, 0), hidden]), json!([]));
        let mut report = Report::default();
        let err = map(&model, &mut report).unwrap_err();
        assert_eq!(err, "no feature in the history builds geometry");
        let loss = report.losses().first().unwrap();
        assert_eq!(loss.subject, "feature \"Hidden\"");
    }
}
