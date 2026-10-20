//! The model's feature history, as SolidCraft parametric features.
//!
//! History is all or nothing: CADIR keeps only the final B-rep, so a history that stops part
//! way cannot be finished with an imported body. Either every active feature maps, or the
//! design is written as one import.

mod params;

use cadmpeg_ir::{Feature, FeatureDefinition, FeatureOperation};
use dutchman_core::{Model, Report, Severity};
use serde_json::Value;

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
    /// These features, in timeline order.
    #[allow(dead_code)] // Built by the feature mappings that follow the framework.
    Features(Vec<Value>),
}

/// Ids and cross-references while a history is mapped.
struct Context {
    next_id: u64,
}

impl Context {
    #[allow(dead_code)] // Used by the feature mappings that follow the framework.
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }
}

/// Maps the model's history, or says why it cannot be replayed. Losses of a history that is
/// written go to `report`.
pub(crate) fn map(model: &Model, report: &mut Report) -> Result<History, String> {
    let mut features: Vec<&Feature> = model.ir.model.features.iter().collect();
    if features.is_empty() {
        return Err("the file carries no feature history".to_owned());
    }
    features.sort_by_key(|feature| feature.ordinal);

    let mut ctx = Context { next_id: 1 };
    let mut history = History::default();
    for feature in features {
        if feature.suppressed == Some(true) {
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
            Ok(Mapped::Features(mapped)) => {
                history.mapped += 1;
                history.features.extend(mapped);
            }
            Err(reason) => return Err(format!("{}: {reason}", describe(feature))),
        }
    }
    if history.features.is_empty() {
        return Err("no feature in the history builds geometry".to_owned());
    }
    history.params = params::map(&model.ir.model.parameters, report);
    history.next_id = ctx.next_id;
    Ok(history)
}

/// One source feature as SolidCraft features, or why it cannot be.
fn map_feature(
    feature: &Feature,
    _ctx: &mut Context,
    _report: &mut Report,
) -> Result<Mapped, String> {
    match feature.evaluation.definition() {
        FeatureDefinition::Operation(FeatureOperation::DatumPrincipalPlane { .. }) => {
            Ok(Mapped::Nothing)
        }
        definition => Err(format!("{} is not mapped yet", kind(definition))),
    }
}

/// A feature as the report names it: its source name, else its kind.
fn describe(feature: &Feature) -> String {
    match &feature.name {
        Some(name) if !name.trim().is_empty() => format!("feature \"{name}\""),
        _ => format!("{} feature", kind(feature.evaluation.definition())),
    }
}

/// The CADIR operation name of a definition (`extrude`, `fillet`, …).
fn kind(definition: &FeatureDefinition) -> String {
    let tag = |value: &Value| {
        value
            .get("definition")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let value = serde_json::to_value(definition).unwrap_or(Value::Null);
    match tag(&value).as_deref() {
        Some("post_process") => value
            .get("operation")
            .and_then(tag)
            .unwrap_or_else(|| "post-processed".to_owned()),
        Some(name) => name.to_owned(),
        None => "unknown".to_owned(),
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
