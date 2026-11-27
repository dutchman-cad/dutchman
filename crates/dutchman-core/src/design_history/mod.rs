//! A format-neutral projection of CADIR v11 `design` history into the model-history rows
//! that Dutchman's history mappers read.
//!
//! No native reader states a history only in `design` except Solid Edge. The mappers read the
//! `model` arenas (`features`, `sketches`, `sketch_entities`), so a document whose `model`
//! history is empty and whose `design` history is not gets those rows built here, once, right
//! after decoding.
//!
//! The projection copies only what `design` states. A field `design` leaves silent is left out
//! of the row, or written as the row's own `unresolved` state where the row requires the field,
//! so the mapper refuses it with its own reason. A feature of a kind not projected
//! yet, or one whose operands are not projected yet, becomes a `native` row: the mapper reports
//! it as an unmapped feature and the history stops there. Every projected row carries
//! `source_properties.projected_from = "design"`, and the report says the history came from the
//! design layer.

mod extent;
mod json;
mod ops;
mod sketch;
mod timeline;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::CadIr;
use serde_json::{json, Value};

use crate::{Report, Severity};

use json::{kind, payload, text};

/// The stable report subject of this projection.
const SUBJECT: &str = "design history";

/// The loss code of a history read from the design layer.
pub const CODE_PROJECTED: &str = "design-history/projected";
/// The loss code of a feature the projection leaves to the mapper as an unmapped feature.
pub const CODE_FEATURE: &str = "design-history/feature-not-projected";
/// The loss code of a history the projection declines as a whole.
pub const CODE_DECLINED: &str = "design-history/not-projected";
/// The loss code of features the source rolled back.
pub const CODE_ROLLED_BACK: &str = "design-history/rolled-back";
/// The loss code of a sketch whose entities the design does not state as construction or not.
pub const CODE_CONSTRUCTION: &str = "design-history/construction-unstated";

/// What the projection wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Projected {
    /// Rows written to `model.features`, synthesized sketch features included.
    pub features: usize,
    /// Rows written to `model.sketches`.
    pub sketches: usize,
    /// Features left as `native` rows.
    pub unprojected: usize,
}

/// Fills `ir.model.{features, sketches, sketch_entities}` from `ir.design` when the model
/// history is empty and the design history is not. It does nothing otherwise.
///
/// Returns what it wrote, or `None` when it wrote nothing; a history it declines is named in
/// the report.
pub fn project(ir: &mut CadIr, report: &mut Report) -> Option<Projected> {
    if !ir.model.features.is_empty() || ir.design.features.is_empty() {
        return None;
    }
    let design = serde_json::to_value(&ir.design).ok()?;
    let plan = match Plan::new(&design) {
        Ok(plan) => plan,
        Err(reason) => {
            report.push_coded(
                Severity::Degraded,
                SUBJECT,
                CODE_DECLINED,
                format!(
                    "the history in the design layer is not projected yet: {reason}; kept the final geometry"
                ),
            );
            return None;
        }
    };
    let mut out = Rows::default();
    let sketches = sketch::Sketches::new(&design);
    for id in &plan.order.members {
        plan.feature(id, &sketches, &mut out, report);
    }
    out.finish(ir, report, &plan)
}

/// The history the design states, ahead of any row.
struct Plan {
    order: timeline::Order,
    /// Design feature records by id.
    features: BTreeMap<String, Value>,
    /// The id of the feature row that carries each sketch, by sketch id.
    sketch_features: BTreeMap<String, String>,
    /// The sketches each timeline feature owns and no timeline feature carries, by owner id.
    owned: BTreeMap<String, Vec<String>>,
}

