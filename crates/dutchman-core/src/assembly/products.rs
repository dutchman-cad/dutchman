//! Project definition-owned CADIR products into the placed tree used by the assembly builder.
//! This is an internal view only; component slices keep the original CADIR records.
use super::*;
use serde_json::json;

pub(crate) fn occurrences(model: &cadmpeg_ir::document::Model) -> Result<Value, String> {
    let definitions =
        serde_json::to_value(&model.product_definitions).map_err(|err| err.to_string())?;
    let occurrences = serde_json::to_value(&model.occurrences).map_err(|err| err.to_string())?;
    let mut view = json!({"product_definitions": definitions, "occurrences": occurrences});
    project(&mut view)?;
    Ok(view
        .as_object_mut()
        .and_then(|fields| fields.remove("occurrences"))
        .unwrap_or_else(|| json!([])))
}

pub(crate) fn project(model: &mut Value) -> Result<(), String> {
    file_containers(model)?;
    let rows = model
        .get("occurrences")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    if rows.len() > MAX_PLACEMENTS {
        return Err(format!("it places over {MAX_PLACEMENTS} components"));
    }
    let definition_rows = model
        .get("product_definitions")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let definitions: BTreeMap<&str, &Value> = definition_rows
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let rows: BTreeMap<&str, &Value> = rows
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let roots: Vec<_> = definition_rows
        .iter()
        .filter(|row| {
            row.pointer("/storage/kind").and_then(Value::as_str) == Some("file")
                && row.pointer("/storage/source").is_none()
        })
        .collect();
    let mut overrides = BTreeMap::new();
    let mut override_count = 0usize;
    for (id, row) in &rows {
        let mut entries = BTreeMap::new();
        for entry in row
            .get("overrides")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            override_count = override_count.saturating_add(1);
            if override_count > MAX_PLACEMENTS {
                return Err(format!(
                    "it states over {MAX_PLACEMENTS} component overrides"
                ));
            }
            let Some(path) = entry.get("path").and_then(Value::as_array) else {
                continue;
            };
            if path.len() > MAX_DEPTH {
                return Err(format!(
                    "a component override nests over {MAX_DEPTH} components"
                ));
            }
            let path: Option<Vec<String>> = path
                .iter()
                .map(|step| {
                    (step.pointer("/occurrence/kind").and_then(Value::as_str) == Some("occurrence"))
                        .then(|| step.pointer("/occurrence/value").and_then(Value::as_str))
                        .flatten()
                        .map(str::to_owned)
                })
                .collect();
            if let Some(path) = path {
                entries.entry(path).or_insert(entry);
            }
        }
        overrides.insert(*id, entries);
    }
    let mut tree = Products {
        definitions: &definitions,
        rows: &rows,
        overrides,
        used: BTreeSet::new(),
        seen: BTreeSet::new(),
        out: Vec::new(),
        path: Vec::new(),
        level: 0,
    };
    for root in roots {
        tree.visit(root, None, &[])?;
    }
    // A row no root definition reaches is placed inside a component the design does not
    // place; the placed tree cannot carry it, so refuse rather than drop it silently.
    if rows.keys().any(|id| !tree.seen.contains(*id)) {
        return Err(
            "some components are placed inside components the design does not place".to_owned(),
        );
    }
    let out = tree.out;
    if let Some(model) = model.as_object_mut() {
        model.insert("occurrences".to_owned(), Value::Array(out));
    }
    Ok(())
}

