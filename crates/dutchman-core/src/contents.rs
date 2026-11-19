//! What a decoded model holds, listed without converting it: its bodies, its parameters, and
//! which timeline features the source suppresses. Everything here is read from CADIR as
//! cadmpeg reports it; nothing is measured, mapped or written.

use std::collections::{BTreeMap, BTreeSet};

use cadmpeg_ir::features::ParameterValue;
use cadmpeg_ir::topology::BodyKind;

use crate::Model;

/// One body of the model.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BodyEntry {
    /// Its name in the source, when it has one.
    pub name: Option<String>,
    /// `solid`, `sheet`, `wire` or `general` (mixed-dimensional topology).
    pub kind: &'static str,
    /// The source hides it.
    pub hidden: bool,
}

/// A parameter's evaluated value, in the unit [`ParameterEntry::unit`] names.
#[derive(Clone, Debug, PartialEq)]
pub enum ParameterAmount {
    Number(f64),
    Integer(i64),
    Boolean(bool),
    Text(String),
}

/// One design parameter of the model.
#[derive(Clone, Debug, PartialEq)]
pub struct ParameterEntry {
    /// Its name in the source.
    pub name: String,
    /// Its evaluated value, when the source stores one.
    pub value: Option<ParameterAmount>,
    /// `mm` for a length, `deg` for an angle, `None` for a plain number, a count, a flag or text.
    pub unit: Option<&'static str>,
    /// The source's expression or literal text, as written there.
    pub expression: String,
    /// The name of the feature it belongs to, `None` for a design (user) parameter.
    pub feature: Option<String>,
}

/// The model's own bodies, in the order the model holds them.
pub fn bodies(model: &Model) -> Vec<BodyEntry> {
    model
        .ir
        .geometry
        .bodies
        .iter()
        .map(|body| BodyEntry {
            name: body.name.clone(),
            kind: match body.kind {
                BodyKind::Solid => "solid",
                BodyKind::Sheet => "sheet",
                BodyKind::Wire => "wire",
                BodyKind::General => "general",
            },
            hidden: body.visible == Some(false),
        })
        .collect()
}

/// The model's parameters: design parameters first, then each feature's, in source order.
pub fn parameters(model: &Model) -> Vec<ParameterEntry> {
    let owners: BTreeMap<&str, &str> = model
        .ir
        .model
        .features
        .iter()
        .filter_map(|feature| Some((feature.id.as_str(), feature.name.as_deref()?)))
        .collect();
    let mut ordered: Vec<_> = model.ir.model.parameters.iter().collect();
    ordered.sort_by_key(|parameter| (parameter.owner.is_some(), parameter.ordinal));
    ordered
        .into_iter()
        .map(|parameter| {
            let (value, unit) = match &parameter.value {
                Some(ParameterValue::Length(length)) => {
                    (Some(ParameterAmount::Number(length.get())), Some("mm"))
                }
                Some(ParameterValue::Angle(angle)) => (
                    Some(ParameterAmount::Number(angle.get().to_degrees())),
                    Some("deg"),
                ),
                Some(ParameterValue::Real(real)) => {
                    (Some(ParameterAmount::Number(real.get())), None)
                }
                Some(ParameterValue::Integer(integer)) => {
                    (Some(ParameterAmount::Integer(*integer)), None)
                }
                Some(ParameterValue::Boolean(flag)) => {
                    (Some(ParameterAmount::Boolean(*flag)), None)
                }
                Some(ParameterValue::String(text)) => {
                    (Some(ParameterAmount::Text(text.clone())), None)
                }
                None => (None, None),
            };
            ParameterEntry {
                name: parameter.name.clone(),
                value,
                unit,
                expression: parameter.expression.clone(),
                feature: parameter
                    .owner
                    .as_ref()
                    .and_then(|owner| owners.get(owner.as_str()))
                    .map(|name| (*name).to_owned()),
            }
        })
        .collect()
}