impl Plan {
    fn new(design: &Value) -> Result<Self, String> {
        let order = timeline::order(design)?;
        let features: BTreeMap<String, Value> = json::rows(design, "features")
            .filter_map(|feature| Some((text(feature, "id")?.to_owned(), feature.clone())))
            .collect();
        let in_timeline: BTreeSet<&str> = order.members.iter().map(String::as_str).collect();
        let mut sketch_features = BTreeMap::new();
        for id in &order.members {
            let Some(feature) = features.get(id) else {
                return Err(format!(
                    "the timeline names a feature that is not stated ({id})"
                ));
            };
            if let Some(sketch) = sketch_of(feature) {
                sketch_features.insert(sketch.to_owned(), id.clone());
            }
        }
        let mut owned: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut sketches: Vec<(&str, &str)> = json::rows(design, "sketches")
            .filter_map(|sketch| Some((text(sketch, "id")?, text(sketch, "owner")?)))
            .collect();
        sketches.sort_unstable();
        for (sketch, owner) in sketches {
            if sketch_features.contains_key(sketch) || !in_timeline.contains(owner) {
                continue;
            }
            sketch_features.insert(sketch.to_owned(), synthetic(owner, sketch));
            owned
                .entry(owner.to_owned())
                .or_default()
                .push(sketch.to_owned());
        }
        Ok(Self {
            order,
            features,
            sketch_features,
            owned,
        })
    }

    /// Writes the rows of one timeline feature: the sketches it owns, then itself.
    fn feature(
        &self,
        id: &str,
        sketches: &sketch::Sketches<'_>,
        out: &mut Rows,
        report: &mut Report,
    ) {
        let Some(feature) = self.features.get(id) else {
            return;
        };
        let name = text(feature, "name").unwrap_or(id).to_owned();
        let state = suppressed(feature);
        let rolled_back = self.order.rolled_back.contains(id);
        let owner_state = state.as_ref().ok().copied().flatten().unwrap_or(false);
        for sketch in self.owned.get(id).into_iter().flatten() {
            let row_id = self
                .sketch_features
                .get(sketch)
                .cloned()
                .unwrap_or_default();
            let label = sketches.name(sketch).unwrap_or(&name).to_owned();
            let built = sketches.build(sketch).map(|(row, entities, unstated)| {
                (sketch_operation(sketch), row, entities, unstated)
            });
            out.sketch_feature(
                Header {
                    id: row_id,
                    name: label,
                    suppressed: Some(owner_state),
                    rolled_back,
                    native_ref: None,
                },
                built,
                report,
            );
        }
        let operation = feature
            .pointer("/definition/operation")
            .unwrap_or(&Value::Null);
        let (suppressed, refusal) = match (&state, unsupported_envelope(feature)) {
            (Err(reason), _) => (None, Some(reason.clone())),
            (Ok(state), refusal) => (*state, refusal),
        };
        let header = Header {
            id: id.to_owned(),
            name,
            suppressed,
            rolled_back,
            native_ref: text(feature, "native_ref").map(str::to_owned),
        };
        if let Some(sketch) = sketch_of(feature) {
            let built = match refusal {
                Some(reason) => Err(reason),
                None => sketches.build(sketch).map(|(row, entities, unstated)| {
                    (sketch_operation(sketch), row, entities, unstated)
                }),
            };
            out.sketch_feature(header, built, report);
            return;
        }
        let built = match refusal {
            Some(reason) => Err(reason),
            None => ops::project(operation, self, sketches),
        };
        out.operation_feature(header, operation, built, report);
    }
}

fn sketch_operation(sketch: &str) -> Value {
    json!({"kind": "sketch", "value": {"sketch": {"kind": "planar", "sketch": sketch}}})
}

/// The sketch a feature of the `sketch` operation names.
fn sketch_of(feature: &Value) -> Option<&str> {
    let operation = feature.pointer("/definition/operation")?;
    (kind(operation) == Some("sketch")).then(|| payload(operation).as_str())?
}

/// The id of the feature row that carries a sketch no timeline feature carries: its owner's id
/// and the sketch's key, so the same document gives the same ids on every decode.
fn synthetic(owner: &str, sketch: &str) -> String {
    let key = sketch.split_once('#').map_or(sketch, |(_, key)| key);
    format!("{owner}/sketch/{key}")
}

/// The suppression the design states. The inner `None` is a state the source does not state.
fn suppressed(feature: &Value) -> Result<Option<bool>, String> {
    let Some(state) = feature.get("suppressed") else {
        return Ok(None);
    };
    match json::literal(state) {
        Some(Value::Bool(state)) => Ok(Some(*state)),
        _ => Err("its suppression is bound to a parameter, which is not projected yet".to_owned()),
    }
}