/// Fusion and SolidWorks file roots are now the components themselves. Keep
/// the document containers Dutchman wrote around them before cadmpeg #1465.
/// Component identities survive the fold; Fusion archive document identities
/// use the separate `f3d:archive:product-definition` namespace. This is a placed
/// view only: neither the decoded definitions nor their geometry are rewritten.
fn file_containers(model: &mut Value) -> Result<(), String> {
    let Some(definitions) = model.get("product_definitions").and_then(Value::as_array) else {
        return Ok(());
    };
    let component_file = |definition: &Value| {
        let id = definition.get("id").and_then(Value::as_str).unwrap_or("");
        let namespace = id
            .rsplit_once('#')
            .map(|(namespace, _)| namespace)
            .unwrap_or("");
        (namespace == "f3d:model:component"
            || namespace == "sldprt:assembly:model"
            || (namespace.starts_with("f3d:xref/") && namespace.ends_with("/model:component")))
            && definition.pointer("/storage/kind").and_then(Value::as_str) == Some("file")
    };
    if !definitions.iter().any(component_file) {
        return Ok(());
    }
    let rows: BTreeMap<&str, &Value> = model
        .get("occurrences")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let definitions_by_id: BTreeMap<&str, &Value> = definitions
        .iter()
        .filter_map(|row| Some((row.get("id")?.as_str()?, row)))
        .collect();
    let mut parts = Vec::with_capacity(definitions.len());
    let mut containers = Vec::new();
    let mut roots = Vec::new();
    let mut replaced = BTreeMap::new();
    for definition in definitions {
        let id = definition.get("id").and_then(Value::as_str).unwrap_or("");
        if !component_file(definition) {
            parts.push(definition.clone());
            continue;
        }
        let container_id = json!([id, "file"]).to_string();
        let root_id = json!([id, "root"]).to_string();
        let mut part_children = Vec::new();
        let mut file_children = Vec::new();
        let mut inserted = false;
        for child in definition
            .get("occurrences")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let row = child.as_str().and_then(|id| rows.get(id)).copied();
            let reference = row
                .and_then(|row| row.get("native_ref"))
                .and_then(Value::as_str)
                .unwrap_or("");
            let namespace = reference
                .rsplit_once('#')
                .map(|(namespace, _)| namespace)
                .unwrap_or("");
            // Root-level xrefs, unresolved insert declarations, and stored-frame
            // groups stayed in the file container before root-component folding.
            let xref = namespace == "f3d:xref:reference"
                || (namespace.starts_with("f3d:xref/") && namespace.ends_with("/xref:reference"));
            let unresolved = namespace.ends_with(":design-parameter-scope")
                && row
                    .and_then(|row| row.get("prototype"))
                    .and_then(Value::as_str)
                    .and_then(|id| definitions_by_id.get(id))
                    .and_then(|definition| definition.pointer("/storage/kind"))
                    .and_then(Value::as_str)
                    == Some("external");
            let stored = child
                .as_str()
                .is_some_and(|id| id.ends_with("/stored-frame-occurrence"));
            if xref || unresolved || stored {
                file_children.push(child.clone());
            } else {
                part_children.push(child.clone());
                if !inserted {
                    file_children.push(json!(root_id));
                    inserted = true;
                }
            }
        }
        if !inserted {
            file_children.push(json!(root_id));
        }
        let mut part = definition.clone();
        let fields = part
            .as_object_mut()
            .ok_or("a component definition has no fields")?;
        // Fusion component names are source_name. The fold adopts the archive
        // document's label; that label belongs on its surrounding container.
        if id.starts_with("f3d:") {
            fields.remove("label");
        }
        fields.insert("storage".to_owned(), json!({"kind":"embedded"}));
        fields.insert("occurrences".to_owned(), Value::Array(part_children));
        parts.push(part);
        let mut container = definition.clone();
        let fields = container
            .as_object_mut()
            .ok_or("a file definition has no fields")?;
        fields.insert("id".to_owned(), json!(container_id));
        fields.remove("source_name");
        fields.insert("bodies".to_owned(), json!([]));
        fields.insert("sketches".to_owned(), json!([]));
        fields.insert("occurrences".to_owned(), Value::Array(file_children));
        containers.push(container);
        let mut root = json!({"id":root_id, "prototype":id, "transform":IDENTITY,
            "overrides":[], "file_root_component":true});
        if id.starts_with("sldprt:") {
            let fields = root
                .as_object_mut()
                .ok_or("a root component placement has no fields")?;
            for (key, source) in [("name", "source_name"), ("native_ref", "native_ref")] {
                if let Some(value) = definition.get(source) {
                    fields.insert(key.to_owned(), value.clone());
                }
            }
        }
        roots.push(root);
        replaced.insert(id.to_owned(), container_id);
    }
    if replaced.is_empty() {
        return Ok(());
    }
    let mut occurrences = model
        .get("occurrences")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for occurrence in &mut occurrences {
        if let Some(replacement) = occurrence
            .get("prototype")
            .and_then(Value::as_str)
            .and_then(|id| replaced.get(id))
        {
            if let Some(fields) = occurrence.as_object_mut() {
                fields.insert("prototype".to_owned(), json!(replacement));
            }
        }
    }
    parts.extend(containers);
    occurrences.extend(roots);
    if let Some(fields) = model.as_object_mut() {
        fields.insert("product_definitions".to_owned(), Value::Array(parts));
        fields.insert("occurrences".to_owned(), Value::Array(occurrences));
    }
    Ok(())
}

