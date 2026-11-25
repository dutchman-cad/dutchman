//! One shared row index for component extraction. Geometry stays in compact JSON until
//! a component reaches it; only structure and history need an expanded JSON view.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use serde_json::{value::RawValue, Value};

/// The arena of history bodies: bodies only an earlier history state holds.
const HISTORY_BODIES: &str = "history_bodies";

#[derive(Deserialize)]
pub(super) struct Packed<'a> {
    #[serde(borrow)]
    geometry: BTreeMap<&'a str, Vec<&'a RawValue>>,
    #[serde(borrow)]
    model: BTreeMap<&'a str, Vec<&'a RawValue>>,
    #[serde(borrow)]
    native: Option<&'a RawValue>,
    #[serde(flatten)]
    fields: serde_json::Map<String, Value>,
}

impl<'a> Packed<'a> {
    pub fn read(text: &'a str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|err| err.to_string())
    }

    /// The assembly algorithms inspect these arenas and native placement records.
    /// Extracted parts retain every header field and omit native records, as before.
    pub fn document(&self) -> Result<Value, String> {
        let mut fields = self.fields.clone();
        let mut model = serde_json::Map::new();
        for name in [
            "bodies",
            "product_definitions",
            "occurrences",
            "assembly_joints",
            "features",
            "parameters",
            "appearance_bindings",
        ] {
            let home = if name == "bodies" {
                &self.geometry
            } else {
                &self.model
            };
            if let Some(rows) = home.get(name) {
                let rows: Result<Vec<Value>, _> = rows
                    .iter()
                    .map(|row| serde_json::from_str(row.get()))
                    .collect();
                model.insert(
                    name.to_owned(),
                    Value::Array(rows.map_err(|err| err.to_string())?),
                );
            }
        }
        let bodies = model
            .remove("bodies")
            .unwrap_or_else(|| Value::Array(Vec::new()));
        fields.insert("geometry".to_owned(), serde_json::json!({"bodies": bodies}));
        fields.insert("model".to_owned(), Value::Object(model));
        if let Some(native) = self.native {
            let namespaces: BTreeMap<String, &RawValue> =
                serde_json::from_str(native.get()).map_err(|err| err.to_string())?;
            if let Some(f3d) = namespaces.get("f3d") {
                let arenas: BTreeMap<String, &RawValue> =
                    serde_json::from_str(f3d.get()).map_err(|err| err.to_string())?;
                let mut selected = serde_json::Map::new();
                for name in [
                    "xref_designs",
                    "xref_references",
                    "design_component_occurrences",
                    "display_poses",
                ] {
                    if let Some(rows) = arenas.get(name) {
                        selected.insert(
                            name.to_owned(),
                            serde_json::from_str(rows.get()).map_err(|err| err.to_string())?,
                        );
                    }
                }
                fields.insert("native".to_owned(), serde_json::json!({ "f3d": selected }));
            }
        }
        Ok(Value::Object(fields))
    }
}