/// A refusal for what a feature states outside its operation that changes its result.
fn unsupported_envelope(feature: &Value) -> Option<String> {
    for (key, what) in [
        ("placement", "a placement of its result"),
        ("component", "a component other than the root"),
        ("driven_by", "a driving feature"),
    ] {
        if feature.get(key).is_some() {
            return Some(format!("it states {what}, which is not projected yet"));
        }
    }
    None
}

/// The rows written so far.
#[derive(Default)]
struct Rows {
    features: Vec<Value>,
    sketches: Vec<Value>,
    entities: Vec<Value>,
    unprojected: usize,
    rolled_back: usize,
}

/// What every feature row states about the feature itself.
struct Header {
    id: String,
    name: String,
    suppressed: Option<bool>,
    rolled_back: bool,
    native_ref: Option<String>,
}

type BuiltSketch = Result<(Value, Value, Vec<Value>, bool), String>;
type BuiltOperation = Result<(Value, Vec<String>), String>;

impl Rows {
    fn row(&self, header: &Header) -> Value {
        let mut properties = serde_json::Map::new();
        properties.insert("projected_from".to_owned(), json!("design"));
        if header.rolled_back {
            properties.insert("rolled_back".to_owned(), json!("true"));
        }
        let mut row = json!({
            "id": header.id,
            "ordinal": self.features.len(),
            "name": header.name,
            "dependencies": [],
            "source_properties": properties,
            "source_content": [],
            "outputs": [],
        });
        if let Some(object) = row.as_object_mut() {
            if let Some(state) = header.suppressed {
                object.insert("suppressed".to_owned(), json!(state));
            }
            if let Some(native_ref) = &header.native_ref {
                object.insert("native_ref".to_owned(), json!(native_ref));
            }
        }
        row
    }

    fn sketch_feature(&mut self, header: Header, built: BuiltSketch, report: &mut Report) {
        let mut row = self.row(&header);
        let reason = match built {
            Ok((operation, sketch, entities, unstated)) => {
                set_operation(&mut row, operation);
                if unstated {
                    if let Some(properties) = row
                        .get_mut("source_properties")
                        .and_then(Value::as_object_mut)
                    {
                        properties.insert("construction_stated".to_owned(), json!("false"));
                    }
                }
                if admits_sketch(&row, &sketch, &entities) {
                    if unstated {
                        let subject = sketch
                            .get("name")
                            .and_then(Value::as_str)
                            .or_else(|| sketch.get("id").and_then(Value::as_str))
                            .unwrap_or(&header.name)
                            .to_owned();
                        report.push_coded(
                            Severity::Info,
                            subject,
                            CODE_CONSTRUCTION,
                            "the source did not state which sketch entities are construction geometry; they were read as profile geometry",
                        );
                    }
                    self.rolled_back += usize::from(header.rolled_back);
                    self.features.push(row);
                    self.sketches.push(sketch);
                    self.entities.extend(entities);
                    return;
                }
                "its rows are not accepted".to_owned()
            }
            Err(reason) => reason,
        };
        self.native(
            &header,
            "sketch",
            &native_operation("sketch"),
            &reason,
            report,
        );
    }

    fn operation_feature(
        &mut self,
        header: Header,
        operation: &Value,
        built: BuiltOperation,
        report: &mut Report,
    ) {
        let label = kind(operation).unwrap_or("unknown");
        let reason = match built {
            Ok((definition, dependencies)) => {
                let mut row = self.row(&header);
                set_dependencies(&mut row, &dependencies);
                set_operation(&mut row, definition);
                if admits(&row) {
                    self.rolled_back += usize::from(header.rolled_back);
                    self.features.push(row);
                    return;
                }
                "its row is not accepted".to_owned()
            }
            Err(reason) => reason,
        };
        let native = match kind(operation) {
            Some("unresolved") => payload(operation)
                .get("family")
                .and_then(Value::as_str)
                .map_or_else(
                    || "unresolved".to_owned(),
                    |family| format!("unresolved {family}"),
                ),
            _ => label.to_owned(),
        };
        self.native(&header, label, &native_operation(&native), &reason, report);
    }