struct Products<'a> {
    definitions: &'a BTreeMap<&'a str, &'a Value>,
    rows: &'a BTreeMap<&'a str, &'a Value>,
    overrides: BTreeMap<&'a str, BTreeMap<Vec<String>, &'a Value>>,
    used: BTreeSet<String>,
    /// Rows the walk reached, or deliberately left out with what they hold.
    seen: BTreeSet<String>,
    out: Vec<Value>,
    path: Vec<String>,
    level: usize,
}

impl Products<'_> {
    fn parent_value(&self, parent: Option<&str>) -> Value {
        parent.map_or_else(
            || json!({"kind":"root"}),
            |id| json!({"kind":"occurrence", "occurrence":id}),
        )
    }

    fn fresh_id(&mut self, original: &str) -> String {
        if self.used.insert(original.to_owned()) {
            original.to_owned()
        } else {
            // A JSON pair cannot alias a typed source id. Keep the native key
            // at the end so explicit nested joint paths can select this use.
            let key = original.rsplit(['#', ':']).next().unwrap_or(original);
            format!("{}:{key}", json!([original, self.out.len()]))
        }
    }

    /// Mark what a deliberately left-out component holds as accounted for.
    fn account_for(&mut self, definition: &Value, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for child in definition
            .get("occurrences")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            if !self.seen.insert(child.to_owned()) {
                continue;
            }
            let inner = self
                .rows
                .get(child)
                .and_then(|row| row.get("prototype"))
                .and_then(Value::as_str)
                .and_then(|id| self.definitions.get(id))
                .copied();
            if let Some(inner) = inner {
                self.account_for(inner, depth + 1);
            }
        }
    }

    fn visit(
        &mut self,
        definition: &Value,
        parent: Option<&str>,
        overrides: &[(&str, usize)],
    ) -> Result<(), String> {
        if self.level > MAX_DEPTH {
            return Err(format!(
                "it nests over {MAX_DEPTH} components or places a component inside itself"
            ));
        }
        let children = definition
            .get("occurrences")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        for child in children {
            if self.out.len() >= MAX_PLACEMENTS {
                return Err(format!("it places over {MAX_PLACEMENTS} components"));
            }
            let original = child
                .as_str()
                .ok_or("a component placement has no identity")?;
            // FreeCAD can remove bookkeeping occurrences before building the tree.
            let Some(row) = self.rows.get(original) else {
                continue;
            };
            self.seen.insert(original.to_owned());
            let Some(prototype) = row
                .get("prototype")
                .and_then(Value::as_str)
                .and_then(|id| self.definitions.get(id))
            else {
                // The definition is not stated (an unresolved link). Place it as an external
                // component with no file, which the assembly reports as left out.
                let id = self.fresh_id(original);
                let mut placed = (*row).clone();
                let fields = placed
                    .as_object_mut()
                    .ok_or("a component placement has no fields")?;
                fields
                    .entry("transform".to_owned())
                    .or_insert_with(|| json!(IDENTITY));
                fields.insert("id".to_owned(), json!(id));
                fields.insert("ordinal".to_owned(), json!(self.out.len()));
                fields.insert("parent".to_owned(), self.parent_value(parent));
                fields.insert(
                    "prototype".to_owned(),
                    json!({"kind":"external", "document":null, "object":null,
                        "definition":row.get("prototype")}),
                );
                self.out.push(placed);
                continue;
            };
            let mut placed = (*row).clone();
            let source_node = row.get("file_root_component").and_then(Value::as_bool) != Some(true);
            if source_node {
                self.path.push(original.to_owned());
            }
            // Paths are relative to the occurrence carrying the override. Inner
            // values apply first; an enclosing insertion can override them.
            for (scope, start) in overrides.iter().rev() {
                if let Some(entry) = self
                    .overrides
                    .get(scope)
                    .and_then(|entries| self.path.get(*start..).and_then(|path| entries.get(path)))
                {
                    for field in ["transform", "suppressed", "visible", "representation"] {
                        if let Some(value) = entry.get(field).filter(|value| !value.is_null()) {
                            if let Some(fields) = placed.as_object_mut() {
                                fields.insert(field.to_owned(), value.clone());
                            }
                        }
                    }
                }
            }
            // An occurrence the source itself states suppressed is kept, with what it holds,
            // and placed hidden (see `suppresses`). Suppression an override states (Fusion
            // selects each stored layout of a component by suppressing the children an
            // insertion does not hold) still leaves the occurrence out: there it names which
            // children exist, not a component the user suppressed. One whose alternate
            // representation or instance bodies cannot be placed is left out as before, and so
            // is a deactivated definition, which the source removes from the product.
            let stated = row.get("suppressed").and_then(Value::as_bool) == Some(true);
            let suppressed = placed.get("suppressed").and_then(Value::as_bool) == Some(true);
            let by_override = suppressed && !stated;
            let unplaceable = placed.get("representation").is_some()
                || prototype
                    .get("instance_bodies")
                    .and_then(Value::as_array)
                    .is_some_and(|rows| !rows.is_empty());
            if by_override
                || (suppressed && unplaceable)
                || prototype.get("active").and_then(Value::as_bool) == Some(false)
            {
                if source_node {
                    self.path.pop();
                }
                self.account_for(prototype, 0);
                continue;
            }
            if unplaceable {
                return Err(
                    "alternate representations and instance bodies are not supported yet"
                        .to_owned(),
                );
            }
            let id = self.fresh_id(original);
            let storage = prototype.get("storage").unwrap_or(&Value::Null);
            let external = storage.get("kind").and_then(Value::as_str) == Some("external");
            let fields = placed
                .as_object_mut()
                .ok_or("a component placement has no fields")?;
            fields
                .entry("transform".to_owned())
                .or_insert_with(|| json!(IDENTITY));
            fields.insert("id".to_owned(), json!(id));
            fields.insert("ordinal".to_owned(), json!(self.out.len()));
            fields.insert("parent".to_owned(), self.parent_value(parent));
            fields.insert(
                "prototype".to_owned(),
                if external {
                    json!({"kind":"external", "document":storage, "object":storage.get("object")})
                } else {
                    json!({"kind":"local", "definition":row.get("prototype"), "document":storage.get("document")})
                },
            );
            self.out.push(placed);
            let mut nested = overrides.to_vec();
            if self
                .overrides
                .get(original)
                .is_some_and(|entries| !entries.is_empty())
            {
                nested.push((original, self.path.len()));
            }
            self.level += 1;
            self.visit(prototype, Some(&id), &nested)?;
            self.level -= 1;
            if source_node {
                self.path.pop();
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solidworks_file_root_keeps_its_name_and_children_without_changing_cadir() {
        use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
        use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
        let definition = |id| ProductDefinitionId::mint(id).unwrap();
        let occurrence = |key| OccurrenceId::mint(format!("test:model:occurrence#{key}")).unwrap();
        let mut model = crate::read(
            Format::SolidWorksPart,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/solidworks_cube.sldprt"),
            &mut Report::default(),
        )
        .unwrap();
        let mut root = ProductDefinition::root(
            definition("sldprt:assembly:model#synthetic"),
            ExternalDocument::default(),
        );
        root.source_name = Some("Root component".to_owned());
        root.label = Some("Root label".to_owned());
        root.native_ref = Some("sldprt:assembly:model#synthetic".to_owned());
        root.occurrences = vec![occurrence("first"), occurrence("second")];
        let mut part =
            ProductDefinition::new(definition("test:model:product#cube"), Storage::Embedded);
        part.bodies = model
            .ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect();
        model.ir.model.product_definitions = vec![root, part.clone()];
        model.ir.model.occurrences = ["first", "second"]
            .into_iter()
            .map(|key| Occurrence::new(occurrence(key), part.id.clone(), None))
            .collect();
        let before = model.ir.clone();
        let assembly = local_assembly(&model).unwrap().unwrap();
        assert_eq!(assembly.placements.len(), 1);
        assert_eq!(
            assembly.placements[0].name.as_deref(),
            Some("Root component")
        );
        assert_eq!(assembly.placements[0].transform, IDENTITY);
        let root = &assembly.parts[assembly.placements[0].part];
        assert_eq!(root.name, "Root label");
        let children = root.assembly.as_ref().unwrap();
        assert_eq!(children.placements.len(), 2);
        assert_eq!(children.parts.len(), 1);
        assert_eq!(children.parts[0].ir.geometry, model.ir.geometry);
        assert_eq!(model.ir, before);
    }

    #[test]
    fn shared_definitions_expand_in_source_order_and_keep_external_identity() {
        let mut model = json!({"product_definitions":[
            {"id":"root", "storage":{"kind":"file", "document":{}}, "occurrences":["second","first"]},
            {"id":"sub", "storage":{"kind":"embedded"}, "occurrences":["child"]},
            {"id":"part", "storage":{"kind":"external", "path":"part.sldprt", "object":"Default"}, "occurrences":[]}
        ], "occurrences":[
            {"id":"first", "prototype":"sub"},
            {"id":"second", "prototype":"sub"},
            {"id":"child", "prototype":"part"}
        ]});
        project(&mut model).unwrap();
        let rows = model["occurrences"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        assert_eq!(rows[0]["id"], "second");
        assert_eq!(rows[1]["parent"]["occurrence"], "second");
        assert_eq!(rows[2]["id"], "first");
        assert_eq!(rows[3]["parent"]["occurrence"], "first");
        assert_ne!(rows[1]["id"], rows[3]["id"]);
        for index in [1, 3] {
            assert_eq!(rows[index]["prototype"]["document"]["path"], "part.sldprt");
            assert_eq!(rows[index]["prototype"]["object"], "Default");
        }
    }

    #[test]
    fn nested_overrides_follow_each_insertion_path() {
        let at = |x| json!([[1., 0., 0., x], [0., 1., 0., 0.], [0., 0., 1., 0.]]);
        let path = |ids: &[&str]| {
            ids.iter()
                .map(|id| json!({"occurrence":{"kind":"occurrence","value":id}}))
                .collect::<Vec<_>>()
        };
        let mut model = json!({"product_definitions":[
            {"id":"root", "storage":{"kind":"file"}, "occurrences":["a","b"]},
            {"id":"sub", "storage":{"kind":"embedded"}, "occurrences":["child"]},
            {"id":"nested", "storage":{"kind":"embedded"}, "occurrences":["leaf","hidden"]},
            {"id":"part", "storage":{"kind":"embedded"}, "occurrences":[]}
        ], "occurrences":[
            {"id":"a", "prototype":"sub", "transform":at(10.)},
            {"id":"b", "prototype":"sub", "transform":at(20.), "overrides":[
                {"path":path(&["child"]), "transform":at(4.)},
                {"path":path(&["child","leaf"]), "transform":at(7.), "visible":false},
                {"path":path(&["child","hidden"]), "suppressed":true}
            ]},
            {"id":"child", "prototype":"nested", "transform":at(2.)},
            {"id":"leaf", "prototype":"part", "transform":at(3.)},
            {"id":"hidden", "prototype":"part", "transform":at(1.)}
        ]});
        project(&mut model).unwrap();
        let rows = model["occurrences"].as_array().unwrap();
        assert_eq!(rows.len(), 7);
        let mut world = BTreeMap::new();
        for row in rows {
            let parent = row
                .pointer("/parent/occurrence")
                .and_then(Value::as_str)
                .and_then(|id| world.get(id))
                .copied()
                .unwrap_or(IDENTITY);
            world.insert(
                row["id"].as_str().unwrap().to_owned(),
                crate::placement::compose(&parent, &transform(row).unwrap()),
            );
        }
        assert_eq!(world["leaf"][0][3], 15.);
        assert_eq!(rows[6]["visible"], false);
        assert_eq!(world[rows[6]["id"].as_str().unwrap()][0][3], 31.);
        assert!(rows[2].get("visible").is_none());
        assert_eq!(rows[5]["parent"]["occurrence"], "b");
    }

    #[test]
    fn shared_products_slice_each_overridden_layout_without_moving_geometry() {
        use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
        use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
        let definition =
            |key| ProductDefinitionId::mint(format!("test:model:product#{key}")).unwrap();
        let occurrence = |key| OccurrenceId::mint(format!("test:model:occurrence#{key}")).unwrap();
        let at = |x| {
            serde_json::from_value(json!([[1., 0., 0., x], [0., 1., 0., 0.], [0., 0., 1., 0.]]))
                .unwrap()
        };
        let mut model = crate::read(
            Format::InventorPart,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/synthetic_cube.ipt"),
            &mut Report::default(),
        )
        .unwrap();
        let geometry = serde_json::to_value(&model.ir.geometry).unwrap();
        let mut root = ProductDefinition::root(definition("root"), ExternalDocument::default());
        root.occurrences = vec![occurrence("a"), occurrence("b")];
        let mut sub = ProductDefinition::new(definition("sub"), Storage::Embedded);
        sub.label = Some("Shared".to_owned());
        sub.occurrences = vec![occurrence("child")];
        let mut part = ProductDefinition::new(definition("part"), Storage::Embedded);
        part.label = Some("Cube".to_owned());
        part.bodies = model
            .ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect();
        model.ir.model.product_definitions = vec![root, sub, part];
        let mut second = Occurrence::new(occurrence("b"), definition("sub"), Some(at(20.)));
        second.overrides = serde_json::from_value(json!([{
            "path":[{"occurrence":{"kind":"occurrence","value":occurrence("child")}}],
            "transform":at(7.), "visible":false
        }]))
        .unwrap();
        model.ir.model.occurrences = vec![
            Occurrence::new(occurrence("a"), definition("sub"), Some(at(10.))),
            second,
            Occurrence::new(occurrence("child"), definition("part"), Some(at(2.))),
        ];
        model.ir.model.assembly_joints = vec![cadmpeg_ir::products::AssemblyJoint::grounded(
            cadmpeg_ir::products::JointId::mint("test:model:joint#second-child").unwrap(),
            cadmpeg_ir::products::JointConnector {
                operand: cadmpeg_ir::products::JointOperand::nested_occurrence(
                    occurrence("b"),
                    vec!["child".to_owned()],
                    "b",
                    Vec::new(),
                ),
                frame: cadmpeg_ir::transform::Transform::identity(),
                detached: false,
            },
            None,
        )];
        for (offset, base) in [(7., Some(at(2.))), (2., Some(at(2.))), (7., None)] {
            model.ir.model.occurrences[1].overrides[0].transform = Some(at(offset));
            model.ir.model.occurrences[2].transform = base;
            let assembly = local_assembly(&model).unwrap().unwrap();
            assert_eq!(assembly.placements.len(), 2);
            assert_eq!(assembly.parts.len(), 2);
            assert_eq!(assembly.joints.len(), 1);
            assert_eq!(assembly.joints[0].b, vec![1, 0]);
            let mut worlds = Vec::new();
            for placement in &assembly.placements {
                let nested = assembly.parts[placement.part].assembly.as_ref().unwrap();
                assert_eq!(nested.placements.len(), 1);
                let child = &nested.placements[0];
                assert_eq!(nested.parts[child.part].ir.geometry.bodies.len(), 1);
                worlds.push((
                    crate::placement::compose(&placement.transform, &child.transform)[0][3],
                    child.hidden,
                ));
            }
            assert_eq!(
                worlds,
                [
                    (if base.is_some() { 12. } else { 10. }, false),
                    (20. + offset, true)
                ]
            );
        }
        assert_eq!(serde_json::to_value(&model.ir.geometry).unwrap(), geometry);
        assert_eq!(model.ir.model.occurrences.len(), 3);
    }

    #[test]
    fn nested_joint_paths_select_the_requested_shared_insertion() {
        let at = |x| json!([[1., 0., 0., x], [0., 1., 0., 0.], [0., 0., 1., 0.]]);
        let mut model = json!({"product_definitions":[
            {"id":"root", "storage":{"kind":"file"}, "occurrences":["a"]},
            {"id":"sub", "storage":{"kind":"embedded"}, "occurrences":["left","right"]},
            {"id":"group", "storage":{"kind":"embedded"}, "occurrences":["leaf"]},
            {"id":"part", "storage":{"kind":"embedded"}, "occurrences":[]}
        ], "occurrences":[
            {"id":"a", "prototype":"sub", "transform":at(10.)},
            {"id":"left", "prototype":"group", "transform":at(2.)},
            {"id":"right", "prototype":"group", "transform":at(3.)},
            {"id":"leaf", "prototype":"part", "transform":at(4.)}
        ]});
        project(&mut model).unwrap();
        let mut positions = BTreeMap::new();
        for row in model["occurrences"].as_array().unwrap() {
            let outer = row
                .pointer("/parent/occurrence")
                .and_then(Value::as_str)
                .and_then(|id| positions.get(id))
                .copied()
                .unwrap_or(IDENTITY);
            positions.insert(
                row["id"].as_str().unwrap().to_owned(),
                compose(&outer, &transform(row).unwrap()),
            );
        }
        let document = json!({"model":model});
        let skeleton =
            super::super::position_skeleton::Skeleton::build(&document, &positions).unwrap();
        let worlds = positions
            .iter()
            .map(|(id, pose)| (id.as_str(), *pose))
            .collect();
        for (group, path, x) in [("left", vec![0, 0, 0], 16.), ("right", vec![0, 1, 0], 17.)] {
            let operand = json!({"container":{"kind":"occurrence", "occurrence":"a", "occurrence_path":[group,"leaf"]},
                "object":"a", "subelements":[]});
            let (located, outer, selected) =
                side_path(&operand, &skeleton.paths, &worlds, &BTreeMap::new()).unwrap();
            assert_eq!(located.own, path);
            assert_eq!(outer[0][3], 10.);
            assert_eq!(selected[0][3], x);
        }
    }

    #[test]
    fn file_root_keeps_explicitly_owned_history_and_sketches() {
        use cadmpeg_ir::ids::ProductDefinitionId;
        use cadmpeg_ir::products::{ExternalDocument, ProductDefinition};
        let mut model = crate::read(
            Format::FusionArchive,
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/design_sketch_constraints.f3d"),
            &mut Report::default(),
        )
        .unwrap();
        assert!(!model.ir.model.features.is_empty());
        assert!(!model.ir.model.sketches.is_empty());
        let mut root = ProductDefinition::root(
            ProductDefinitionId::mint("test:model:product#root").unwrap(),
            ExternalDocument::default(),
        );
        root.bodies = model
            .ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect();
        root.sketches = model
            .ir
            .model
            .sketches
            .iter()
            .map(|sketch| sketch.id.clone())
            .collect();
        for feature in &mut model.ir.model.features {
            feature.product_definition = Some(root.id.clone());
        }
        for parameter in &mut model.ir.model.parameters {
            parameter.product_definition = Some(root.id.clone());
        }
        model.ir.model.product_definitions = vec![root];
        let document = serde_json::to_value(&model.ir).unwrap();
        let bodies = model
            .ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.as_str())
            .collect::<Vec<_>>();
        let part = only(
            &model,
            &document,
            &bodies,
            HistoryOwner::Root,
            "Root".to_owned(),
        )
        .unwrap();
        assert_eq!(part.ir.model.features.len(), model.ir.model.features.len());
        assert_eq!(part.ir.model.sketches.len(), model.ir.model.sketches.len());
        assert_eq!(
            part.ir.model.parameters.len(),
            model.ir.model.parameters.len()
        );
    }

    #[test]
    fn folded_file_components_keep_their_container_names_and_override_paths() {
        use cadmpeg_ir::ids::{OccurrenceId, ProductDefinitionId};
        use cadmpeg_ir::products::{ExternalDocument, Occurrence, ProductDefinition, Storage};
        let definition = |id| ProductDefinitionId::mint(id).unwrap();
        let occurrence = |key| OccurrenceId::mint(format!("test:model:occurrence#{key}")).unwrap();
        let at = |x| {
            serde_json::from_value(json!([[1., 0., 0., x], [0., 1., 0., 0.], [0., 0., 1., 0.]]))
                .unwrap()
        };
        let mut model = crate::read(
            Format::FusionArchive,
            &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/generated_unit_cube.f3d"),
            &mut Report::default(),
        )
        .unwrap();
        let geometry = serde_json::to_value(&model.ir.geometry).unwrap();
        let mut root = ProductDefinition::root(
            definition("test:model:product#root"),
            ExternalDocument::default(),
        );
        root.occurrences = vec![occurrence("a"), occurrence("b")];
        let mut member = ProductDefinition::new(
            definition("f3d:xref/test/model:component#folded"),
            Storage::File {
                document: ExternalDocument::path("member.f3d"),
                source: model.ir.source.clone(),
            },
        );
        assert!(model.ir.source.is_some());
        member.label = Some("Member document".to_owned());
        member.source_name = Some("Root component".to_owned());
        member.occurrences = vec![
            occurrence("before"),
            occurrence("leaf"),
            occurrence("after"),
        ];
        let mut part =
            ProductDefinition::new(definition("test:model:product#part"), Storage::Embedded);
        part.label = Some("Cube".to_owned());
        part.bodies = model
            .ir
            .geometry
            .bodies
            .iter()
            .map(|body| body.id.clone())
            .collect();
        model.ir.model.product_definitions = vec![root, member, part];
        let mut second = Occurrence::new(
            occurrence("b"),
            definition("f3d:xref/test/model:component#folded"),
            Some(at(20.)),
        );
        second.overrides = serde_json::from_value(json!([{
            "path":[{"occurrence":{"kind":"occurrence","value":occurrence("leaf")}}], "transform":at(7.)
        }])).unwrap();
        let mut before = Occurrence::new(
            occurrence("before"),
            definition("test:model:product#part"),
            Some(at(-10.)),
        );
        before.native_ref = Some("f3d:xref/test/xref:reference#before".to_owned());
        let mut after = Occurrence::new(
            occurrence("after"),
            definition("test:model:product#part"),
            Some(at(20.)),
        );
        after.native_ref = Some("f3d:xref/test/xref:reference#after".to_owned());
        model.ir.model.occurrences = vec![
            Occurrence::new(
                occurrence("a"),
                definition("f3d:xref/test/model:component#folded"),
                Some(at(10.)),
            ),
            second,
            before,
            after,
            Occurrence::new(
                occurrence("leaf"),
                definition("test:model:product#part"),
                Some(at(2.)),
            ),
        ];
        let assembly = local_assembly(&model).unwrap().unwrap();
        let mut worlds = Vec::new();
        for (insertion, expected_name) in assembly
            .placements
            .iter()
            .zip(["Root component", "Root component (2)"])
        {
            let file = &assembly.parts[insertion.part];
            assert!(file.name.starts_with("Member document"));
            let file = file.assembly.as_ref().unwrap();
            assert_eq!(file.placements.len(), 3);
            assert_eq!(file.placements[0].transform[0][3], -10.);
            assert_eq!(file.placements[2].transform[0][3], 20.);
            let placed_root = &file.placements[1];
            assert_eq!(placed_root.transform, IDENTITY);
            let component = &file.parts[placed_root.part];
            assert_eq!(component.name, expected_name);
            let component = component.assembly.as_ref().unwrap();
            assert_eq!(component.placements.len(), 1);
            let child = &component.placements[0];
            worlds.push(crate::placement::compose(&insertion.transform, &child.transform)[0][3]);
        }
        assert_eq!(worlds, [12., 27.]);
        assert_eq!(serde_json::to_value(&model.ir.geometry).unwrap(), geometry);
        assert_eq!(model.ir.model.product_definitions.len(), 3);
        assert_eq!(model.ir.model.occurrences.len(), 5);
    }

    #[test]
    fn cyclic_products_return_an_error() {
        let mut model = json!({"product_definitions":[
            {"id":"root", "storage":{"kind":"file", "document":{}}, "occurrences":["self"]}
        ], "occurrences":[{"id":"self", "prototype":"root"}]});
        assert!(project(&mut model).unwrap_err().contains("inside itself"));
    }

    #[test]
    fn rows_no_root_reaches_are_refused_but_removed_bookkeeping_is_not() {
        // Bookkeeping removed before the tree is built leaves a child id with no row.
        let mut model = json!({"product_definitions":[
            {"id":"root", "storage":{"kind":"file", "document":{}}, "occurrences":["a", "gone"]},
            {"id":"part", "storage":{"kind":"embedded"}, "occurrences":[]}
        ], "occurrences":[{"id":"a", "prototype":"part"}]});
        project(&mut model).unwrap();
        assert_eq!(model["occurrences"].as_array().unwrap().len(), 1);
        // A row placed inside a component nothing places is not silently dropped.
        let mut model = json!({"product_definitions":[
            {"id":"root", "storage":{"kind":"file", "document":{}}, "occurrences":["a"]},
            {"id":"part", "storage":{"kind":"embedded"}, "occurrences":[]},
            {"id":"lost", "storage":{"kind":"embedded"}, "occurrences":["orphan"]}
        ], "occurrences":[{"id":"a", "prototype":"part"}, {"id":"orphan", "prototype":"part"}]});
        assert!(project(&mut model)
            .unwrap_err()
            .contains("the design does not place"));
        // What a suppressed component holds is accounted for, not refused.
        let mut model = json!({"product_definitions":[
            {"id":"root", "storage":{"kind":"file", "document":{}}, "occurrences":["a"]},
            {"id":"sub", "storage":{"kind":"embedded"}, "occurrences":["inner"]},
            {"id":"part", "storage":{"kind":"embedded"}, "occurrences":[]}
        ], "occurrences":[
            {"id":"a", "prototype":"sub", "suppressed":true, "visible":false, "overrides":[]},
            {"id":"inner", "prototype":"part"}
        ]});
        model["occurrences"][0]["representation"] = json!("x");
        project(&mut model).unwrap();
    }

    #[test]
    fn a_dangling_prototype_is_placed_as_an_unresolved_external_component() {
        let mut model = json!({"product_definitions":[
            {"id":"root", "storage":{"kind":"file", "document":{}}, "occurrences":["a"]}
        ], "occurrences":[{"id":"a", "name":"lost link", "prototype":"absent/unresolved"}]});
        project(&mut model).unwrap();
        let rows = model["occurrences"].as_array().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["prototype"]["kind"], "external");
        assert_eq!(rows[0]["prototype"]["definition"], "absent/unresolved");
        assert_eq!(rows[0]["parent"]["kind"], "root");
    }
}
