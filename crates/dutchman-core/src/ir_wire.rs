//! A document's full contents as JSON, and back.
//!
//! cadmpeg's CADIR wire leaves out the design arenas (`ArenaName::WIRE_OMITTED`: features,
//! parameters, configurations, sketches and their entities and constraints, spreadsheets,
//! assembly joints, history bodies, the feature input and result topologies), the
//! regeneration parent of each feature, and the sketches each product definition owns.
//! `serde_json::to_value(&ir)` therefore drops a Fusion design's whole feature history, and
//! `CadIr::from_json` refuses a document that states those arenas. Everything here that works
//! on a document as JSON (slicing a design into parts, reading rows by pointer, cutting a
//! reproduction) goes through these two functions instead, which carry the omitted arenas
//! the way cadmpeg states them: `Model::omitted_arenas_value` out, typed records back in.

use cadmpeg_ir::document::{ArenaName, Model};
use cadmpeg_ir::features::evaluation::{FeatureInputTopology, FeatureResultTopology};
use cadmpeg_ir::features::parameters::{DesignConfiguration, DesignParameter};
use cadmpeg_ir::features::{Feature, FeatureId};
use cadmpeg_ir::history_body::HistoryBody;
use cadmpeg_ir::products::AssemblyJoint;
use cadmpeg_ir::sketches::{
    Sketch, SketchConstraint, SketchEntity, SketchId, SpatialSketch, SpatialSketchConstraint,
    SpatialSketchEntity,
};
use cadmpeg_ir::spreadsheets::Spreadsheet;
use cadmpeg_ir::CadIr;
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Each product definition's `sketches`, which the wire omits, stated on the rows of
/// `product_definitions`.
fn state_definition_sketches(ir: &CadIr, rows: &mut [Value]) -> Result<(), serde_json::Error> {
    for row in rows {
        let id = row.get("id").and_then(Value::as_str);
        let sketches = ir
            .model
            .product_definitions
            .iter()
            .find(|definition| Some(definition.id.as_str()) == id)
            .map_or_else(Vec::new, |definition| definition.sketches.clone());
        if let Some(row) = row.as_object_mut() {
            row.insert("sketches".to_owned(), serde_json::to_value(&sketches)?);
        }
    }
    Ok(())
}

/// `ir` as a JSON document that also states what the CADIR wire omits: the omitted arenas under
/// `model`, each feature's `regeneration_parent`, and each product definition's `sketches`.
pub fn to_value(ir: &CadIr) -> Result<Value, serde_json::Error> {
    let mut document = serde_json::to_value(ir)?;
    let omitted = ir.model.omitted_arenas_value()?;
    let Some(model) = document.get_mut("model").and_then(Value::as_object_mut) else {
        return Ok(document);
    };
    if let Value::Object(arenas) = omitted {
        for (name, rows) in arenas {
            model.insert(name, rows);
        }
    }
    if let Some(definitions) = model
        .get_mut("product_definitions")
        .and_then(Value::as_array_mut)
    {
        state_definition_sketches(ir, definitions)?;
    }
    Ok(document)
}

/// A JSON object written member by member: the members of `kept` as they stand, except those
/// `stated` replaces, then `stated`.
struct Merged<'a> {
    kept: &'a BTreeMap<&'a str, &'a RawValue>,
    stated: &'a [(String, Box<RawValue>)],
}

impl Serialize for Merged<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let replaced = |name: &str| self.stated.iter().any(|(stated, _)| stated == name);
        let mut map = serializer.serialize_map(None)?;
        for (name, rows) in self.kept.iter().filter(|(name, _)| !replaced(name)) {
            map.serialize_entry(name, rows)?;
        }
        for (name, rows) in self.stated {
            map.serialize_entry(name, rows)?;
        }
        map.end()
    }
}

