//! Assemblies, as SolidCraft components and occurrences.
//!
//! Each part becomes a component holding the part's feature history, when it maps and checks
//! out as a single part's would, followed by its geometry as a suppressed Import; else its
//! geometry as a live Import. Each sub-assembly
//! becomes a component whose own parts are components beneath it. Each placement becomes an
//! occurrence of the placed component inside the component of the assembly that places it
//! (the root for the top assembly), at the placement's transform.

use std::collections::{BTreeMap, BTreeSet};

use dutchman_core::placement::{compose, IDENTITY};
use dutchman_core::{Assembly, Joint, Motion};
use dutchman_core::{Check, Error, FeatureOutcome, Mode, Model, Outcome, Report, Severity, Stage};
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
        face_looks: Vec::new(),
        bodies: BTreeSet::new(),
        names: Vec::new(),
        placed: BTreeMap::new(),
        histories: Vec::new(),
        outcomes: Vec::new(),
        inner: BTreeMap::new(),
        placements: BTreeMap::new(),
    };
    let top = builder.assembly(assembly, 0)?;
    let joints = builder.joints(&assembly.joints, &top);
    if builder.features.is_empty() {
        return Err(Error::Encode(
            "none of the assembly's parts has geometry SolidCraft can import".to_owned(),
        ));
    }
    history::base::store(&mut builder.features, &mut builder.container)?;
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
    let mut appearances = serde_json::Map::new();
    if !builder.looks.is_empty() {
        appearances.insert("bodies".to_owned(), json!(builder.looks));
    }
    if !builder.face_looks.is_empty() {
        appearances.insert("faces".to_owned(), json!(builder.face_looks));
    }
    if let (false, Some(fields)) = (appearances.is_empty(), doc.as_object_mut()) {
        fields.insert("appearances".to_owned(), Value::Object(appearances));
    }
    if let (false, Some(fields)) = (joints.is_empty(), doc.as_object_mut()) {
        fields.insert("assembly".to_owned(), json!({"joints": joints}));
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
    /// Looks of those bodies' faces, each by a point in its part's own frame, which SolidCraft
    /// applies to every occurrence of the part.
    face_looks: Vec<Value>,
    /// Body names the parts' histories use so far, kept unique across parts.
    bodies: BTreeSet<String>,
    /// What became of each part's history.
    histories: Vec<Check>,
    /// Every part's features, for the outcome.
    outcomes: Vec<FeatureOutcome>,
    /// Component names given so far, kept unique.
    names: Vec<String>,
    /// Occurrences made so far of each component, for naming the next.
    placed: BTreeMap<u64, usize>,
    /// The occurrence each placement of a sub-assembly's became, by the sub-assembly's
    /// component (`None` for a part left out).
    inner: BTreeMap<u64, Vec<Option<u64>>>,
    /// Each occurrence's component and placement in its parent.
    placements: BTreeMap<u64, (u64, [[f64; 4]; 3])>,
}

