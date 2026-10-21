//! Assemblies, as SolidCraft components and occurrences.
//!
//! Each part becomes a component holding the part's feature history, when it maps and checks
//! out as a single part's would, followed by its geometry as a suppressed Import; else its
//! geometry as a live Import. Each sub-assembly
//! becomes a component whose own parts are components beneath it. Each placement becomes an
//! occurrence of the placed component inside the component of the assembly that places it
//! (the root for the top assembly), at the placement's transform.

use std::collections::{BTreeMap, BTreeSet};

use dutchman_core::Assembly;
use dutchman_core::{Check, Error, FeatureOutcome, Mode, Model, Outcome, Report, Severity};
use serde_json::{json, Value};

use crate::container::Container;
use crate::document::{column_major, component_name, feature_name, import_feature, Live, ORIGINAL};
use crate::{history, step, verify, SolidCraftWriter, FORMAT, HISTORY};

/// Writes `assembly`, the assembly `model` places, as a SolidCraft design container, and
/// records what became of its parts' histories.
pub(crate) fn write(
    writer: &SolidCraftWriter,
    model: &Model,
    assembly: &Assembly,
    report: &mut Report,
) -> Result<Vec<u8>, Error> {
    let mut builder = Builder {
        writer,
        container: Container::empty(),
        report,
        next_id: 1,
        components: Vec::new(),
        occurrences: Vec::new(),
        features: Vec::new(),
        params: Vec::new(),
        taken: BTreeSet::new(),
        looks: BTreeMap::new(),
        names: Vec::new(),
        placed: BTreeMap::new(),
        histories: Vec::new(),
        outcomes: Vec::new(),
    };
    builder.assembly(assembly, 0)?;
    if builder.features.is_empty() {
        return Err(Error::Encode(
            "none of the assembly's parts has geometry SolidCraft can import".to_owned(),
        ));
    }
    let mut doc = json!({
        "format": FORMAT,
        "name": model.name,
        "units": "mm",
        "params": builder.params,
        "next_id": builder.next_id,
        "components": builder.components,
        "occurrences": builder.occurrences,
        "features": builder.features,
    });
    if let (false, Some(fields)) = (builder.looks.is_empty(), doc.as_object_mut()) {
        fields.insert("appearances".to_owned(), json!({"bodies": builder.looks}));
    }
    builder
        .report
        .set_outcome(outcome(&builder.histories, builder.outcomes));
    builder.container.finish(&doc)
}

/// What writing an assembly would do with its parts' histories, without restating geometry,
/// writing anything or running SolidCraft.
pub(crate) fn plan(writer: &SolidCraftWriter, assembly: &Assembly, report: &mut Report) -> Outcome {
    let mut checks = Vec::new();
    let mut features = Vec::new();
    plan_parts(writer, assembly, report, &mut checks, &mut features);
    outcome(&checks, features)
}

fn plan_parts(
    writer: &SolidCraftWriter,
    assembly: &Assembly,
    report: &mut Report,
    checks: &mut Vec<Check>,
    features: &mut Vec<FeatureOutcome>,
) {
    for part in &assembly.parts {
        if let Some(sub) = &part.assembly {
            plan_parts(writer, sub, report, checks, features);
            continue;
        }
        if writer.import_only {
            continue;
        }
        let file = part.source_file.as_deref().unwrap_or(&part.name);
        let mut part_report = Report::default();
        let mapping = history::map(part, &mut part_report);
        features.extend(mapping.features.iter().cloned().map(|mut feature| {
            feature.name = Some(format!("{file}: {}", feature.name.unwrap_or_default()));
            feature
        }));
        match mapping.result {
            Ok(_) => {
                report.append_prefixed(part_report, file);
                checks.push(Check::Unverified(
                    "not checked: inspect writes nothing".to_owned(),
                ));
            }
            Err(reason) if !mapping.features.is_empty() => {
                report.push(
                    Severity::Info,
                    HISTORY,
                    format!("{file}: flattened: {reason}"),
                );
                checks.push(Check::Flattened(reason));
            }
            Err(_) => {}
        }
    }
}

/// What became of the parts' histories, as one outcome for the design.
fn outcome(histories: &[Check], features: Vec<FeatureOutcome>) -> Outcome {
    let written: Vec<&Check> = histories
        .iter()
        .filter(|check| matches!(check, Check::Verified | Check::Unverified(_)))
        .collect();
    let check = if written.is_empty() {
        Check::NotApplicable(
            "no part's history was written; the parts are placed as geometry".to_owned(),
        )
    } else if written.iter().all(|check| **check == Check::Verified) {
        Check::Verified
    } else {
        Check::Unverified("not every part's history was checked in SolidCraft".to_owned())
    };
    Outcome {
        mode: if written.is_empty() {
            Mode::Import
        } else {
            Mode::History
        },
        check,
        features,
    }
}