/// [`to_value`], as text. The document is written once as the CADIR wire states it and the
/// members the wire omits are spliced into its `model`, so a design with a large geometry never
/// stands as a JSON tree beside its text.
pub fn to_string(ir: &CadIr) -> Result<String, serde_json::Error> {
    let bare = serde_json::to_string(ir)?;
    let mut top: BTreeMap<&str, &RawValue> = serde_json::from_str(&bare)?;
    let Some(model_text) = top.get("model").copied() else {
        return Ok(bare);
    };
    let model: BTreeMap<&str, &RawValue> = serde_json::from_str(model_text.get())?;
    let mut stated: Vec<(String, Box<RawValue>)> = Vec::new();
    if let Value::Object(arenas) = ir.model.omitted_arenas_value()? {
        for (name, rows) in arenas {
            stated.push((name, serde_json::value::to_raw_value(&rows)?));
        }
    }
    if let Some(rows) = model.get("product_definitions") {
        let mut definitions: Vec<Value> = serde_json::from_str(rows.get())?;
        state_definition_sketches(ir, &mut definitions)?;
        stated.push((
            "product_definitions".to_owned(),
            serde_json::value::to_raw_value(&definitions)?,
        ));
    }
    top.remove("model");
    serde_json::to_string(&Document {
        top: &top,
        model: Merged {
            kept: &model,
            stated: &stated,
        },
    })
}

/// The document of [`to_string`]: its members as the wire wrote them, and the `model` merged.
struct Document<'a> {
    top: &'a BTreeMap<&'a str, &'a RawValue>,
    model: Merged<'a>,
}

impl Serialize for Document<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(None)?;
        for (name, member) in self.top {
            map.serialize_entry(name, member)?;
        }
        map.serialize_entry("model", &self.model)?;
        map.end()
    }
}

/// The arenas the wire omits, as a document states them.
#[derive(Default, Deserialize)]
#[serde(default)]
struct Omitted {
    features: Vec<Feature>,
    feature_input_topologies: Vec<FeatureInputTopology>,
    feature_result_topologies: Vec<FeatureResultTopology>,
    configurations: Vec<DesignConfiguration>,
    parameters: Vec<DesignParameter>,
    sketches: Vec<Sketch>,
    sketch_entities: Vec<SketchEntity>,
    sketch_constraints: Vec<SketchConstraint>,
    spatial_sketches: Vec<SpatialSketch>,
    spatial_sketch_entities: Vec<SpatialSketchEntity>,
    spatial_sketch_constraints: Vec<SpatialSketchConstraint>,
    spreadsheets: Vec<Spreadsheet>,
    assembly_joints: Vec<AssemblyJoint>,
    history_bodies: Vec<HistoryBody>,
}