impl Builder<'_> {
    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Adds `assembly`'s parts as components beneath component `parent` (0: the root), and its
    /// placements as occurrences inside it; returns the occurrence each placement became.
    fn assembly(&mut self, assembly: &Assembly, parent: u64) -> Result<Vec<Option<u64>>, Error> {
        let mut components = Vec::with_capacity(assembly.parts.len());
        for part in &assembly.parts {
            components.push(self.part(part, parent)?);
        }
        let mut occurrences = Vec::with_capacity(assembly.placements.len());
        for placement in &assembly.placements {
            let Some(Some((component, name))) = components.get(placement.part).cloned() else {
                occurrences.push(None);
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
            self.placements.insert(id, (component, placement.transform));
            occurrences.push(Some(id));
        }
        Ok(occurrences)
    }

    /// The joints, as SolidCraft rigid joints written as built: each holds its two
    /// occurrences where they are placed now. Side A's joint frame is A's own origin; side B's
    /// is the same place in the world, given in B's component.
    fn joints(&mut self, joints: &[Joint], top: &[Option<u64>]) -> Vec<Value> {
        let mut out = Vec::new();
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for joint in joints {
            let (Some((a, _)), Some((b, _))) =
                (self.resolve(&joint.a, top), self.resolve(&joint.b, top))
            else {
                self.report.push(
                    Severity::Dropped,
                    "joints",
                    "a joint is left out: a part it joins was left out".to_owned(),
                );
                continue;
            };
            // Each frame is in its own part, and the parts are placed where the joint puts
            // them (B's frame facing A's, moved by the joint's value), so solving moves nothing.
            let (kind, values, limits) = match &joint.motion {
                Motion::Rigid => ("rigid", json!([]), json!([])),
                Motion::Revolute { angle, limits } => (
                    "revolute",
                    json!([angle]),
                    json!([limits.map(|(min, max)| [min, max])]),
                ),
                Motion::Slider { distance } => ("slider", json!([distance]), json!([null])),
            };
            let (a_frame, b_frame) = (joint.frame_a, joint.frame_b);
            let id = self.id();
            *counts.entry(kind).or_insert(0) += 1;
            let mut row = json!({
                "id": id,
                "name": format!("{} {}", title(kind), counts.get(kind).copied().unwrap_or(1)),
                "kind": kind,
                "a": {"occurrence": a, "snap": snap(&a_frame)},
                "b": {"occurrence": b, "snap": snap(&b_frame)},
                "values": values,
                "limits": limits,
                "flip": false,
                "offset": 0.0,
                "angle": 0.0,
            });
            if let (true, Some(fields)) = (joint.suppressed, row.as_object_mut()) {
                fields.insert("suppressed".to_owned(), json!(true));
            }
            out.push(row);
        }
        self.report.set_joints(
            counts
                .iter()
                .map(|(kind, n)| ((*kind).to_owned(), u64::try_from(*n).unwrap_or(u64::MAX)))
                .collect(),
        );
        if !out.is_empty() {
            let kinds: Vec<String> = counts
                .iter()
                .map(|(kind, n)| format!("{n} {kind}"))
                .collect();
            self.report.push(
                Severity::Info,
                "joints",
                format!(
                    "{} joint(s) written as built ({}), holding their parts as placed",
                    out.len(),
                    kinds.join(", ")
                ),
            );
        }
        out
    }

    /// The occurrence a placement path leads to (0, the design itself, for an empty path) and
    /// its placement in the world.
    fn resolve(&self, path: &[usize], top: &[Option<u64>]) -> Option<(u64, [[f64; 4]; 3])> {
        let mut occurrence = 0;
        let mut world = IDENTITY;
        let mut level = top;
        for (depth, index) in path.iter().enumerate() {
            occurrence = (*level.get(*index)?)?;
            let (component, placement) = self.placements.get(&occurrence)?;
            world = compose(&world, placement);
            if depth + 1 < path.len() {
                level = self.inner.get(component)?;
            }
        }
        Some((occurrence, world))
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
            (Some(sub), _) => {
                let occurrences = self.assembly(sub, component)?;
                self.inner.insert(component, occurrences);
            }
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
        let restated = mapping
            .result
            .and_then(|mut history| history::base::restate(&mut history, part).map(|()| history));
        let mut history = match restated {
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
                self.report.push_at(
                    Stage::SolidCraft,
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
        history.rename_bodies(name, &mut self.bodies);
        self.next_id = history.next_id;
        self.features.append(&mut history.features);
        self.params.append(&mut history.params);
        self.looks.append(&mut history.looks);
        self.face_looks.append(&mut history.face_looks);
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

/// A joint kind's name for the joints' own names.
fn title(kind: &str) -> &'static str {
    match kind {
        "revolute" => "Revolute",
        "slider" => "Slider",
        _ => "Rigid",
    }
}

/// A SolidCraft joint origin at a placement: its origin, and its z and x axes.
fn snap(frame: &[[f64; 4]; 3]) -> Value {
    let column = |c: usize| -> [f64; 3] {
        [0, 1, 2].map(|r| {
            frame
                .get(r)
                .and_then(|row| row.get(c))
                .copied()
                .unwrap_or(0.0)
        })
    };
    json!({"type": "frame", "origin": column(3), "z": column(2), "x": column(0)})
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