    /// Writes a feature as an unmapped `native` row and says why.
    fn native(
        &mut self,
        header: &Header,
        label: &str,
        operation: &Value,
        reason: &str,
        report: &mut Report,
    ) {
        let mut row = self.row(header);
        set_operation(&mut row, operation.clone());
        self.unprojected += 1;
        report.push_coded(
            Severity::Degraded,
            &header.name,
            CODE_FEATURE,
            format!(
                "the {label} feature is not projected from the design layer yet: {reason}; the history stops here"
            ),
        );
        self.features.push(row);
    }

    fn finish(self, ir: &mut CadIr, report: &mut Report, plan: &Plan) -> Option<Projected> {
        if self.features.is_empty() {
            return None;
        }
        let mut written = Projected {
            features: 0,
            sketches: 0,
            unprojected: self.unprojected,
        };
        let model = &mut ir.model;
        for row in self.features {
            match serde_json::from_value(row) {
                Ok(feature) => {
                    model.features.push(feature);
                    written.features += 1;
                }
                Err(_) => return decline(ir, report, "a projected feature row was not accepted"),
            }
        }
        for row in self.sketches {
            match serde_json::from_value(row) {
                Ok(sketch) => {
                    model.sketches.push(sketch);
                    written.sketches += 1;
                }
                Err(_) => return decline(ir, report, "a projected sketch row was not accepted"),
            }
        }
        for row in self.entities {
            match serde_json::from_value(row) {
                Ok(entity) => model.sketch_entities.push(entity),
                Err(_) => return decline(ir, report, "a projected sketch entity was not accepted"),
            }
        }
        if self.rolled_back > 0 {
            report.push_coded(
                Severity::Degraded,
                SUBJECT,
                CODE_ROLLED_BACK,
                format!(
                    "the source rolled its history back before {} of {} features; they are projected as active features because model history has no rolled-back state",
                    self.rolled_back,
                    plan.order.members.len()
                ),
            );
        }
        report.push_coded(
            Severity::Info,
            SUBJECT,
            CODE_PROJECTED,
            format!(
                "the history was read from the design layer: {} features and {} sketches; every projected row carries source property projected_from=design",
                written.features, written.sketches
            ),
        );
        Some(written)
    }
}

/// A feature row the mappers cannot run, standing for a feature that is not projected yet.
fn native_operation(label: &str) -> Value {
    json!({"kind": "native", "value": {"kind": label, "parameters": {}}})
}

/// Whether the row reads as a model-history feature.
fn admits(row: &Value) -> bool {
    serde_json::from_value::<cadmpeg_ir::features::Feature>(row.clone()).is_ok()
}

fn admits_sketch(row: &Value, sketch: &Value, entities: &[Value]) -> bool {
    admits(row)
        && serde_json::from_value::<cadmpeg_ir::sketches::Sketch>(sketch.clone()).is_ok()
        && entities.iter().all(|entity| {
            serde_json::from_value::<cadmpeg_ir::sketches::SketchEntity>(entity.clone()).is_ok()
        })
}

/// Removes what a failed write left and records that the history is declined.
fn decline(ir: &mut CadIr, report: &mut Report, reason: &str) -> Option<Projected> {
    ir.model.features.clear();
    ir.model.sketches.clear();
    ir.model.sketch_entities.clear();
    report.push_coded(
        Severity::Degraded,
        SUBJECT,
        CODE_DECLINED,
        format!(
            "the history in the design layer is not projected yet: {reason}; kept the final geometry"
        ),
    );
    None
}

fn set_dependencies(row: &mut Value, dependencies: &[String]) {
    if let Some(object) = row.as_object_mut() {
        object.insert("dependencies".to_owned(), json!(dependencies));
    }
}

fn set_operation(row: &mut Value, operation: Value) {
    if let Some(object) = row.as_object_mut() {
        object.insert(
            "definition".to_owned(),
            json!({"kind": "operation", "value": operation}),
        );
    }
}