/// The document `value` states, with the arenas [`to_value`] adds read back as typed records.
/// A document that holds none of them reads as `CadIr::from_json` reads it.
pub fn from_value(mut value: Value) -> Result<CadIr, String> {
    sort_arenas(&mut value);
    let mut taken = Map::new();
    let mut parents = Vec::new();
    let mut sketches_of: Vec<(String, Vec<SketchId>)> = Vec::new();
    if let Some(model) = value.get_mut("model").and_then(Value::as_object_mut) {
        for arena in ArenaName::WIRE_OMITTED {
            if let Some(rows) = model.remove(arena.key()) {
                taken.insert(arena.key().to_owned(), rows);
            }
        }
        if let Some(Value::Array(features)) = taken.get_mut("features") {
            for feature in features {
                let Some(row) = feature.as_object_mut() else {
                    continue;
                };
                let parent = row.remove("regeneration_parent");
                if let (Some(id), Some(parent)) = (
                    row.get("id").and_then(Value::as_str),
                    parent.as_ref().and_then(Value::as_str),
                ) {
                    parents.push((id.to_owned(), parent.to_owned()));
                }
            }
        }
        if let Some(definitions) = model
            .get_mut("product_definitions")
            .and_then(Value::as_array_mut)
        {
            for row in definitions {
                let Some(row) = row.as_object_mut() else {
                    continue;
                };
                let Some(Value::Array(rows)) = row.remove("sketches") else {
                    continue;
                };
                let id = row.get("id").and_then(Value::as_str).unwrap_or_default();
                let ids = rows
                    .into_iter()
                    .map(serde_json::from_value::<SketchId>)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|error| error.to_string())?;
                sketches_of.push((id.to_owned(), ids));
            }
        }
    }
    let mut ir = CadIr::from_json(&value.to_string()).map_err(|error| error.to_string())?;
    let omitted: Omitted = serde_json::from_value(Value::Object(taken))
        .map_err(|error| format!("a design arena of the document is malformed: {error}"))?;
    let model: &mut Model = &mut ir.model;
    model.features = omitted.features;
    model.feature_input_topologies = omitted.feature_input_topologies;
    model.feature_result_topologies = omitted.feature_result_topologies;
    model.configurations = omitted.configurations;
    model.parameters = omitted.parameters;
    model.sketches = omitted.sketches;
    model.sketch_entities = omitted.sketch_entities;
    model.sketch_constraints = omitted.sketch_constraints;
    model.spatial_sketches = omitted.spatial_sketches;
    model.spatial_sketch_entities = omitted.spatial_sketch_entities;
    model.spatial_sketch_constraints = omitted.spatial_sketch_constraints;
    model.spreadsheets = omitted.spreadsheets;
    model.assembly_joints = omitted.assembly_joints;
    model.history_bodies = omitted.history_bodies;
    for (child, parent) in parents {
        let (child, parent) = (
            FeatureId::mint(&child).map_err(|error| error.to_string())?,
            FeatureId::mint(&parent).map_err(|error| error.to_string())?,
        );
        model
            .set_feature_regeneration_parent(&child, &parent)
            .map_err(|error| error.to_string())?;
    }
    for (id, sketches) in sketches_of {
        if let Some(definition) = model
            .product_definitions
            .iter_mut()
            .find(|definition| definition.id.as_str() == id)
        {
            definition.sketches = sketches;
        }
    }
    Ok(ir)
}

/// CADIR states each arena in id order and the reader refuses a document that does not. A
/// document assembled by editing rows of another (appending a copy of a part, say) is put in
/// that order first; the rows themselves are not touched.
fn sort_arenas(value: &mut Value) {
    for home in ["geometry", "model", "design", "process"] {
        let Some(arenas) = value.get_mut(home).and_then(Value::as_object_mut) else {
            continue;
        };
        for rows in arenas.values_mut() {
            let Some(rows) = rows.as_array_mut() else {
                continue;
            };
            if rows
                .iter()
                .all(|row| row.get("id").is_some_and(Value::is_string))
            {
                rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
            }
        }
    }
    let Some(native) = value.get_mut("native").and_then(Value::as_object_mut) else {
        return;
    };
    for codec in native.values_mut().filter_map(Value::as_object_mut) {
        for rows in codec.values_mut() {
            let Some(rows) = rows.as_array_mut() else {
                continue;
            };
            if rows
                .iter()
                .all(|row| row.get("id").is_some_and(Value::is_string))
            {
                rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
            }
        }
    }
}

