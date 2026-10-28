//! Assemblies, as SolidCraft components and occurrences.
//!
//! Each part becomes a component holding the part's feature history, when it maps and checks
//! out as a single part's would, followed by its geometry as a suppressed Import; else its
//! geometry as a live Import. Each sub-assembly
//! becomes a component whose own parts are components beneath it. Each placement becomes an
//! occurrence of the placed component inside the component of the assembly that places it
//! (the root for the top assembly), at the placement's transform.

use std::collections::{BTreeMap, BTreeSet};

/// Most hidden-occurrence paths written: a hidden occurrence deep inside components placed
/// many times can be reached many ways.
const MAX_HIDDEN_PATHS: usize = 4096;

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
    joint_notes(assembly, report);
    let mut builder = Builder {
        writer,
        container: Container::empty(),
        report,
        next_id: 1,
        components: Vec::new(),
        occurrences: Vec::new(),
        features: Vec::new(),
        params: Vec::new(),
        sheet_rules: Vec::new(),
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
        groups: Vec::new(),
        own_group: None,
        hidden: BTreeSet::new(),
    };
    let top = builder.assembly(assembly, 0)?;
    builder.hold(&assembly.held, &top);
    let joints = builder.joints(&assembly.joints, &top);
    if builder.features.is_empty() {
        return Err(Error::Encode(
            "none of the assembly's parts has geometry SolidCraft can import".to_owned(),
        ));
    }
    let start = builder.features.len();
    let hidden = builder.sketches(model);
    if builder.features.len() > start {
        let own = builder
            .own_group
            .and_then(|index| builder.groups.get_mut(index));
        match own {
            Some((_, range)) if range.end == start => range.end = builder.features.len(),
            _ => {
                let base = builder
                    .own_group
                    .and_then(|index| builder.groups.get(index))
                    .map(|(name, _)| name.clone())
                    .unwrap_or_else(|| component_name(&model.name));
                let name = builder.unique(base);
                builder.groups.push((name, start..builder.features.len()));
            }
        }
    }
    let groups = builder.timeline_groups();
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
    if let (false, Some(fields)) = (groups.is_empty(), doc.as_object_mut()) {
        fields.insert("timeline_groups".to_owned(), json!(groups));
    }
    if !builder.looks.is_empty() {
        appearances.insert("bodies".to_owned(), json!(builder.looks));
    }
    if !builder.face_looks.is_empty() {
        appearances.insert("faces".to_owned(), json!(builder.face_looks));
    }
    if let (false, Some(fields)) = (appearances.is_empty(), doc.as_object_mut()) {
        fields.insert("appearances".to_owned(), Value::Object(appearances));
    }
    if let (Some(first), Some(fields)) = (builder.sheet_rules.first(), doc.as_object_mut()) {
        fields.insert(
            "sheet".to_owned(),
            json!({"active": first.get("name"), "rules": builder.sheet_rules}),
        );
    }
    if let (false, Some(fields)) = (joints.is_empty(), doc.as_object_mut()) {
        fields.insert("assembly".to_owned(), json!({"joints": joints}));
    }
    let (hidden_occurrences, left_shown) = hidden_paths(
        &builder.occurrences,
        &builder.placements,
        &builder.hidden,
        MAX_HIDDEN_PATHS,
    );
    if left_shown != 0 {
        builder.report.push(
            Severity::Degraded,
            "visibility",
            format!(
                "{left_shown} or more placement(s) the source hides are left shown: hidden \
                 placements are written up to {MAX_HIDDEN_PATHS}"
            ),
        );
    }
    if !hidden_occurrences.is_empty() {
        builder.report.push(
            Severity::Info,
            "visibility",
            format!(
                "{} component placement(s) the source hides are placed hidden",
                hidden_occurrences.len()
            ),
        );
    }
    let mut visibility = serde_json::Map::new();
    if !hidden.is_empty() {
        visibility.insert("hidden_sketches".to_owned(), json!(hidden));
    }
    if !hidden_occurrences.is_empty() {
        visibility.insert("hidden_occurrences".to_owned(), json!(hidden_occurrences));
    }
    if let (false, Some(fields)) = (visibility.is_empty(), doc.as_object_mut()) {
        fields.insert("visibility".to_owned(), Value::Object(visibility));
    }
    builder
        .report
        .set_outcome(outcome(&builder.histories, builder.outcomes));
    builder.container.finish(&doc)
}