#[derive(Clone, Copy)]
pub(super) enum Row<'a> {
    Value(&'a Value),
    Packed(&'a RawValue),
}

impl<'a> Row<'a> {
    pub fn value(self) -> Result<Cow<'a, Value>, String> {
        match self {
            Self::Value(value) => Ok(Cow::Borrowed(value)),
            Self::Packed(value) => serde_json::from_str(value.get())
                .map(Cow::Owned)
                .map_err(|err| err.to_string()),
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Entry<'a> {
    pub arena: &'a str,
    pub row: Row<'a>,
}

pub(super) struct Index<'a> {
    pub document: &'a Value,
    pub rows: BTreeMap<String, Entry<'a>>,
    /// Unordered children whose v11 wire stores only the parent pointer.
    pub children: BTreeMap<String, Vec<String>>,
    /// Reverse presentation edges, shared by all component slices. Ids are
    /// complete identities, including instance scope; suffixes are not aliases.
    pub bindings: BTreeMap<&'a str, Vec<&'a Value>>,
    /// `body_centers` lists that features of more than one definition carry. Such a list
    /// can't be the state of each owning component, so a component's slice leaves it out.
    pub shared_centers: BTreeSet<&'a str>,
    /// History bodies by the feature that produces or consumes them. A slice keeping
    /// either feature carries the complete nested body with it.
    pub nests: BTreeMap<String, Vec<String>>,
}

impl<'a> Index<'a> {
    fn new(document: &'a Value) -> Self {
        let mut bindings: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
        for binding in document
            .pointer("/model/appearance_bindings")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            // Parts are shared by every placement of their definition. Scoped
            // assignments cannot be copied into that shared definition.
            if ["path", "state"].iter().any(|key| {
                binding
                    .get("target")
                    .and_then(|target| target.get(key))
                    .is_some_and(|value| !value.is_null())
            }) {
                continue;
            }
            if let Some(target) = binding
                .pointer("/target/entity/value")
                .and_then(Value::as_str)
            {
                bindings.entry(target).or_default().push(binding);
            }
        }
        let mut owners: BTreeMap<&str, BTreeSet<Option<&str>>> = BTreeMap::new();
        for feature in document
            .pointer("/model/features")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if let Some(centers) = feature
                .pointer("/source_properties/body_centers")
                .and_then(Value::as_str)
            {
                owners
                    .entry(centers)
                    .or_default()
                    .insert(feature.get("product_definition").and_then(Value::as_str));
            }
        }
        let shared_centers = owners
            .into_iter()
            .filter(|(_, definitions)| definitions.len() > 1)
            .map(|(centers, _)| centers)
            .collect();
        Self {
            document,
            rows: BTreeMap::new(),
            children: BTreeMap::new(),
            bindings,
            shared_centers,
            nests: BTreeMap::new(),
        }
    }

    fn insert(&mut self, id: String, entry: Entry<'a>) {
        self.rows.insert(id, entry);
    }

    pub fn packed(source: &Packed<'a>, document: &'a Value) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct Identity {
            id: Option<String>,
            body: Option<String>,
            shell: Option<String>,
            produced_by: Option<String>,
            consumed_by: Option<String>,
        }
        let mut index = Self::new(document);
        for (&arena, rows) in source.geometry.iter().chain(&source.model) {
            for row in rows.iter() {
                let identity: Identity =
                    serde_json::from_str(row.get()).map_err(|err| err.to_string())?;
                if let Some(id) = identity.id {
                    if arena == HISTORY_BODIES {
                        for feature in [identity.produced_by, identity.consumed_by]
                            .into_iter()
                            .flatten()
                        {
                            index.nests.entry(feature).or_default().push(id.clone());
                        }
                    }
                    let parent = match arena {
                        "regions" => identity.body,
                        "faces" => identity.shell,
                        _ => None,
                    };
                    if let Some(parent) = parent {
                        index.children.entry(parent).or_default().push(id.clone());
                    }
                    index.insert(
                        id,
                        Entry {
                            arena,
                            row: Row::Packed(row),
                        },
                    );
                }
            }
        }
        Ok(index)
    }

    pub fn value(document: &'a Value) -> Result<Self, String> {
        let model = document
            .get("model")
            .and_then(Value::as_object)
            .ok_or("the model is malformed")?;
        let geometry = document
            .get("geometry")
            .and_then(Value::as_object)
            .ok_or("the geometry is malformed")?;
        let mut index = Self::new(document);
        for (arena, values) in geometry.iter().chain(model) {
            for row in values.as_array().into_iter().flatten() {
                if let Some(id) = row.get("id").and_then(Value::as_str) {
                    if arena == HISTORY_BODIES {
                        for field in ["produced_by", "consumed_by"] {
                            if let Some(feature) = row.get(field).and_then(Value::as_str) {
                                index
                                    .nests
                                    .entry(feature.to_owned())
                                    .or_default()
                                    .push(id.to_owned());
                            }
                        }
                    }
                    let parent = match arena.as_str() {
                        "regions" => row.get("body"),
                        "faces" => row.get("shell"),
                        _ => None,
                    };
                    if let Some(parent) = parent.and_then(Value::as_str) {
                        index
                            .children
                            .entry(parent.to_owned())
                            .or_default()
                            .push(id.to_owned());
                    }
                    index.insert(
                        id.to_owned(),
                        Entry {
                            arena,
                            row: Row::Value(row),
                        },
                    );
                }
            }
        }
        Ok(index)
    }
}