/// The design's components, occurrences and Import features as they are built.
struct Builder<'r> {
    writer: &'r SolidCraftWriter,
    container: Container,
    report: &'r mut Report,
    next_id: u64,
    components: Vec<Value>,
    occurrences: Vec<Value>,
    features: Vec<Value>,
    /// The design's parameters, from every part whose history is written.
    params: Vec<Value>,
    /// Parameter names given so far.
    taken: BTreeSet<String>,
    /// Looks of the bodies the parts' histories make, by body name.
    looks: BTreeMap<String, Value>,
    /// What became of each part's history.
    histories: Vec<Check>,
    /// Every part's features, for the outcome.
    outcomes: Vec<FeatureOutcome>,
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
                let replayed = self.history(part, &step, component, &name)?;
                let id = self.id();
                let (label, suppressed) = if replayed {
                    (ORIGINAL.to_owned(), true)
                } else {
                    (feature_name(part), false)
                };
                self.features.push(import_feature(
                    part,
                    id,
                    &label,
                    Some(component),
                    suppressed,
                    &blob,
                ));
            }
            (None, None) => {}
        }
        Ok(Some((component, name)))
    }

    /// Adds `part`'s feature history to component `component` when it maps and its replay
    /// checks out (or cannot be checked), as for a single part; says whether it did.
    fn history(
        &mut self,
        part: &Model,
        step: &str,
        component: u64,
        name: &str,
    ) -> Result<bool, Error> {
        let file = part.source_file.as_deref().unwrap_or(&part.name).to_owned();
        if self.writer.import_only {
            return Ok(false);
        }
        let mut part_report = Report::default();
        let mapping = history::map(part, &mut part_report);
        self.outcomes
            .extend(mapping.features.iter().cloned().map(|mut feature| {
                feature.name = Some(format!("{file}: {}", feature.name.unwrap_or_default()));
                feature
            }));
        let mut history = match mapping.result {
            Ok(history) => history,
            Err(reason) => {
                if !mapping.features.is_empty() {
                    self.report.push(
                        Severity::Info,
                        HISTORY,
                        format!("{file}: flattened: {reason}"),
                    );
                    self.histories.push(Check::Flattened(reason));
                }
                return Ok(false);
            }
        };
        let written = self
            .writer
            .history_design(part, &history, step, Live::History)?;
        let summary = format!("{} of {} features mapped", history.mapped, history.total);
        let check = match self.writer.check(part, &history, step, &written)? {
            verify::Outcome::Matches => {
                self.report.push(
                    Severity::Info,
                    HISTORY,
                    format!("{file}: {summary}; verified against the original in SolidCraft"),
                );
                Check::Verified
            }
            verify::Outcome::Unverified(why) => {
                self.report.push(
                    Severity::Info,
                    HISTORY,
                    format!("{file}: {summary}; unverified: {why}"),
                );
                Check::Unverified(why)
            }
            verify::Outcome::Differs(why) => {
                let reason = format!("replay differs ({why})");
                self.report.push(
                    Severity::Info,
                    HISTORY,
                    format!("{file}: flattened: {reason}"),
                );
                self.histories.push(Check::Flattened(reason));
                return Ok(false);
            }
        };
        self.report.append_prefixed(part_report, &file);
        history.relocate(self.next_id - 1, component, &prefix(name), &mut self.taken);
        self.next_id = history.next_id;
        self.features.append(&mut history.features);
        self.params.append(&mut history.params);
        self.looks.append(&mut history.looks);
        if !history.face_looks.is_empty() {
            // A face look's point is in the design's frame, which a part placed by occurrences
            // does not have one of.
            self.report.push(
                Severity::Info,
                "appearance",
                format!(
                    "{file}: {} face colours keep their body's colour inside an assembly",
                    history.face_looks.len()
                ),
            );
        }
        self.histories.push(check);
        Ok(true)
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

/// A component name as a parameter-name prefix: letters, digits and underscores.
fn prefix(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if cleaned.starts_with(|c: char| c.is_ascii_alphabetic()) {
        cleaned
    } else {
        format!("c_{cleaned}")
    }
}