/// [`from_value`], from text.
pub fn from_str(text: &str) -> Result<CadIr, String> {
    from_value(serde_json::from_str(text).map_err(|error| error.to_string())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Format, Report};
    use std::path::Path;

    /// Every arena the CADIR wire omits, as `(name, rows as JSON)`.
    fn arenas(ir: &CadIr) -> Vec<(&'static str, Value)> {
        let m = &ir.model;
        fn rows<T: serde::Serialize>(value: &T) -> Value {
            serde_json::to_value(value).unwrap()
        }
        vec![
            ("features", rows(&m.features)),
            (
                "feature_input_topologies",
                rows(&m.feature_input_topologies),
            ),
            (
                "feature_result_topologies",
                rows(&m.feature_result_topologies),
            ),
            ("configurations", rows(&m.configurations)),
            ("parameters", rows(&m.parameters)),
            ("sketches", rows(&m.sketches)),
            ("sketch_entities", rows(&m.sketch_entities)),
            ("sketch_constraints", rows(&m.sketch_constraints)),
            ("spatial_sketches", rows(&m.spatial_sketches)),
            ("spatial_sketch_entities", rows(&m.spatial_sketch_entities)),
            (
                "spatial_sketch_constraints",
                rows(&m.spatial_sketch_constraints),
            ),
            ("spreadsheets", rows(&m.spreadsheets)),
            ("assembly_joints", rows(&m.assembly_joints)),
            ("history_bodies", rows(&m.history_bodies)),
        ]
    }

    fn fixture(name: &str) -> crate::Model {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name);
        let format = Format::from_path(&path).expect("a known format");
        crate::read(format, &path, &mut Report::default()).unwrap()
    }

    /// The JSON document and back keeps all fourteen omitted arenas, each feature's regeneration
    /// parent and the sketches of each product definition. The bare CADIR wire keeps none.
    #[test]
    fn a_designs_history_survives_the_json_document() {
        let mut populated = std::collections::BTreeSet::new();
        for name in [
            "design_sketch_constraints.f3d",
            "design_copy_paste.f3d",
            "generated_unit_cube.f3d",
        ] {
            let model = fixture(name);
            let ir = &model.ir;
            let again = from_value(to_value(ir).unwrap()).unwrap();
            assert_eq!(arenas(&again), arenas(ir), "{name}");
            for (arena, rows) in arenas(ir) {
                if rows.as_array().is_some_and(|rows| !rows.is_empty()) {
                    populated.insert(arena);
                }
            }
            for feature in &ir.model.features {
                assert_eq!(
                    again.model.feature_regeneration_parent(&feature.id),
                    ir.model.feature_regeneration_parent(&feature.id),
                    "{name}: {}",
                    feature.id
                );
            }
            let owned = |ir: &CadIr| {
                ir.model
                    .product_definitions
                    .iter()
                    .map(|definition| (definition.id.clone(), definition.sketches.clone()))
                    .collect::<Vec<_>>()
            };
            assert_eq!(owned(&again), owned(ir), "{name}");
            // The same through text.
            assert_eq!(
                arenas(&from_str(&to_string(ir).unwrap()).unwrap()),
                arenas(ir),
                "{name}"
            );
            // The text is the same document, written without a JSON tree beside it.
            assert_eq!(
                serde_json::from_str::<Value>(&to_string(ir).unwrap()).unwrap(),
                to_value(ir).unwrap(),
                "{name}"
            );
            // The bare wire is what loses them.
            if !ir.model.features.is_empty() {
                let bare = CadIr::from_json(&serde_json::to_string(ir).unwrap()).unwrap();
                assert!(bare.model.features.is_empty(), "{name}");
            }
        }
        for arena in [
            "features",
            "parameters",
            "sketches",
            "sketch_entities",
            "sketch_constraints",
        ] {
            assert!(populated.contains(arena), "no fixture holds {arena}");
        }
    }

    /// Assembly joints, which the wire omits and placements depend on, survive any number of
    /// passes through the document.
    #[test]
    fn assembly_joints_survive_repeated_passes() {
        let root = serde_json::json!({"operand": {"container": {"kind": "root"},
            "object": "root:origin", "subelements": []},
            "frame": crate::placement::IDENTITY, "detached": false});
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        ir.model.assembly_joints = serde_json::from_value(serde_json::json!([
            {"id": "f3d:model:joint#1", "operands": {"kind": "pair", "value":
                {"kind": {"kind": "fixed"}, "connectors": [root, root]}},
             "suppressed": false, "drives": []}]))
        .unwrap();
        assert_eq!(ir.model.assembly_joints.len(), 1);
        let once = from_value(to_value(&ir).unwrap()).unwrap();
        let twice = from_str(&to_string(&once).unwrap()).unwrap();
        assert_eq!(arenas(&once), arenas(&ir));
        assert_eq!(arenas(&twice), arenas(&ir));
    }
}