/// The timeline ordinals of the features the source suppresses.
pub fn suppressed(model: &Model) -> BTreeSet<u64> {
    model
        .ir
        .model
        .features
        .iter()
        .filter(|feature| feature.suppressed == Some(true))
        .map(|feature| feature.ordinal)
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::{Format, Report};

    /// The synthetic unit cube with these feature and parameter rows.
    fn cube(features: Value, parameters: Value) -> Model {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/generated_unit_cube.f3d");
        let model = crate::read(Format::FusionArchive, &path, &mut Report::default()).unwrap();
        let mut ir = serde_json::to_value(&model.ir).unwrap();
        ir["model"]["features"] = features;
        ir["model"]["parameters"] = parameters;
        let ir = cadmpeg_ir::CadIr::from_json(&ir.to_string()).unwrap();
        Model { ir, ..model }
    }

    fn feature(key: u32, ordinal: u64, name: &str, suppressed: bool) -> Value {
        json!({"id": format!("f3d:model:feature#{key}"), "ordinal": ordinal, "name": name,
            "suppressed": suppressed,
            "definition": {"kind": "operation", "value": {"kind": "native", "value": {"kind": "X", "parameters": {}}}},
            "dependencies": [], "source_properties": {}, "source_content": [], "outputs": []})
    }

    fn parameter(
        key: u32,
        name: &str,
        expression: &str,
        value: Value,
        owner: Option<u32>,
    ) -> Value {
        let mut row = json!({"id": format!("f3d:model:parameter#{key}"), "ordinal": key,
            "name": name, "expression": expression, "value": value, "dependencies": [],
            "properties": {}});
        if row["value"].is_null() {
            row.as_object_mut().unwrap().remove("value");
        }
        if let Some(owner) = owner {
            row["owner"] = json!(format!("f3d:model:feature#{owner}"));
        }
        row
    }

    #[test]
    fn the_cube_is_one_solid_with_no_parameters() {
        let model = cube(json!([]), json!([]));
        let listed = bodies(&model);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].kind, "solid");
        assert!(!listed[0].hidden);
        assert!(parameters(&model).is_empty());
        assert!(suppressed(&model).is_empty());
    }

    #[test]
    fn parameters_list_design_ones_first_with_values_units_and_owners() {
        let model = cube(
            json!([
                feature(1, 0, "Extrude1", false),
                feature(2, 1, "Fillet1", true)
            ]),
            json!([
                parameter(
                    1,
                    "d1",
                    "width / 2",
                    json!({"kind": "length", "value": 20.0}),
                    Some(1)
                ),
                parameter(
                    2,
                    "width",
                    "40 mm",
                    json!({"kind": "length", "value": 40.0}),
                    None
                ),
                parameter(
                    3,
                    "a1",
                    "90 deg",
                    json!({"kind": "angle", "value": std::f64::consts::FRAC_PI_2}),
                    Some(2)
                ),
                parameter(
                    4,
                    "count",
                    "3",
                    json!({"kind": "integer", "value": 3}),
                    None
                ),
                parameter(5, "pending", "width * k", Value::Null, None),
            ]),
        );
        let listed = parameters(&model);
        let names: Vec<&str> = listed.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["width", "count", "pending", "d1", "a1"]);
        assert_eq!(listed[0].value, Some(ParameterAmount::Number(40.0)));
        assert_eq!(listed[0].unit, Some("mm"));
        assert_eq!(listed[0].feature, None);
        assert_eq!(listed[1].value, Some(ParameterAmount::Integer(3)));
        assert_eq!(listed[1].unit, None);
        assert_eq!(listed[2].value, None);
        assert_eq!(listed[2].expression, "width * k");
        assert_eq!(listed[3].feature.as_deref(), Some("Extrude1"));
        assert_eq!(listed[3].expression, "width / 2");
        let Some(ParameterAmount::Number(degrees)) = listed[4].value else {
            panic!("an angle has a number");
        };
        assert!((degrees - 90.0).abs() < 1e-9);
        assert_eq!(listed[4].unit, Some("deg"));
        assert_eq!(listed[4].feature.as_deref(), Some("Fillet1"));
        assert_eq!(suppressed(&model), [1].into_iter().collect());
    }
}
