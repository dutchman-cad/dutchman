//! Assemblies, as SolidCraft components and occurrences.
//!
//! Each part becomes a component holding the part's geometry as an Import; each sub-assembly
//! becomes a component whose own parts are components beneath it. Each placement becomes an
//! occurrence of the placed component inside the component of the assembly that places it
//! (the root for the top assembly), at the placement's transform.

use std::collections::BTreeMap;

use dutchman_core::{Assembly, Error, Model, Report, Severity};
use serde_json::{json, Value};

use crate::container::Container;
use crate::document::{column_major, component_name, feature_name, import_feature};
use crate::{step, FORMAT};

/// Writes `assembly`, the assembly `model` places, as a SolidCraft design container.
pub(crate) fn write(
    model: &Model,
    assembly: &Assembly,
    report: &mut Report,
) -> Result<Vec<u8>, Error> {
    let mut builder = Builder {
        container: Container::empty(),
        report,
        next_id: 1,
        components: Vec::new(),
        occurrences: Vec::new(),
        features: Vec::new(),
        names: Vec::new(),
        placed: BTreeMap::new(),
    };
    builder.assembly(assembly, 0)?;
    if builder.features.is_empty() {
        return Err(Error::Encode(
            "none of the assembly's parts has geometry SolidCraft can import".to_owned(),
        ));
    }
    let doc = json!({
        "format": FORMAT,
        "name": model.name,
        "units": "mm",
        "params": [],
        "next_id": builder.next_id,
        "components": builder.components,
        "occurrences": builder.occurrences,
        "features": builder.features,
    });
    builder.container.finish(&doc)
}

/// The design's components, occurrences and Import features as they are built.
struct Builder<'r> {
    container: Container,
    report: &'r mut Report,
    next_id: u64,
    components: Vec<Value>,
    occurrences: Vec<Value>,
    features: Vec<Value>,
    /// Component names given so far, kept unique.
    names: Vec<String>,
    /// Occurrences made so far of each component, for naming the next.
    placed: BTreeMap<u64, usize>,
}

impl Builder<'_> {
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Adds `assembly`'s parts as components beneath component `parent` (0: the root), and its
    /// placements as occurrences inside it.
    fn assembly(&mut self, assembly: &Assembly, parent: u64) -> Result<(), Error> {
        let mut components = Vec::with_capacity(assembly.parts.len());
        for part in &assembly.parts {
            components.push(self.part(part, parent)?);
        }
        for placement in &assembly.placements {
            let Some(Some((component, name))) = components.get(placement.part).cloned() else {
                continue;
            };
            let count = self.placed.entry(component).or_insert(0);
            *count += 1;
            let label = placement
                .name
                .clone()
                .unwrap_or_else(|| format!("{name}:{count}"));
            let id = self.id();
            self.occurrences.push(json!({
                "id": id,
                "name": label,
                "component": component,
                "parent": parent,
                "grounded": false,
                "transform": column_major(&placement.transform),
            }));
        }
        Ok(())
    }

    /// The component a part or sub-assembly becomes, with its name; `None` when it has nothing
    /// SolidCraft can show and is left out.
    fn part(&mut self, part: &Model, parent: u64) -> Result<Option<(u64, String)>, Error> {
        let step = match &part.assembly {
            Some(_) => None,
            None => match step::restate(part, self.report) {
                Ok(step) => Some(step),
                Err(err) => {
                    let file = part.source_file.as_deref().unwrap_or(&part.name);
                    self.report
                        .push(Severity::Dropped, file, format!("left out: {err}"));
                    return Ok(None);
                }
            },
        };
        let component = self.id();
        let name = self.unique(component_name(&part.name));
        self.components
            .push(json!({"id": component, "name": name, "parent": parent}));
        match (&part.assembly, step) {
            (Some(sub), _) => self.assembly(sub, component)?,
            (None, Some(step)) => {
                let blob = self.container.add_step(step.as_bytes())?;
                let id = self.id();
                self.features.push(import_feature(
                    part,
                    id,
                    &feature_name(part),
                    Some(component),
                    false,
                    &blob,
                ));
            }
            (None, None) => {}
        }
        Ok(Some((component, name)))
    }

    fn unique(&mut self, base: String) -> String {
        let mut name = base.clone();
        let mut n = 2;
        while self.names.contains(&name) {
            name = format!("{base} ({n})");
            n += 1;
        }
        self.names.push(name.clone());
        name
    }
}