/// Every path from the design to a hidden occurrence, as SolidCraft hides occurrences: the
/// occurrence ids from the root down. A hidden occurrence inside a component placed more than
/// once is hidden in each placement of it. At most `cap` paths are made; the second value
/// counts the paths left out past it (at least that many: making them stops at the cap).
fn hidden_paths(
    occurrences: &[Value],
    placements: &BTreeMap<u64, (u64, [[f64; 4]; 3])>,
    hidden: &BTreeSet<u64>,
    cap: usize,
) -> (Vec<Vec<u64>>, usize) {
    let parent_of: BTreeMap<u64, u64> = occurrences
        .iter()
        .filter_map(|o| Some((o.get("id")?.as_u64()?, o.get("parent")?.as_u64()?)))
        .collect();
    let mut paths = Vec::new();
    let mut left = 0usize;
    for &id in hidden {
        // Paths from the root to `id`, built upwards: each placement of the component the
        // occurrence sits in leads one level out.
        let mut partial = vec![vec![id]];
        for _ in 0..=occurrences.len() {
            let mut next = Vec::new();
            for path in partial {
                let Some(&parent) = path.first().and_then(|first| parent_of.get(first)) else {
                    continue;
                };
                if parent == 0 {
                    if paths.len() < cap {
                        paths.push(path);
                    } else {
                        left += 1;
                    }
                    continue;
                }
                for (&outer, &(component, _)) in placements {
                    if component == parent && !path.contains(&outer) {
                        let mut longer = vec![outer];
                        longer.extend(&path);
                        next.push(longer);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            // Past the cap, the paths still being made are left out too.
            if paths.len() + next.len() > cap.saturating_mul(2) {
                left += next.len();
                break;
            }
            partial = next;
        }
    }
    paths.sort();
    paths.dedup();
    (paths, left)
}

/// What writing an assembly would do with its parts' histories, without restating geometry,
/// writing anything or running SolidCraft.
/// Reports each joint the assembly leaves out or changes: SolidCraft gets the joints Dutchman
/// converted and no other.
fn joint_notes(assembly: &Assembly, report: &mut Report) {
    for note in &assembly.joint_notes {
        report.push(Severity::Dropped, "joints", note.text.clone());
    }
}

pub(crate) fn plan(writer: &SolidCraftWriter, assembly: &Assembly, report: &mut Report) -> Outcome {
    joint_notes(assembly, report);
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
        let mut mapping = history::map(part, &mut part_report, writer.resolved_version());
        writer.plan_blends(&mut mapping);
        features.extend(mapping.features.iter().cloned().map(|mut feature| {
            feature.name = Some(format!("{file}: {}", feature.name.unwrap_or_default()));
            feature
        }));
        if mapping.result.is_err() {
            report.append_prefixed(history::helper_notes(&part_report), file);
        }
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
    /// The sheet rules the parts' histories use, each once by name.
    sheet_rules: Vec<Value>,
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
    /// Each root design's contiguous feature range, including its nested components.
    groups: Vec<(String, std::ops::Range<usize>)>,
    own_group: Option<usize>,
    /// The occurrences whose source placement is hidden.
    hidden: BTreeSet<u64>,
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
        for (index, part) in assembly.parts.iter().enumerate() {
            let start = self.features.len();
            let component = self.part(part, parent)?;
            if parent == 0 {
                if let Some((_, name)) = &component {
                    if assembly.own_part == Some(index) {
                        self.own_group = Some(self.groups.len());
                    }
                    self.groups.push((name.clone(), start..self.features.len()));
                }
            }
            components.push(component);
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
            if placement.hidden {
                self.hidden.insert(id);
            }
            occurrences.push(Some(id));
        }
        Ok(occurrences)
    }

    fn timeline_groups(&mut self) -> Vec<Value> {
        if self.groups.len() < 2 {
            return Vec::new();
        }
        let mut groups = Vec::new();
        for (name, range) in std::mem::take(&mut self.groups) {
            let features: Vec<Value> = self
                .features
                .get(range)
                .unwrap_or_default()
                .iter()
                .filter_map(|feature| feature.get("id").cloned())
                .collect();
            if features.is_empty() {
                continue;
            }
            let id = self.id();
            self.report.push(
                Severity::Info,
                "timeline folders",
                format!("\"{name}\": {} feature(s) grouped", features.len()),
            );
            groups.push(json!({"id": id, "name": name, "features": features, "parent": null}));
        }
        groups
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
                "flip": joint.flip,
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
        self.ground(&out);
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

    /// Grounds each held placement: held where it is in what holds it, as a design's root
    /// component is held by the document, or by the occurrence that inserts the design.
    fn hold(&mut self, held: &[Vec<usize>], top: &[Option<u64>]) {
        for path in held {
            let Some((id, _)) = self.resolve(path, top) else {
                continue;
            };
            if let Some(occurrence) = self
                .occurrences
                .iter_mut()
                .find(|occurrence| occurrence.get("id").and_then(Value::as_u64) == Some(id))
            {
                occurrence["grounded"] = json!(true);
            }
        }
    }

    /// Grounds one occurrence in each set of occurrences the joints tie together but none
    /// ties to the design: the second side of the set's first joint, the side a joint moves
    /// the other towards. A source that grounds nothing leaves SolidCraft's solver free to
    /// move the whole set; holding that side keeps every part where the source places it.
    fn ground(&mut self, joints: &[Value]) {
        let side = |row: &Value, key: &str| row.pointer(&format!("/{key}/occurrence"))?.as_u64();
        // Union-find over the occurrences the joints name, 0 being the design.
        let mut parent: BTreeMap<u64, u64> = BTreeMap::new();
        fn root(parent: &BTreeMap<u64, u64>, mut at: u64) -> u64 {
            let mut steps = 0;
            while let Some(&up) = parent.get(&at) {
                if up == at || steps > parent.len() {
                    break;
                }
                at = up;
                steps += 1;
            }
            at
        }
        for row in joints {
            let (Some(a), Some(b)) = (side(row, "a"), side(row, "b")) else {
                continue;
            };
            parent.entry(a).or_insert(a);
            parent.entry(b).or_insert(b);
            let (ra, rb) = (root(&parent, a), root(&parent, b));
            if ra != rb {
                // The design's own set keeps 0 as its root, so it is never grounded again.
                let (keep, join) = if rb == 0 { (rb, ra) } else { (ra, rb) };
                parent.insert(join, keep);
            }
        }
        let mut grounded = BTreeSet::new();
        for row in joints {
            let (Some(a), Some(b)) = (side(row, "a"), side(row, "b")) else {
                continue;
            };
            let set = root(&parent, a);
            if set == 0 || root(&parent, 0) == set || !grounded.insert(set) {
                continue;
            }
            let joint = row.get("name").and_then(Value::as_str).unwrap_or("a joint");
            if let Some(occurrence) = self
                .occurrences
                .iter_mut()
                .find(|occurrence| occurrence.get("id").and_then(Value::as_u64) == Some(b))
            {
                occurrence["grounded"] = json!(true);
                let name = occurrence
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("an occurrence")
                    .to_owned();
                self.report.push(
                    Severity::Info,
                    "joints",
                    format!(
                        "\"{name}\" is grounded, inferred: the source grounds none of the parts \
                         its joints tie together, so the fixed side of \"{joint}\" holds them"
                    ),
                );
            }
        }
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

    /// The component a part or sub-assembly becomes, with its name; `None` when it has
    /// geometry SolidCraft cannot show and is left out. A part with no bodies at all, as a
    /// component empty in its source, becomes an empty component, so joints to it still hold.
    fn part(&mut self, part: &Model, parent: u64) -> Result<Option<(u64, String)>, Error> {
        let step = match &part.assembly {
            Some(_) => None,
            None if part.ir.model.bodies.is_empty() => {
                let file = part.source_file.as_deref().unwrap_or(&part.name);
                self.report.push(
                    Severity::Info,
                    file,
                    format!(
                        "{}: placed as an empty component: it has no bodies",
                        part.name
                    ),
                );
                None
            }
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
        let mut mapping = history::map(part, &mut part_report, self.writer.resolved_version());
        if let Ok(history) = &mut mapping.result {
            if let Err(why) = history::base::restate(history, part) {
                mapping.result = Err(why);
            }
        }
        self.writer.prove_blends(part, &mut mapping, step)?;
        self.outcomes
            .extend(mapping.features.iter().cloned().map(|mut feature| {
                feature.name = Some(format!("{file}: {}", feature.name.unwrap_or_default()));
                feature
            }));
        let mut history = match mapping.result {
            Ok(history) => history,
            Err(reason) => {
                self.report
                    .append_prefixed(history::helper_notes(&part_report), &file);
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
        let summary = self.writer.history_summary(&history);
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
                self.report
                    .append_prefixed(history::helper_notes(&part_report), &file);
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
        for rule in history.sheet_rules.drain(..) {
            if !self
                .sheet_rules
                .iter()
                .any(|known| known.get("name") == rule.get("name"))
            {
                self.sheet_rules.push(rule);
            }
        }
        self.looks.append(&mut history.looks);
        self.face_looks.append(&mut history.face_looks);
        self.histories.push(check);
        Ok(true)
    }

    /// Adds the sketches of the assembly's own file, those that map, to the design itself
    /// (SolidCraft's root component), hidden for reference as a flattened part's are; returns
    /// their ids. CADIR places a sketch in model space without saying which component it
    /// belongs to, so a component's sketch is kept where it was drawn, not moved with it.
    fn sketches(&mut self, model: &Model) -> Vec<Value> {
        let (mut kept, left_out) = history::sketches(model, self.report);
        let count = kept.features.len();
        if count > 0 || left_out > 0 {
            let noun = if count == 1 { "sketch" } else { "sketches" };
            let detail = if left_out > 0 {
                format!(
                    "kept {count} {noun} in the design itself, for reference; {left_out} left out"
                )
            } else {
                format!("kept {count} {noun} in the design itself, for reference")
            };
            self.report.push(Severity::Info, "sketches", detail);
        }
        if count == 0 {
            return Vec::new();
        }
        let offset = self.next_id.saturating_sub(1);
        kept.relocate(offset, 0, &prefix(&model.name), &mut self.taken);
        self.next_id = kept.next_id;
        let hidden = kept
            .features
            .iter()
            .filter_map(|feature| feature.get("id").cloned())
            .collect();
        self.features.append(&mut kept.features);
        self.params.append(&mut kept.params);
        hidden
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod hidden_tests {
    use super::*;

    #[test]
    fn hidden_paths_past_the_cap_are_counted_as_left_shown() {
        // Component 10 is placed by occurrences 1 and 2 at the root, and holds hidden 3.
        let occurrences = vec![
            json!({"id": 1, "parent": 0, "component": 10}),
            json!({"id": 2, "parent": 0, "component": 10}),
            json!({"id": 3, "parent": 10, "component": 20}),
        ];
        let identity = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let placements: BTreeMap<u64, (u64, [[f64; 4]; 3])> = [
            (1, (10, identity)),
            (2, (10, identity)),
            (3, (20, identity)),
        ]
        .into();
        let hidden = BTreeSet::from([3]);
        let (paths, left) = hidden_paths(&occurrences, &placements, &hidden, 8);
        assert_eq!(paths, [vec![1, 3], vec![2, 3]]);
        assert_eq!(left, 0);
        let (paths, left) = hidden_paths(&occurrences, &placements, &hidden, 1);
        assert_eq!(paths.len(), 1);
        assert_eq!(left, 1);
    }
}
